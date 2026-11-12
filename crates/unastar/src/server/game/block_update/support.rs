//! Whether a block can stay where it is, measured on BDS 1.26.51.
//!
//! Evidence (`crates/unastar-data/data/measured/block_updates/`):
//! `support.json` (every block's default state and each single-property
//! variation in a stone shell whose sides are removed one at a time) gives
//! which neighbour each block needs; `surfaces.json` gives which blocks hold
//! each kind of attached block (derived: `support_holders.json`);
//! `soils.json` which blocks each plant stays on (derived:
//! `plant_soils.json`); `halves.json` how doors and tall plants come apart.
//!
//! BDS checks a plant or a door half only when a neighbour changes, never
//! when the block itself is placed (a dandelion set on stone stays); an
//! attached block is also checked when it is placed (a torch set against air
//! drops at once). Blocks with the native `falling` flag are left to
//! gravity (`server::game::falling`). Blocks this module does not list stay:
//! candles, end rods, chains and hanging signs fixed to a wall never fall in
//! BDS when the block holding them goes. Attached blocks in an orientation no
//! sweep measured (a lever or amethyst bud under a ceiling, a hanging bell)
//! need only some block on that side.

use glam::IVec3;
use unastar_data::block_updates::{HolderRule, plant_soil_states, plant_soils, support_holders};

use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::registry::collision::block_collision_aabbs;

/// A side of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Face {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Face {
    pub(crate) fn offset(self) -> IVec3 {
        match self {
            Self::Down => IVec3::NEG_Y,
            Self::Up => IVec3::Y,
            Self::North => IVec3::NEG_Z,
            Self::South => IVec3::Z,
            Self::West => IVec3::NEG_X,
            Self::East => IVec3::X,
        }
    }

    pub(crate) fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }

    /// The face a `facing_direction` ordinal points to (0 down, 1 up, 2
    /// north, 3 south, 4 west, 5 east).
    fn from_facing(facing: i32) -> Option<Self> {
        Some(match facing {
            0 => Self::Down,
            1 => Self::Up,
            2 => Self::North,
            3 => Self::South,
            4 => Self::West,
            5 => Self::East,
            _ => return None,
        })
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "down" => Self::Down,
            "up" => Self::Up,
            "north" => Self::North,
            "south" => Self::South,
            "west" => Self::West,
            "east" => Self::East,
            _ => return None,
        })
    }
}

/// What the lower half of a two-block structure stands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Floor {
    /// Any block but air.
    Present,
    /// The measured soils of a plant (key into `plant_soils.json`).
    Soil(&'static str),
}

/// What a block needs to stay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Needs {
    /// Nothing: it stays wherever it is.
    Nothing,
    /// A block on `face` that holds it: a holder of the measured `class`
    /// (`<attached block>/<support side>` in `support_holders.json`), or any
    /// block but air when no sweep measured that kind.
    Holder {
        face: Face,
        class: Option<&'static str>,
    },
    /// One of the measured soils of `plant` below.
    Soil { plant: &'static str },
    /// The other half of a two-block structure on `other`; the lower half
    /// also needs `floor` below.
    Half { other: Face, floor: Option<Floor> },
    /// A rail: a holder below and, sloped, one on the side it rises toward
    /// (BDS 1.26.51 pops a rail rising east when the block east of it goes;
    /// `support.json`).
    Rail { slope: Option<Face> },
}

fn int(entry: &BlockEntry, state: u32, property: &str) -> Option<i32> {
    match entry.state_value(state, property)? {
        StateValue::Int(value) => Some(value),
        _ => None,
    }
}

fn text<'a>(entry: &'a BlockEntry, state: u32, property: &str) -> Option<&'a str> {
    match entry.state_value(state, property)? {
        StateValue::Str(value) => Some(value),
        _ => None,
    }
}

fn byte(entry: &BlockEntry, state: u32, property: &str) -> Option<bool> {
    match entry.state_value(state, property)? {
        StateValue::Byte(value) => Some(value != 0),
        _ => None,
    }
}

/// The measured class for an attached block family on `face`: floor, wall
/// (every horizontal side was measured as east) or ceiling.
fn class_on(
    face: Face,
    floor: Option<&'static str>,
    wall: Option<&'static str>,
    ceiling: Option<&'static str>,
) -> Needs {
    let class = match face {
        Face::Down => floor,
        Face::Up => ceiling,
        _ => wall,
    };
    Needs::Holder { face, class }
}

