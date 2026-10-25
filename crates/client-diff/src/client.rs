use std::{
    collections::HashMap,
    fs::File,
    net::SocketAddr,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use client_diff::recording::{self, Recorder};
use jolyne::{
    ClientConfig, JolyneError,
    stream::{BedrockStream, Client, Handshake},
    valentine::*,
};
use p384::{
    SecretKey,
    ecdsa::{Signature, SigningKey, signature::Signer},
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
struct Scenarios {
    identity: Identity,
    scenarios: HashMap<String, Scenario>,
}

#[derive(Deserialize)]
struct Identity {
    display_name: String,
    uuid: Uuid,
    device_id: String,
    client_random_id: i64,
    device_os: i32,
}

#[derive(Deserialize)]
struct Scenario {
    idle_ms: u64,
    steps: Vec<Step>,
    /// Tick like a vanilla client: PlayerAuthInput every 50ms plus respawn and
    /// dimension-change acknowledgements (see `gt-client`).
    #[serde(default)]
    auth_input: bool,
}

/// Interaction kinds (`move`, `place`, ...) are gophertunnel-only; Jolyne
/// rejects scenarios containing them before connecting.
#[derive(Deserialize)]
struct Step {
    after_ms: u64,
    kind: String,
    #[serde(default)]
    text: String,
}

/// Player state needed to tick and to expand `{x}`/`{y}`/`{z}` (the spawn
/// feet block) in command text, as `gt-client` does.
struct PlayerState {
    runtime_id: u64,
    position: Vec3,
    /// Fixed on first use: in a fresh world StartGame carries a placeholder
    /// height (y=32767) that MovePlayer corrects once the spawn is resolved.
    anchor: Option<[i32; 3]>,
    tick: u64,
}

/// Eye height above the feet in StartGame/MovePlayer/PlayerAuthInput positions.
const EYE_HEIGHT: f32 = 1.62;

impl PlayerState {
    fn new(start_game: &StartGamePacket) -> Self {
        Self {
            runtime_id: start_game.runtime_id.actor_runtime_id,
            position: start_game.position.clone(),
            anchor: None,
            tick: 0,
        }
    }

    fn expand(&mut self, text: &str) -> String {
        let position = &self.position;
        let anchor = *self.anchor.get_or_insert_with(|| {
            [
                position.x.floor() as i32,
                (position.y - EYE_HEIGHT + 0.01).floor() as i32,
                position.z.floor() as i32,
            ]
        });
        text.replace("{x}", &anchor[0].to_string())
            .replace("{y}", &anchor[1].to_string())
            .replace("{z}", &anchor[2].to_string())
    }

    fn auth_input(&mut self) -> McpePacket {
        self.tick += 1;
        McpePacket::from(PlayerAuthInputPacket {
            position: self.position.clone(),
            input_mode: EnumsInputMode::Mouse,
            play_mode: EnumsClientPlayMode::Normal,
            new_interaction_model: EnumsNewInteractionModel::Crosshair,
            client_tick: PlayerInputTick {
                inputtick: self.tick,
            },
            ..Default::default()
        })
    }

    fn action(&self, action: EnumsPlayerActionType) -> McpePacket {
        McpePacket::from(PlayerActionPacket {
            player_runtime_id: ActorRuntimeId {
                actor_runtime_id: self.runtime_id,
            },
            action,
            face: -1,
            ..Default::default()
        })
    }

    /// Tracks the own position and returns the acknowledgements a vanilla
    /// client sends for respawns and dimension changes.
    fn handle(&mut self, packet: McpePacketData) -> Vec<McpePacket> {
        match packet {
            McpePacketData::MovePlayerPacket(packet)
                if packet.player_runtime_id.actor_runtime_id == self.runtime_id =>
            {
                self.position = packet.position;
            }
            McpePacketData::CorrectPlayerMovePredictionPacket(packet) => {
                self.position = packet.pos;
            }
            McpePacketData::ChangeDimensionPacket(packet) => {
                self.position = packet.position;
                return vec![self.action(EnumsPlayerActionType::Changedimensionack)];
            }
            McpePacketData::RespawnPacket(packet) => match packet.state {
                EnumsPlayerRespawnState::Searchingforspawn => {
                    return vec![
                        McpePacket::from(RespawnPacket {
                            position: packet.position,
                            state: EnumsPlayerRespawnState::Clientreadytospawn,
                            player_runtime_id: ActorRuntimeId {
                                actor_runtime_id: self.runtime_id,
                            },
                        }),
                        self.action(EnumsPlayerActionType::Respawn),
                    ];
                }
                EnumsPlayerRespawnState::Readytospawn => self.position = packet.position,
                _ => {}
            },
            _ => {}
        }
        Vec::new()
    }
}

struct ScenarioClock<'a> {
    scenario: &'a Scenario,
    index: usize,
    completed: tokio::time::Instant,
}

impl ScenarioClock<'_> {
    fn deadline(&self) -> Result<tokio::time::Instant> {
        let delay = self
            .scenario
            .steps
            .get(self.index)
            .map_or(self.scenario.idle_ms, |step| step.after_ms);
        self.completed
            .checked_add(Duration::from_millis(delay))
            .context("scenario timing overflow")
    }

    fn step_completed(&mut self, completed: tokio::time::Instant) {
        self.completed = completed;
        self.index += 1;
    }
}

