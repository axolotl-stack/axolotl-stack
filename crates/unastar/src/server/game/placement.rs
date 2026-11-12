//! Block placement rules: target position, placed state and obstruction.
//!
//! Compared with BDS 1.26.51 click by click (`testdata/bds-placements.json`,
//! replayed by `placement_capture_tests`):
//!
//! - A replaceable clicked block (air, liquids, fire, grass, ferns, vines,
//!   snow layers below a full stack, ...) is replaced in place; otherwise the
//!   block goes next to the clicked face, which must be replaceable.
//! - States from the click: `pillar_axis` from the clicked face's axis; slabs
//!   on the top or bottom half (upper half of a side face or the underside of
//!   a block gives a top slab) and merge into a double slab; stairs face where
//!   the player looks and flip on the upper half; trapdoors take their
//!   direction from the look and their half from the click on any face;
//!   `minecraft:cardinal_direction` blocks face the player, except doors (a
//!   quarter turn clockwise from the look), anvils (counter-clockwise) and
//!   campfires and straw beds (the look); pistons and observers face the look
//!   in 3D, dispensers, droppers and barrels the player; hoppers point into
//!   the block they were placed against (down from a top or bottom face);
//!   buttons, lightning rods, frames, ladders, amethyst buds and levers take
//!   the clicked face (a lever on a floor or ceiling lies along the look); an
//!   end rod on a side face points into its block; torches attach to the
//!   clicked face (never under a block; `torch_facing_direction` names the
//!   side the torch is fixed to); lanterns and hanging signs hang under a
//!   block; signs stand on a top face in sixteenths of a turn or hang on a
//!   wall; doors and beds take two blocks.
//! - Round 2 (`testdata/bds-placements-*.json`): door hinges from the
//!   neighbouring doors and solid blocks (on the upper half); coral fans on
//!   floors and walls, dead out of water; skulls, bells, grindstones, shelf
//!   mushrooms, beehives, bee nests, looms, chiseled bookshelves, decorated
//!   pots, crafters and calibrated sculk sensors oriented as measured; the
//!   faces of vines, glow lichen, sculk veins and resin clumps; candles,
//!   snow layers, sea pickles, turtle eggs and petals growing when placed
//!   into again, candles onto cakes; planting items; persistent leaves.
//! - A block that could not stay where it would go is refused (support and
//!   soil, see `block_update::support`), as is a bed whose halves would not
//!   both stand on a block.
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
use crate::server::game::block_update::support::{Face, face_covered};

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

    pub(crate) fn cardinal(self) -> &'static str {
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

    /// The next direction clockwise seen from above (south -> west).
    fn clockwise(self) -> Self {
        match self {
            Self::South => Self::West,
            Self::West => Self::North,
            Self::North => Self::East,
            Self::East => Self::South,
        }
    }

    /// Legacy `direction` ordinal (0 south, 1 west, 2 north, 3 east).
    fn legacy(self) -> i32 {
        match self {
            Self::South => 0,
            Self::West => 1,
            Self::North => 2,
            Self::East => 3,
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
    /// The placer's eye position.
    pub eye: DVec3,
}

/// Where and what to place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub position: IVec3,
    pub state: u32,
    /// Second block (upper door half).
    pub upper: Option<(IVec3, u32)>,
}

/// The standing sign a sign item places: `oak_sign` -> `standing_sign`,
/// `dark_oak_sign` -> `darkoak_standing_sign`, `spruce_sign` ->
/// `spruce_standing_sign`. Clicked on a side face it places the matching wall
/// sign instead ([`wall_sign`]).
pub fn sign_block<'a>(registry: &'a BlockRegistry, item: &str) -> Option<&'a BlockEntry> {
    let wood = item.strip_prefix("minecraft:")?.strip_suffix("_sign")?;
    let prefix = match wood {
        "oak" => String::new(),
        "dark_oak" => "darkoak_".to_owned(),
        _ => format!("{wood}_"),
    };
    registry.get_by_name(&format!("minecraft:{prefix}standing_sign"))
}

/// The block an item places when no block shares its name (measured on BDS
/// 1.26.51, `testdata/bds-placements.json`): redstone dust the wire,
/// repeaters and comparators their unpowered blocks, sign items their
/// standing sign ([`sign_block`]).
pub fn item_placed_block<'a>(registry: &'a BlockRegistry, item: &str) -> Option<&'a BlockEntry> {
    let block = match item {
        "minecraft:redstone" => "minecraft:redstone_wire",
        "minecraft:repeater" => "minecraft:unpowered_repeater",
        "minecraft:comparator" => "minecraft:unpowered_comparator",
        // Measured in round 2 (`testdata/bds-placements-crops.json`,
        // `-actors.json`): the item plants or places this block.
        "minecraft:wheat_seeds" => "minecraft:wheat",
        "minecraft:carrot" => "minecraft:carrots",
        "minecraft:potato" => "minecraft:potatoes",
        "minecraft:beetroot_seeds" => "minecraft:beetroot",
        "minecraft:melon_seeds" => "minecraft:melon_stem",
        "minecraft:pumpkin_seeds" => "minecraft:pumpkin_stem",
        "minecraft:torchflower_seeds" => "minecraft:torchflower_crop",
        "minecraft:pitcher_pod" => "minecraft:pitcher_crop",
        "minecraft:sweet_berries" => "minecraft:sweet_berry_bush",
        "minecraft:glow_berries" => "minecraft:cave_vines",
        "minecraft:sugar_cane" => "minecraft:reeds",
        "minecraft:string" => "minecraft:trip_wire",
        "minecraft:cocoa_beans" => "minecraft:cocoa",
        "minecraft:banner" => "minecraft:standing_banner",
        _ => return sign_block(registry, item),
    };
    registry.get_by_name(block)
}