/// The faces of a multi-face block and its bit for each (BDS 1.26.51,
/// `multiface_support.json`): vines name only the four sides
/// (`vine_direction_bits`), glow lichen, sculk veins and resin clumps all six
/// (`multi_face_direction_bits`).
pub(crate) fn multiface_bits(entry: &BlockEntry) -> Option<(&'static str, &'static [(Face, i32)])> {
    const VINE: [(Face, i32); 4] = [
        (Face::South, 1),
        (Face::West, 2),
        (Face::North, 4),
        (Face::East, 8),
    ];
    const MULTI: [(Face, i32); 6] = [
        (Face::Down, 1),
        (Face::Up, 2),
        (Face::South, 4),
        (Face::West, 8),
        (Face::North, 16),
        (Face::East, 32),
    ];
    match entry.string_id.as_str() {
        "minecraft:vine" => Some(("vine_direction_bits", &VINE)),
        "minecraft:glow_lichen" | "minecraft:sculk_vein" | "minecraft:resin_clump" => {
            Some(("multi_face_direction_bits", &MULTI))
        }
        _ => None,
    }
}

/// What a multi-face block does when the block on `from` changes: a face
/// on that side whose holder is gone comes off (`Some(Some(state))`), and
/// the block pops (`Some(None)`) when no face is left, a block without faces
/// at its first neighbour change. `None` when nothing happens.
pub(crate) fn multiface_after(
    blocks: &impl BlockSource,
    pos: IVec3,
    entry: &BlockEntry,
    state: u32,
    from: Face,
) -> Option<Option<u32>> {
    let (property, faces) = multiface_bits(entry)?;
    let bits = int(entry, state, property)?;
    if bits == 0 {
        return Some(None);
    }
    let &(face, bit) = faces.iter().find(|(face, _)| *face == from)?;
    if bits & bit == 0 {
        return None;
    }
    let held = blocks
        .block_at(pos + face.offset())
        .is_none_or(|holder| holds(blocks, holder, face.opposite(), None));
    if held {
        return None;
    }
    let rest = bits & !bit;
    Some((rest != 0).then(|| {
        entry
            .with_state_value(state, property, StateValue::Int(rest))
            .unwrap_or(state)
    }))
}

/// The measured holders of a rail (the floor under it; a sloped rail's
/// rising side is held the same way).
const RAIL_HOLDER: &str = "minecraft:rail/down";

/// The plant whose measured soils a plant uses.
fn soil_plant(id: &str) -> Option<&'static str> {
    let name = id.strip_prefix("minecraft:")?;
    Some(match name {
        "dandelion" | "golden_dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet"
        | "red_tulip" | "orange_tulip" | "white_tulip" | "pink_tulip" | "oxeye_daisy"
        | "cornflower" | "lily_of_the_valley" | "sunflower" | "lilac" | "rose_bush" | "peony"
        | "pitcher_plant" => "minecraft:dandelion",
        "torchflower" => "minecraft:torchflower",
        "open_eyeblossom" | "closed_eyeblossom" => "minecraft:closed_eyeblossom",
        "short_grass" | "fern" | "tall_grass" | "large_fern" => "minecraft:short_grass",
        "deadbush" => "minecraft:deadbush",
        "short_dry_grass" | "tall_dry_grass" => "minecraft:short_dry_grass",
        "bush" => "minecraft:bush",
        "firefly_bush" => "minecraft:firefly_bush",
        "azalea" | "flowering_azalea" => "minecraft:azalea",
        "crimson_fungus" | "warped_fungus" => "minecraft:crimson_fungus",
        "crimson_roots" | "warped_roots" => "minecraft:crimson_roots",
        "nether_sprouts" => "minecraft:nether_sprouts",
        "sweet_berry_bush" => "minecraft:sweet_berry_bush",
        "wheat" | "carrots" | "potatoes" | "beetroot" | "torchflower_crop" => "minecraft:wheat",
        "melon_stem" | "pumpkin_stem" => "minecraft:melon_stem",
        "nether_wart" => "minecraft:nether_wart",
        "cactus" => "minecraft:cactus",
        "bamboo" | "bamboo_sapling" => "minecraft:bamboo",
        "pink_petals" | "wildflowers" => "minecraft:pink_petals",
        "mangrove_propagule" => "minecraft:mangrove_propagule",
        "waterlily" => "minecraft:waterlily",
        "wither_rose" => "minecraft:wither_rose",
        "red_shrub" => "minecraft:red_shrub",
        "cactus_flower" => "minecraft:cactus_flower",
        "frog_spawn" => "minecraft:frog_spawn",
        _ if name.ends_with("_sapling") => "minecraft:oak_sapling",
        _ => return None,
    })
}

