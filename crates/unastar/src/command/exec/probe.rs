//! The BDS 1.26.51 command probe (`testdata/bds-command-probe.json`, written
//! by `tools/bds-sweep/command_probe.py`): console answers and an operator
//! player's `CommandOutput` for commands the main command capture does not
//! cover.

use super::evidence::ConsoleRecord;
use super::test_support::Harness;
use crate::command::CommandMessage;
use bevy_ecs::prelude::Entity;
use glam::DVec3;
use std::sync::LazyLock;

fn probe() -> &'static serde_json::Value {
    static PROBE: LazyLock<serde_json::Value> = LazyLock::new(|| {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/command/exec/testdata/bds-command-probe.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("command probe"))
            .expect("command probe JSON")
    });
    &PROBE
}

fn step<'a>(list: &'a str, command: &str) -> &'a serde_json::Value {
    probe()[list]
        .as_array()
        .expect("steps")
        .iter()
        .find(|step| step["command"] == command)
        .unwrap_or_else(|| panic!("{command} is not in the probe's {list}"))
}

/// What BDS printed on its console for `command`.
pub fn captured_console(command: &str) -> Vec<ConsoleRecord> {
    step("console", command)["records"]
        .as_array()
        .expect("records")
        .iter()
        .map(|r| {
            ConsoleRecord::new(
                r["level"].as_str().expect("level"),
                r["text"].as_str().expect("text"),
            )
        })
        .collect()
}

/// The messages BDS answered the operator player's `command` with.
pub fn captured_player(command: &str) -> Vec<CommandMessage> {
    step("player", command)["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .map(|m| CommandMessage {
            key: m["key"].as_str().expect("key").to_owned(),
            params: m["params"]
                .as_array()
                .expect("params")
                .iter()
                .map(|p| p.as_str().expect("param").to_owned())
                .collect(),
            success: m["success"].as_bool().expect("success"),
        })
        .collect()
}

/// A harness for the probe's commands.
pub struct Probe {
    pub h: Harness,
}

impl Probe {
    pub fn new() -> Self {
        Self { h: Harness::new() }
    }

    /// The probe's operator player, at the flat world's spawn.
    pub fn player(&mut self) -> Entity {
        self.h
            .player_at("BDSExtractor", true, DVec3::new(0.5, -60.0, 0.5))
    }

    /// The extra console lines the last command printed.
    pub fn notes(&mut self) -> Vec<ConsoleRecord> {
        self.h
            .world()
            .get_resource_mut::<crate::command::ConsoleNotes>()
            .map(|mut notes| std::mem::take(&mut notes.0))
            .unwrap_or_default()
            .into_iter()
            .map(|note| ConsoleRecord::new(if note.error { "ERROR" } else { "INFO" }, note.text))
            .collect()
    }
}
