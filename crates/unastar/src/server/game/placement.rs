//! Block placement rules: target position, placed state and obstruction.
//!
//! Vanilla placement behaviour (as implemented by dragonfly's
//! `block.place`/`item.UseOnBlock` handling), to be confirmed against BDS:
//!
//! - A replaceable clicked block (air, liquids, fire, grass, ferns, vines,
//!   snow layers below a full stack, ...) is replaced in place; otherwise the
//!   block goes next to the clicked face, which must be replaceable.
//! - States from the click: `pillar_axis` from the clicked face's axis; slabs
//!   on the top or bottom half (upper half of a side face or the underside of
//!   a block gives a top slab) and merge into a double slab; stairs face where
//!   the player looks and flip on the upper half; `minecraft:cardinal_direction`
//!   blocks face the player; `facing_direction` blocks (pistons, dispensers,
//!   droppers, barrels) face the player in 3D; ladders attach to the clicked
//!   side; torches attach to the clicked face (never under a block); doors take
//!   two blocks and face where the player looks.
//! - The new block's collision boxes may not intersect a player or a
//!   non-item physics body (items do not block placement).

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};

use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::{DroppedItem, PhysicsBody, Player, Position};
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::{BlockEntry, StateValue};
use crate::registry::collision::{Aabb, block_collision_boxes};

/// Offset of each block face (`0` down, `1` up, `2` north, `3` south,
/// `4` west, `5` east).
pub fn face_offset(face: u8) -> Option<IVec3> {
    Some(match face {
        0 => IVec3::NEG_Y,
        1 => IVec3::Y,
        2 => IVec3::NEG_Z,
        3 => IVec3::Z,
        4 => IVec3::NEG_X,
        5 => IVec3::X,
        _ => return None,
    })
}

/// Horizontal direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Horizontal {
    South,
    West,
    North,
    East,
}

impl Horizontal {
    /// Bedrock yaw: 0 looks south (+Z), 90 west, 180 north, 270 east.
    pub fn from_yaw(yaw: f32) -> Self {
        match (f64::from(yaw) / 90.0).round().rem_euclid(4.0) as i32 {
            0 => Self::South,
            1 => Self::West,
            2 => Self::North,
            _ => Self::East,
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::South => Self::North,
            Self::West => Self::East,
            Self::North => Self::South,
            Self::East => Self::West,
        }
    }

    fn cardinal(self) -> &'static str {
        match self {
            Self::South => "south",
            Self::West => "west",
            Self::North => "north",
            Self::East => "east",
        }
    }

    /// Stairs `weirdo_direction`: 0 east, 1 west, 2 south, 3 north.
    fn weirdo(self) -> i32 {
        match self {
            Self::East => 0,
            Self::West => 1,
            Self::South => 2,
            Self::North => 3,
        }
    }

    /// `facing_direction` ordinal (2 north, 3 south, 4 west, 5 east).
    fn facing(self) -> i32 {
        match self {
            Self::North => 2,
            Self::South => 3,
            Self::West => 4,
            Self::East => 5,
        }
    }
}

/// Inputs of one `ItemUse` click.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacementInput {
    pub clicked: IVec3,
    pub face: u8,
    /// Click position relative to the clicked block (0..1 per axis).
    pub click: DVec3,
    pub yaw: f32,
    pub pitch: f32,
}

/// Where and what to place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub position: IVec3,
    pub state: u32,
    /// Second block (upper door half).
    pub upper: Option<(IVec3, u32)>,
}