/// What `state` of `entry` needs to stay.
pub(crate) fn needs(entry: &BlockEntry, state: u32) -> Needs {
    if entry.data.falling {
        return Needs::Nothing;
    }
    let id = entry.string_id.as_str();
    // Doors and tall plants: each half needs the other, the lower half its
    // floor (`halves.json`).
    if let Some(upper) = byte(entry, state, "upper_block_bit") {
        let floor = if entry.state_value(state, "door_hinge_bit").is_some() {
            Floor::Present
        } else {
            match soil_plant(id) {
                Some(plant) => Floor::Soil(plant),
                None => return Needs::Nothing,
            }
        };
        return if upper {
            Needs::Half {
                other: Face::Down,
                floor: None,
            }
        } else {
            Needs::Half {
                other: Face::Up,
                floor: Some(floor),
            }
        };
    }
    if let Some(direction) = text(entry, state, "torch_facing_direction") {
        // The value names the side the torch is fixed to; `top` stands on
        // the block below. The technical `unknown` value is left alone.
        let face = match direction {
            "top" => Face::Down,
            other => match Face::from_name(other) {
                Some(face) => face,
                None => return Needs::Nothing,
            },
        };
        return class_on(
            face,
            Some("minecraft:torch/down"),
            Some("minecraft:torch/east"),
            None,
        );
    }
    let facing = || {
        int(entry, state, "facing_direction")
            .and_then(Face::from_facing)
            .map(Face::opposite)
    };
    if id.ends_with("_button") {
        return facing().map_or(Needs::Nothing, |face| {
            class_on(
                face,
                Some("minecraft:stone_button/down"),
                Some("minecraft:stone_button/east"),
                Some("minecraft:stone_button/up"),
            )
        });
    }
    let wall_class = match id {
        "minecraft:ladder" => Some("minecraft:ladder/east"),
        "minecraft:frame" | "minecraft:glow_frame" => Some("minecraft:frame/east"),
        "minecraft:wall_banner" => Some("minecraft:wall_banner/east"),
        _ if id.ends_with("wall_sign") => Some("minecraft:wall_sign/east"),
        _ => None,
    };
    if let Some(wall) = wall_class {
        return facing().map_or(Needs::Nothing, |face| {
            class_on(face, None, Some(wall), None)
        });
    }
    if let Some(direction) = text(entry, state, "lever_direction") {
        let face = if direction.starts_with("up_") {
            Face::Down
        } else if direction.starts_with("down_") {
            Face::Up
        } else {
            Face::from_name(direction).map_or(Face::Down, Face::opposite)
        };
        return class_on(
            face,
            Some("minecraft:lever/down"),
            Some("minecraft:lever/east"),
            None,
        );
    }
    if let Some(face) = text(entry, state, "minecraft:block_face").and_then(Face::from_name) {
        return class_on(
            face.opposite(),
            Some("minecraft:small_amethyst_bud/down"),
            Some("minecraft:small_amethyst_bud/east"),
            None,
        );
    }
    if id.starts_with("minecraft:dead_")
        && let Some(direction) = int(entry, state, "coral_direction")
    {
        let face = [Face::East, Face::West, Face::South, Face::North]
            .get(usize::try_from(direction).unwrap_or(usize::MAX))
            .copied();
        return face.map_or(Needs::Nothing, |face| {
            class_on(
                face,
                None,
                Some("minecraft:dead_brain_coral_wall_fan/east"),
                None,
            )
        });
    }
    if id == "minecraft:tripwire_hook" {
        let face = [Face::North, Face::East, Face::South, Face::West]
            .get(
                usize::try_from(int(entry, state, "direction").unwrap_or(-1)).unwrap_or(usize::MAX),
            )
            .copied();
        return face.map_or(Needs::Nothing, |face| {
            class_on(face, None, Some("minecraft:tripwire_hook/east"), None)
        });
    }
    if id.ends_with("lantern")
        && let Some(hanging) = byte(entry, state, "hanging")
    {
        return if hanging {
            class_on(Face::Up, None, None, Some("minecraft:lantern/up"))
        } else {
            class_on(Face::Down, Some("minecraft:lantern/down"), None, None)
        };
    }
    if id.ends_with("_hanging_sign") {
        return if byte(entry, state, "hanging") == Some(true) {
            class_on(Face::Up, None, None, Some("minecraft:oak_hanging_sign/up"))
        } else {
            Needs::Nothing
        };
    }
    if id == "minecraft:bell" {
        return match text(entry, state, "attachment") {
            Some("standing") => class_on(Face::Down, Some("minecraft:bell/down"), None, None),
            Some("hanging") => class_on(Face::Up, None, None, None),
            _ => Needs::Nothing,
        };
    }
    let floor_class = match id {
        "minecraft:moss_carpet" => Some("minecraft:white_carpet/down"),
        "minecraft:redstone_wire" => Some("minecraft:redstone_wire/down"),
        "minecraft:unpowered_repeater"
        | "minecraft:powered_repeater"
        | "minecraft:unpowered_comparator"
        | "minecraft:powered_comparator" => Some("minecraft:unpowered_repeater/down"),
        "minecraft:standing_banner" => Some("minecraft:standing_banner/down"),
        "minecraft:cake" => Some("minecraft:cake/down"),
        "minecraft:flower_pot" => Some("minecraft:flower_pot/down"),
        "minecraft:sea_pickle" => Some("minecraft:sea_pickle/down"),
        "minecraft:leaf_litter" => Some("minecraft:leaf_litter/down"),

        _ if id.ends_with("_carpet") && id != "minecraft:pale_moss_carpet" => {
            Some("minecraft:white_carpet/down")
        }
        _ if id.ends_with("_pressure_plate") => Some("minecraft:stone_pressure_plate/down"),

        _ if id.ends_with("standing_sign") => Some("minecraft:standing_sign/down"),
        _ if id.ends_with("candle_cake") => Some("minecraft:cake/down"),
        _ if id.starts_with("minecraft:dead_") && id.ends_with("_coral_fan") => {
            Some("minecraft:dead_brain_coral_fan/down")
        }
        _ => None,
    };
    if let Some(entry_kind) = super::rails::rail_kind(entry).map(|_| entry) {
        let slope = int(entry_kind, state, "rail_direction")
            .and_then(super::rails::slope)
            .and_then(|offset| {
                [Face::East, Face::West, Face::North, Face::South]
                    .into_iter()
                    .find(|face| face.offset() == offset)
            });
        return Needs::Rail { slope };
    }
    if let Some(class) = floor_class {
        return class_on(Face::Down, Some(class), None, None);
    }
    match id {
        "minecraft:hanging_roots" => {
            return class_on(Face::Up, None, None, Some("minecraft:hanging_roots/up"));
        }
        "minecraft:spore_blossom" => {
            return class_on(Face::Up, None, None, Some("minecraft:spore_blossom/up"));
        }
        "minecraft:cave_vines"
        | "minecraft:cave_vines_body_with_berries"
        | "minecraft:cave_vines_head_with_berries"
        | "minecraft:weeping_vines"
        | "minecraft:pale_hanging_moss" => return class_on(Face::Up, None, None, None),
        _ if id.starts_with("minecraft:dead_") && id.ends_with("_coral") => {
            return class_on(Face::Down, None, None, None);
        }
        _ => {}
    }
    // A hanging mangrove propagule hangs from leaves, which no sweep covered.
    if byte(entry, state, "hanging") == Some(true) {
        return Needs::Nothing;
    }
    soil_plant(id).map_or(Needs::Nothing, |plant| Needs::Soil { plant })
}

