//! The committed BDS 1.26.51 command capture and a replay of it on unastar.
//!
//! `crates/bds-extractor/captures/1.26.51/operator-commands/feedback.json`
//! holds, for every line of `tools/bds-sweep/command-script.txt`, the bytes
//! of the packets BDS answered with (`tools/bds-sweep/commit_fixtures.py
//! --commands`). [`Replay`] runs the same lines on a [`Harness`] arranged like
//! the capture (the operator `BDSExtractor` on a stone platform at y 199,
//! feet at 8.5 200 8.5, `max-players` 2, 12 ticks between commands as the
//! capture's 600 ms) so tests compare unastar's answers with BDS's.
//!
//! Besides the player's commands a script has three directives: `!console
//! COMMAND` runs on the server console (BDS's printed records are in the
//! capture), and `!join NAME` / `!leave NAME` bring a second player in and
//! out (the capture holds the packets that player received too).

use super::test_support::Harness;
use crate::command::{CommandMessage, CommandOutput, CommandSource, execute};
use crate::ecs::resources::TickCounter;
use crate::server::game::ServerConfigResource;
use bevy_ecs::prelude::Entity;
use glam::DVec3;
use jolyne::valentine::{McpePacketArgs, McpePacketData};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// One record BDS printed on its console (`[… INFO] text`), timestamp
/// dropped; a multi-line message is one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleRecord {
    /// `INFO` or `ERROR`.
    pub level: String,
    pub text: String,
}

impl ConsoleRecord {
    pub fn new(level: &str, text: impl Into<String>) -> Self {
        Self {
            level: level.to_owned(),
            text: text.into(),
        }
    }
}

/// One scripted step and the packets BDS answered with, decoded.
pub struct Captured {
    pub segment: String,
    pub command: String,
    pub packets: Vec<McpePacketData>,
    /// What BDS printed for a `!console` step.
    pub console: Vec<ConsoleRecord>,
    /// Packets each second player online during the step received.
    pub clients: BTreeMap<String, Vec<McpePacketData>>,
}

impl Captured {
    /// BDS's command output, as unastar's type; `None` when BDS sent none.
    pub fn output(&self) -> Option<CommandOutput> {
        self.packets.iter().find_map(|packet| match packet {
            McpePacketData::CommandOutputPacket(packet) => Some(CommandOutput {
                success_count: packet.output.success_count,
                messages: packet
                    .output
                    .output_messages
                    .iter()
                    .map(|m| CommandMessage {
                        key: m.message_id.to_string(),
                        params: m.parameters.iter().map(|p| p.to_string()).collect(),
                        success: m.successful,
                    })
                    .collect(),
            }),
            _ => None,
        })
    }

    /// BDS's packets of one kind (`"SetTitlePacket"`), in order.
    pub fn named(&self, name: &str) -> Vec<&McpePacketData> {
        named(&self.packets, name)
    }

    /// The packets of one kind a second player received.
    pub fn client_named(&self, client: &str, name: &str) -> Vec<&McpePacketData> {
        self.clients
            .get(client)
            .map_or_else(Vec::new, |packets| named(packets, name))
    }

    /// The step's directive: `("console", "list")`, `("join", "Alice")`, or
    /// `None` for a player command.
    pub fn directive(&self) -> Option<(&str, &str)> {
        let rest = self.command.strip_prefix('!')?;
        rest.split_once(' ')
    }
}

fn named<'a>(packets: &'a [McpePacketData], name: &str) -> Vec<&'a McpePacketData> {
    packets
        .iter()
        .filter(|packet| packet_name(packet) == name)
        .collect()
}

/// The packet's type name (`McpePacketData` variant).
pub fn packet_name(packet: &McpePacketData) -> String {
    let debug = format!("{packet:?}");
    debug.split('(').next().unwrap_or_default().to_owned()
}

fn decode_hex(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("capture hex"))
        .collect()
}

/// Decodes one packet (header and payload, as the capture stores it).
fn decode(bytes: &[u8]) -> McpePacketData {
    let mut framed = Vec::with_capacity(bytes.len() + 5);
    let mut length = bytes.len() as u32;
    while length >= 0x80 {
        framed.push(length as u8 | 0x80);
        length >>= 7;
    }
    framed.push(length as u8);
    framed.extend_from_slice(bytes);
    let mut buf = bytes::Bytes::from(framed);
    McpePacketData::decode_inner_bytes(&mut buf, McpePacketArgs)
        .expect("capture packets decode")
        .1
}

/// Decodes one committed packet fixture under
/// `crates/bds-extractor/captures/1.26.51/` (`member/join/…bin`).
pub fn decode_file(relative: &str) -> McpePacketData {
    let path = format!(
        "{}/../bds-extractor/captures/1.26.51/{relative}",
        env!("CARGO_MANIFEST_DIR")
    );
    decode(&std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}")))
}

fn decode_packets(list: &serde_json::Value) -> Vec<McpePacketData> {
    list.as_array()
        .expect("packets")
        .iter()
        .map(|p| decode(&decode_hex(p["hex"].as_str().expect("hex"))))
        .collect()
}