/// The block an item places where a block of the item's name exists but the
/// item places another (BDS 1.26.51: the bamboo item plants a bamboo
/// sapling).
pub fn item_block_override(item: &str) -> Option<&'static str> {
    match item {
        "minecraft:bamboo" => Some("minecraft:bamboo_sapling"),
        _ => None,
    }
}

/// The wall sign of a standing sign block.
pub fn wall_sign<'a>(registry: &'a BlockRegistry, standing: &BlockEntry) -> Option<&'a BlockEntry> {
    let prefix = standing.string_id.strip_suffix("standing_sign")?;
    registry.get_by_name(&format!("{prefix}wall_sign"))
}

/// Whether `placed` is a block the item placing `held` may put down: the
/// held block itself, its wall variant (signs, banners, coral fans), a coral
/// fan's dead form, or the candle cake of a candle.
pub fn places_as(held: &BlockEntry, placed: &BlockEntry) -> bool {
    let (held, placed) = (held.string_id.as_str(), placed.string_id.as_str());
    let alive = |id: &str| {
        id.strip_prefix("minecraft:dead_")
            .map(|rest| format!("minecraft:{rest}"))
    };
    let fan_floor = |id: &str| id.replace("_coral_wall_fan", "_coral_fan");
    placed == held
        || held
            .strip_suffix("standing_sign")
            .is_some_and(|prefix| placed.strip_suffix("wall_sign") == Some(prefix))
        || (held == "minecraft:standing_banner" && placed == "minecraft:wall_banner")
        || (held.ends_with("_coral_fan")
            && [placed.to_owned(), alive(placed).unwrap_or_default()]
                .iter()
                .any(|id| {
                    fan_floor(id) == held || fan_floor(id) == alive(held).unwrap_or_default()
                }))
        || (held.ends_with("candle") && placed == format!("{held}_cake"))
}

/// The wall variant of a block placed on a side face: wall signs, wall
/// banners and coral wall fans.
fn wall_variant<'a>(registry: &'a BlockRegistry, held: &BlockEntry) -> Option<&'a BlockEntry> {
    if held.string_id == "minecraft:standing_banner" {
        return registry.get_by_name("minecraft:wall_banner");
    }
    if let Some(prefix) = held.string_id.strip_suffix("_coral_fan") {
        return registry.get_by_name(&format!("{prefix}_coral_wall_fan"));
    }
    wall_sign(registry, held)
}

/// The dead form of a live coral fan (BDS 1.26.51 places a live fan out of
/// water already dead; `testdata/bds-placements-coral.json`).
fn dead_coral<'a>(registry: &'a BlockRegistry, block: &BlockEntry) -> Option<&'a BlockEntry> {
    let name = block.string_id.strip_prefix("minecraft:")?;
    if name.starts_with("dead_") || !name.contains("_coral_") || !name.contains("fan") {
        return None;
    }
    registry.get_by_name(&format!("minecraft:dead_{name}"))
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