/// Whether `state` of `entry` may be placed at `pos`: BDS refuses a block
/// that could not stay there (a sapling on stone; `testdata/bds-placements.json`).
/// A door or tall plant is checked for its floor only, its upper half being
/// placed with it.
pub(crate) fn can_place(
    blocks: &impl BlockSource,
    pos: IVec3,
    entry: &BlockEntry,
    state: u32,
) -> bool {
    match needs(entry, state) {
        Needs::Half {
            floor: Some(floor), ..
        } => match floor {
            Floor::Present => blocks
                .block_at(pos + IVec3::NEG_Y)
                .is_none_or(|below| !is_air(blocks, below)),
            Floor::Soil(plant) => blocks
                .block_at(pos + IVec3::NEG_Y)
                .is_none_or(|below| soil_holds(blocks, plant, below)),
        },
        Needs::Half { .. } => true,
        _ => stays(blocks, pos, entry, state),
    }
}

/// What triggered a support check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Check {
    /// No check (a neighbour only re-derived its state).
    None,
    /// The block was just placed.
    Placed,
    /// The neighbour on this face was placed or removed.
    Neighbour(Face),
}

/// Whether `state` of `entry` at `pos` stays after `check` (BDS 1.26.51:
/// attached blocks are checked when placed and when the block on their
/// support side changes, so an unsupported amethyst cluster next to a newly
/// placed frame stays; plants and door halves on any neighbour change but
/// not when placed).
pub(crate) fn stays_after(
    blocks: &impl BlockSource,
    pos: IVec3,
    entry: &BlockEntry,
    state: u32,
    check: Check,
) -> bool {
    let needs = needs(entry, state);
    let checked = match (check, needs) {
        (Check::None, _) | (_, Needs::Nothing) => false,
        (Check::Placed, Needs::Holder { .. } | Needs::Rail { .. }) => true,
        (Check::Neighbour(from), Needs::Rail { slope }) => {
            from == Face::Down || Some(from) == slope
        }
        (Check::Placed, _) => false,
        (Check::Neighbour(from), Needs::Holder { face, .. }) => from == face,
        // A tall plant half checks itself only when the block above or below
        // changes: a lever placed beside a lone lower half of a large fern
        // leaves it standing (`junctions`/`support` scenarios).
        (
            Check::Neighbour(from),
            Needs::Half {
                floor: Some(Floor::Soil(_)),
                ..
            },
        ) => matches!(from, Face::Up | Face::Down),
        (Check::Neighbour(_), _) => true,
    };
    !checked || stays(blocks, pos, entry, state)
}

