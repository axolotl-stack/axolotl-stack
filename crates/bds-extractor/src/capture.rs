//! Packet-capture mode: records a whole BDS session and writes raw and
//! normalised fixtures.
//!
//! The client joins through `client_diff::recording`, so every packet of the
//! session (login included, both directions) lands in `packets.jsonl` exactly
//! as it crossed the wire. The session has up to three phases:
//!
//! 1. **join**: login, spawn, then every post-spawn definition packet
//!    ([`REQUIRED_AFTER_SPAWN`]) plus a quiet `settle` window;
//! 2. **summon** (`--summon`, needs operator permission): `/summon` of every
//!    summonable identifier from `AvailableActorIdentifiers`, one at a time,
//!    keeping the first matching `AddActor` (or `AddPainting`);
//! 3. **commands** (`--commands FILE`): each scripted command, one at a time,
//!    with a fixed wait after it so every server packet is attributable to
//!    the command that preceded it.
//!
//! Permission levels are compared across sessions (one member join, one
//! operator join): BDS 1.26.51 does not resend `AvailableCommands` when a
//! player is opped at runtime, it only sends `UpdateAbilities`.
//!
//! [`write_outputs`] then derives, from the recording alone:
//! `join-sequence.json` (every server packet up to the end of the join
//! phase, in order), `join/<seq>-<Packet>.{bin,json}` for every non-chunk
//! server packet of the join phase, `commands/<permission>.{bin,json}` for
//! each `AvailableCommands`, `summons/<identifier>.{bin,json}`,
//! `command-outputs.json` (see [`command_outputs`]) and `manifest.json`
//! (sizes and SHA-256 of every file). `.bin` files hold the packet header
//! and payload; `.json` is [`crate::debug_json`].

use crate::debug_json;
use anyhow::{Context, Result, bail, ensure};
use client_diff::{
    Record,
    recording::{self, Recorder},
};
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::{
    ClientConfig, JolyneError,
    proto::raw::RawPacket,
    stream::{BedrockStream, Client, Handshake, Play},
    valentine::{
        CommandOriginDatajson, CommandRequestPacket, EnumsConnectionDisconnectFailReason,
        McpePacket, McpePacketArgs, McpePacketData, McpePacketName,
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;
use tracing::{info, warn};

/// Server packets that follow `PlayStatus(PlayerSpawn)` and complete the join.
pub const REQUIRED_AFTER_SPAWN: [McpePacketName; 4] = [
    McpePacketName::AvailableActorIdentifiersPacket,
    McpePacketName::CreativeContentPacket,
    McpePacketName::CraftingDataPacket,
    McpePacketName::AvailableCommandsPacket,
];

/// World payload packets: listed in `join-sequence.json` but not written as
/// separate fixtures.
const CHUNK_PACKETS: [&str; 2] = ["LevelChunkPacket", "SubChunkPacket"];

/// Server packets left out of `command-outputs.json`: world payload, chunk
/// publishing and latency probes arrive whatever the command.
const COMMAND_NOISE: [&str; 4] = [
    "LevelChunkPacket",
    "SubChunkPacket",
    "NetworkChunkPublisherUpdatePacket",
    "NetworkStackLatencyPacket",
];

/// Session options for [`run`].
pub struct Options {
    pub addr: SocketAddr,
    pub name: String,
    pub out_dir: PathBuf,
    /// Bounds the login and every wait for an expected packet.
    pub timeout: Duration,
    /// Quiet time after the last required post-spawn packet.
    pub settle: Duration,
    /// Run the summon phase after the join.
    pub summon: bool,
    /// How long to wait for each summoned actor to appear.
    pub summon_wait: Duration,
    /// Commands (without the leading `/`) to run after the summon phase.
    pub commands: Vec<String>,
    /// Wait after each scripted command.
    pub command_wait: Duration,
}

/// `next_seq` of the recorder at each phase boundary.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct Phases {
    pub join_end: u64,
    pub summon_end: Option<u64>,
    /// The commands phase starts at `summon_end`, or `join_end` without one.
    pub commands_end: Option<u64>,
}

/// One `/summon` and what the server answered.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Summon {
    pub identifier: String,
    pub command: String,
    /// Hex of the first AddActor/AddPainting after the command (header and
    /// payload), if one arrived; resolved to a recorded `seq` by
    /// [`write_outputs`]. The world has mob spawning off and is cleared
    /// between summons, so the first new actor is the summoned one.
    #[serde(skip)]
    pub packet_hex: Option<String>,
    /// The actor type that packet carries; it can differ from the summoned
    /// identifier (`minecraft:villager` spawns `minecraft:villager_v2`).
    pub actor_type: Option<String>,
    /// `seq` of that packet in `packets.jsonl`.
    pub seq: Option<u64>,
    /// Feedback the server sent for the command (CommandOutput message ids
    /// and parameters, or Text messages).
    pub feedback: Vec<String>,
}