/// The double-slab block of a slab: `oak_slab` -> `oak_double_slab`, and
/// for cut copper `waxed_cut_copper_slab` -> `waxed_double_cut_copper_slab`
/// (the pairs of the BDS drop sweep: each double slab drops two of its slab).
pub fn double_slab<'a>(registry: &'a BlockRegistry, slab: &BlockEntry) -> Option<&'a BlockEntry> {
    let base = slab.string_id.strip_suffix("_slab")?;
    registry
        .get_by_name(&format!("{base}_double_slab"))
        .or_else(|| {
            let prefix = base.strip_suffix("cut_copper")?;
            registry.get_by_name(&format!("{prefix}double_cut_copper_slab"))
        })
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
    if block
        .state_value(state, "minecraft:cardinal_direction")
        .is_some()
    {
        // Measured on BDS 1.26.51 (`testdata/bds-placements.json`): most
        // blocks face the player; a door turns a quarter clockwise from the
        // look, an anvil a quarter counter-clockwise, a campfire and a straw
        // bed (its head goes that way) face away.
        let id = block.string_id.as_str();
        let cardinal = if block.state_value(state, "door_hinge_bit").is_some() {
            looking.clockwise()
        } else if id.ends_with("anvil") {
            looking.clockwise().opposite()
        } else if id.ends_with("campfire")
            || id == "minecraft:calibrated_sculk_sensor"
            || block.state_value(state, "head_piece_bit").is_some()
        {
            looking
        } else {
            looking.opposite()
        };
        state = set(
            block,
            state,
            "minecraft:cardinal_direction",
            StateValue::Str(cardinal.cardinal()),
        );
    }
    // Trapdoors take their direction from the look and their half from the
    // click, whichever face was clicked.
    if block.has_tag("trapdoors") {
        state = set(
            block,
            state,
            "direction",
            StateValue::Int(looking.opposite().weirdo()),
        );
        state = set(
            block,
            state,
            "upside_down_bit",
            StateValue::Byte(u8::from(upper_half(input))),
        );
    }
    if block.string_id.ends_with("glazed_terracotta") {
        state = set(
            block,
            state,
            "facing_direction",
            StateValue::Int(looking.opposite().facing()),
        );
    }
    // A bed's foot takes the look direction (legacy `direction`); its head
    // goes one block further that way ([`second_block`]).
    if block.state_value(state, "head_piece_bit").is_some() {
        state = set(block, state, "direction", StateValue::Int(looking.legacy()));
    }
    // Measured: a hanging sign hangs under a block facing the player
    // ([`resolve_placement`] turns it in sixteenths under a narrow block); on
    // a side face it sticks out of the wall along it, facing away from the
    // side the player looks toward (east or south when the look runs straight
    // into the wall); it does not go on a top face.
    if block.string_id.ends_with("_hanging_sign") {
        let (hanging, facing) = match input.face {
            0 => (1, looking.opposite().facing()),
            1 => return None,
            face => (0, wall_hanging_facing(face, input.yaw)),
        };
        state = set(block, state, "hanging", StateValue::Byte(hanging));
        state = set(block, state, "facing_direction", StateValue::Int(facing));
    }
    // Buttons, lightning rods and item frames point out of the clicked face;
    // an end rod on a side face takes the opposite side (on BDS an end rod on
    // a block's north face is `facing_direction` 3).
    if block.string_id.ends_with("_button")
        || matches!(
            block.string_id.as_str(),
            "minecraft:frame" | "minecraft:glow_frame"
        )
        || block.string_id.ends_with("lightning_rod")
    {
        state = set(
            block,
            state,
            "facing_direction",
            StateValue::Int(i32::from(input.face)),
        );
    }
    if block.string_id == "minecraft:end_rod" {
        let facing = match input.face {
            0 | 1 => i32::from(input.face),
            face => opposite_facing(i32::from(face)),
        };
        state = set(block, state, "facing_direction", StateValue::Int(facing));
    }
    // Amethyst buds and clusters grow out of the clicked face.
    if block.state_value(state, "minecraft:block_face").is_some() {
        let face = match input.face {
            0 => "down",
            1 => "up",
            2 => "north",
            3 => "south",
            4 => "west",
            _ => "east",
        };
        state = set(block, state, "minecraft:block_face", StateValue::Str(face));
    }
    // Bells and grindstones hang under a block, stand on one or hang on a
    // wall (a bell with a block behind it too: `multiple`, set in
    // [`resolve_placement`]); on a wall they face out of it, otherwise the
    // player.
    if matches!(
        block.string_id.as_str(),
        "minecraft:bell" | "minecraft:grindstone"
    ) {
        let (attachment, direction) = match input.face {
            0 => ("hanging", looking.opposite().legacy()),
            1 => ("standing", looking.opposite().legacy()),
            face => ("side", horizontal_of_face(face)?.legacy()),
        };
        state = set(block, state, "attachment", StateValue::Str(attachment));
        state = set(block, state, "direction", StateValue::Int(direction));
    }
    // Measured (round 2, `testdata/bds-placements-directions.json`): these
    // face the player (legacy `direction`).
    if matches!(
        block.string_id.as_str(),
        "minecraft:bee_nest"
            | "minecraft:beehive"
            | "minecraft:loom"
            | "minecraft:chiseled_bookshelf"
            | "minecraft:decorated_pot"
    ) {
        state = set(
            block,
            state,
            "direction",
            StateValue::Int(looking.opposite().legacy()),
        );
    }
    // A crafter faces the player with its top up, whatever the pitch.
    if block.string_id == "minecraft:crafter" {
        let orientation = match looking.opposite() {
            Horizontal::South => "south_up",
            Horizontal::West => "west_up",
            Horizontal::North => "north_up",
            Horizontal::East => "east_up",
        };
        state = block
            .with_state_value(state, "orientation", StateValue::Str(orientation))
            .unwrap_or(state);
    }
    // A shelf mushroom only goes on a side face and faces out of it.
    if block.string_id == "minecraft:shelf_mushroom" {
        let side = horizontal_of_face(input.face)?;
        state = set(
            block,
            state,
            "minecraft:cardinal_direction",
            StateValue::Str(side.cardinal()),
        );
    }
    // Skulls stand on a top face (their actor keeps the rotation) and hang
    // on side faces; not under a block.
    if block.string_id.ends_with("_skull") || block.string_id.ends_with("_head") {
        if input.face == 0 {
            return None;
        }
        state = set(
            block,
            state,
            "facing_direction",
            StateValue::Int(i32::from(input.face)),
        );
    }
    // Floor coral fans: 0 looking south, 1 every other way (BDS 1.26.51,
    // four looks measured); wall fans name the side they hang from; coral
    // plants and fans do not go under a block, plants not on walls.
    if block.string_id.ends_with("_coral_fan") {
        if input.face != 1 {
            return None;
        }
        // 0 for a placer north of the block, 1 east, south or west of it,
        // whatever the look (BDS 1.26.51: 18 looks from the north all gave 0).
        let to_player = input.eye - (input.clicked + IVec3::Y).as_dvec3() - DVec3::splat(0.5);
        let north = to_player.z < 0.0 && to_player.z.abs() >= to_player.x.abs();
        state = set(
            block,
            state,
            "coral_fan_direction",
            StateValue::Int(i32::from(!north)),
        );
    }
    if block.string_id.ends_with("_coral_wall_fan") {
        let direction = match input.face {
            4 => 0,
            5 => 1,
            2 => 2,
            3 => 3,
            _ => return None,
        };
        state = set(block, state, "coral_direction", StateValue::Int(direction));
    }
    if block.string_id.ends_with("_coral") && input.face != 1 {
        return None;
    }
    // A mushroom stem is placed with its item's faces (all stem).
    if block.string_id == "minecraft:mushroom_stem" {
        state = set(block, state, "huge_mushroom_bits", StateValue::Int(15));
    }
    // Leaves placed by a player never decay.
    if block.state_value(state, "persistent_bit").is_some() && block.string_id.contains("leaves") {
        state = set(block, state, "persistent_bit", StateValue::Byte(1));
    }
    // A standing banner faces the player in sixteenths of a turn like a sign,
    // a wall banner out of its wall; neither goes under a block.
    if block.string_id == "minecraft:standing_banner" {
        if input.face == 0 {
            return None;
        }
        state = set(
            block,
            state,
            "ground_sign_direction",
            StateValue::Int(sign_direction(input.yaw)),
        );
    }
    if block.string_id == "minecraft:wall_banner" {
        state = set(
            block,
            state,
            "facing_direction",
            StateValue::Int(i32::from(input.face)),
        );
    }
    // Vines, glow lichen, sculk veins and resin clumps cover the face they
    // were placed against (vines only walls).
    if let Some((property, faces)) =
        crate::server::game::block_update::support::multiface_bits(block)
    {
        let side = attach_face(input.face)?;
        let &(_, bit) = faces.iter().find(|(face, _)| *face == side)?;
        state = set(block, state, property, StateValue::Int(bit));
    }
    // Cocoa only goes on a side face; `direction` points at the log.
    if block.string_id == "minecraft:cocoa" {
        let toward = horizontal_of_face(input.face)?.opposite();
        state = set(block, state, "direction", StateValue::Int(toward.legacy()));
    }
    if block.string_id == "minecraft:observer" {
        let facing = match look_facing(input.yaw, input.pitch) {
            0 => "down",
            1 => "up",
            _ => looking.cardinal(),
        };
        state = set(
            block,
            state,
            "minecraft:facing_direction",
            StateValue::Str(facing),
        );
    }
    if let Some(StateValue::Str(_)) = block.state_value(state, "torch_facing_direction") {
        // The value names the side the torch is fixed to (BDS 1.26.51: a
        // torch `east` falls when the block east of it goes;
        // `measured/block_updates/support.json`), so a click on a block's
        // west face gives `east`.
        let facing = match input.face {
            1 => "top",
            2 => "south",
            3 => "north",
            4 => "east",
            5 => "west",
            _ => return None,
        };
        state = set(
            block,
            state,
            "torch_facing_direction",
            StateValue::Str(facing),
        );
    }
    // A lantern clicked onto the underside of a block hangs from it.
    if block.string_id.ends_with("lantern") && block.state_value(state, "hanging").is_some() {
        state = set(
            block,
            state,
            "hanging",
            StateValue::Byte(u8::from(input.face == 0)),
        );
    }
    // Measured: a standing sign faces the player in sixteenths of a turn
    // (looking south gives 8); a wall sign faces out of the clicked face; a
    // sign does not go under a block.
    if block.string_id.ends_with("standing_sign") {
        if input.face == 0 {
            return None;
        }
        state = set(
            block,
            state,
            "ground_sign_direction",
            StateValue::Int(sign_direction(input.yaw)),
        );
    }
    if block.string_id.ends_with("wall_sign") {
        state = set(
            block,
            state,
            "facing_direction",
            StateValue::Int(i32::from(input.face)),
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
        // A lever names the clicked face; on a top or bottom face its axis
        // follows the look (`up_north_south` looking south).
        "minecraft:lever" => {
            let along_z = matches!(looking, Horizontal::North | Horizontal::South);
            let direction = match (input.face, along_z) {
                (0, true) => "down_north_south",
                (0, false) => "down_east_west",
                (1, true) => "up_north_south",
                (1, false) => "up_east_west",
                (2, _) => "north",
                (3, _) => "south",
                (4, _) => "west",
                _ => "east",
            };
            state = set(block, state, "lever_direction", StateValue::Str(direction));
        }
        // A tripwire hook only goes on a side face and names that side.
        "minecraft:tripwire_hook" => {
            let side = match input.face {
                2 => Horizontal::North,
                3 => Horizontal::South,
                4 => Horizontal::West,
                5 => Horizontal::East,
                _ => return None,
            };
            state = set(block, state, "direction", StateValue::Int(side.legacy()));
        }
        // Measured: a piston's `facing_direction` is the look direction.
        "minecraft:piston" | "minecraft:sticky_piston" => {
            let facing = look_facing(input.yaw, input.pitch);
            state = set(block, state, "facing_direction", StateValue::Int(facing));
        }
        "minecraft:dispenser" | "minecraft:dropper" | "minecraft:barrel" => {
            let facing = opposite_facing(look_facing(input.yaw, input.pitch));
            state = set(block, state, "facing_direction", StateValue::Int(facing));
        }
        // A hopper points into the block it was placed against; placed on a
        // top or bottom face it points down.
        "minecraft:hopper" => {
            let facing = match input.face {
                2 => 3,
                3 => 2,
                4 => 5,
                5 => 4,
                _ => 0,
            };
            state = set(block, state, "facing_direction", StateValue::Int(facing));
        }
        _ => {}
    }
    Some(state)
}

