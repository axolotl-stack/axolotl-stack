//! The committed BDS 1.26.51 command capture and a replay of it on unastar.
//!
//! `crates/bds-extractor/captures/1.26.51/operator-commands/feedback.json`
//! holds, for every line of `tools/bds-sweep/command-script.txt`, the bytes
//! of the packets BDS answered with (`tools/bds-sweep/commit_fixtures.py
//! --commands`). [`Replay`] runs the same lines on a [`Harness`] arranged like
//! the capture (the operator `BDSExtractor` on a stone platform at y 199,
//! feet at 8.5 200 8.5, `max-players` 2, 12 ticks between commands as the
//! capture's 600 ms) so tests compare unastar's answers with BDS's.

use super::test_support::Harness;
use crate::command::{CommandMessage, CommandOutput};
use crate::ecs::resources::TickCounter;
use crate::server::game::ServerConfigResource;
use bevy_ecs::prelude::Entity;
use glam::DVec3;
use jolyne::valentine::{McpePacketArgs, McpePacketData};
use std::sync::LazyLock;

/// One scripted command and the packets BDS answered with, decoded.
pub struct Captured {
    pub segment: String,
    pub command: String,
    pub packets: Vec<McpePacketData>,
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
        self.packets
            .iter()
            .filter(|packet| packet_name(packet) == name)
            .collect()
    }
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
                packets: entry["packets"]
                    .as_array()
                    .expect("packets")
                    .iter()
                    .map(|p| decode(&decode_hex(p["hex"].as_str().expect("hex"))))
                    .collect(),
            })
            .collect()
    });
    &CAPTURE
}

/// The captured commands of one segment.
pub fn segment(name: &str) -> Vec<&'static Captured> {
    let found: Vec<_> = capture().iter().filter(|c| c.segment == name).collect();
    assert!(!found.is_empty(), "no captured segment {name}");
    found
}

/// One replayed command: BDS's answer next to unastar's.
pub struct Step {
    pub captured: &'static Captured,
    pub ours: CommandOutput,
    /// Everything unastar sent the player while running it.
    pub sent: Vec<McpePacketData>,
}

impl Step {
    /// unastar's packets of one kind, in order.
    pub fn sent_named(&self, name: &str) -> Vec<&McpePacketData> {
        self.sent
            .iter()
            .filter(|packet| packet_name(packet) == name)
            .collect()
    }
}

/// A harness arranged like the capture, past its prelude.
pub struct Replay {
    pub h: Harness,
    pub player: Entity,
}

/// Ticks between two scripted commands (the capture waited 600 ms).
pub const TICKS_PER_COMMAND: u64 = 12;

impl Replay {
    pub fn new() -> Self {
        let mut h = Harness::new();
        if let Some(mut config) = h.world().get_resource_mut::<ServerConfigResource>() {
            config.0.max_players = 2;
        }
        let player = h.player_at("BDSExtractor", true, DVec3::new(8.5, 200.0, 8.5));
        let mut replay = Self { h, player };
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
        self.h
            .world()
            .get_resource_or_insert_with(TickCounter::default)
            .current += TICKS_PER_COMMAND;
        (output, sent)
    }

    /// Replays a captured segment.
    pub fn segment(&mut self, name: &str) -> Vec<Step> {
        segment(name)
            .into_iter()
            .map(|captured| {
                let (ours, sent) = self.run(&captured.command);
                Step {
                    captured,
                    ours,
                    sent,
                }
            })
            .collect()
    }

    /// Replays a segment and asserts every command's output equals BDS's,
    /// except the commands listed in `differ` (the caller says why).
    pub fn assert_segment(&mut self, name: &str, differ: &[&str]) -> Vec<Step> {
        let steps = self.segment(name);
        let mut mismatches = Vec::new();
        for step in &steps {
            if differ.contains(&step.captured.command.as_str()) {
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