/// Whether placing a block may replace `state` (the block is "built over").
pub fn is_replaceable(entry: &BlockEntry, state: u32) -> bool {
    // Air and light blocks (0), water (5), lava (6), fire (10), structure void (22).
    if matches!(entry.data.material, 0 | 5 | 6 | 10 | 22) {
        return true;
    }
    match entry.string_id.as_str() {
        // Snow layers below a full stack.
        "minecraft:snow_layer" => {
            !matches!(entry.state_value(state, "height"), Some(StateValue::Int(7)))
        }
        "minecraft:short_grass"
        | "minecraft:fern"
        | "minecraft:tall_grass"
        | "minecraft:large_fern"
        | "minecraft:deadbush"
        | "minecraft:seagrass"
        | "minecraft:vine"
        | "minecraft:glow_lichen"
        | "minecraft:crimson_roots"
        | "minecraft:warped_roots"
        | "minecraft:nether_sprouts"
        | "minecraft:hanging_roots"
        | "minecraft:short_dry_grass"
        | "minecraft:tall_dry_grass"
        | "minecraft:bush" => true,
        _ => false,
    }
}

fn set(entry: &BlockEntry, state: u32, property: &str, value: StateValue) -> u32 {
    entry
        .with_state_value(state, property, value)
        .unwrap_or(state)
}

/// Direction the player looks in 3D as a `facing_direction` ordinal.
fn look_facing(yaw: f32, pitch: f32) -> i32 {
    if pitch > 45.0 {
        0
    } else if pitch < -45.0 {
        1
    } else {
        Horizontal::from_yaw(yaw).facing()
    }
}

fn opposite_facing(facing: i32) -> i32 {
    match facing {
        0 => 1,
        1 => 0,
        2 => 3,
        3 => 2,
        4 => 5,
        _ => 4,
    }
}

/// The double-slab block of a slab (`oak_slab` -> `oak_double_slab`).
pub fn double_slab<'a>(registry: &'a BlockRegistry, slab: &BlockEntry) -> Option<&'a BlockEntry> {
    let base = slab.string_id.strip_suffix("_slab")?;
    registry.get_by_name(&format!("{base}_double_slab"))
}

fn upper_half(input: &PlacementInput) -> bool {
    match input.face {
        0 => true,
        1 => false,
        _ => input.click.y > 0.5,
    }
}

/// State of `block` placed by `input`, or `None` when the block cannot go
/// on that face (torches and ladders under blocks).
fn placed_state(block: &BlockEntry, input: &PlacementInput) -> Option<u32> {
    let mut state = block.default_state_id;
    let looking = Horizontal::from_yaw(input.yaw);
    if block.state_value(state, "pillar_axis").is_some() {
        let axis = match input.face {
            0 | 1 => "y",
            2 | 3 => "z",
            _ => "x",
        };
        state = set(block, state, "pillar_axis", StateValue::Str(axis));
    }
    if block.state_value(state, "weirdo_direction").is_some() {
        state = set(
            block,
            state,
            "weirdo_direction",
            StateValue::Int(looking.weirdo()),
        );
        state = set(
            block,
            state,
            "upside_down_bit",
            StateValue::Byte(u8::from(upper_half(input))),
        );
    } else if block.string_id.ends_with("_slab") {
        let half = if upper_half(input) { "top" } else { "bottom" };
        state = set(
            block,
            state,
            "minecraft:vertical_half",
            StateValue::Str(half),
        );
    }
    if block.state_value(state, "door_hinge_bit").is_some() {
        state = set(
            block,
            state,
            "minecraft:cardinal_direction",
            StateValue::Str(looking.cardinal()),
        );
    } else if block
        .state_value(state, "minecraft:cardinal_direction")
        .is_some()
    {
        state = set(
            block,
            state,
            "minecraft:cardinal_direction",
            StateValue::Str(looking.opposite().cardinal()),
        );
    }
    if let Some(StateValue::Str(_)) = block.state_value(state, "torch_facing_direction") {
        let facing = match input.face {
            1 => "top",
            2 => "north",
            3 => "south",
            4 => "west",
            5 => "east",
            _ => return None,
        };
        state = set(
            block,
            state,
            "torch_facing_direction",
            StateValue::Str(facing),
        );
    }
    match block.string_id.as_str() {
        "minecraft:ladder" => {
            if input.face < 2 {
                return None;
            }
            state = set(
                block,
                state,
                "facing_direction",
                StateValue::Int(i32::from(input.face)),
            );
        }
        "minecraft:piston"
        | "minecraft:sticky_piston"
        | "minecraft:dispenser"
        | "minecraft:dropper"
        | "minecraft:barrel" => {
            let facing = opposite_facing(look_facing(input.yaw, input.pitch));
            state = set(block, state, "facing_direction", StateValue::Int(facing));
        }
        _ => {}
    }
    Some(state)
}