/// Runs a capture session and writes all outputs into `options.out_dir`.
pub async fn run(options: &Options) -> Result<()> {
    std::fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("creating {}", options.out_dir.display()))?;
    let packets_path = options.out_dir.join("packets.jsonl");
    let config = ClientHandshakeConfig::random(options.addr, &options.name);
    let recorder = Arc::new(Mutex::new(Recorder::create(
        &packets_path,
        config.identity_key.clone(),
    )?));
    let transport = recording::connect(options.addr, recorder.clone()).await?;
    let (mut play, _game) = tokio::time::timeout(options.timeout, async {
        let stream = BedrockStream::<Handshake, Client, _>::from_transport(
            transport,
            ClientConfig::default(),
        )
        .await?;
        stream.join(config).await
    })
    .await
    .context("login timed out")?
    .context("login")?;
    info!("Spawned; collecting post-spawn definitions");

    let session = async {
        let mut session = Session {
            play: &mut play,
            recorder: &recorder,
            timeout: options.timeout,
        };
        let mut phases = Phases::default();
        let mut identifiers = None;
        let mut seen = Vec::new();
        session
            .pump_until(options.timeout, |name, packet| {
                if let Some(McpePacketData::AvailableActorIdentifiersPacket(ids)) = packet {
                    identifiers = Some(ids.identifier_list.clone());
                }
                if REQUIRED_AFTER_SPAWN.contains(&name) && !seen.contains(&name) {
                    seen.push(name);
                }
                seen.len() == REQUIRED_AFTER_SPAWN.len()
            })
            .await
            .with_context(|| {
                format!(
                    "post-spawn packets missing after {:?}: have {seen:?}",
                    options.timeout
                )
            })?;
        session.pump_for(options.settle).await?;
        phases.join_end = session.next_seq()?;

        let mut summons = Vec::new();
        if options.summon {
            let identifiers = identifiers.context("AvailableActorIdentifiers was not received")?;
            let list = summonable_identifiers(&debug_json::nbt_json(&identifiers.0)?)?;
            info!("Summoning {} identifiers", list.len());
            // Keep the world quiet: no natural spawns that could be mistaken for
            // a summoned actor, no drops from the clean-up kills.
            for command in [
                "gamemode creative @s",
                "gamerule domobspawning false",
                "gamerule domobloot false",
                "gamerule doentitydrops false",
                "kill @e[type=!player]",
            ] {
                session.command(command).await?;
            }
            session.pump_for(Duration::from_secs(2)).await?;
            for identifier in list {
                let summon = session.summon(&identifier, options.summon_wait).await?;
                match &summon.actor_type {
                    None => warn!("{identifier}: no actor packet ({:?})", summon.feedback),
                    Some(actor) if actor != &identifier => {
                        warn!("{identifier}: first new actor is {actor}")
                    }
                    Some(_) => {}
                }
                summons.push(summon);
                // Slimes and sulfur cubes split when they die (after the
                // death animation): kill twice so no child is mistaken for
                // the next summon.
                session.command("kill @e[type=!player]").await?;
                session.pump_for(Duration::from_millis(1500)).await?;
                session.command("kill @e[type=!player]").await?;
                session.pump_for(Duration::from_millis(300)).await?;
            }
            phases.summon_end = Some(session.next_seq()?);
        }

        if !options.commands.is_empty() {
            info!("Running {} scripted commands", options.commands.len());
            for command in &options.commands {
                info!("/{command}");
                session.command(command).await?;
                session.pump_for(options.command_wait).await?;
            }
            phases.commands_end = Some(session.next_seq()?);
        }
        Ok::<_, anyhow::Error>((phases, summons))
    }
    .await;

    let disconnect = tokio::time::timeout(
        Duration::from_secs(2),
        play.disconnect(EnumsConnectionDisconnectFailReason::Disconnected, ""),
    )
    .await;
    recorder
        .lock()
        .map_err(|_| anyhow::anyhow!("capture lock poisoned"))?
        .flush()?;
    let (phases, summons) = session?;
    if let Err(error) = disconnect
        .context("disconnect timed out")
        .and_then(|r| r.context("disconnect"))
    {
        warn!("{error:#}");
    }

    let records = read_records(&packets_path)?;
    let manifest = write_outputs(&options.out_dir, &records, phases, &summons)?;
    info!(
        "Wrote {} files to {} ({} summons captured)",
        manifest["files"].as_array().map_or(0, Vec::len),
        options.out_dir.display(),
        manifest["summons"].as_array().map_or(0, |list| list
            .iter()
            .filter(|s| !s["seq"].is_null())
            .count())
    );
    Ok(())
}