/// The direction a side face points (`None` for the top and bottom faces).
fn horizontal_of_face(face: u8) -> Option<Horizontal> {
    Some(match face {
        2 => Horizontal::North,
        3 => Horizontal::South,
        4 => Horizontal::West,
        5 => Horizontal::East,
        _ => return None,
    })
}

/// The side of a placed block that touches the clicked face's block.
fn attach_face(face: u8) -> Option<Face> {
    Some(match face {
        0 => Face::Up,
        1 => Face::Down,
        2 => Face::South,
        3 => Face::North,
        4 => Face::East,
        5 => Face::West,
        _ => return None,
    })
}

/// `facing_direction` of a hanging sign on side `face` of a block: along the
/// wall, away from the way the player looks along it; east (on a north or
/// south face) or south (on a west or east face) when the look has no part
/// along the wall (BDS 1.26.51: on a north face, yaw 30 gives east, yaw -30
/// west, yaw 0 east).
fn wall_hanging_facing(face: u8, yaw: f32) -> i32 {
    let yaw = f64::from(yaw).to_radians();
    // Bedrock yaw 0 looks south (+z), 90 west (-x).
    let (look_x, look_z) = (-yaw.sin(), yaw.cos());
    const ALONG: f64 = 1e-6;
    match face {
        2 | 3 if look_x > ALONG => 4,
        2 | 3 => 5,
        _ if look_z > ALONG => 2,
        _ => 3,
    }
}

