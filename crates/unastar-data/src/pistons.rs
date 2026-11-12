//! Measured piston facts, decoded once at startup.
//!
//! Source: `data/measured/pistons/blocks.json`, derived by `summarize.py
//! round4` from the BDS 1.26.51 piston sweep (`tools/bds-sweep/pistons.py`,
//! family `pistons_blocks`): every block's default state in front of a sticky
//! piston powered for 20 ticks. The piston family files are the evidence the
//! server's piston tests replay; this module only exposes how a piston treats
//! each block.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

/// How a piston treats a block in its way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Movement {
    /// Pushed, and pulled back by a sticky piston.
    PushPull,
    /// Pushed, never pulled back.
    PushOnly,
    /// Removed (with its drops) as the head comes out.
    Breaks,
    /// Holds the piston: the head does not come out.
    Immovable,
}

#[derive(Debug, Deserialize)]
struct BlocksFile {
    /// `[block, class]`.
    rows: Vec<(String, String)>,
}

/// The measured class of every block whose default state the sweep could
/// stand in front of the piston, by identifier (`minecraft:` left out).
#[derive(Debug, Default)]
pub struct PistonBlocks {
    classes: HashMap<String, Movement>,
}

impl PistonBlocks {
    /// The measured class of `identifier` (with or without `minecraft:`).
    pub fn movement(&self, identifier: &str) -> Option<Movement> {
        let short = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
        self.classes.get(short).copied()
    }
}

/// Decode `pistons/blocks.json`.
pub fn decode_blocks() -> Result<PistonBlocks, String> {
    let file: BlocksFile =
        serde_json::from_str(include_str!("../data/measured/pistons/blocks.json"))
            .map_err(|error| error.to_string())?;
    let mut classes = HashMap::new();
    for (block, class) in file.rows {
        let movement = match class.as_str() {
            "push_pull" => Movement::PushPull,
            "push_only" => Movement::PushOnly,
            "breaks" => Movement::Breaks,
            "immovable" => Movement::Immovable,
            other => {
                return Err(format!(
                    "pistons/blocks.json: {block}: unknown class {other}"
                ));
            }
        };
        classes.insert(block, movement);
    }
    Ok(PistonBlocks { classes })
}

/// The measured piston block classes (decoded on first use).
pub fn blocks() -> &'static PistonBlocks {
    static BLOCKS: LazyLock<PistonBlocks> =
        LazyLock::new(|| decode_blocks().expect("data/measured/pistons/blocks.json"));
    &BLOCKS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_piston_blocks_decode() {
        let blocks = decode_blocks().unwrap();
        assert_eq!(blocks.movement("minecraft:stone"), Some(Movement::PushPull));
        assert_eq!(blocks.movement("obsidian"), Some(Movement::Immovable));
        assert_eq!(
            blocks.movement("minecraft:white_glazed_terracotta"),
            Some(Movement::PushOnly)
        );
        assert_eq!(blocks.movement("minecraft:torch"), Some(Movement::Breaks));
        assert_eq!(blocks.movement("minecraft:chest"), Some(Movement::PushPull));
    }
}