struct Session<'a, T: jolyne::stream::transport::Transport> {
    play: &'a mut BedrockStream<Play, Client, T>,
    recorder: &'a Mutex<Recorder>,
    timeout: Duration,
}

impl<T: jolyne::stream::transport::Transport> Session<'_, T> {
    fn next_seq(&self) -> Result<u64> {
        Ok(self
            .recorder
            .lock()
            .map_err(|_| anyhow::anyhow!("capture lock poisoned"))?
            .next_seq())
    }

    /// Receives one packet, answering server latency probes like a client.
    async fn recv(&mut self) -> Result<RawPacket> {
        let packet = match self.play.recv_packet_raw().await {
            Ok(packet) => packet,
            Err(JolyneError::Disconnected(disconnect)) => {
                bail!("server disconnected: {:?}", disconnect.packet.body())
            }
            Err(error) => return Err(error.into()),
        };
        if packet.is(McpePacketName::NetworkStackLatencyPacket)
            && let McpePacketData::NetworkStackLatencyPacket(mut latency) =
                packet.clone().decode()?.data
            && latency.is_from_server
        {
            latency.is_from_server = false;
            self.play.send_packet(McpePacket::from(latency)).await?;
        }
        Ok(packet)
    }

    /// Pumps packets until `done` returns true; `done` sees every packet's
    /// name and, for packets it may need, the decoded body.
    async fn pump_until(
        &mut self,
        limit: Duration,
        mut done: impl FnMut(McpePacketName, Option<&McpePacketData>) -> bool,
    ) -> Result<()> {
        let deadline = Instant::now() + limit;
        loop {
            let packet = tokio::time::timeout_at(deadline, self.recv())
                .await
                .context("timed out")??;
            let Some(name) = packet.name() else { continue };
            let decoded = if name == McpePacketName::AvailableActorIdentifiersPacket {
                Some(packet.decode()?.data)
            } else {
                None
            };
            if done(name, decoded.as_ref()) {
                return Ok(());
            }
        }
    }

    async fn pump_for(&mut self, duration: Duration) -> Result<()> {
        let deadline = Instant::now() + duration;
        while let Ok(packet) = tokio::time::timeout_at(deadline, self.recv()).await {
            packet?;
        }
        Ok(())
    }

    async fn command(&mut self, command: &str) -> Result<()> {
        self.play
            .send_packet(McpePacket::from(CommandRequestPacket {
                command: format!("/{command}").into(),
                origin: CommandOriginDatajson {
                    type_: "player".into(),
                    ..Default::default()
                },
                is_internal: false,
                version: "latest".into(),
            }))
            .await?;
        Ok(())
    }

    async fn summon(&mut self, identifier: &str, wait: Duration) -> Result<Summon> {
        let command = format!("summon {identifier} ~2 ~ ~2");
        self.command(&command).await?;
        let mut summon = Summon {
            identifier: identifier.to_owned(),
            command,
            packet_hex: None,
            actor_type: None,
            seq: None,
            feedback: Vec::new(),
        };
        let deadline = Instant::now() + wait.min(self.timeout);
        while let Ok(packet) = tokio::time::timeout_at(deadline, self.recv()).await {
            let packet = packet?;
            match packet.name() {
                Some(
                    McpePacketName::AddActorPacket
                    | McpePacketName::AddPaintingPacket
                    | McpePacketName::CommandOutputPacket
                    | McpePacketName::TextPacket,
                ) => {}
                _ => continue,
            }
            let hex = hex::encode(strip_length(packet.inner_frame())?);
            match packet.decode()?.data {
                McpePacketData::AddActorPacket(actor) if summon.packet_hex.is_none() => {
                    summon.packet_hex = Some(hex);
                    summon.actor_type = Some(actor.actor_type.to_string());
                }
                McpePacketData::AddPaintingPacket(_) if summon.packet_hex.is_none() => {
                    summon.packet_hex = Some(hex);
                    summon.actor_type = Some("minecraft:painting".into());
                }
                McpePacketData::CommandOutputPacket(output) => {
                    for message in output.output.output_messages {
                        let parameters: Vec<&str> =
                            message.parameters.iter().map(|p| &**p).collect();
                        summon.feedback.push(format!(
                            "{} {}",
                            &*message.message_id,
                            parameters.join(" ")
                        ));
                    }
                    if output.output.success_count == 0 {
                        break;
                    }
                }
                McpePacketData::TextPacket(text) => {
                    summon.feedback.push(format!("{:?}", text.body));
                }
                _ => {}
            }
            if summon.packet_hex.is_some() && !summon.feedback.is_empty() {
                break;
            }
        }
        Ok(summon)
    }
}