/// Sixteenths of a turn a sign faces, toward the player (yaw 0 looking south
/// gives 8).
fn sign_direction(yaw: f32) -> i32 {
    (((f64::from(yaw) + 180.0) / 22.5).round() as i32).rem_euclid(16)
}

/// Where-it-may-go rules measured in round 2
/// (`testdata/bds-placements-crops.json`, `-plants_and_stacks.json`): kelp
/// only into water; cocoa only on the side of a jungle log; sugar cane only
/// next to water.
fn placeable_here(
    blocks: &impl BlockSource,
    held: &BlockEntry,
    position: IVec3,
    target: &BlockEntry,
    target_state: u32,
    input: &PlacementInput,
) -> bool {
    let registry = blocks.registry();
    let name_at = |pos: IVec3| {
        blocks
            .block_at(pos)
            .and_then(|id| registry.get_by_runtime_id(id))
            .map_or("", |entry| entry.string_id.as_str())
    };
    match held.string_id.as_str() {
        "minecraft:kelp" => is_water(registry, target, target_state),
        "minecraft:cocoa" => matches!(
            name_at(input.clicked),
            "minecraft:jungle_log" | "minecraft:jungle_wood"
        ),
        "minecraft:reeds" => {
            let below = position - IVec3::Y;
            name_at(below) == "minecraft:reeds"
                || [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z]
                    .into_iter()
                    .any(|side| {
                        matches!(
                            name_at(below + side),
                            "minecraft:water" | "minecraft:flowing_water"
                        )
                    })
        }
        _ => true,
    }
}

fn is_water(registry: &BlockRegistry, entry: &BlockEntry, _state: u32) -> bool {
    let _ = registry;
    matches!(
        entry.string_id.as_str(),
        "minecraft:water" | "minecraft:flowing_water"
    )
}

/// Whether `block` grows when its item is placed into it again (candles,
/// snow layers, sea pickles, turtle eggs, petals, multi-face blocks).
pub fn stacks(block: &BlockEntry) -> bool {
    crate::server::game::block_update::support::multiface_bits(block).is_some()
        || block.string_id.ends_with("candle")
        || matches!(
            block.string_id.as_str(),
            "minecraft:snow_layer"
                | "minecraft:sea_pickle"
                | "minecraft:turtle_egg"
                | "minecraft:pink_petals"
                | "minecraft:wildflowers"
                | "minecraft:leaf_litter"
        )
}

/// The next state of a block that grows by one when its item is placed into
/// it again (BDS 1.26.51, `testdata/bds-placements-candles.json` and
/// `-plants_and_stacks.json`): candles of the same colour up to four, snow
/// layers, sea pickles, turtle eggs, pink petals, wildflowers and leaf
/// litter; a multi-face block gains the clicked face. `Some(None)` when the
/// block is full or the face already covered.
fn grown(
    held: &BlockEntry,
    current: &BlockEntry,
    state: u32,
    face: Option<Face>,
) -> Option<Option<u32>> {
    if held.string_id != current.string_id {
        return None;
    }
    let int = |name: &str| match current.state_value(state, name) {
        Some(StateValue::Int(value)) => Some(value),
        _ => None,
    };
    let bump = |name: &str, max: i32| {
        let value = int(name)?;
        Some((value < max).then(|| set(current, state, name, StateValue::Int(value + 1))))
    };
    if let Some((property, faces)) =
        crate::server::game::block_update::support::multiface_bits(current)
    {
        let face = face?;
        let bits = int(property)?;
        let &(_, bit) = faces.iter().find(|(f, _)| *f == face)?;
        return Some(
            (bits & bit == 0).then(|| set(current, state, property, StateValue::Int(bits | bit))),
        );
    }
    match current.string_id.as_str() {
        id if id.ends_with("candle") => bump("candles", 3),
        "minecraft:snow_layer" => bump("height", 7),
        "minecraft:sea_pickle" => bump("cluster_count", 3),
        "minecraft:pink_petals" | "minecraft:wildflowers" | "minecraft:leaf_litter" => {
            bump("growth", 3)
        }
        "minecraft:turtle_egg" => {
            let next = match current.state_value(state, "turtle_egg_count") {
                Some(StateValue::Str("one_egg")) => "two_egg",
                Some(StateValue::Str("two_egg")) => "three_egg",
                Some(StateValue::Str("three_egg")) => "four_egg",
                _ => return Some(None),
            };
            Some(Some(set(
                current,
                state,
                "turtle_egg_count",
                StateValue::Str(next),
            )))
        }
        _ => None,
    }
}

