//! Immediate state results of Java 1.21.11 attachment `updateShape` hooks.
//!
//! Survival is checked only when the native override observes its support side.
//! The general survival helper has no attachment cases; these paths instead use
//! the native FULL/CENTER support masks through the parent module. Water ticks
//! and delayed leaf-distance ticks are scheduling side effects, not immediate
//! state changes, and are deliberately not simulated here.

use super::super::blocks::{BlockPos, Direction, JavaState};
use super::super::eval::FeatureWorld;
use super::super::java_blocks::{blocks as b, tags};
use super::super::survival::facing_of;

pub(super) fn update_shape<W: FeatureWorld + ?Sized>(
    world: &mut W,
    state: JavaState,
    pos: BlockPos,
    dir: Direction,
    neighbour: JavaState,
) -> Option<JavaState> {
    Some(match state.block().def().kind {
        "minecraft:door" | "minecraft:weathering_copper_door" => {
            door(world, state, pos, dir, neighbour)
        }
        "minecraft:chest" | "minecraft:trapped_chest" => chest(state, dir, neighbour, false),
        "minecraft:copper_chest" | "minecraft:weathering_copper_chest" => {
            chest(state, dir, neighbour, true)
        }
        "minecraft:ladder" | "minecraft:wall_torch" | "minecraft:redstone_wall_torch" => {
            let facing = facing_of(state).expect("native wall attachment has facing");
            supported(world, state, pos, dir, facing.opposite(), false)
        }
        "minecraft:button" | "minecraft:lever" => {
            // FaceAttachedHorizontalDirectionalBlock.getConnectedDirection points
            // away from the support, just like FACING for a wall attachment.
            let support = match state.value("face") {
                Some("ceiling") => Direction::Up,
                Some("floor") => Direction::Down,
                Some("wall") => facing_of(state)
                    .expect("native face attachment has facing")
                    .opposite(),
                _ => unreachable!("native face attachment has a valid face"),
            };
            supported(world, state, pos, dir, support, false)
        }
        "minecraft:torch" | "minecraft:redstone_torch" => {
            supported(world, state, pos, dir, Direction::Down, true)
        }
        "minecraft:lantern" | "minecraft:weathering_lantern" => {
            let support = if state.value("hanging") == Some("true") {
                Direction::Up
            } else {
                Direction::Down
            };
            supported(world, state, pos, dir, support, true)
        }
        // SlabBlock, TrapDoorBlock and EnderChestBlock only schedule a water
        // fluid tick when waterlogged, then delegate to the identity base hook.
        // Their weathering subclasses do not override updateShape.
        "minecraft:slab"
        | "minecraft:weathering_copper_slab"
        | "minecraft:trapdoor"
        | "minecraft:weathering_copper_trap_door"
        | "minecraft:ender_chest" => state,
        // LeavesBlock returns the original state even when distance is stale:
        // distance recomputation occurs in a scheduled block tick (delay 1).
        // It also schedules the normal water tick when waterlogged. All three
        // concrete leaf subclasses inherit this hook without an override.
        "minecraft:leaves"
        | "minecraft:tinted_particle_leaves"
        | "minecraft:untinted_particle_leaves"
        | "minecraft:mangrove_leaves" => state,
        "minecraft:vine" => vine(world, state, pos, dir),
        _ => return None,
    })
}

/// `VineBlock.updateShape`: except from below, recompute every face
/// (`getUpdatedState`) and become air without one. Native tree shape-edge
/// updates remove unsupported vine tops this way (seed 2097595355691761853,
/// jungle chunk -104,104).
fn vine<W: FeatureWorld + ?Sized>(world: &mut W, mut state: JavaState, pos: BlockPos, dir: Direction) -> JavaState {
    use super::super::kinds::multiface_growth::can_attach_to;
    if dir == Direction::Down {
        return state;
    }
    let above = world.get_state(pos.above());
    if state.value("up") == Some("true") {
        // Java passes DOWN here, so the block above is tested on its top face.
        state = state.with_bool("up", can_attach_to(above, Direction::Down));
    }
    for side in Direction::HORIZONTAL {
        let prop = side.name();
        if state.value(prop) != Some("true") {
            continue;
        }
        let ok = can_attach_to(world.get_state(pos.rel(side)), side)
            || (above.is(b::VINE) && above.value(prop) == Some("true"));
        state = state.with_bool(prop, ok);
    }
    let has_face = ["up", "north", "south", "west", "east"].iter().any(|p| state.value(p) == Some("true"));
    if has_face { state } else { super::super::java_blocks::states::AIR }
}

