use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufWriter, Read, Write},
    net::SocketAddr,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context as TaskContext, Poll},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD},
};
use bytes::Bytes;
use ctr::cipher::{KeyIvInit, StreamCipher};
use jolyne::{
    ClientConfig, JolyneError,
    proto::raw::decode_packet_raw,
    stream::transport::{Datagram, Framing, RakNetTransport, Transmit, Transport},
    stream::{BedrockStream, Client, Handshake},
    valentine::*,
};
use p384::{
    PublicKey, SecretKey,
    ecdsa::{Signature, SigningKey, signature::Signer},
    pkcs8::DecodePublicKey,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
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

fn packet_bytes(frame: &[u8]) -> Result<&[u8]> {
    for (i, byte) in frame.iter().take(5).enumerate() {
        if byte & 0x80 == 0 {
            return Ok(&frame[i + 1..]);
        }
    }
    bail!("invalid packet length prefix")
}

fn numeric_varint(data: &[u8]) -> Result<(u32, usize)> {
    let mut value = 0u32;
    for (index, byte) in data.iter().take(5).enumerate() {
        ensure!(index != 4 || *byte <= 15, "capture varuint overflow");
        value |= u32::from(byte & 127) << (index * 7);
        if byte & 128 == 0 {
            return Ok((value, index + 1));
        }
    }
    bail!("truncated capture varuint")
}

// Split numeric packet frames before typed dispatch so future/unknown packets
// are retained verbatim, even when Jolyne itself cannot interpret them.
fn capture_entries(frame: &Bytes, compression: bool) -> Result<Vec<Bytes>> {
    const MAX: usize = 4 * 1024 * 1024;
    let payload = if frame.first() == Some(&0xfe) {
        frame.slice(1..)
    } else {
        frame.clone()
    };
    let payload = if compression {
        let (&algorithm, compressed) = payload.split_first().context("empty compressed capture")?;
        let decompressed = match algorithm {
            0 => {
                let mut output = Vec::new();
                flate2::read::DeflateDecoder::new(compressed)
                    .take((MAX + 1) as u64)
                    .read_to_end(&mut output)?;
                output
            }
            1 => {
                ensure!(
                    snap::raw::decompress_len(compressed)? <= MAX,
                    "capture decompression limit"
                );
                snap::raw::Decoder::new().decompress_vec(compressed)?
            }
            0xff => compressed.to_vec(),
            _ => bail!("unknown capture compression algorithm {algorithm}"),
        };
        Bytes::from(decompressed)
    } else {
        payload
    };
    ensure!(payload.len() <= MAX, "capture decompression limit");
    let mut packets = Vec::new();
    let mut offset = 0;
    while offset < payload.len() {
        let (length, prefix) = numeric_varint(&payload[offset..])?;
        ensure!(length != 0, "empty capture packet");
        let end = offset
            .checked_add(prefix)
            .and_then(|n| n.checked_add(length as usize))
            .context("capture frame length overflow")?;
        ensure!(end <= payload.len(), "truncated capture packet");
        packets.push(payload.slice(offset..end));
        offset = end;
    }
    Ok(packets)
}

type Cipher = ctr::Ctr32BE<aes::Aes256>;

struct Capture {
    writer: BufWriter<File>,
    started: Instant,
    seq: u64,
    compression: bool,
    identity_key: SecretKey,
    key: Option<[u8; 32]>,
    ciphers: Option<[Cipher; 2]>,
    counters: [u64; 2],
}

impl Capture {
    fn frame(&mut self, direction: usize, frame: &Bytes) -> Result<()> {
        if frame.is_empty() {
            return Ok(());
        }
        let data = if let Some(ciphers) = &mut self.ciphers {
            ensure!(frame.len() >= 9, "truncated encrypted frame");
            let mut plain = frame.to_vec();
            ciphers[direction].apply_keystream(&mut plain[1..]);
            let end = plain.len() - 8;
            let mut digest = Sha256::new();
            digest.update(self.counters[direction].to_le_bytes());
            digest.update(&plain[1..end]);
            digest.update(self.key.context("capture cipher has no key")?);
            ensure!(
                digest.finalize()[..8] == plain[end..],
                "capture encryption checksum mismatch"
            );
            self.counters[direction] += 1;
            plain.truncate(end);
            Bytes::from(plain)
        } else {
            frame.clone()
        };
        let packets = capture_entries(&data, self.compression)?;
        for packet in packets {
            let payload = packet_bytes(&packet)?;
            let id = numeric_varint(payload)?.0 & 1023;
            let name = decode_packet_raw(&mut packet.clone())
                .ok()
                .and_then(|known| known.name())
                .map_or_else(|| format!("Unknown_{id}"), |name| format!("{name:?}"));
            let record = serde_json::json!({
                "seq": self.seq,
                "t_ms": self.started.elapsed().as_millis(),
                "dir": if direction == 0 { "c2s" } else { "s2c" },
                "id": id,
                "name": name,
                "hex": hex::encode(payload),
            });
            serde_json::to_writer(&mut self.writer, &record)?;
            self.writer.write_all(b"\n")?;
            // Surface storage errors at the packet that caused them, not on drop.
            self.writer.flush()?;
            self.seq += 1;
            if direction == 1 {
                match id {
                    143 => self.compression = true,
                    3 => {
                        if let (_, McpePacketData::ServerToClientHandshakePacket(handshake)) =
                            McpePacketData::decode_inner_bytes(&mut packet.clone(), McpePacketArgs)?
                        {
                            self.enable_encryption(&handshake.handshake_web_token)?;
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn enable_encryption(&mut self, token: &str) -> Result<()> {
        // The real client independently verifies the signature before accepting it.
        let parts: Vec<_> = token.split('.').collect();
        ensure!(parts.len() == 3, "invalid server handshake JWT");
        let header: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0])?)?;
        let claims: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1])?)?;
        let der = STANDARD.decode(header["x5u"].as_str().context("handshake missing x5u")?)?;
        let public = PublicKey::from_public_key_der(&der)?;
        let salt = claims["salt"].as_str().context("handshake missing salt")?;
        let salt = STANDARD
            .decode(salt)
            .or_else(|_| STANDARD_NO_PAD.decode(salt))?;
        let secret =
            p384::ecdh::diffie_hellman(self.identity_key.to_nonzero_scalar(), public.as_affine());
        let mut digest = Sha256::new();
        digest.update(salt);
        digest.update(secret.raw_secret_bytes());
        let key: [u8; 32] = digest.finalize().into();
        let mut iv = [0; 16];
        iv[..12].copy_from_slice(&key[..12]);
        iv[15] = 2;
        let cipher = Cipher::new_from_slices(&key, &iv)
            .map_err(|_| anyhow::anyhow!("invalid capture cipher key"))?;
        self.ciphers = Some([cipher.clone(), cipher]);
        self.key = Some(key);
        Ok(())
    }
}

struct ObservedTransport {
    inner: RakNetTransport,
    capture: Arc<Mutex<Capture>>,
}

impl ObservedTransport {
    fn observe(&self, direction: usize, bytes: &Bytes) -> io::Result<()> {
        self.capture
            .lock()
            .map_err(|_| io::Error::other("capture lock poisoned"))?
            .frame(direction, bytes)
            .map_err(|error| io::Error::other(format!("packet capture: {error:#}")))
    }
}

impl Transport for ObservedTransport {
    type Error = io::Error;
    const FRAMING: Framing = Framing::RakNet;

    fn poll_send_ready(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner)
            .poll_send_ready(cx)
            .map_err(io::Error::other)
    }

    fn start_send(mut self: Pin<&mut Self>, message: Transmit) -> io::Result<()> {
        let bytes = message.payload.clone();
        Pin::new(&mut self.inner)
            .start_send(message)
            .map_err(io::Error::other)?;
        self.observe(0, &bytes)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner)
            .poll_flush(cx)
            .map_err(io::Error::other)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner)
            .poll_close(cx)
            .map_err(io::Error::other)
    }

    fn poll_recv(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<Option<io::Result<Datagram>>> {
        match Pin::new(&mut self.inner).poll_recv(cx) {
            Poll::Ready(Some(Ok(datagram))) => {
                let bytes = datagram.into_bytes();
                Poll::Ready(Some(
                    self.observe(1, &bytes)
                        .map(|()| Datagram::Contiguous(bytes)),
                ))
            }
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(io::Error::other(error)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn peer_addr(&self) -> SocketAddr {
        self.inner.peer_addr()
    }

    fn latency(&self) -> Option<Duration> {
        self.inner.latency()
    }
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
    let capture = Arc::new(Mutex::new(Capture {
        writer: BufWriter::new(File::create(output).context("creating capture")?),
        started: Instant::now(),
        seq: 0,
        compression: false,
        identity_key: key.clone(),
        key: None,
        ciphers: None,
        counters: [0, 0],
    }));
    let socket = tokio::time::timeout(
        Duration::from_secs(30),
        tokio_raknet::RaknetStream::connect(address),
    )
    .await
    .context("RakNet connection timeout")??;
    let transport = ObservedTransport {
        inner: RakNetTransport::new(socket),
        capture: capture.clone(),
    };
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
        .writer
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
    fn raw_capture_removes_only_length_prefix() {
        let frame = Bytes::from_static(&[4, 0x81, 0x02, 0xaa, 0xbb]);
        assert_eq!(packet_bytes(&frame).unwrap(), &[0x81, 0x02, 0xaa, 0xbb]);
        assert!(packet_bytes(&Bytes::from_static(&[0x80])).is_err());
    }

    #[test]
    fn capture_preserves_wire_headers_and_sequences_both_directions() {
        let path = std::env::temp_dir().join(format!("client-diff-{}.jsonl", Uuid::new_v4()));
        let mut capture = Capture {
            writer: BufWriter::new(File::create(&path).unwrap()),
            started: Instant::now(),
            seq: 0,
            compression: false,
            identity_key: SecretKey::from_slice(&[1; 48]).unwrap(),
            key: None,
            ciphers: None,
            counters: [0, 0],
        };
        // RequestNetworkSettings with a two-byte varuint packet header, and
        // PlayStatus, each in an uncompressed pre-negotiation RakNet batch.
        capture
            .frame(0, &Bytes::from_static(&[0xfe, 6, 0xc1, 1, 0, 0, 8, 0x91]))
            .unwrap();
        capture
            .frame(1, &Bytes::from_static(&[0xfe, 5, 2, 0, 0, 0, 0]))
            .unwrap();
        // An unknown numeric ID must be retained without losing neighboring packets.
        capture
            .frame(
                1,
                &Bytes::from_static(&[0xfe, 3, 0xff, 0x07, 0xaa, 5, 2, 0, 0, 0, 0]),
            )
            .unwrap();
        drop(capture);
        let records: Vec<serde_json::Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        std::fs::remove_file(path).unwrap();
        assert_eq!(records.len(), 4);
        assert_eq!(records[0]["seq"], 0);
        assert_eq!(records[1]["seq"], 1);
        assert_eq!(records[0]["dir"], "c2s");
        assert_eq!(records[1]["dir"], "s2c");
        assert_eq!(records[0]["id"], 193);
        assert_eq!(records[0]["hex"], "c10100000891");
        assert_eq!(records[1]["hex"], "0200000000");
        assert_eq!(records[2]["id"], 1023);
        assert_eq!(records[2]["hex"], "ff07aa");
        assert_eq!(records[3]["hex"], "0200000000");
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