/// A placement into an existing block: the clicked block grows (stacking
/// blocks), the block on the clicked face gains a face (multi-face blocks),
/// or a candle goes onto an uneaten cake. `None` when this is not such a
/// click; `Some(None)` when it is but BDS refuses it.
fn stack(
    blocks: &impl BlockSource,
    held: &BlockEntry,
    input: &PlacementInput,
    target: IVec3,
) -> Option<Option<Placement>> {
    let registry = blocks.registry();
    let clicked_state = blocks.block_at(input.clicked)?;
    let clicked = registry.get_by_runtime_id(clicked_state)?;
    let one = |position: IVec3, state: u32| {
        Some(Placement {
            position,
            state,
            upper: None,
        })
    };
    // A candle onto a cake nobody has eaten from.
    if held.string_id.ends_with("candle") && clicked.string_id == "minecraft:cake" {
        if !matches!(
            clicked.state_value(clicked_state, "bite_counter"),
            Some(StateValue::Int(0))
        ) {
            return None;
        }
        let cake = registry.get_by_name(&format!("{}_cake", held.string_id))?;
        return Some(one(input.clicked, cake.default_state_id));
    }
    let multiface = crate::server::game::block_update::support::multiface_bits(held).is_some();
    if multiface {
        let target_state = blocks.block_at(target)?;
        let current = registry.get_by_runtime_id(target_state)?;
        let grown = grown(held, current, target_state, attach_face(input.face))?;
        return Some(grown.and_then(|state| one(target, state)));
    }
    let grown = grown(held, clicked, clicked_state, None)?;
    Some(grown.and_then(|state| one(input.clicked, state)))
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
    // Blocks that stack when placed again into the clicked block (or the
    // block on the clicked face).
    if let Some(stacked) = stack(blocks, held, input, position) {
        return stacked;
    }
    // A candle clicked onto a candle of another colour is refused.
    if held.string_id.ends_with("candle") && clicked.string_id.ends_with("candle") {
        return None;
    }
    // A lily pad or frog spawn clicked onto water goes on top of it.
    let (position, target_state, target) = if matches!(
        held.string_id.as_str(),
        "minecraft:waterlily" | "minecraft:frog_spawn"
    ) && is_water(registry, clicked, clicked_state)
    {
        let above = input.clicked + face_offset(input.face)?;
        let state = blocks.block_at(above)?;
        (above, state, registry.get_by_runtime_id(state)?)
    } else {
        (position, target_state, target)
    };
    if !placeable_here(blocks, held, position, target, target_state, input) {
        return None;
    }
    // A sign, banner or coral fan clicked onto a side face goes up as its
    // wall variant.
    let held = match wall_variant(registry, held) {
        Some(wall) if input.face >= 2 => wall,
        _ => held,
    };
    // A live coral fan out of water is placed dead.
    let held = match dead_coral(registry, held) {
        Some(dead) if !is_water(registry, target, target_state) => dead,
        _ => held,
    };
    let mut state = placed_state(held, input)?;
    // A sea pickle out of water is placed dead.
    if held.string_id == "minecraft:sea_pickle" && !is_water(registry, target, target_state) {
        state = set(held, state, "dead_bit", StateValue::Byte(1));
    }
    // A bell on a wall with a block on its other side hangs between them.
    if held.string_id == "minecraft:bell" && input.face >= 2 {
        let beyond = position + face_offset(input.face)?;
        let solid = blocks.block_at(beyond).is_some_and(|id| {
            registry
                .get_by_runtime_id(id)
                .is_some_and(|block| !is_replaceable(block, id))
        });
        if solid {
            state = set(held, state, "attachment", StateValue::Str("multiple"));
        }
    }
    // A hanging sign under a block whose bottom face is not full (a fence)
    // hangs on chains: `attached_bit`, turned in sixteenths like a standing
    // sign, `facing_direction` left at 0 (BDS 1.26.51).
    if held.string_id.ends_with("_hanging_sign")
        && input.face == 0
        && !face_covered(clicked_state, Face::Down, [0.0, 1.0, 0.0, 1.0])
    {
        state = set(held, state, "attached_bit", StateValue::Byte(1));
        state = set(held, state, "facing_direction", StateValue::Int(0));
        state = set(
            held,
            state,
            "ground_sign_direction",
            StateValue::Int(sign_direction(input.yaw)),
        );
    }

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

    // BDS refuses a block that could not stay where it would go.
    if !crate::server::game::block_update::support::can_place(blocks, position, held, state) {
        return None;
    }
    let upper = second_block(blocks, held, position, state)?;
    Some(Placement {
        position,
        state,
        upper,
    })
}