fn supported<W: FeatureWorld + ?Sized>(
    world: &mut W,
    state: JavaState,
    pos: BlockPos,
    dir: Direction,
    support: Direction,
    center: bool,
) -> JavaState {
    if dir != support {
        return state;
    }
    // Native canSurvive reads the support from the level, rather than trusting
    // the supplied neighbour state. This matters to callers evaluating a shape
    // against a snapshot before committing a replacement.
    let below_or_attached = world.get_state(pos.rel(support));
    let face = support.opposite();
    let survives = if center {
        super::supports_center(below_or_attached, face)
    } else {
        super::sturdy(below_or_attached, face)
    };
    if survives { state } else { b::AIR.default_state() }
}

fn is_door(state: JavaState) -> bool {
    matches!(
        state.block().def().kind,
        "minecraft:door" | "minecraft:weathering_copper_door"
    )
}

fn door<W: FeatureWorld + ?Sized>(
    world: &mut W,
    state: JavaState,
    pos: BlockPos,
    dir: Direction,
    neighbour: JavaState,
) -> JavaState {
    let half = state.value("half").expect("native door has half");
    let lower = half == "lower";
    if !dir.is_horizontal() && lower == (dir == Direction::Up) {
        // DoorBlock uses instanceof DoorBlock, not same-block identity. It
        // returns the counterpart's entire state with only HALF changed, so
        // material/oxidation, facing, hinge, open and powered all come from it.
        return if is_door(neighbour) && neighbour.value("half") != Some(half) {
            neighbour.with("half", half)
        } else {
            b::AIR.default_state()
        };
    }
    if lower && dir == Direction::Down
        && !super::sturdy(world.get_state(pos.below()), Direction::Up)
    {
        return b::AIR.default_state();
    }
    state
}

fn chest_connection(state: JavaState) -> Direction {
    let facing = facing_of(state).expect("native chest has facing");
    // ChestBlock.getConnectedDirection also defines this direction for SINGLE
    // (counterclockwise), though setting its type to SINGLE is then a no-op.
    if state.value("type") == Some("left") {
        facing.clockwise()
    } else {
        facing.counter_clockwise()
    }
}

