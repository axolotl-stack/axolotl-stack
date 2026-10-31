//! Runs a [`Scenario`] over a real offline client connection (Jolyne over
//! RakNet), sending the same client packets the gophertunnel bot sends and
//! recording the session with [`Recorder`] in the bot's JSONL format.
//!
//! Timing follows the bot: each step waits `after_ms` after the previous
//! step was issued; interaction ticks are queued and sent one per 50 ms
//! PlayerAuthInput tick; the scenario idles `idle_ms` after its last step.

mod session;
mod steps;

use std::{
    collections::VecDeque,
    io::Write,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jolyne::{
    ClientConfig, JolyneError,
    stream::{BedrockStream, Client, Handshake},
    valentine::*,
};
use p384::{
    SecretKey,
    ecdsa::{Signature, SigningKey, signature::Signer},
};

use crate::recording::{self, Recorder};
use crate::scenario::{Identity, Scenario, StepKind};
pub use session::Session;
use steps::InputSpec;

/// What a run produced besides its recording.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The player's runtime and unique entity IDs from StartGame.
    pub runtime_id: u64,
    pub unique_id: i64,
    /// Steps skipped (lenient), refused requests and wait_event timeouts,
    /// in order: the bot reports the same events on stderr.
    pub notes: Vec<String>,
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

/// When the next step (or the final idle) is due: `after_ms` after the
/// previous step completed, so slow sends never shorten later delays.
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

/// A `wait_event` step in progress.
struct EventWait {
    event: u8,
    until: tokio::time::Instant,
}

fn actor_event_id(event: &EnumsActorEvent) -> u8 {
    use jolyne::valentine::bedrock::codec::BedrockCodec;
    let mut buf = Vec::with_capacity(1);
    event
        .encode(&mut buf)
        .expect("encoding into a Vec cannot fail");
    buf[0]
}

/// Connects to `address` as `identity`, runs `scenario` and writes the
/// session's packets to `capture` as recording JSONL.
pub async fn run(
    address: SocketAddr,
    identity: &Identity,
    scenario: &Scenario,
    capture: Box<dyn Write + Send>,
) -> Result<Outcome> {
    scenario.validate()?;
    let key = SecretKey::random(&mut p384::elliptic_curve::rand_core::OsRng);
    let recorder = Arc::new(Mutex::new(Recorder::new(capture, key.clone())));
    let (chain, token) = offline_tokens(&key, identity, address)?;
    let transport = recording::connect(address, recorder.clone()).await?;
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
    let start_game = &game_data.start_game;
    let mut outcome = Outcome {
        runtime_id: start_game.runtime_id.actor_runtime_id,
        unique_id: start_game.entity_id.actor_unique_id,
        notes: Vec::new(),
    };
    let mut session = Session::new(outcome.runtime_id, &start_game.position);
    let mut inputs: VecDeque<InputSpec> = VecDeque::new();
    let mut waiting: Option<EventWait> = None;
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut clock = ScenarioClock {
        scenario,
        index: 0,
        completed: tokio::time::Instant::now(),
    };
    loop {
        let deadline = match &waiting {
            Some(wait) => wait.until,
            None => clock.deadline()?,
        };
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => {
                if let Some(wait) = waiting.take() {
                    outcome.notes.push(format!(
                        "step {}: wait_event: no actor event {} in time",
                        clock.index, wait.event
                    ));
                    clock.step_completed(tokio::time::Instant::now());
                    continue;
                }
                let Some(step) = scenario.steps.get(clock.index) else { break };
                match step.kind {
                    StepKind::Chat => {
                        let message = session.expand(&step.text);
                        play.send_packet(McpePacket::from(TextPacket {
                            body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
                                message_type: TextPacketPayloadAuthorAndMessageMessageType::Chat,
                                player_name: identity.display_name.clone().into(),
                                message: message.into(),
                            }),
                            ..Default::default()
                        })).await?;
                        play.flush().await?;
                    }
                    StepKind::Command => {
                        let command = session.expand(&step.text);
                        play.send_packet(McpePacket::from(CommandRequestPacket {
                            command: command.into(),
                            origin: CommandOriginDatajson {
                                type_: "player".into(),
                                uuid: identity.uuid,
                                ..Default::default()
                            },
                            is_internal: false,
                            version: "latest".into(),
                        })).await?;
                        play.flush().await?;
                    }
                    StepKind::WaitEvent => {
                        // Only events after the step starts count.
                        waiting = Some(EventWait {
                            event: u8::try_from(step.event).context("wait_event event out of range")?,
                            until: tokio::time::Instant::now() + Duration::from_millis(step.timeout_ms),
                        });
                        continue;
                    }
                    _ => match session.interaction(step) {
                        Ok((packets, specs)) => {
                            inputs.extend(specs);
                            if !packets.is_empty() {
                                for packet in packets {
                                    play.send_packet(packet).await?;
                                }
                                play.flush().await?;
                            }
                        }
                        // The step depends on an earlier server decision
                        // (e.g. a craft whose inputs were already used).
                        Err(error) if step.lenient => outcome.notes.push(format!(
                            "step {} ({}) skipped: {error:#}",
                            clock.index,
                            step.kind.name()
                        )),
                        Err(error) => {
                            return Err(error).with_context(|| {
                                format!("step {} ({})", clock.index, step.kind.name())
                            });
                        }
                    },
                }
                clock.step_completed(tokio::time::Instant::now());
            }
            _ = ticker.tick(), if scenario.auth_input => {
                let spec = inputs.pop_front().unwrap_or_default();
                for packet in session.tick(spec) {
                    play.send_packet(packet).await?;
                }
                play.flush().await?;
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
                let data = packet.decode()?.data;
                let mut replies = Vec::new();
                if let McpePacketData::NetworkStackLatencyPacket(latency) = &data
                    && latency.is_from_server
                {
                    replies.push(McpePacket::from(NetworkStackLatencyPacket {
                        creation_time: latency.creation_time,
                        is_from_server: false,
                    }));
                }
                if let (Some(wait), McpePacketData::ActorEventPacket(event)) = (&waiting, &data)
                    && actor_event_id(&event.event_id) == wait.event
                {
                    waiting = None;
                    clock.step_completed(tokio::time::Instant::now());
                }
                if scenario.auth_input {
                    replies.extend(session.handle(data));
                }
                if !replies.is_empty() {
                    for reply in replies {
                        play.send_packet(reply).await?;
                    }
                    play.flush().await?;
                }
            }
        }
    }
    // Closing drains pending writes and shuts the RakNet connection; no
    // invented Disconnect packet is sent (that packet is server-to-client).
    play.close().await?;
    recorder
        .lock()
        .map_err(|_| anyhow::anyhow!("capture lock poisoned"))?
        .flush()?;
    outcome.notes.append(&mut session.notes);
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Step;
    use p384::ecdsa::{Signature, VerifyingKey, signature::Verifier};
    use uuid::Uuid;

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
        let step = |after_ms, kind| Step {
            after_ms,
            kind,
            text: "x".into(),
            ..Step::default()
        };
        let scenario = Scenario {
            idle_ms: 5000,
            auth_input: false,
            steps: vec![step(500, StepKind::Chat), step(1000, StepKind::Command)],
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
    fn actor_event_ids_are_wire_values() {
        assert_eq!(actor_event_id(&EnumsActorEvent::FishhookHooktime), 13);
        assert_eq!(actor_event_id(&EnumsActorEvent::Hurt), 2);
    }
}