/// The second block of a two-block placement of `state` at `position`: a
/// door's (or tall plant's) upper half above it, a bed's head one block
/// further in its direction. `Some(None)` for a one-block placement, `None`
/// when the second block cannot go: its position is not replaceable or, for a
/// bed, either half has nothing to stand on (BDS 1.26.51 refuses a bed whose
/// head would be over air).
pub fn second_block(
    blocks: &impl BlockSource,
    entry: &BlockEntry,
    position: IVec3,
    state: u32,
) -> Option<Option<(IVec3, u32)>> {
    let registry = blocks.registry();
    let replaceable = |pos: IVec3| {
        blocks.block_at(pos).is_some_and(|id| {
            registry
                .get_by_runtime_id(id)
                .is_some_and(|block| is_replaceable(block, id))
        })
    };
    match (
        entry.state_value(state, "upper_block_bit"),
        entry.state_value(state, "head_piece_bit"),
    ) {
        // A pitcher pod plants only the lower half (BDS 1.26.51).
        (Some(StateValue::Byte(0)), _) if entry.string_id == "minecraft:pitcher_crop" => Some(None),
        (Some(StateValue::Byte(0)), _) => {
            let above = position + IVec3::Y;
            let mut upper = set(entry, state, "upper_block_bit", StateValue::Byte(1));
            if entry.state_value(state, "door_hinge_bit").is_some() {
                let hinge = door_hinge(blocks, entry, state, position);
                upper = set(
                    entry,
                    upper,
                    "door_hinge_bit",
                    StateValue::Byte(u8::from(hinge)),
                );
            }
            replaceable(above).then_some(Some((above, upper)))
        }
        (_, Some(StateValue::Byte(0))) => {
            let toward = match (
                entry.state_value(state, "direction"),
                entry.state_value(state, "minecraft:cardinal_direction"),
            ) {
                (Some(StateValue::Int(0)), _) | (_, Some(StateValue::Str("south"))) => IVec3::Z,
                (Some(StateValue::Int(1)), _) | (_, Some(StateValue::Str("west"))) => IVec3::NEG_X,
                (Some(StateValue::Int(2)), _) | (_, Some(StateValue::Str("north"))) => IVec3::NEG_Z,
                (Some(StateValue::Int(3)), _) | (_, Some(StateValue::Str("east"))) => IVec3::X,
                _ => return None,
            };
            let head = position + toward;
            let stands = |pos: IVec3| bed_floor(blocks, pos - IVec3::Y);
            (replaceable(head) && stands(position) && stands(head)).then(|| {
                Some((
                    head,
                    set(entry, state, "head_piece_bit", StateValue::Byte(1)),
                ))
            })
        }
        (Some(_), _) | (_, Some(_)) => None,
        (None, None) => Some(None),
    }
}

/// The hinge of a door placed at `position` (BDS 1.26.51 stores it on the
/// upper half only; `testdata/bds-placements-door_hinges.json` and
/// `-followup.json`): with doors beside it, set when a door of any kind
/// stands on the placer's left (the side the door faces away from) and none
/// on the right; otherwise set when a solid block stands on the right (the
/// side it faces, lower or upper half) and none on the left. The click
/// position does not matter.
fn door_hinge(blocks: &impl BlockSource, entry: &BlockEntry, state: u32, position: IVec3) -> bool {
    let facing = match entry.state_value(state, "minecraft:cardinal_direction") {
        Some(StateValue::Str("north")) => Face::North,
        Some(StateValue::Str("east")) => Face::East,
        Some(StateValue::Str("west")) => Face::West,
        _ => Face::South,
    };
    let registry = blocks.registry();
    let left = position + facing.opposite().offset();
    let right = position + facing.offset();
    let door_at = |pos: IVec3| {
        blocks.block_at(pos).is_some_and(|id| {
            registry
                .get_by_runtime_id(id)
                .is_some_and(|block| block.state_value(id, "door_hinge_bit").is_some())
        })
    };
    let (left_door, right_door) = (
        door_at(left) || door_at(left + IVec3::Y),
        door_at(right) || door_at(right + IVec3::Y),
    );
    if left_door || right_door {
        return left_door && !right_door;
    }
    // An opaque full block that is not glass-like (stone counts; glowstone,
    // ice, glass, leaves, chests, slabs, stairs, fences, walls, trapdoors and
    // scaffolding do not).
    let solid = |pos: IVec3| {
        [pos, pos + IVec3::Y].into_iter().any(|at| {
            blocks
                .block_at(at)
                .and_then(|id| registry.get_by_runtime_id(id))
                .is_some_and(|block| {
                    !block.data.is_transparent && block.data.material != GLASS_MATERIAL
                })
        })
    };
    solid(right) && !solid(left)
}

/// Native material type of glass-like blocks (glass, glowstone).
const GLASS_MATERIAL: u8 = 11;