fn chest(state: JavaState, dir: Direction, neighbour: JavaState, copper: bool) -> JavaState {
    let connects = if copper {
        // CopperChestBlock overrides chestCanConnectTo to accept every member
        // of the actual native tag, including other oxidation/wax stages.
        neighbour.is_in(tags::COPPER_CHESTS) && neighbour.has("type")
    } else {
        neighbour.is(state.block())
    };
    let mut result = state;
    if connects && dir.is_horizontal() {
        let other_type = neighbour.value("type").expect("native chest has type");
        if state.value("type") == Some("single")
            && other_type != "single"
            && state.value("facing") == neighbour.value("facing")
            && chest_connection(neighbour) == dir.opposite()
        {
            result = state.with("type", if other_type == "left" { "right" } else { "left" });
        }
        // A connectable neighbour of the wrong type/facing does NOT split an
        // existing double chest: that branch belongs only to the outer else.
    } else if chest_connection(state) == dir {
        result = state.with("type", "single");
    }
    if copper && connects && result.value("type") != Some("single")
        && chest_connection(result) == dir && result.block() != neighbour.block()
    {
        // CopperChestBlock's final hook adopts its connected neighbour's block
        // while copying this side's properties, including its waterlogged bit.
        return neighbour.block().default_state()
            .with("facing", result.value("facing").expect("native chest has facing"))
            .with("type", result.value("type").expect("native chest has type"))
            .with("waterlogged", result.value("waterlogged").expect("native chest is waterloggable"));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::test_world::TestWorld;

    #[test]
    fn door_observes_only_counterpart_and_lower_support_sides() {
        let mut world = TestWorld::default();
        let pos = BlockPos::new(0, 64, 0);
        let lower = b::OAK_DOOR.default_state().with("half", "lower");
        let upper = b::IRON_DOOR.default_state()
            .with("half", "upper")
            .with("facing", "east")
            .with("hinge", "right")
            .with("open", "true")
            .with("powered", "true");
        assert_eq!(update_shape(&mut world, lower, pos, Direction::Up, upper),
            Some(upper.with("half", "lower")));
        assert_eq!(update_shape(&mut world, lower, pos, Direction::Up, lower),
            Some(b::AIR.default_state()));
        assert_eq!(update_shape(&mut world, lower, pos, Direction::North, b::AIR.default_state()),
            Some(lower));
        assert_eq!(update_shape(&mut world, lower, pos, Direction::Down, b::STONE.default_state()),
            Some(b::AIR.default_state()));
        assert_eq!(update_shape(&mut world, upper, pos, Direction::Up, b::AIR.default_state()),
            Some(upper));
    }

    #[test]
    fn chest_pair_directions_and_unusual_native_no_split_branch() {
        for facing in Direction::HORIZONTAL {
            let single = b::CHEST.default_state().with("facing", facing.name())
                .with("waterlogged", "true");
            for (other_type, own_type) in [("left", "right"), ("right", "left")] {
                let other = single.with("type", other_type).with("waterlogged", "false");
                let direction = chest_connection(other).opposite();
                let joined = single.with("type", own_type);
                assert_eq!(chest(single, direction, other, false), joined);
                assert_eq!(chest(single, direction.opposite(), other, false), single);
                assert_eq!(chest(joined, direction, b::AIR.default_state(), false), single);
                assert_eq!(chest(joined, direction, single, false), joined);
                assert_eq!(chest(joined, direction, other.with("facing", facing.opposite().name()), false), joined);
                assert_eq!(chest(joined, Direction::Up, b::AIR.default_state(), false), joined);
            }
        }
    }

    #[test]
    fn copper_chest_adopts_partner_block_but_preserves_own_water() {
        let single = b::COPPER_CHEST.default_state()
            .with("facing", "north").with("waterlogged", "true");
        let partner = b::WAXED_OXIDIZED_COPPER_CHEST.default_state()
            .with("facing", "north").with("type", "left").with("waterlogged", "false");
        let result = chest(single, Direction::West, partner, true);
        assert_eq!(result, partner.with("type", "right").with("waterlogged", "true"));
        assert_eq!(chest(single, Direction::West, b::CHEST.default_state(), true), single);
    }

    #[test]
    fn attachments_ignore_unrelated_updates_and_use_level_support() {
        let pos = BlockPos::new(0, 64, 0);
        let mut world = TestWorld::default();
        let air = b::AIR.default_state();
        let ladder = b::LADDER.default_state().with("facing", "north")
            .with("waterlogged", "true");
        assert_eq!(update_shape(&mut world, ladder, pos, Direction::Down, air), Some(ladder));
        assert_eq!(update_shape(&mut world, ladder, pos, Direction::South, air), Some(air));
        world.blocks.insert(pos.rel(Direction::South), b::STONE.default_state());
        assert_eq!(update_shape(&mut world, ladder, pos, Direction::South, air), Some(ladder));
        for (face, direction) in [("floor", Direction::Down), ("ceiling", Direction::Up), ("wall", Direction::North)] {
            let lever = b::LEVER.default_state().with("face", face).with("facing", "south")
                .with("powered", "true");
            assert_eq!(update_shape(&mut world, lever, pos, direction.opposite(), air), Some(lever));
            assert_eq!(update_shape(&mut world, lever, pos, direction, air), Some(air));
        }
    }

    #[test]
    fn center_support_and_scheduling_only_identity() {
        let pos = BlockPos::new(0, 64, 0);
        let mut world = TestWorld::default();
        let air = b::AIR.default_state();
        let torch = b::TORCH.default_state();
        world.blocks.insert(pos.below(), b::OAK_FENCE.default_state());
        assert_eq!(update_shape(&mut world, torch, pos, Direction::Down, air), Some(torch));
        let hanging = b::LANTERN.default_state().with("hanging", "true").with("waterlogged", "true");
        world.blocks.insert(pos.above(), b::OAK_FENCE_GATE.default_state());
        assert_eq!(update_shape(&mut world, hanging, pos, Direction::Up, air), Some(air));
        for state in [b::OAK_TRAPDOOR.default_state().with("waterlogged", "true"),
            b::OAK_SLAB.default_state().with("waterlogged", "true"),
            b::OAK_LEAVES.default_state().with("distance", "7").with("waterlogged", "true")]
        {
            for dir in Direction::ALL {
                assert_eq!(update_shape(&mut world, state, pos, dir, air), Some(state));
            }
        }
    }
}
