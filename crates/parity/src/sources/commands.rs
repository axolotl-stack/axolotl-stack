//! Command rows: every command BDS 1.26.51 advertises in its
//! `AvailableCommands` packet, from the commands lane's decoded capture
//! `crates/unastar-data/output/available_commands.json`.
//!
//! Only the command list is read here; the full syntax stays in the artifact
//! and its loader. The family is the command's permission level.

use std::path::Path;

use anyhow::Context as _;
use serde::Deserialize;

use crate::catalog::Row;

pub const ARTIFACT: &str = "crates/unastar-data/output/available_commands.json";

#[derive(Deserialize)]
struct Artifact {
    bds_version: String,
    commands: Vec<Command>,
}

#[derive(Deserialize)]
struct Command {
    name: String,
    permission: String,
}

pub fn rows(root: &Path) -> anyhow::Result<Vec<Row>> {
    let path = root.join(ARTIFACT);
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let artifact: Artifact =
        serde_json::from_slice(&bytes).with_context(|| format!("decoding {}", path.display()))?;
    anyhow::ensure!(
        artifact.bds_version == "1.26.51",
        "{ARTIFACT} is BDS {}, the catalog targets 1.26.51",
        artifact.bds_version
    );
    Ok(artifact
        .commands
        .into_iter()
        .map(|command| Row::new(command.name, command.permission))
        .collect())
}