/// Resolve where `held` goes for `input`. `None` when nothing can be placed.
pub fn resolve_placement(
    blocks: &impl BlockSource,
    held: &BlockEntry,
    input: &PlacementInput,
) -> Option<Placement> {
    let registry = blocks.registry();
    let clicked_state = blocks.block_at(input.clicked)?;
    let clicked = registry.get_by_runtime_id(clicked_state)?;

    // Slab onto the open half of the same slab: merge into a double slab.
    if held.string_id.ends_with("_slab") && clicked.string_id == held.string_id {
        let half = clicked.state_value(clicked_state, "minecraft:vertical_half");
        let merges = matches!(
            (input.face, half),
            (1, Some(StateValue::Str("bottom"))) | (0, Some(StateValue::Str("top")))
        );
        if merges {
            let double = double_slab(registry, held)?;
            return Some(Placement {
                position: input.clicked,
                state: double.default_state_id,
                upper: None,
            });
        }
    }

    let position = if is_replaceable(clicked, clicked_state) {
        input.clicked
    } else {
        input.clicked + face_offset(input.face)?
    };
    let target_state = blocks.block_at(position)?;
    let target = registry.get_by_runtime_id(target_state)?;
    let state = placed_state(held, input)?;

    // Slab next to the same slab of the other half: merge.
    if held.string_id.ends_with("_slab") && target.string_id == held.string_id {
        let existing = target.state_value(target_state, "minecraft:vertical_half");
        let new = held.state_value(state, "minecraft:vertical_half");
        if existing.is_some() && existing != new {
            let double = double_slab(registry, held)?;
            return Some(Placement {
                position,
                state: double.default_state_id,
                upper: None,
            });
        }
        return None;
    }
    if !is_replaceable(target, target_state) {
        return None;
    }

    let upper = if held.state_value(state, "upper_block_bit").is_some() {
        let above = position + IVec3::Y;
        let above_state = blocks.block_at(above)?;
        if !registry
            .get_by_runtime_id(above_state)
            .is_some_and(|entry| is_replaceable(entry, above_state))
        {
            return None;
        }
        Some((
            above,
            set(held, state, "upper_block_bit", StateValue::Byte(1)),
        ))
    } else {
        None
    };
    Some(Placement {
        position,
        state,
        upper,
    })
}

/// World-space collision boxes of `state` placed at `position`.
pub fn placed_boxes(registry: &BlockRegistry, position: IVec3, state: u32) -> Vec<Aabb> {
    let mut boxes = Vec::new();
    if registry.get_by_runtime_id(state).is_some() {
        block_collision_boxes(state, &mut boxes);
    }
    boxes
        .into_iter()
        .map(|b| b.offset(position.as_dvec3()))
        .collect()
}