/// Whether a bed half may stand on the block at `pos`: its collision covers
/// the centre of its top face (BDS 1.26.51 keeps a bed's head over stone,
/// glass, a top slab or a fence and refuses it over air, a bottom slab, a
/// carpet or short grass).
fn bed_floor(blocks: &impl BlockSource, pos: IVec3) -> bool {
    blocks.block_at(pos).is_some_and(|id| {
        face_covered(
            id,
            Face::Up,
            [7.0 / 16.0, 9.0 / 16.0, 7.0 / 16.0, 9.0 / 16.0],
        )
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
            eye: DVec3::new(0.5, 66.0, -2.0),
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

    /// Every slab merges into the double slab that drops two of it in the
    /// BDS drop sweep (`crates/unastar-data/data/measured/block_drops/`).
    #[test]
    fn slabs_merge_into_the_double_slab_that_drops_them() {
        let dir = format!(
            "{}/../unastar-data/data/measured/block_drops",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut pairs = 0;
        for file in std::fs::read_dir(&dir).unwrap() {
            let path = file.unwrap().path();
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            // The petrified oak double slab drops oak slabs too, but two oak
            // slabs merge into the oak double slab.
            if !(name.contains("double") && name.ends_with("slab"))
                || name == "petrified_oak_double_slab"
            {
                continue;
            }
            let measured: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let slab = measured["cases"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|case| case["observed"].as_object())
                .flat_map(|observed| observed.keys())
                .find(|outcome| !outcome.is_empty())
                .and_then(|outcome| outcome.split('@').next())
                .unwrap()
                .to_owned();
            let double = double_slab(BlockRegistry::vanilla(), entry(&slab)).expect(&slab);
            assert_eq!(double.string_id, format!("minecraft:{name}"), "{slab}");
            pairs += 1;
        }
        assert_eq!(pairs, 100);
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
        // BDS turns a door a quarter clockwise from the look: looking east
        // gives a door facing south.
        let door = resolve_placement(
            &world(),
            entry("minecraft:wooden_door"),
            &input(1, 1.0, 270.0),
        )
        .unwrap();
        assert_eq!(
            value(&door, "minecraft:cardinal_direction"),
            Some(StateValue::Str("south"))
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
    fn hoppers_point_into_the_clicked_block_or_down() {
        let hopper = entry("minecraft:hopper");
        for (face, facing) in [(1, 0), (0, 0), (2, 3), (3, 2), (4, 5), (5, 4)] {
            let mut blocks = world();
            // A bottom-face click needs room below the clicked block.
            blocks.set(IVec3::new(0, 63, 0), "minecraft:air");
            let placed = resolve_placement(&blocks, hopper, &input(face, 0.5, 0.0)).unwrap();
            assert_eq!(
                value(&placed, "facing_direction"),
                Some(StateValue::Int(facing)),
                "face {face}"
            );
        }
    }

    #[test]
    fn torches_attach_to_the_clicked_face_but_not_underneath() {
        let torch = entry("minecraft:torch");
        // On the south face the torch hangs on the block north of it.
        let wall = resolve_placement(&world(), torch, &input(3, 0.5, 0.0)).unwrap();
        assert_eq!(
            value(&wall, "torch_facing_direction"),
            Some(StateValue::Str("north"))
        );
        let floor = resolve_placement(&world(), torch, &input(1, 1.0, 0.0)).unwrap();
        assert_eq!(
            value(&floor, "torch_facing_direction"),
            Some(StateValue::Str("top"))
        );
        assert!(resolve_placement(&world(), torch, &input(0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn lanterns_hang_under_blocks_and_stand_on_them() {
        for name in [
            "minecraft:lantern",
            "minecraft:soul_lantern",
            "minecraft:copper_lantern",
        ] {
            let mut blocks = world();
            blocks.set(IVec3::new(0, 63, 0), "minecraft:air");
            let under = resolve_placement(&blocks, entry(name), &input(0, 0.0, 0.0)).unwrap();
            assert_eq!(under.position, IVec3::new(0, 63, 0));
            assert_eq!(
                value(&under, "hanging"),
                Some(StateValue::Byte(1)),
                "{name}"
            );
            let on = resolve_placement(&world(), entry(name), &input(1, 1.0, 0.0)).unwrap();
            assert_eq!(value(&on, "hanging"), Some(StateValue::Byte(0)), "{name}");
        }
    }

    /// Every sign item places a standing sign on a top face and its wall sign
    /// on a side face, never under a block.
    #[test]
    fn sign_items_place_standing_and_wall_signs() {
        let registry = BlockRegistry::vanilla();
        let items = unastar_data::items::vanilla()
            .iter()
            .map(|item| item.identifier.as_str())
            .filter(|name| name.ends_with("_sign") && !name.contains("hanging"))
            .filter(|name| !name.ends_with("standing_sign") && !name.ends_with("wall_sign"));
        let mut count = 0;
        for item in items {
            let standing = sign_block(registry, item).unwrap_or_else(|| panic!("{item}"));
            let wall = wall_sign(registry, standing).unwrap_or_else(|| panic!("{item}"));
            assert!(places_as(standing, wall), "{item}");
            // Looking west (yaw 90) at the top face: direction 12.
            let top = resolve_placement(&world(), standing, &input(1, 1.0, 90.0)).unwrap();
            assert_eq!(
                value(&top, "ground_sign_direction"),
                Some(StateValue::Int(12)),
                "{item}"
            );
            let side = resolve_placement(&world(), standing, &input(2, 0.5, 0.0)).unwrap();
            assert_eq!(
                registry
                    .get_by_runtime_id(side.state)
                    .map(|b| b.string_id.as_str()),
                Some(wall.string_id.as_str())
            );
            assert_eq!(value(&side, "facing_direction"), Some(StateValue::Int(2)));
            assert!(resolve_placement(&world(), standing, &input(0, 0.0, 0.0)).is_none());
            count += 1;
        }
        assert_eq!(count, 13, "sign woods");
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
