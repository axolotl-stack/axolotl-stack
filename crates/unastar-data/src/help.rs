//! The usage lines BDS 1.26.51's `/help` lists for an operator, in its order.
//!
//! `/help` is not in `AvailableCommands`, and its listing is not a pure
//! function of the packet: BDS prints every name of a command (aliases
//! included) with its internal signatures (`/gamerule locatorbar …`,
//! `/execute` variants the packet folds together). So the listing is
//! measured: `unastar-data-gen --help-capture <command-outputs.json>` reads a
//! bds-extractor `--commands` capture of `/help 1` … `/help 39` and writes
//! `output/help_usage.json`.

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Lines per `/help` page.
pub const PAGE_SIZE: usize = 7;

/// Path of the committed artefact, relative to `output/`.
pub const ARTIFACT: &str = "help_usage.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelpUsageArtifact {
    pub bds_version: String,
    pub protocol: u32,
    /// Permission of the player the listing was captured for.
    pub permission: String,
    /// SHA-256 of the capture's `command-outputs.json`.
    pub capture_sha256: String,
    /// `/<name> <params…>` lines, sorted as BDS lists them.
    pub lines: Vec<String>,
}

static VANILLA: LazyLock<HelpUsageArtifact> = LazyLock::new(|| {
    let artifact: HelpUsageArtifact =
        serde_json::from_str(include_str!("../output/help_usage.json")).expect("help usage");
    assert_eq!(artifact.bds_version, "1.26.51");
    artifact
});

/// The captured operator `/help` listing.
pub fn vanilla() -> &'static [String] {
    &VANILLA.lines
}

/// The command name a usage line is for (`/tp <…>` → `tp`).
pub fn line_command(line: &str) -> &str {
    let line = line.strip_prefix('/').unwrap_or(line);
    line.split(' ').next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_is_sorted_and_names_every_alias() {
        let lines = vanilla();
        assert_eq!(lines.len().div_ceil(PAGE_SIZE), 39);
        assert!(lines.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(lines[0], "/? <page: int>");
        for name in [
            "?",
            "help",
            "tp",
            "teleport",
            "w",
            "msg",
            "tell",
            "daylock",
            "alwaysday",
        ] {
            assert!(lines.iter().any(|l| line_command(l) == name), "{name}");
        }
    }
}