/// The header and payload of a batched inner frame (drops the length prefix,
/// matching the `hex` of recorded packets).
fn strip_length(frame: &[u8]) -> Result<&[u8]> {
    let prefix = frame
        .iter()
        .take(5)
        .position(|byte| byte & 0x80 == 0)
        .context("invalid packet length prefix")?;
    Ok(&frame[prefix + 1..])
}

/// Identifiers marked `summonable` in an `AvailableActorIdentifiers` NBT
/// tree, sorted, without the player.
pub fn summonable_identifiers(tree: &Value) -> Result<Vec<String>> {
    let list = tree["idlist"]
        .as_array()
        .context("AvailableActorIdentifiers has no idlist")?;
    let mut identifiers: Vec<String> = list
        .iter()
        .filter(|entry| entry["summonable"].as_i64() == Some(1))
        .filter_map(|entry| entry["id"].as_str())
        .filter(|id| *id != "minecraft:player")
        .map(str::to_owned)
        .collect();
    identifiers.sort();
    identifiers.dedup();
    Ok(identifiers)
}

/// Reads a capture JSONL written by the recorder.
pub fn read_records(path: &Path) -> Result<Vec<Record>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line).with_context(|| format!("{}:{}", path.display(), index + 1))
        })
        .collect()
}

/// Decodes one recorded packet (header and payload, no length prefix).
pub fn decode_record(bytes: &[u8]) -> Result<McpePacketData> {
    let mut framed = Vec::with_capacity(bytes.len() + 5);
    let mut length = u32::try_from(bytes.len())?;
    while length >= 0x80 {
        framed.push(length as u8 | 0x80);
        length >>= 7;
    }
    framed.push(length as u8);
    framed.extend_from_slice(bytes);
    let (_, data) = McpePacketData::decode_inner_bytes(&mut framed.into(), McpePacketArgs)
        .map_err(|error| anyhow::anyhow!("decode: {error:?}"))?;
    Ok(data)
}

/// The normalised JSON of a decoded packet, without the outer
/// `McpePacketData` variant.
pub fn packet_json(data: &McpePacketData) -> Result<Value> {
    let value = debug_json::to_json(&format!("{data:?}"))?;
    match value {
        Value::Object(mut map) if map.len() == 1 => {
            Ok(map.values_mut().next().map(Value::take).unwrap_or_default())
        }
        other => Ok(other),
    }
}