fn offline_tokens(
    key: &SecretKey,
    identity: &Identity,
    address: SocketAddr,
) -> Result<(String, String)> {
    let (chain, token) = jolyne::auth::client::generate_self_signed_chain(
        key,
        &identity.display_name,
        identity.uuid,
    )?;
    let parts: Vec<_> = token.split('.').collect();
    ensure!(parts.len() == 3, "generated client token is not a JWT");
    let mut claims: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1])?)?;
    claims["DeviceOS"] = identity.device_os.into();
    claims["DeviceId"] = identity.device_id.clone().into();
    claims["ClientRandomId"] = identity.client_random_id.into();
    claims["DeviceModel"] = "ClientDiff".into();
    claims["ServerAddress"] = address.to_string().into();
    claims["PlayFabId"] = "0000000000000000".into();
    let unsigned = format!(
        "{}.{}",
        parts[0],
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?)
    );
    let signing = SigningKey::from(key.clone());
    let signature: Signature = signing.sign(unsigned.as_bytes());
    Ok((
        chain,
        format!(
            "{unsigned}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        ),
    ))
}

/// Runs a real offline Jolyne connection, capturing login and play wire packets.
pub async fn run(
    address: SocketAddr,
    scenario: &str,
    scenarios: &Path,
    output: &Path,
) -> Result<()> {
    let config: Scenarios =
        serde_json::from_reader(File::open(scenarios).context("opening scenarios")?)?;
    let scenario = config.scenarios.get(scenario).context("unknown scenario")?;
    ensure!(
        config.identity.device_os == 1,
        "client-diff requires Android DeviceOS 1"
    );
    // Validate before opening the connection.
    for step in &scenario.steps {
        ensure!(
            matches!(step.kind.as_str(), "chat" | "command"),
            "step kind {} is not supported by the Jolyne client (gophertunnel-only interaction)",
            step.kind
        );
        ensure!(!step.text.is_empty(), "{} step needs text", step.kind);
    }
    let key = SecretKey::random(&mut p384::elliptic_curve::rand_core::OsRng);
    let (chain, token) = offline_tokens(&key, &config.identity, address)?;
    let capture = Arc::new(Mutex::new(Recorder::create(output, key.clone())?));
    let transport = recording::connect(address, capture.clone()).await?;
    // `ClientConfig::join_timeout` (120 s by default) bounds the whole login.
    let stream =
        BedrockStream::<Handshake, Client, _>::from_transport(transport, ClientConfig::default())
            .await?;
    let (mut play, game_data) = async {
        let login = stream.request_settings().await?;
        let secure = login.send_login_with_tokens(&key, &chain, &token).await?;
        let packs = secure.await_handshake().await?;
        Ok::<_, JolyneError>(packs.handle_packs().await?.await_start_game().await?)
    }
    .await
    .context("full login")?;
    let mut player = PlayerState::new(&game_data.start_game);
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    let mut clock = ScenarioClock {
        scenario,
        index: 0,
        completed: tokio::time::Instant::now(),
    };
    loop {
        let deadline = clock.deadline()?;
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => {
                let Some(step) = scenario.steps.get(clock.index) else { break };
                let text = player.expand(&step.text);
                let packet = match step.kind.as_str() {
                    "chat" => McpePacket::from(TextPacket {
                        body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
                            message_type: TextPacketPayloadAuthorAndMessageMessageType::Chat,
                            player_name: config.identity.display_name.clone().into(),
                            message: text.into(),
                        }),
                        ..Default::default()
                    }),
                    "command" => McpePacket::from(CommandRequestPacket {
                        command: text.into(),
                        origin: CommandOriginDatajson {
                            type_: "player".into(), uuid: config.identity.uuid,
                            ..Default::default()
                        },
                        is_internal: false,
                        version: "latest".into(),
                    }),
                    _ => unreachable!("validated before connection"),
                };
                play.send_packet(packet).await?;
                clock.step_completed(tokio::time::Instant::now());
            }
            _ = ticker.tick(), if scenario.auth_input => {
                play.send_packet(player.auth_input()).await?;
            }
            packet = play.recv_packet_raw() => {
                let packet = match packet {
                    Ok(packet) => packet,
                    Err(JolyneError::Disconnected(disconnect)) => bail!(
                        "server disconnected during scenario: {:?}",
                        disconnect.packet.body()
                    ),
                    Err(error) => return Err(error.into()),
                };
                match packet.name() {
                    Some(McpePacketName::NetworkStackLatencyPacket) => {
                        if let McpePacketData::NetworkStackLatencyPacket(mut latency) = packet.decode()?.data
                            && latency.is_from_server
                        {
                            latency.is_from_server = false;
                            play.send_packet(McpePacket::from(latency)).await?;
                        }
                    }
                    Some(
                        McpePacketName::MovePlayerPacket
                        | McpePacketName::CorrectPlayerMovePredictionPacket
                        | McpePacketName::ChangeDimensionPacket
                        | McpePacketName::RespawnPacket,
                    ) if scenario.auth_input => {
                        for reply in player.handle(packet.decode()?.data) {
                            play.send_packet(reply).await?;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    // Closing drains pending writes and shuts the RakNet connection; no
    // invented Bedrock Disconnect packet is sent (that packet is server-to-client).
    play.close().await?;
    capture
        .lock()
        .map_err(|_| anyhow::anyhow!("capture lock poisoned"))?
        .flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use p384::ecdsa::{Signature, VerifyingKey, signature::Verifier};

    #[test]
    fn fixed_identity_is_signed_into_client_data() {
        let key = SecretKey::from_slice(&[1; 48]).unwrap();
        let identity = Identity {
            display_name: "ClientDiff".into(),
            uuid: Uuid::nil(),
            device_id: "fixed-device".into(),
            client_random_id: 712345678901,
            device_os: 7,
        };
        let (_, token) =
            offline_tokens(&key, &identity, "127.0.0.1:19132".parse().unwrap()).unwrap();
        let parts: Vec<_> = token.split('.').collect();
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["DeviceOS"], 7);
        assert_eq!(claims["DeviceId"], "fixed-device");
        assert_eq!(claims["ClientRandomId"], 712345678901_i64);
        assert_eq!(claims["ThirdPartyName"], "ClientDiff");
        assert_eq!(claims["ServerAddress"], "127.0.0.1:19132");
        let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
        VerifyingKey::from(key.public_key())
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .unwrap();
    }

    #[test]
    fn slow_sends_do_not_shorten_following_delays_or_final_idle() {
        let scenario = Scenario {
            idle_ms: 5000,
            auth_input: false,
            steps: vec![
                Step {
                    after_ms: 500,
                    kind: "chat".into(),
                    text: "one".into(),
                },
                Step {
                    after_ms: 1000,
                    kind: "command".into(),
                    text: "/say two".into(),
                },
            ],
        };
        let start = tokio::time::Instant::now();
        let mut clock = ScenarioClock {
            scenario: &scenario,
            index: 0,
            completed: start,
        };
        assert_eq!(
            clock.deadline().unwrap() - start,
            Duration::from_millis(500)
        );
        let first_send_done = start + Duration::from_secs(10);
        clock.step_completed(first_send_done);
        assert_eq!(
            clock.deadline().unwrap() - first_send_done,
            Duration::from_millis(1000)
        );
        let final_send_done = first_send_done + Duration::from_secs(20);
        clock.step_completed(final_send_done);
        assert_eq!(
            clock.deadline().unwrap() - final_send_done,
            Duration::from_secs(5)
        );
    }

    #[test]
    fn anchor_uses_the_corrected_spawn_and_respawn_is_acknowledged() {
        let start_game = StartGamePacket {
            runtime_id: ActorRuntimeId {
                actor_runtime_id: 7,
            },
            // Fresh worlds announce a placeholder height before MovePlayer.
            position: Vec3 {
                x: 0.5,
                y: 32767.0,
                z: -0.5,
            },
            ..Default::default()
        };
        let mut player = PlayerState::new(&start_game);
        player.handle(McpePacketData::MovePlayerPacket(MovePlayerPacket {
            player_runtime_id: ActorRuntimeId {
                actor_runtime_id: 7,
            },
            position: Vec3 {
                x: 0.5,
                y: -58.38,
                z: -0.5,
            },
            ..Default::default()
        }));
        assert_eq!(player.expand("/tp @s {x} {y} {z}"), "/tp @s 0 -60 -1");
        // The anchor stays fixed after the player moves on.
        player.position.x = 20.0;
        assert_eq!(player.expand("{x}"), "0");

        let replies = player.handle(McpePacketData::RespawnPacket(RespawnPacket {
            state: EnumsPlayerRespawnState::Searchingforspawn,
            ..Default::default()
        }));
        assert!(matches!(
            &replies[0].data,
            McpePacketData::RespawnPacket(packet)
                if packet.state == EnumsPlayerRespawnState::Clientreadytospawn
                    && packet.player_runtime_id.actor_runtime_id == 7
        ));
        assert!(matches!(
            &replies[1].data,
            McpePacketData::PlayerActionPacket(packet)
                if packet.action == EnumsPlayerActionType::Respawn
        ));
    }
}