/// Whether a player or a non-item physics body is in the way of `boxes`.
pub fn obstructed(world: &mut World, boxes: &[Aabb]) -> bool {
    if boxes.is_empty() {
        return false;
    }
    let mut query = world
        .query_filtered::<(&Position, Option<&PhysicsBody>, Has<Player>), Without<DroppedItem>>();
    query.iter(world).any(|(position, body, is_player)| {
        let aabb = if is_player {
            Aabb::from_feet(
                position.0 - DVec3::new(0.0, PLAYER_EYE_HEIGHT, 0.0),
                0.6,
                1.8,
            )
        } else if let Some(body) = body {
            Aabb::from_feet(position.0, body.width, body.height)
        } else {
            return false;
        };
        boxes.iter().any(|b| b.intersects(&aabb))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::systems::physics::tests::TestBlocks;

    fn entry(name: &str) -> &'static BlockEntry {
        BlockRegistry::vanilla().get_by_name(name).expect(name)
    }

    fn input(face: u8, click_y: f64, yaw: f32) -> PlacementInput {
        PlacementInput {
            clicked: IVec3::new(0, 64, 0),
            face,
            click: DVec3::new(0.5, click_y, 0.5),
            yaw,
            pitch: 0.0,
        }
    }

    fn world() -> TestBlocks {
        let mut blocks = TestBlocks::default();
        blocks.set(IVec3::new(0, 64, 0), "minecraft:stone");
        blocks
    }

    fn value(placement: &Placement, property: &str) -> Option<StateValue> {
        BlockRegistry::vanilla()
            .get_by_runtime_id(placement.state)?
            .state_value(placement.state, property)
    }

    #[test]
    fn logs_take_the_axis_of_the_clicked_face() {
        let log = entry("minecraft:oak_log");
        let up = resolve_placement(&world(), log, &input(1, 1.0, 0.0)).unwrap();
        assert_eq!(up.position, IVec3::new(0, 65, 0));
        assert_eq!(value(&up, "pillar_axis"), Some(StateValue::Str("y")));
        let side = resolve_placement(&world(), log, &input(5, 0.5, 0.0)).unwrap();
        assert_eq!(side.position, IVec3::new(1, 64, 0));
        assert_eq!(value(&side, "pillar_axis"), Some(StateValue::Str("x")));
        let north = resolve_placement(&world(), log, &input(2, 0.5, 0.0)).unwrap();
        assert_eq!(value(&north, "pillar_axis"), Some(StateValue::Str("z")));
    }

    #[test]
    fn stairs_face_the_look_direction_and_flip_on_upper_halves() {
        let stairs = entry("minecraft:oak_stairs");
        // Looking north (yaw 180) at the east side, lower half.
        let low = resolve_placement(&world(), stairs, &input(5, 0.25, 180.0)).unwrap();
        assert_eq!(value(&low, "weirdo_direction"), Some(StateValue::Int(3)));
        assert_eq!(value(&low, "upside_down_bit"), Some(StateValue::Byte(0)));
        let high = resolve_placement(&world(), stairs, &input(5, 0.75, 90.0)).unwrap();
        assert_eq!(value(&high, "weirdo_direction"), Some(StateValue::Int(1)));
        assert_eq!(value(&high, "upside_down_bit"), Some(StateValue::Byte(1)));
    }

    #[test]
    fn slabs_pick_halves_and_merge_into_double_slabs() {
        let slab = entry("minecraft:oak_slab");
        let bottom = resolve_placement(&world(), slab, &input(1, 1.0, 0.0)).unwrap();
        assert_eq!(
            value(&bottom, "minecraft:vertical_half"),
            Some(StateValue::Str("bottom"))
        );
        let top = resolve_placement(&world(), slab, &input(4, 0.8, 0.0)).unwrap();
        assert_eq!(
            value(&top, "minecraft:vertical_half"),
            Some(StateValue::Str("top"))
        );

        let mut blocks = world();
        blocks.set_state(IVec3::new(0, 65, 0), bottom.state);
        let mut onto = input(1, 0.5, 0.0);
        onto.clicked = IVec3::new(0, 65, 0);
        let merged = resolve_placement(&blocks, slab, &onto).unwrap();
        assert_eq!(merged.position, IVec3::new(0, 65, 0));
        assert_eq!(
            BlockRegistry::vanilla()
                .get_by_runtime_id(merged.state)
                .unwrap()
                .string_id,
            "minecraft:oak_double_slab"
        );
    }

    #[test]
    fn facing_blocks_face_the_player_and_doors_take_two_blocks() {
        // Player looks south (yaw 0): a furnace faces north, back at them.
        let furnace =
            resolve_placement(&world(), entry("minecraft:furnace"), &input(1, 1.0, 0.0)).unwrap();
        assert_eq!(
            value(&furnace, "minecraft:cardinal_direction"),
            Some(StateValue::Str("north"))
        );
        let door = resolve_placement(
            &world(),
            entry("minecraft:wooden_door"),
            &input(1, 1.0, 270.0),
        )
        .unwrap();
        assert_eq!(
            value(&door, "minecraft:cardinal_direction"),
            Some(StateValue::Str("east"))
        );
        assert_eq!(value(&door, "upper_block_bit"), Some(StateValue::Byte(0)));
        let (above, upper) = door.upper.unwrap();
        assert_eq!(above, IVec3::new(0, 66, 0));
        let upper_entry = BlockRegistry::vanilla().get_by_runtime_id(upper).unwrap();
        assert_eq!(
            upper_entry.state_value(upper, "upper_block_bit"),
            Some(StateValue::Byte(1))
        );

        let mut blocked = world();
        blocked.set(IVec3::new(0, 66, 0), "minecraft:stone");
        assert!(
            resolve_placement(
                &blocked,
                entry("minecraft:wooden_door"),
                &input(1, 1.0, 0.0)
            )
            .is_none()
        );
    }

    #[test]
    fn torches_attach_to_the_clicked_face_but_not_underneath() {
        let torch = entry("minecraft:torch");
        let wall = resolve_placement(&world(), torch, &input(3, 0.5, 0.0)).unwrap();
        assert_eq!(
            value(&wall, "torch_facing_direction"),
            Some(StateValue::Str("south"))
        );
        let floor = resolve_placement(&world(), torch, &input(1, 1.0, 0.0)).unwrap();
        assert_eq!(
            value(&floor, "torch_facing_direction"),
            Some(StateValue::Str("top"))
        );
        assert!(resolve_placement(&world(), torch, &input(0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn replaceable_blocks_are_built_over_and_solid_targets_refuse() {
        let mut blocks = world();
        blocks.set(IVec3::new(0, 65, 0), "minecraft:short_grass");
        let mut on_grass = input(1, 0.5, 0.0);
        on_grass.clicked = IVec3::new(0, 65, 0);
        let placed = resolve_placement(&blocks, entry("minecraft:dirt"), &on_grass).unwrap();
        assert_eq!(
            placed.position,
            IVec3::new(0, 65, 0),
            "grass is replaced in place"
        );

        blocks.set(IVec3::new(1, 64, 0), "minecraft:stone");
        assert!(resolve_placement(&blocks, entry("minecraft:dirt"), &input(5, 0.5, 0.0)).is_none());
    }

    #[test]
    fn players_and_mobs_obstruct_but_items_do_not() {
        let mut world = World::new();
        let boxes = placed_boxes(
            BlockRegistry::vanilla(),
            IVec3::new(0, 64, 0),
            BlockRegistry::vanilla()
                .default_state_id_by_name("minecraft:stone")
                .unwrap(),
        );
        world.spawn((
            Position(DVec3::new(0.5, 64.0, 0.5)),
            PhysicsBody::ITEM,
            DroppedItem,
        ));
        assert!(!obstructed(&mut world, &boxes));
        world.spawn((
            Player,
            Position(DVec3::new(0.5, 63.0 + PLAYER_EYE_HEIGHT, 0.5)),
        ));
        assert!(
            obstructed(&mut world, &boxes),
            "the block would be inside the player"
        );
        let mut world = World::new();
        world.spawn((
            Player,
            Position(DVec3::new(0.5, 65.0 + PLAYER_EYE_HEIGHT, 0.5)),
        ));
        assert!(!obstructed(&mut world, &boxes), "standing on top is fine");
        world.spawn((
            Position(DVec3::new(0.9, 64.5, 0.5)),
            PhysicsBody::living(0.6, 1.8),
        ));
        assert!(obstructed(&mut world, &boxes));
    }
}
