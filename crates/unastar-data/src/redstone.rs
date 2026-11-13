//! Measured redstone facts, decoded once at startup.
//!
//! Source: `data/measured/redstone/blocks.json`, derived by `summarize.py
//! circuits` from the BDS 1.26.51 circuit sweeps (`tools/bds-sweep/circuits.py`,
//! families `redstone_conductors`, `redstone_dust_cut` and
//! `redstone_dust_blocks`). The circuit files themselves are the evidence the
//! server's redstone tests replay; this module only exposes what the server
//! needs at runtime:
//!
//! - `conductors`: blocks a repeater powers through (a lamp beyond lights);
//! - `cuts`: blocks that stop dust climbing past them when they sit above
//!   the lower dust;
//! - per measured block state, whether power climbs onto a dust on top of
//!   it from the dust below beside it, and whether it comes down the far side.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use serde::Deserialize;

/// Block state properties as the sweep wrote them.
pub type States = serde_json::Map<String, serde_json::Value>;

/// One measured climb state: its properties, whether power climbs onto dust
/// on top of it, whether it comes down the far side.
pub type ClimbState = (States, bool, bool);

#[derive(Debug, Deserialize)]
struct BlocksFile {
    conductors: Vec<String>,
    cuts: Vec<String>,
    /// `[block, states, up, down]`.
    rows: Vec<(String, States, bool, bool)>,
}

/// The measured redstone block facts.
#[derive(Debug, Default)]
pub struct RedstoneBlocks {
    /// Blocks that can be powered and power what touches them.
    pub conductors: HashSet<String>,
    /// Blocks that cut dust climbing past them.
    pub cuts: HashSet<String>,
    /// Per block, the measured states with whether power climbs up onto dust
    /// on top of them and comes down their far side.
    climb: HashMap<String, Vec<ClimbState>>,
}

impl RedstoneBlocks {
    /// `(up, down)` for the measured state of `identifier` whose properties
    /// `value` returns, or `None` when that state was not measured. A state
    /// that differs from a measured one only in a property the circuit
    /// itself changes (`toggle_bit`, `powered_bit`, `triggered_bit`, `lit`,
    /// ...) counts as measured.
    pub fn climb(
        &self,
        identifier: &str,
        value: impl Fn(&str) -> Option<serde_json::Value>,
    ) -> Option<(bool, bool)> {
        const CIRCUIT_SET: &[&str] = &[
            "toggle_bit",
            "powered_bit",
            "triggered_bit",
            "lit",
            "output_lit_bit",
            "rail_data_bit",
            "redstone_signal",
        ];
        let rows = self.climb.get(identifier)?;
        let matches = |skip: &[&str]| {
            rows.iter().find_map(|(states, up, down)| {
                states
                    .iter()
                    .filter(|(property, _)| !skip.contains(&property.as_str()))
                    .all(|(property, expected)| value(property).as_ref() == Some(expected))
                    .then_some((*up, *down))
            })
        };
        matches(&[]).or_else(|| matches(CIRCUIT_SET))
    }

    /// Every measured climb state of `identifier`.
    pub fn climb_states(&self, identifier: &str) -> impl Iterator<Item = &ClimbState> {
        self.climb.get(identifier).into_iter().flatten()
    }
}

/// Decode `redstone/blocks.json`.
pub fn decode_blocks() -> Result<RedstoneBlocks, serde_json::Error> {
    let file: BlocksFile =
        serde_json::from_str(include_str!("../data/measured/redstone/blocks.json"))?;
    let mut climb: HashMap<_, Vec<_>> = HashMap::new();
    for (block, states, up, down) in file.rows {
        climb.entry(block).or_default().push((states, up, down));
    }
    Ok(RedstoneBlocks {
        conductors: file.conductors.into_iter().collect(),
        cuts: file.cuts.into_iter().collect(),
        climb,
    })
}

/// The measured redstone block facts (decoded on first use).
pub fn blocks() -> &'static RedstoneBlocks {
    static BLOCKS: LazyLock<RedstoneBlocks> =
        LazyLock::new(|| decode_blocks().expect("data/measured/redstone/blocks.json"));
    &BLOCKS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_redstone_blocks_decode() {
        let blocks = decode_blocks().unwrap();
        assert!(blocks.conductors.contains("minecraft:stone"));
        assert!(blocks.conductors.contains("minecraft:redstone_lamp"));
        assert!(!blocks.conductors.contains("minecraft:glass"));
        assert!(blocks.cuts.contains("minecraft:stone"));
        assert_eq!(
            blocks.climb("minecraft:glowstone", |_| None),
            Some((true, false))
        );
    }
}