/// Whether `state` of `entry` at `pos` has what it needs to stay.
pub(crate) fn stays(blocks: &impl BlockSource, pos: IVec3, entry: &BlockEntry, state: u32) -> bool {
    let present = |at: IVec3| {
        blocks
            .block_at(at)
            .is_none_or(|holder| !is_air(blocks, holder))
    };
    let soil = |plant: &str| {
        blocks
            .block_at(pos + IVec3::NEG_Y)
            .is_none_or(|below| soil_holds(blocks, plant, below))
    };
    match needs(entry, state) {
        Needs::Nothing => true,
        Needs::Holder { face, class } => blocks
            .block_at(pos + face.offset())
            .is_none_or(|holder| holds(blocks, holder, face.opposite(), class)),
        Needs::Soil { plant } => soil(plant),
        Needs::Rail { slope } => [Some(Face::Down), slope].into_iter().flatten().all(|face| {
            blocks
                .block_at(pos + face.offset())
                .is_none_or(|holder| holds(blocks, holder, Face::Up, Some(RAIL_HOLDER)))
        }),
        Needs::Half { other, floor } => {
            let upper = byte(entry, state, "upper_block_bit");
            let partner = blocks.block_at(pos + other.offset()).is_none_or(|partner| {
                (entry.min_state_id..=entry.max_state_id).contains(&partner)
                    && byte(entry, partner, "upper_block_bit") != upper
            });
            partner
                && match floor {
                    None => true,
                    Some(Floor::Present) => present(pos + IVec3::NEG_Y),
                    Some(Floor::Soil(plant)) => soil(plant),
                }
        }
    }
}

/// Whether `plant` stays on the block `below`: a soil all of whose measured
/// states hold it, or one of the measured states that do (a lily pad on a
/// water source).
fn soil_holds(blocks: &impl BlockSource, plant: &str, below: u32) -> bool {
    let Some(entry) = blocks.registry().get_by_runtime_id(below) else {
        return false;
    };
    let name = entry.string_id.as_str();
    plant_soils()
        .get(plant)
        .is_some_and(|soils| soils.contains(name))
        || plant_soil_states().get(plant).is_some_and(|states| {
            states
                .iter()
                .any(|(block, values)| block == name && matches_states(entry, below, values))
        })
}