/// The whole capture, in script order.
pub fn capture() -> &'static [Captured] {
    static CAPTURE: LazyLock<Vec<Captured>> = LazyLock::new(|| {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../bds-extractor/captures/1.26.51/operator-commands/feedback.json"
        );
        let text = std::fs::read_to_string(path).expect("committed command capture");
        let json: serde_json::Value = serde_json::from_str(&text).expect("feedback.json");
        assert_eq!(json["server_version"], "1.26.51");
        assert_eq!(json["protocol_version"], 2193);
        json["commands"]
            .as_array()
            .expect("commands")
            .iter()
            .map(|entry| Captured {
                segment: entry["segment"].as_str().unwrap_or_default().to_owned(),
                command: entry["command"].as_str().unwrap_or_default().to_owned(),
                packets: decode_packets(&entry["packets"]),
                console: entry["console"]
                    .as_array()
                    .map(|records| {
                        records
                            .iter()
                            .map(|r| {
                                ConsoleRecord::new(
                                    r["level"].as_str().unwrap_or_default(),
                                    r["text"].as_str().unwrap_or_default(),
                                )
                            })
                            .filter(|record| !is_session_log(record))
                            .collect()
                    })
                    .unwrap_or_default(),
                clients: entry["clients"]
                    .as_object()
                    .map(|clients| {
                        clients
                            .iter()
                            .map(|(name, packets)| (name.clone(), decode_packets(packets)))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect()
    });
    &CAPTURE
}

/// BDS's log of players connecting and leaving (`Player disconnected: Bob,
/// xuid: , pfid: `), which lands next to a console step that kicks one; it
/// is the session's, not the command's.
fn is_session_log(record: &ConsoleRecord) -> bool {
    [
        "Player connected:",
        "Player disconnected:",
        "Player Spawned:",
        "Player PartyIdUpdate:",
    ]
    .iter()
    .any(|prefix| record.text.starts_with(prefix))
}

/// The captured commands of one segment.
pub fn segment(name: &str) -> Vec<&'static Captured> {
    let found: Vec<_> = capture().iter().filter(|c| c.segment == name).collect();
    assert!(!found.is_empty(), "no captured segment {name}");
    found
}

/// What unastar prints on its console for a command: one record for the
/// output (`INFO` when it succeeded, `ERROR` otherwise; the messages, one per
/// line), none for an output without messages.
pub fn console_records(output: &CommandOutput) -> Vec<ConsoleRecord> {
    if output.messages.is_empty() {
        return Vec::new();
    }
    let level = if output.is_success() { "INFO" } else { "ERROR" };
    vec![ConsoleRecord::new(level, output.to_text().join("\n"))]
}

/// The extra console lines the last command printed (see
/// [`ConsoleNotes`](crate::command::ConsoleNotes)), as records.
fn drain_notes(world: &mut bevy_ecs::world::World) -> Vec<ConsoleRecord> {
    world
        .get_resource_mut::<crate::command::ConsoleNotes>()
        .map(|mut notes| std::mem::take(&mut notes.0))
        .unwrap_or_default()
        .into_iter()
        .map(|note| ConsoleRecord::new(if note.error { "ERROR" } else { "INFO" }, note.text))
        .collect()
}

/// One replayed step: BDS's answer next to unastar's.
pub struct Step {
    pub captured: &'static Captured,
    pub ours: CommandOutput,
    /// Everything unastar sent the player while running it.
    pub sent: Vec<McpePacketData>,
    /// What unastar printed for a `!console` step.
    pub console: Vec<ConsoleRecord>,
    /// Everything unastar sent each second player while running it.
    pub client_sent: BTreeMap<String, Vec<McpePacketData>>,
}

impl Step {
    /// unastar's packets of one kind, in order.
    pub fn sent_named(&self, name: &str) -> Vec<&McpePacketData> {
        named(&self.sent, name)
    }

    /// unastar's packets of one kind to a second player.
    pub fn client_sent_named(&self, client: &str, name: &str) -> Vec<&McpePacketData> {
        self.client_sent
            .get(client)
            .map_or_else(Vec::new, |packets| named(packets, name))
    }
}

/// A harness arranged like the capture, past its prelude.
pub struct Replay {
    pub h: Harness,
    pub player: Entity,
    /// Second players online, by name (`!join`).
    pub others: BTreeMap<String, Entity>,
    /// The server files directory (`allowlist.json`, `keys/`, …), kept
    /// while the replay lives and removed with it.
    _files: tempfile::TempDir,
}

/// Ticks between two scripted commands (the capture waited 600 ms).
pub const TICKS_PER_COMMAND: u64 = 12;

/// Where BDS put a second player before the script teleported it (the
/// seed-12345 world spawn), feet.
const SECOND_PLAYER_SPAWN: DVec3 = DVec3::new(103.5, 136.0, -27.5);

impl Replay {
    pub fn new() -> Self {
        let mut h = Harness::new();
        if let Some(mut config) = h.world().get_resource_mut::<ServerConfigResource>() {
            config.0.max_players = 2;
        }
        let player = h.player_at("BDSExtractor", true, DVec3::new(8.5, 200.0, 8.5));
        let files = tempfile::tempdir().expect("a server files directory");
        h.world()
            .insert_resource(super::ServerFiles(Some(files.path().to_owned())));
        let mut replay = Self {
            h,
            player,
            others: BTreeMap::new(),
            _files: files,
        };
        for captured in segment("prelude") {
            replay.run(&captured.command);
        }
        replay.h.sent(player);
        replay
    }

    /// Runs a command as the player; then the clock moves on as in the capture.
    pub fn run(&mut self, command: &str) -> (CommandOutput, Vec<McpePacketData>) {
        let output = self.h.run(self.player, command);
        let sent = self.h.sent(self.player);
        self.tick();
        (output, sent)
    }

    fn tick(&mut self) {
        self.h
            .world()
            .get_resource_or_insert_with(TickCounter::default)
            .current += TICKS_PER_COMMAND;
    }

    /// Runs one script line (a player command or a directive) and collects
    /// what every online player was sent.
    pub fn step(&mut self, line: &str) -> (CommandOutput, Vec<ConsoleRecord>) {
        let mut console = Vec::new();
        let output = match line.strip_prefix('!').and_then(|rest| rest.split_once(' ')) {
            Some(("console", command)) => {
                let world = self.h.world();
                let output = execute(world, CommandSource::server(), command);
                console.extend(drain_notes(world));
                console.extend(console_records(&output));
                output
            }
            Some(("join", name)) => {
                let entity = self.h.player_at(name, true, SECOND_PLAYER_SPAWN);
                self.others.insert(name.to_owned(), entity);
                self.h.sent(entity);
                CommandOutput::default()
            }
            Some(("leave", name)) => {
                if let Some(entity) = self.others.remove(name) {
                    self.h.sent(entity);
                    self.h.world().despawn(entity);
                }
                CommandOutput::default()
            }
            Some((directive, _)) => panic!("unknown script directive !{directive}"),
            None => {
                let output = self.h.run(self.player, line);
                drain_notes(self.h.world());
                output
            }
        };
        self.tick();
        (output, console)
    }

    /// Replays a captured segment.
    pub fn segment(&mut self, name: &str) -> Vec<Step> {
        segment(name)
            .into_iter()
            .map(|captured| {
                let (ours, console) = self.step(&captured.command);
                let sent = self.h.sent(self.player);
                let client_sent = self
                    .others
                    .clone()
                    .into_iter()
                    .map(|(name, entity)| (name, self.h.sent(entity)))
                    .collect();
                Step {
                    captured,
                    ours,
                    sent,
                    console,
                    client_sent,
                }
            })
            .collect()
    }

    /// Replays a segment and asserts every command's output equals BDS's,
    /// and every console step printed what BDS printed, except the steps
    /// listed in `differ` (the caller says why).
    pub fn assert_segment(&mut self, name: &str, differ: &[&str]) -> Vec<Step> {
        let steps = self.segment(name);
        let mut mismatches = Vec::new();
        for step in &steps {
            if differ.contains(&step.captured.command.as_str()) {
                continue;
            }
            if let Some(("console", _)) = step.captured.directive() {
                if step.console != step.captured.console {
                    mismatches.push(format!(
                        "{}\n  BDS:     {:?}\n  unastar: {:?}",
                        step.captured.command, step.captured.console, step.console
                    ));
                }
                continue;
            }
            let theirs = step.captured.output().unwrap_or_default();
            if step.ours != theirs {
                mismatches.push(format!(
                    "{}\n  BDS:     {theirs:?}\n  unastar: {:?}",
                    step.captured.command, step.ours
                ));
            }
        }
        assert!(
            mismatches.is_empty(),
            "{} of {} commands in segment {name} answer differently from BDS:\n{}",
            mismatches.len(),
            steps.len(),
            mismatches.join("\n")
        );
        for listed in differ {
            assert!(
                steps.iter().any(|s| s.captured.command == *listed),
                "{listed} is not in segment {name}"
            );
        }
        steps
    }
}

/// Asserts that unastar sent the player the same packets of each named kind
/// as BDS did, command by command, except for the commands in `except`.
pub fn assert_same_packets(steps: &[Step], kinds: &[&str], except: &[&str]) {
    for step in steps {
        if except.contains(&step.captured.command.as_str()) {
            continue;
        }
        for kind in kinds {
            let theirs = step.captured.named(kind);
            let ours = step.sent_named(kind);
            assert_eq!(
                ours, theirs,
                "{}: {kind} differs from the BDS capture",
                step.captured.command
            );
        }
    }
}

/// Like [`assert_same_packets`], for the packets a second player received.
pub fn assert_same_client_packets(steps: &[Step], client: &str, kinds: &[&str], except: &[&str]) {
    for step in steps {
        if except.contains(&step.captured.command.as_str()) {
            continue;
        }
        for kind in kinds {
            let theirs = step.captured.client_named(client, kind);
            let ours = step.client_sent_named(client, kind);
            assert_eq!(
                ours, theirs,
                "{}: {kind} to {client} differs from the BDS capture",
                step.captured.command
            );
        }
    }
}
