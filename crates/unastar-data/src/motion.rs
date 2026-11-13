//! What each canonical block state is to player movement
//! (`unastar_rules::movement::Block`), resolved once from the block data:
//! the friction, the movement family and the liquid. The server's movement
//! authority and the scripted sweep client both read it, so they classify
//! blocks the same way.

use std::sync::LazyLock;

use unastar_rules::movement::{Block, BlockKind, LiquidKind, LiquidState};

use crate::blocks::{ALL_BLOCKS, BlockData, StateValue};

/// Every canonical state's movement block, indexed by runtime id.
static TABLE: LazyLock<Vec<Block>> = LazyLock::new(|| {
    let len = ALL_BLOCKS
        .iter()
        .map(|block| block.max_state_id as usize + 1)
        .max()
        .unwrap_or(0);
    let mut table = vec![Block::AIR; len];
    let material = |identifier| {
        ALL_BLOCKS
            .iter()
            .find(|block| block.identifier == identifier)
            .map(|block| block.material)
    };
    let (water, lava) = (material("minecraft:water"), material("minecraft:lava"));
    for block in ALL_BLOCKS.iter() {
        let kind = BlockKind::of(block.identifier);
        let liquid = if !block.liquid {
            None
        } else if Some(block.material) == water {
            Some(LiquidKind::Water)
        } else if Some(block.material) == lava {
            Some(LiquidKind::Lava)
        } else {
            None
        };
        for state in block.min_state_id..=block.max_state_id {
            let kind = match kind {
                BlockKind::BubbleColumn { .. } => BlockKind::BubbleColumn {
                    down: block.state_value(state, "drag_down") == Some(StateValue::Byte(1)),
                },
                kind => kind,
            };
            table[state as usize] = Block {
                friction: block.friction,
                kind,
                liquid: liquid.map(|kind| LiquidState {
                    kind,
                    depth: liquid_depth(block, state)
                        .expect("every water and lava state has a liquid_depth in 0..=15"),
                }),
            };
        }
    }
    table
});

/// The `liquid_depth` of a liquid's state (0..=15).
fn liquid_depth(block: &BlockData, state: u32) -> Option<u8> {
    match block.state_value(state, "liquid_depth")? {
        StateValue::Int(depth) => u8::try_from(depth).ok().filter(|depth| *depth <= 15),
        _ => None,
    }
}

/// The movement block of canonical state `state` (air for an unknown id).
pub fn block(state: u32) -> Block {
    TABLE.get(state as usize).copied().unwrap_or(Block::AIR)
}

/// The liquid of canonical state `state`, if it is water or lava.
pub fn liquid(state: u32) -> Option<LiquidState> {
    block(state).liquid
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::get;

    #[test]
    fn liquids_resolve_with_their_depth() {
        let water = get("minecraft:water").unwrap();
        assert_eq!(
            liquid(water.min_state_id),
            Some(LiquidState {
                kind: LiquidKind::Water,
                depth: 0
            })
        );
        let lava = get("minecraft:flowing_lava").unwrap();
        let deep = (lava.min_state_id..=lava.max_state_id)
            .filter_map(liquid)
            .max_by_key(|l| l.depth);
        assert_eq!(
            deep,
            Some(LiquidState {
                kind: LiquidKind::Lava,
                depth: 15
            })
        );
        assert_eq!(liquid(get("minecraft:stone").unwrap().min_state_id), None);
        assert_eq!(
            block(get("minecraft:slime").unwrap().min_state_id).kind,
            BlockKind::Slime
        );
    }
}