struct Outputs<'a> {
    dir: &'a Path,
    files: Vec<Value>,
}

impl Outputs<'_> {
    fn write(&mut self, relative: &str, bytes: &[u8], meta: Value) -> Result<()> {
        let path = self.dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
        let mut entry = json!({
            "path": relative,
            "bytes": bytes.len(),
            "sha256": hex::encode(Sha256::digest(bytes)),
        });
        if let (Value::Object(entry), Value::Object(meta)) = (&mut entry, meta) {
            entry.extend(meta);
        }
        self.files.push(entry);
        Ok(())
    }

    /// Writes `<stem>.bin` and `<stem>.json` for one recorded packet.
    fn packet(&mut self, stem: &str, record: &Record) -> Result<()> {
        let bytes = hex::decode(&record.hex)?;
        let data = decode_record(&bytes)
            .with_context(|| format!("packet seq {} ({})", record.seq, record.name))?;
        let meta = json!({"seq": record.seq, "t_ms": record.t_ms, "packet": record.name});
        self.write(&format!("{stem}.bin"), &bytes, meta.clone())?;
        let mut json = serde_json::to_vec_pretty(&packet_json(&data)?)?;
        json.push(b'\n');
        self.write(&format!("{stem}.json"), &json, meta)
    }
}

/// Writes every derived file for a recorded session and returns the manifest
/// (also written as `manifest.json`).
pub fn write_outputs(
    dir: &Path,
    records: &[Record],
    phases: Phases,
    summons: &[Summon],
) -> Result<Value> {
    let mut outputs = Outputs {
        dir,
        files: Vec::new(),
    };
    let server: Vec<&Record> = records.iter().filter(|r| r.dir == "s2c").collect();
    ensure!(!server.is_empty(), "the capture has no server packets");

    let join: Vec<&Record> = server
        .iter()
        .copied()
        .filter(|r| r.seq < phases.join_end)
        .collect();
    let sequence: Vec<Value> = join
        .iter()
        .map(|r| json!({"seq": r.seq, "t_ms": r.t_ms, "id": r.id, "packet": r.name, "bytes": r.hex.len() / 2}))
        .collect();
    let mut text = serde_json::to_vec_pretty(&sequence)?;
    text.push(b'\n');
    outputs.write("join-sequence.json", &text, json!({}))?;
    for record in &join {
        if CHUNK_PACKETS.contains(&record.name.as_str()) {
            continue;
        }
        outputs.packet(&format!("join/{:05}-{}", record.seq, record.name), record)?;
    }

    // AvailableCommands, named by the permission level of the abilities update
    // the server sent before it.
    let mut permission = String::from("unknown");
    let mut commands = BTreeMap::<String, usize>::new();
    for record in &server {
        match record.name.as_str() {
            "UpdateAbilitiesPacket" => {
                if let McpePacketData::UpdateAbilitiesPacket(abilities) =
                    decode_record(&hex::decode(&record.hex)?)?
                {
                    permission = format!("{:?}", abilities.data.player_permissions).to_lowercase();
                }
            }
            "AvailableCommandsPacket" => {
                let count = commands.entry(permission.clone()).or_default();
                *count += 1;
                let stem = if *count == 1 {
                    format!("commands/{permission}")
                } else {
                    format!("commands/{permission}-{count}")
                };
                outputs.packet(&stem, record)?;
            }
            _ => {}
        }
    }

    let mut resolved = Vec::with_capacity(summons.len());
    for summon in summons {
        let mut summon = summon.clone();
        if let Some(hex) = &summon.packet_hex {
            let record = server
                .iter()
                .find(|r| r.seq >= phases.join_end && &r.hex == hex)
                .with_context(|| {
                    format!(
                        "summon {} packet is not in the recording",
                        summon.identifier
                    )
                })?;
            summon.seq = Some(record.seq);
            let name = summon
                .identifier
                .trim_start_matches("minecraft:")
                .replace(':', "_");
            outputs.packet(&format!("summons/{name}"), record)?;
        }
        resolved.push(summon);
    }

    let start_game = server
        .iter()
        .find(|r| r.name == "StartGamePacket")
        .map(|r| decode_record(&hex::decode(&r.hex)?))
        .transpose()?;
    let (server_version, player) = match &start_game {
        Some(McpePacketData::StartGamePacket(start)) => (
            Some(start.server_version.to_string()),
            Some(start.runtime_id.actor_runtime_id),
        ),
        _ => (None, None),
    };

    let mut commands = Vec::new();
    if let Some(end) = phases.commands_end {
        let start = phases.summon_end.unwrap_or(phases.join_end);
        commands = command_outputs(records, start..end, player)?;
        let mut text = serde_json::to_vec_pretty(&commands)?;
        text.push(b'\n');
        outputs.write("command-outputs.json", &text, json!({}))?;
    }
    let commands: Vec<Value> = commands
        .iter()
        .map(|c| {
            json!({
                "command": c.command,
                "request_seq": c.request_seq,
                "packets": c.packets.iter().map(|p| p["packet"].clone()).collect::<Vec<_>>(),
            })
        })
        .collect();
    let manifest = json!({
        "format": 1,
        "protocol_version": valentine::bedrock::version::v1_26_51::PROTOCOL_VERSION,
        "server_version": server_version,
        "captured_at": crate::output::chrono_lite_now(),
        "packets": {
            "total": records.len(),
            "s2c": server.len(),
            "c2s": records.len() - server.len(),
        },
        "phases": phases,
        "summons": resolved,
        "commands": commands,
        "files": outputs.files,
    });
    let mut text = serde_json::to_vec_pretty(&manifest)?;
    text.push(b'\n');
    std::fs::write(dir.join("manifest.json"), text)?;
    Ok(manifest)
}

