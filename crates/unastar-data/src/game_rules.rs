//! The game rules BDS 1.26.51 announces at join (`GameRulesChanged`), in its
//! order and with its default values, as data.
//!
//! `unastar-data-gen --commands-only` decodes the committed bds-extractor
//! fixture `member/join/00037-GameRulesChangedPacket.bin` (its SHA-256 is
//! checked against the capture manifest) and writes `output/game_rules.json`.

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameRulesArtifact {
    pub bds_version: String,
    pub protocol: u32,
    /// Committed capture fixture and its manifest digest.
    pub capture: String,
    pub capture_sha256: String,
    pub rules: Vec<GameRuleDefault>,
}

/// One rule as BDS sends it at join.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameRuleDefault {
    /// BDS's spelling (`doDayLightCycle`).
    pub name: String,
    pub editable: bool,
    pub value: GameRuleValue,
}

/// A rule value; the wire distinguishes bool, int and float rules.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GameRuleValue {
    Bool(bool),
    Int(i32),
    Float(f32),
}

impl std::fmt::Display for GameRuleValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{v}"),
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
        }
    }
}

/// Path of the committed artefact, relative to `output/`.
pub const ARTIFACT: &str = "game_rules.json";

static VANILLA: LazyLock<GameRulesArtifact> = LazyLock::new(|| {
    let artifact: GameRulesArtifact =
        serde_json::from_str(include_str!("../output/game_rules.json"))
            .expect("captured game rules");
    assert_eq!(artifact.bds_version, "1.26.51");
    assert_eq!(artifact.protocol, 2193);
    artifact
});

/// BDS 1.26.51's game rules with their defaults, in join order.
pub fn vanilla() -> &'static [GameRuleDefault] {
    &VANILLA.rules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_rules_decode_with_bds_defaults() {
        let rules = vanilla();
        assert_eq!(rules.len(), 39);
        assert_eq!(rules[0].name, "commandBlockOutput");
        let keep = rules.iter().find(|r| r.name == "keepInventory").unwrap();
        assert_eq!(keep.value, GameRuleValue::Bool(false));
        let ticks = rules.iter().find(|r| r.name == "randomTickSpeed").unwrap();
        assert_eq!(ticks.value, GameRuleValue::Int(1));
    }
}
