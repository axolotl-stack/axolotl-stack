//! Measured liquid facts per block, decoded once at startup.
//!
//! Source: `data/measured/liquids/blocks.json`, derived by `summarize.py
//! liquids` from the BDS 1.26.51 sweeps `liquid_blocks_water`,
//! `liquid_blocks_lava` and `liquid_holders` (`tools/bds-sweep/liquids.py`).
//! Every block was measured in its first palette state:
//!
//! - what a water or lava source spreading beside it does to it: the liquid
//!   breaks it and takes its place, water fills its liquid layer, or it
//!   keeps the liquid out;
//! - whether it keeps a water source it is set into (the water moves into
//!   its liquid layer) and through which of its sides that water spreads.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

/// What a spreading liquid does to a block in its way.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reaction {
    /// The liquid breaks it (dropping these items, `minecraft:` left out)
    /// and takes its place.
    Breaks(Vec<String>),
    /// Water spreads into the block's liquid layer; the block stays.
    Fills,
    /// The liquid stays out.
    Blocks,
    /// The block turned into another block (`minecraft:` left out) when the
    /// liquid came beside it (concrete powder hardening); the liquid stays out.
    Becomes(String),
    /// Setting the block crashed the measuring harness; treated as blocking.
    Crashed,
}

/// Whether a block keeps the water it is set into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Holds {
    /// It does not: the water is replaced.
    No,
    /// It does; the water spreads out through the sides marked `true`
    /// (-x, +x, -z, +z, for the measured first palette state).
    Sides([bool; 4]),
    /// It broke on its own once set (no support) and left the water as a
    /// source; it holds water while it stands.
    Popped,
    /// Setting the block crashed the measuring harness.
    Crashed,
}

/// The liquid facts of one block.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LiquidBlock {
    pub water: Reaction,
    pub lava: Reaction,
    pub holds: Holds,
}

impl LiquidBlock {
    /// Whether the block keeps water set into it.
    pub fn holds_water(&self) -> bool {
        matches!(self.holds, Holds::Sides(_) | Holds::Popped)
    }
}

#[derive(Debug, Deserialize)]
struct File {
    /// `[block (no namespace), facts]`.
    rows: Vec<(String, LiquidBlock)>,
}

/// Decode `liquids/blocks.json`: block identifier (`minecraft:` included) to
/// its liquid facts.
pub fn decode_blocks() -> Result<HashMap<String, LiquidBlock>, serde_json::Error> {
    let file: File = serde_json::from_str(include_str!("../data/measured/liquids/blocks.json"))?;
    Ok(file
        .rows
        .into_iter()
        .map(|(name, facts)| (format!("minecraft:{name}"), facts))
        .collect())
}

/// The measured liquid facts per block (decoded on first use).
pub fn blocks() -> &'static HashMap<String, LiquidBlock> {
    static BLOCKS: LazyLock<HashMap<String, LiquidBlock>> =
        LazyLock::new(|| decode_blocks().expect("data/measured/liquids/blocks.json"));
    &BLOCKS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_liquid_blocks_decode() {
        let blocks = decode_blocks().unwrap();
        let fence = &blocks["minecraft:oak_fence"];
        assert_eq!(fence.water, Reaction::Blocks);
        assert_eq!(fence.holds, Holds::Sides([true; 4]));
        assert_eq!(
            blocks["minecraft:oak_stairs"].holds,
            Holds::Sides([true, false, true, true])
        );
        assert_eq!(
            blocks["minecraft:unpowered_repeater"].water,
            Reaction::Fills
        );
        assert_eq!(
            blocks["minecraft:torch"].water,
            Reaction::Breaks(vec!["torch".into()])
        );
        assert_eq!(
            blocks["minecraft:black_concrete_powder"].water,
            Reaction::Becomes("black_concrete".into())
        );
        assert_eq!(blocks["minecraft:stone"].holds, Holds::No);
        assert_eq!(blocks["minecraft:stone"].lava, Reaction::Blocks);
    }
}