/// One scripted command and every server packet recorded after its
/// `CommandRequest` and before the next one.
#[derive(Debug, Serialize)]
pub struct CommandOutputs {
    pub command: String,
    pub request_seq: u64,
    pub t_ms: f64,
    /// `{seq, t_ms, packet, data}`, `data` being [`packet_json`].
    pub packets: Vec<Value>,
    /// Server packets left out, by name: [`COMMAND_NOISE`] and movement of
    /// actors other than the player.
    pub excluded: BTreeMap<String, usize>,
}

/// Groups the records in `range` (the commands phase) by the client's
/// `CommandRequest` packets.
pub fn command_outputs(
    records: &[Record],
    range: Range<u64>,
    player: Option<u64>,
) -> Result<Vec<CommandOutputs>> {
    let mut commands: Vec<CommandOutputs> = Vec::new();
    for record in records.iter().filter(|r| range.contains(&r.seq)) {
        if record.dir != "s2c" {
            if record.name == "CommandRequestPacket" {
                let McpePacketData::CommandRequestPacket(request) =
                    decode_record(&hex::decode(&record.hex)?)?
                else {
                    bail!("packet seq {} is not a CommandRequest", record.seq);
                };
                commands.push(CommandOutputs {
                    command: request.command.to_string(),
                    request_seq: record.seq,
                    t_ms: record.t_ms,
                    packets: Vec::new(),
                    excluded: BTreeMap::new(),
                });
            }
            continue;
        }
        let Some(current) = commands.last_mut() else {
            continue;
        };
        let data = if COMMAND_NOISE.contains(&record.name.as_str()) {
            None
        } else {
            let data = decode_record(&hex::decode(&record.hex)?)
                .with_context(|| format!("packet seq {} ({})", record.seq, record.name))?;
            (!moves_other_actor(&data, player)).then_some(data)
        };
        match data {
            Some(data) => current.packets.push(json!({
                "seq": record.seq,
                "t_ms": record.t_ms,
                "packet": record.name,
                "data": packet_json(&data)?,
            })),
            None => *current.excluded.entry(record.name.clone()).or_default() += 1,
        }
    }
    Ok(commands)
}

