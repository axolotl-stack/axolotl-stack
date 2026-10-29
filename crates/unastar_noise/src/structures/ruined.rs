//! Block rewrites used by ruined portals: `BlackstoneReplaceProcessor` and
//! `BlockAgeProcessor`.

use crate::features::java_blocks::{blocks as b, states, tags};
use crate::features::random::{BitSource, WorldgenRandom};
use crate::features::{Direction, JavaBlock, JavaState};

/// Copy every property `to` shares with `from` (Java `withPropertiesOf`).
pub fn with_properties_of(to: JavaBlock, from: JavaState) -> JavaState {
    let mut s = to.default_state();
    for p in to.def().props {
        if let Some(v) = from.value(p.name) {
            s = s.try_with(p.name, v);
        }
    }
    s
}

/// `BlackstoneReplaceProcessor`: stone family -> blackstone family, keeping
/// shared properties; other blocks are unchanged.
pub fn blackstone_replace(state: JavaState) -> JavaState {
    let block = state.block();
    let to = match block {
        x if x == b::COBBLESTONE || x == b::MOSSY_COBBLESTONE => b::BLACKSTONE,
        x if x == b::STONE => b::POLISHED_BLACKSTONE,
        x if x == b::STONE_BRICKS || x == b::MOSSY_STONE_BRICKS => b::POLISHED_BLACKSTONE_BRICKS,
        x if x == b::COBBLESTONE_STAIRS || x == b::MOSSY_COBBLESTONE_STAIRS => b::BLACKSTONE_STAIRS,
        x if x == b::STONE_STAIRS => b::POLISHED_BLACKSTONE_STAIRS,
        x if x == b::STONE_BRICK_STAIRS || x == b::MOSSY_STONE_BRICK_STAIRS => {
            b::POLISHED_BLACKSTONE_BRICK_STAIRS
        }
        x if x == b::COBBLESTONE_SLAB || x == b::MOSSY_COBBLESTONE_SLAB => b::BLACKSTONE_SLAB,
        x if x == b::SMOOTH_STONE_SLAB || x == b::STONE_SLAB => b::POLISHED_BLACKSTONE_SLAB,
        x if x == b::STONE_BRICK_SLAB || x == b::MOSSY_STONE_BRICK_SLAB => {
            b::POLISHED_BLACKSTONE_BRICK_SLAB
        }
        x if x == b::STONE_BRICK_WALL || x == b::MOSSY_STONE_BRICK_WALL => {
            b::POLISHED_BLACKSTONE_BRICK_WALL
        }
        x if x == b::COBBLESTONE_WALL || x == b::MOSSY_COBBLESTONE_WALL => b::BLACKSTONE_WALL,
        x if x == b::CHISELED_STONE_BRICKS => b::CHISELED_POLISHED_BLACKSTONE,
        x if x == b::CRACKED_STONE_BRICKS => b::CRACKED_POLISHED_BLACKSTONE_BRICKS,
        x if x == b::IRON_BARS => b::IRON_CHAIN,
        _ => return state,
    };
    with_properties_of(to, state)
}

fn random_facing_stairs<S: BitSource, const RETAIN_GAUSSIAN: bool>(
    rng: &mut WorldgenRandom<S, RETAIN_GAUSSIAN>,
    block: JavaBlock,
) -> JavaState {
    let facing = Direction::HORIZONTAL[rng.next_int_bounded(4) as usize];
    let half = ["top", "bottom"][rng.next_int_bounded(2) as usize];
    block
        .default_state()
        .with("facing", facing.name())
        .with("half", half)
}

fn pick<S: BitSource, const RETAIN_GAUSSIAN: bool>(
    rng: &mut WorldgenRandom<S, RETAIN_GAUSSIAN>,
    mossiness: f32,
    normal: &[JavaState],
    mossy: &[JavaState],
) -> JavaState {
    let list = if rng.next_float() < mossiness {
        mossy
    } else {
        normal
    };
    list[rng.next_int_bounded(list.len() as i32) as usize]
}

/// `BlockAgeProcessor.processBlock`: the aged state, or `None` to keep the block.
///
/// Java builds the candidate arrays before choosing, so the random stair
/// facings are drawn even when a non-stair candidate wins.
pub fn block_age<S: BitSource, const RETAIN_GAUSSIAN: bool>(
    state: JavaState,
    mossiness: f32,
    rng: &mut WorldgenRandom<S, RETAIN_GAUSSIAN>,
) -> Option<JavaState> {
    let block = state.block();
    if block == b::STONE_BRICKS || block == b::STONE || block == b::CHISELED_STONE_BRICKS {
        if rng.next_float() >= 0.5 {
            return None;
        }
        let normal = [
            states::CRACKED_STONE_BRICKS,
            random_facing_stairs(rng, b::STONE_BRICK_STAIRS),
        ];
        let mossy = [
            states::MOSSY_STONE_BRICKS,
            random_facing_stairs(rng, b::MOSSY_STONE_BRICK_STAIRS),
        ];
        return Some(pick(rng, mossiness, &normal, &mossy));
    }
    if state.is_in(tags::STAIRS) {
        let facing = state.value("facing").unwrap_or("north");
        let half = state.value("half").unwrap_or("bottom");
        if rng.next_float() >= 0.5 {
            return None;
        }
        let normal = [states::STONE_SLAB, states::STONE_BRICK_SLAB];
        let mossy = [
            states::MOSSY_STONE_BRICK_STAIRS
                .with("facing", facing)
                .with("half", half),
            states::MOSSY_STONE_BRICK_SLAB,
        ];
        return Some(pick(rng, mossiness, &normal, &mossy));
    }
    if state.is_in(tags::SLABS) {
        // The mossy slab keeps the source slab's shape: a native ocean ruined
        // portal turned `stone_brick_slab[type=double]` and `[type=top]`
        // template blocks into mossy slabs of the same type.
        return (rng.next_float() < mossiness).then(|| {
            let mossy = states::MOSSY_STONE_BRICK_SLAB;
            let mossy = state
                .value("type")
                .map_or(mossy, |shape| mossy.with("type", shape));
            state
                .value("waterlogged")
                .map_or(mossy, |wet| mossy.with("waterlogged", wet))
        });
    }
    if state.is_in(tags::WALLS) {
        return (rng.next_float() < mossiness).then_some(states::MOSSY_STONE_BRICK_WALL);
    }
    if block == b::OBSIDIAN {
        return (rng.next_float() < 0.15).then_some(states::CRYING_OBSIDIAN);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::random::LegacyRandom;

    /// Native ocean ruined portal (`giant_portal_2`, mossiness 0.8) kept the
    /// template slab's `type` when it aged `stone_brick_slab` to mossy.
    #[test]
    fn aged_slabs_keep_their_shape() {
        for shape in ["bottom", "top", "double"] {
            let source = states::STONE_BRICK_SLAB.with("type", shape);
            let aged = block_age(source, 1.0, &mut LegacyRandom::new(0)).unwrap();
            assert_eq!(
                aged,
                states::MOSSY_STONE_BRICK_SLAB
                    .with("type", shape)
                    .with("waterlogged", "false")
            );
        }
    }
}