fn is_air(blocks: &impl BlockSource, state: u32) -> bool {
    blocks
        .registry()
        .get_by_runtime_id(state)
        .is_none_or(|entry| entry.string_id == "minecraft:air")
}

/// Whether `holder` holds an attached block of `class` fixed to its `face`.
pub(crate) fn holds(
    blocks: &impl BlockSource,
    holder: u32,
    face: Face,
    class: Option<&str>,
) -> bool {
    let Some(entry) = blocks.registry().get_by_runtime_id(holder) else {
        return false;
    };
    let id = entry.string_id.as_str();
    let Some(class) = class.and_then(|key| support_holders().classes.get(key)) else {
        return id != "minecraft:air";
    };
    if class.never.contains(id) {
        return false;
    }
    if class.always.contains(id) {
        return true;
    }
    let listed = |list: &[(String, serde_json::Map<String, serde_json::Value>)]| {
        list.iter()
            .any(|(block, states)| block == id && matches_states(entry, holder, states))
    };
    if listed(&class.never_states) {
        return false;
    }
    if listed(&class.always_states) {
        return true;
    }
    match class.rule {
        HolderRule::Present => id != "minecraft:air",
        HolderRule::Collision => block_collision_aabbs(holder).next().is_some(),
        HolderRule::FaceFull => face_covered(holder, face, [0.0, 1.0, 0.0, 1.0]),
        HolderRule::FaceCentre => face_covered(
            holder,
            face,
            [7.0 / 16.0, 9.0 / 16.0, 7.0 / 16.0, 9.0 / 16.0],
        ),
    }
}

/// Whether `state` of `entry` has every measured property value of `states`
/// (booleans are byte properties).
fn matches_states(
    entry: &BlockEntry,
    state: u32,
    states: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    states.iter().all(
        |(property, value)| match (entry.state_value(state, property), value) {
            (Some(StateValue::Byte(b)), serde_json::Value::Bool(v)) => (b != 0) == *v,
            (Some(StateValue::Int(i)), serde_json::Value::Number(v)) => {
                v.as_i64() == Some(i64::from(i))
            }
            (Some(StateValue::Str(s)), serde_json::Value::String(v)) => s == v,
            _ => false,
        },
    )
}

/// Whether the collision boxes of `state` cover `area` (`[u0, u1, v0, v1]`)
/// of the block's `face`: x/z on the top and bottom, z/y on the west and
/// east faces, x/y on the north and south faces.
pub(crate) fn face_covered(state: u32, face: Face, area: [f64; 4]) -> bool {
    let rects: Vec<[f64; 4]> = block_collision_aabbs(state)
        .filter_map(|b| {
            let (min, max) = (b.min, b.max);
            let rect = match face {
                Face::Up if max.y >= 1.0 => [min.x, max.x, min.z, max.z],
                Face::Down if min.y <= 0.0 => [min.x, max.x, min.z, max.z],
                Face::West if min.x <= 0.0 => [min.z, max.z, min.y, max.y],
                Face::East if max.x >= 1.0 => [min.z, max.z, min.y, max.y],
                Face::North if min.z <= 0.0 => [min.x, max.x, min.y, max.y],
                Face::South if max.z >= 1.0 => [min.x, max.x, min.y, max.y],
                _ => return None,
            };
            Some(rect.map(|v| v.clamp(0.0, 1.0)))
        })
        .collect();
    let [u0, u1, v0, v1] = area;
    let cuts = |lo: f64, hi: f64, a: usize, b: usize| {
        let mut cuts: Vec<f64> = rects
            .iter()
            .flat_map(|r| [r[a].clamp(lo, hi), r[b].clamp(lo, hi)])
            .chain([lo, hi])
            .collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        cuts
    };
    let (us, vs) = (cuts(u0, u1, 0, 1), cuts(v0, v1, 2, 3));
    us.windows(2).filter(|u| u[1] > u[0]).all(|u| {
        vs.windows(2).filter(|v| v[1] > v[0]).all(|v| {
            rects
                .iter()
                .any(|r| r[0] <= u[0] && u[1] <= r[1] && r[2] <= v[0] && v[1] <= r[3])
        })
    })
}