/// Movement of an actor other than the player (mob wandering, not command
/// feedback).
fn moves_other_actor(data: &McpePacketData, player: Option<u64>) -> bool {
    let actor = match data {
        McpePacketData::MoveActorDeltaPacket(packet) => &packet.move_data.actor_runtime_id,
        McpePacketData::MoveActorAbsolutePacket(packet) => &packet.move_data.actor_runtime_id,
        McpePacketData::SetActorMotionPacket(packet) => &packet.target_runtime_id,
        _ => return false,
    };
    Some(actor.actor_runtime_id) != player
}

#[cfg(test)]
mod tests {
    use super::*;
    use jolyne::valentine::bedrock::codec::{BedrockCodec, Nbt};
    use jolyne::valentine::{
        ActorRuntimeId, AddActorPacket, AvailableActorIdentifiersPacket, CommandOutputPacket,
        EnumsPlayerPermissionLevel, GameRulesChangedPacket, MoveActorDeltaData,
        MoveActorDeltaPacket, NetworkChunkPublisherUpdatePacket, SerializedAbilitiesData,
        UpdateAbilitiesPacket,
    };

    fn record(seq: u64, dir: &str, packet: impl Into<McpePacket>) -> Record {
        let packet: McpePacket = packet.into();
        let raw = RawPacket::from_packet(&packet).unwrap();
        let name = format!("{:?}", raw.name().unwrap());
        Record {
            seq,
            t_ms: seq as f64,
            dir: dir.into(),
            id: u32::from(raw.id()),
            name,
            hex: hex::encode(strip_length(raw.inner_frame()).unwrap()),
        }
    }

    fn abilities(permission: EnumsPlayerPermissionLevel) -> UpdateAbilitiesPacket {
        UpdateAbilitiesPacket {
            data: SerializedAbilitiesData {
                player_permissions: permission,
                ..Default::default()
            },
        }
    }

    #[test]
    fn outputs_split_phases_label_commands_and_hash_files() {
        let dir =
            std::env::temp_dir().join(format!("bds-extractor-capture-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let commands = jolyne::valentine::AvailableCommandsPacket::default();
        let actor = AddActorPacket {
            actor_type: "minecraft:cow".into(),
            ..Default::default()
        };
        let records = vec![
            record(0, "s2c", GameRulesChangedPacket::default()),
            record(1, "c2s", GameRulesChangedPacket::default()),
            record(2, "s2c", abilities(EnumsPlayerPermissionLevel::Member)),
            record(3, "s2c", commands.clone()),
            // A later abilities update relabels the next AvailableCommands.
            record(4, "s2c", abilities(EnumsPlayerPermissionLevel::Operator)),
            record(5, "s2c", commands),
            // Summon phase.
            record(6, "s2c", actor.clone()),
        ];
        let phases = Phases {
            join_end: 4,
            summon_end: Some(7),
            commands_end: None,
        };
        let summons = [Summon {
            identifier: "minecraft:cow".into(),
            command: "summon minecraft:cow ~2 ~ ~2".into(),
            packet_hex: Some(records[6].hex.clone()),
            actor_type: Some("minecraft:cow".into()),
            seq: None,
            feedback: vec![],
        }];
        let manifest = write_outputs(&dir, &records, phases, &summons).unwrap();

        let sequence: Vec<Value> =
            serde_json::from_slice(&std::fs::read(dir.join("join-sequence.json")).unwrap())
                .unwrap();
        let seqs: Vec<u64> = sequence
            .iter()
            .map(|r| r["seq"].as_u64().unwrap())
            .collect();
        assert_eq!(seqs, [0, 2, 3], "join phase only, server packets only");
        assert!(dir.join("join/00000-GameRulesChangedPacket.bin").exists());
        assert!(dir.join("commands/member.bin").exists());
        assert!(dir.join("commands/operator.json").exists());

        let bin = std::fs::read(dir.join("summons/cow.bin")).unwrap();
        let McpePacketData::AddActorPacket(decoded) = decode_record(&bin).unwrap() else {
            panic!("summon fixture is not AddActor");
        };
        assert_eq!(*decoded, actor);
        let json: Value =
            serde_json::from_slice(&std::fs::read(dir.join("summons/cow.json")).unwrap()).unwrap();
        assert_eq!(json["actor_type"], "minecraft:cow");

        let files = manifest["files"].as_array().unwrap();
        let entry = files
            .iter()
            .find(|f| f["path"] == "summons/cow.bin")
            .unwrap();
        assert_eq!(entry["sha256"], hex::encode(Sha256::digest(&bin)));
        assert_eq!(entry["seq"], 6);
        assert_eq!(manifest["packets"]["c2s"], 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn command_outputs_attribute_packets_to_the_preceding_request() {
        let request = |command: &str| CommandRequestPacket {
            command: command.into(),
            ..Default::default()
        };
        let movement = |actor: u64| MoveActorDeltaPacket {
            move_data: MoveActorDeltaData {
                actor_runtime_id: ActorRuntimeId {
                    actor_runtime_id: actor,
                },
                ..Default::default()
            },
        };
        let records = vec![
            record(0, "c2s", request("/before")),
            record(1, "c2s", request("/list")),
            record(2, "s2c", CommandOutputPacket::default()),
            record(3, "s2c", NetworkChunkPublisherUpdatePacket::default()),
            record(4, "s2c", movement(7)),
            record(5, "s2c", movement(1)),
            record(6, "c2s", request("/time query daytime")),
            record(7, "s2c", CommandOutputPacket::default()),
            record(8, "c2s", request("/after")),
        ];
        let outputs = command_outputs(&records, 1..8, Some(1)).unwrap();
        let commands: Vec<&str> = outputs.iter().map(|c| c.command.as_str()).collect();
        assert_eq!(commands, ["/list", "/time query daytime"]);
        let packets: Vec<&Value> = outputs[0].packets.iter().map(|p| &p["packet"]).collect();
        assert_eq!(packets, ["CommandOutputPacket", "MoveActorDeltaPacket"]);
        assert_eq!(
            outputs[0].packets[1]["seq"], 5,
            "the player's movement is kept"
        );
        assert_eq!(
            outputs[0].excluded,
            BTreeMap::from([
                ("MoveActorDeltaPacket".to_owned(), 1),
                ("NetworkChunkPublisherUpdatePacket".to_owned(), 1),
            ])
        );
        assert_eq!(outputs[1].packets.len(), 1);
    }

    #[test]
    fn summonable_identifiers_skip_player_and_unsummonable() {
        let tree = json!({"idlist": [
            {"id": "minecraft:zombie", "summonable": 1},
            {"id": "minecraft:player", "summonable": 1},
            {"id": "minecraft:cow", "summonable": 1},
            {"id": "minecraft:fishing_hook", "summonable": 0},
        ]});
        assert_eq!(
            summonable_identifiers(&tree).unwrap(),
            ["minecraft:cow", "minecraft:zombie"]
        );
    }

    #[test]
    fn identifier_nbt_round_trips_through_record_decoding() {
        // Compound "" { idlist: [ { id: "minecraft:cow", summonable: 1b } ] }.
        let mut nbt = vec![0x0a, 0x00, 0x09, 0x06];
        nbt.extend(b"idlist");
        // List element type compound, length 1 (zigzag varint 0x02).
        nbt.extend([0x0a, 0x02, 0x08, 0x02, b'i', b'd', 0x0d]);
        nbt.extend(b"minecraft:cow");
        nbt.extend([0x01, 0x0a]);
        nbt.extend(b"summonable");
        nbt.extend([0x01, 0x00, 0x00]);
        let packet = AvailableActorIdentifiersPacket {
            identifier_list: Nbt(nbt.clone().into()),
        };
        let mut encoded = Vec::new();
        packet.encode(&mut encoded).unwrap();
        let record = record(0, "s2c", packet);
        let McpePacketData::AvailableActorIdentifiersPacket(decoded) =
            decode_record(&hex::decode(&record.hex).unwrap()).unwrap()
        else {
            panic!("wrong packet");
        };
        let tree = debug_json::nbt_json(&decoded.identifier_list.0).unwrap();
        assert_eq!(summonable_identifiers(&tree).unwrap(), ["minecraft:cow"]);
    }
}
