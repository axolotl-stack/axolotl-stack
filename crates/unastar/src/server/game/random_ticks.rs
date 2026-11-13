//! Random block ticks, as BDS 1.26.51 gives them (measured with the
//! random tick fields of `tools/bds-sweep/random_ticks.py`,
//! `crates/unastar-data/data/measured/random_ticks/`; the tests compare
//! unastar's fields with BDS's by chi-square).
//!
//! - Every game tick, each sub-chunk of a ticking chunk gets
//!   `2 x randomtickspeed` random positions: a block receives on average
//!   `randomtickspeed / 2048` random ticks per game tick (BDS's measured
//!   rate, twice a 16 x 16 x 16 pick per tick).
//! - Crops (wheat, carrots, potatoes, beetroots) grow one stage with
//!   chance `1 / (floor(25 / f) + 1)`, `f` counting the farmland under
//!   them and their neighbours (3 moist, 1 dry; a quarter for the eight
//!   around) and halved when the same crop stands on both axes or on a
//!   diagonal. Beetroots grow as fast as wheat.
//! - Sugar cane and cacti age one step per random tick on the top block of
//!   a column under three tall and grow a block at age 15.
//! - Farmland away from water (four blocks around, the same level or one
//!   up) loses one moisture step per random tick and turns to dirt at 0
//!   unless a crop stands on it; near water it is moist (7).
//! - Grass spreads to dirt (four tries in a 3 x 5 x 3 box) where the block
//!   above lets light through.
//! - Leaves waiting for their update (`update_bit`, not persistent) with
//!   no log near them decay; with a log near they stop waiting.
//! - Copper oxidises one stage with chance 0.0569 x (the share of nearby
//!   copper that is not younger)^2 (x 0.75 for fresh copper); copper
//!   younger nearby holds it back.
//!
//! Light is not checked (unastar has no block light).

use std::sync::LazyLock;

use glam::IVec3;
use rand::Rng;

use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::block_update::{UpdateTarget, changed};

/// Random picks per sub-chunk per game tick for each step of
/// `randomtickspeed`.
pub(crate) const PICKS_PER_SPEED: u32 = 2;

/// Most random picks one sub-chunk gets in a game tick: as many as it has
/// cells. A safety cap, not a measured rule: how BDS handles a huge
/// `randomtickspeed` is not measured, and without the cap
/// `/gamerule randomtickspeed 2147483647` stalls the tick.
const MAX_PICKS: u32 = 16 * 16 * 16;

/// What a block does on a random tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    None,
    Crop,
    Cane,
    Cactus,
    Farmland,
    Grass,
    Leaves,
    /// Copper of age 0-2 and the state it oxidises to.
    Copper {
        age: u8,
        next: u32,
    },
    /// Oxidised copper: counts as age 3 for its neighbours.
    Oxidised,
}

struct Rules {
    rules: Vec<Rule>,
}

const CROPS: &[&str] = &[
    "minecraft:wheat",
    "minecraft:carrots",
    "minecraft:potatoes",
    "minecraft:beetroot",
];

const STAGES: [&str; 4] = ["", "exposed_", "weathered_", "oxidized_"];

/// The copper stage of `id` (`minecraft:<stage><base>`, `copper_block` for
/// the unaged full block) and its base name.
fn copper_stage(id: &str) -> Option<(usize, String)> {
    let rest = id.strip_prefix("minecraft:")?;
    if rest.starts_with("waxed_") {
        return None;
    }
    let (age, base) = STAGES
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(age, prefix)| rest.strip_prefix(prefix).map(|base| (age, base)))
        .unwrap_or((0, rest));
    let base = if base == "copper_block" || (age > 0 && base == "copper") {
        "copper".to_string()
    } else {
        base.to_string()
    };
    let copper = base == "copper" || base.contains("copper") || base == "lightning_rod";
    if !copper || base.contains("ore") || base.starts_with("raw_") || base == "copper_torch" {
        return None;
    }
    Some((age, base))
}

fn copper_name(age: usize, base: &str) -> String {
    if base == "copper" {
        if age == 0 {
            "minecraft:copper_block".to_string()
        } else {
            format!("minecraft:{}copper", STAGES[age])
        }
    } else {
        format!("minecraft:{}{base}", STAGES[age])
    }
}

/// The same state under another block (copper stages keep their shape).
fn renamed(registry: &BlockRegistry, entry: &BlockEntry, state: u32, name: &str) -> Option<u32> {
    let target = registry.get_by_name(name)?;
    let mut out = target.default_state_id;
    for property in entry.data.properties {
        if let Some(value) = entry.state_value(state, property.name) {
            out = target
                .with_state_value(out, property.name, value)
                .unwrap_or(out);
        }
    }
    Some(out)
}

impl Rules {
    fn build(registry: &BlockRegistry) -> Self {
        let mut rules = Vec::new();
        for entry in registry.iter() {
            let id = entry.string_id.as_str();
            for state in entry.min_state_id..=entry.max_state_id {
                let index = state as usize;
                if rules.len() <= index {
                    rules.resize(index + 1, Rule::None);
                }
                rules[index] = if CROPS.contains(&id) {
                    Rule::Crop
                } else {
                    match id {
                        "minecraft:reeds" => Rule::Cane,
                        "minecraft:cactus" => Rule::Cactus,
                        "minecraft:farmland" => Rule::Farmland,
                        "minecraft:grass_block" => Rule::Grass,
                        _ if id.ends_with("_leaves")
                            || id == "minecraft:azalea_leaves_flowered" =>
                        {
                            Rule::Leaves
                        }
                        _ => match copper_stage(id) {
                            Some((3, _)) => Rule::Oxidised,
                            Some((age, base)) => {
                                renamed(registry, entry, state, &copper_name(age + 1, &base))
                                    .map_or(Rule::None, |next| Rule::Copper {
                                        age: age as u8,
                                        next,
                                    })
                            }
                            None => Rule::None,
                        },
                    }
                };
            }
        }
        Self { rules }
    }

    fn get(&self, state: u32) -> Rule {
        self.rules
            .get(state as usize)
            .copied()
            .unwrap_or(Rule::None)
    }
}

/// The rules for `registry`: `None` unless it is the vanilla registry the
/// rules are built for (the random tick rules are measured on vanilla
/// blocks; another registry's runtime ids would pick the wrong rules).
fn rules(registry: &BlockRegistry) -> Option<&'static Rules> {
    static VANILLA: LazyLock<Rules> = LazyLock::new(|| Rules::build(BlockRegistry::vanilla()));
    (registry.len() == BlockRegistry::vanilla().len()).then(|| &*VANILLA)
}

fn entry(blocks: &impl BlockSource, state: u32) -> Option<&BlockEntry> {
    blocks.registry().get_by_runtime_id(state)
}

fn int(blocks: &impl BlockSource, state: u32, property: &str) -> Option<i32> {
    match entry(blocks, state)?.state_value(state, property)? {
        StateValue::Int(value) => Some(value),
        _ => None,
    }
}

fn byte(blocks: &impl BlockSource, state: u32, property: &str) -> Option<bool> {
    match entry(blocks, state)?.state_value(state, property)? {
        StateValue::Byte(value) => Some(value != 0),
        _ => None,
    }
}

fn with(blocks: &impl BlockSource, state: u32, property: &str, value: StateValue) -> u32 {
    entry(blocks, state)
        .and_then(|entry| entry.with_state_value(state, property, value))
        .unwrap_or(state)
}

fn named(blocks: &impl BlockSource, pos: IVec3) -> Option<&str> {
    let state = blocks.block_at(pos)?;
    entry(blocks, state).map(|entry| entry.string_id.as_str())
}

fn default_state(blocks: &impl BlockSource, name: &str) -> Option<u32> {
    blocks.registry().default_state_id_by_name(name)
}

fn set<T: UpdateTarget>(world: &mut T, pos: IVec3, state: u32) {
    world.write(pos, state);
    changed(world, pos, false);
}

/// Give the block at `pos` (state `state`) one random tick.
pub(crate) fn random_tick<T: UpdateTarget>(
    world: &mut T,
    pos: IVec3,
    state: u32,
    rng: &mut impl Rng,
) {
    let Some(rules) = rules(world.registry()) else {
        return;
    };
    match rules.get(state) {
        Rule::None | Rule::Oxidised => {}
        Rule::Crop => crop(world, pos, state, rng),
        Rule::Cane | Rule::Cactus => column(world, pos, state),
        Rule::Farmland => farmland(world, pos, state),
        Rule::Grass => grass(world, pos, rng),
        Rule::Leaves => leaves(world, pos, state),
        Rule::Copper { age, next } => copper(world, pos, age, next, rng),
    }
}

/// The growth factor of the crop at `pos` (see the module docs).
fn growth_speed(world: &impl BlockSource, pos: IVec3, crop: &str) -> f32 {
    let mut f = 1.0f32;
    let below = pos - IVec3::Y;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let soil = below + IVec3::new(dx, 0, dz);
            let mut g = 0.0f32;
            if let Some(state) = world.block_at(soil)
                && named(world, soil) == Some("minecraft:farmland")
            {
                g = if int(world, state, "moisturized_amount").unwrap_or(0) > 0 {
                    3.0
                } else {
                    1.0
                };
            }
            if dx != 0 || dz != 0 {
                g /= 4.0;
            }
            f += g;
        }
    }
    let same = |dx: i32, dz: i32| named(world, pos + IVec3::new(dx, 0, dz)) == Some(crop);
    // The same crop on both axes, or on a diagonal, halves it.
    let east_west = same(-1, 0) || same(1, 0);
    let north_south = same(0, -1) || same(0, 1);
    let diagonal = same(-1, -1) || same(1, -1) || same(1, 1) || same(-1, 1);
    if (east_west && north_south) || diagonal {
        f /= 2.0;
    }
    f
}

fn crop<T: UpdateTarget>(world: &mut T, pos: IVec3, state: u32, rng: &mut impl Rng) {
    let Some(growth) = int(world, state, "growth") else {
        return;
    };
    if growth >= 7 {
        return;
    }
    let Some(crop) = named(world, pos).map(str::to_owned) else {
        return;
    };
    let f = growth_speed(world, pos, &crop);
    let range = (25.0 / f) as i32;
    if rng.gen_range(0..=range) == 0 {
        let next = with(world, state, "growth", StateValue::Int(growth + 1));
        set(world, pos, next);
    }
}

/// Sugar cane and cacti: the top block of a column under three tall ages,
/// and grows a block at age 15.
fn column<T: UpdateTarget>(world: &mut T, pos: IVec3, state: u32) {
    let above = pos + IVec3::Y;
    if named(world, above) != Some("minecraft:air") {
        return;
    }
    let Some(name) = named(world, pos).map(str::to_owned) else {
        return;
    };
    let mut height = 1;
    while named(world, pos - IVec3::Y * height) == Some(name.as_str()) {
        height += 1;
    }
    if height >= 3 {
        return;
    }
    let age = int(world, state, "age").unwrap_or(0);
    if age == 15 {
        if let Some(fresh) = default_state(world, &name) {
            set(world, above, fresh);
        }
        let reset = with(world, state, "age", StateValue::Int(0));
        set(world, pos, reset);
    } else {
        let next = with(world, state, "age", StateValue::Int(age + 1));
        set(world, pos, next);
    }
}

fn near_water(world: &impl BlockSource, pos: IVec3) -> bool {
    (-4..=4).any(|dx| {
        (0..=1).any(|dy| {
            (-4..=4).any(|dz| {
                matches!(
                    named(world, pos + IVec3::new(dx, dy, dz)),
                    Some("minecraft:water" | "minecraft:flowing_water")
                )
            })
        })
    })
}

fn farmland<T: UpdateTarget>(world: &mut T, pos: IVec3, state: u32) {
    let moisture = int(world, state, "moisturized_amount").unwrap_or(0);
    if near_water(world, pos) {
        if moisture < 7 {
            let next = with(world, state, "moisturized_amount", StateValue::Int(7));
            set(world, pos, next);
        }
        return;
    }
    if moisture > 0 {
        let next = with(
            world,
            state,
            "moisturized_amount",
            StateValue::Int(moisture - 1),
        );
        set(world, pos, next);
        return;
    }
    let above = named(world, pos + IVec3::Y).unwrap_or("minecraft:air");
    let planted =
        CROPS.contains(&above) || above.ends_with("_stem") || above == "minecraft:torchflower_crop";
    if !planted && let Some(dirt) = default_state(world, "minecraft:dirt") {
        set(world, pos, dirt);
    }
}

/// Whether grass can be on the block at `pos`: the block above lets light
/// through.
fn grass_can_live(world: &impl BlockSource, pos: IVec3) -> bool {
    world
        .block_at(pos + IVec3::Y)
        .and_then(|state| entry(world, state))
        .is_none_or(|above| above.is_transparent)
}

fn grass<T: UpdateTarget>(world: &mut T, pos: IVec3, rng: &mut impl Rng) {
    if !grass_can_live(world, pos) {
        if let Some(dirt) = default_state(world, "minecraft:dirt") {
            set(world, pos, dirt);
        }
        return;
    }
    let Some(grass) = default_state(world, "minecraft:grass_block") else {
        return;
    };
    for _ in 0..4 {
        let target = pos
            + IVec3::new(
                rng.gen_range(-1..=1),
                rng.gen_range(-3..=1),
                rng.gen_range(-1..=1),
            );
        let Some(state) = world.block_at(target) else {
            continue;
        };
        let is_dirt = named(world, target) == Some("minecraft:dirt")
            && entry(world, state)
                .and_then(|e| e.state_value(state, "dirt_type"))
                .is_none_or(|value| value == StateValue::Str("normal"));
        if is_dirt && grass_can_live(world, target) {
            set(world, target, grass);
        }
    }
}

/// Leaves stay while a log is at most this many steps away through leaves
/// (`random_leaves_logs.json`: a line of leaves keeps four from the trunk;
/// leaves standing apart from the others keep only when they touch a log).
const LEAF_RANGE: usize = 4;

fn is_log(id: &str) -> bool {
    (id.ends_with("_log") || id.ends_with("_wood") || id.ends_with("_hyphae"))
        || (id.ends_with("_stem") && !id.ends_with("melon_stem") && !id.ends_with("pumpkin_stem"))
}

fn is_leaves(id: &str) -> bool {
    id.ends_with("_leaves") || id == "minecraft:azalea_leaves_flowered"
}

/// Whether a log is at most [`LEAF_RANGE`] steps from `pos` through leaves.
fn near_log(world: &impl BlockSource, pos: IVec3) -> bool {
    let mut seen = std::collections::HashSet::from([pos]);
    let mut frontier = vec![pos];
    for _ in 0..LEAF_RANGE {
        let mut next = Vec::new();
        for at in frontier {
            for offset in super::block_update::NEIGHBOURS {
                let to = at + offset;
                if !seen.insert(to) {
                    continue;
                }
                match named(world, to) {
                    Some(id) if is_log(id) => return true,
                    Some(id) if is_leaves(id) => next.push(to),
                    _ => {}
                }
            }
        }
        frontier = next;
    }
    false
}

fn leaves<T: UpdateTarget>(world: &mut T, pos: IVec3, state: u32) {
    if byte(world, state, "persistent_bit") == Some(true)
        || byte(world, state, "update_bit") != Some(true)
    {
        return;
    }
    if near_log(world, pos) {
        let settled = with(world, state, "update_bit", StateValue::Byte(0));
        world.write(pos, settled);
    } else {
        world.pop(pos, state);
        changed(world, pos, false);
    }
}

/// The oxidation age of the copper at `pos`, if it is unwaxed copper.
fn copper_age(world: &impl BlockSource, pos: IVec3) -> Option<u8> {
    let state = world.block_at(pos)?;
    match rules(world.registry())?.get(state) {
        Rule::Copper { age, .. } => Some(age),
        Rule::Oxidised => Some(3),
        _ => None,
    }
}

fn copper<T: UpdateTarget>(world: &mut T, pos: IVec3, age: u8, next: u32, rng: &mut impl Rng) {
    if rng.r#gen::<f32>() >= 0.056_888_89 {
        return;
    }
    let (mut same, mut older) = (0u32, 0u32);
    for dx in -4i32..=4 {
        for dy in -4i32..=4 {
            for dz in -4i32..=4 {
                let distance = dx.abs() + dy.abs() + dz.abs();
                if distance == 0 || distance > 4 {
                    continue;
                }
                let Some(other) = copper_age(world, pos + IVec3::new(dx, dy, dz)) else {
                    continue;
                };
                if other < age {
                    return;
                }
                if other > age {
                    older += 1;
                } else {
                    same += 1;
                }
            }
        }
    }
    let share = (older + 1) as f32 / (older + same + 1) as f32;
    let modifier = if age == 0 { 0.75 } else { 1.0 };
    if rng.r#gen::<f32>() < share * share * modifier {
        // A door's two halves age together (both change before either
        // updates its neighbours, or each would see the other half gone).
        let other = byte(world, next, "upper_block_bit").and_then(|upper| {
            let other = if upper {
                pos - IVec3::Y
            } else {
                pos + IVec3::Y
            };
            match rules(world.registry())?.get(world.block_at(other)?) {
                Rule::Copper { next, .. } => Some((other, next)),
                _ => None,
            }
        });
        world.write(pos, next);
        if let Some((other, other_next)) = other {
            world.write(other, other_next);
            changed(world, other, false);
        }
        changed(world, pos, false);
    }
}

/// Give one sub-chunk (minimum corner `origin`) its random ticks of a game
/// tick at `speed`.
pub(crate) fn tick_subchunk<T: UpdateTarget>(
    world: &mut T,
    origin: IVec3,
    speed: u32,
    rng: &mut impl Rng,
) {
    for _ in 0..speed.saturating_mul(PICKS_PER_SPEED).min(MAX_PICKS) {
        // One draw: 12 bits are a uniform position in the sub-chunk.
        let bits = rng.next_u32();
        let pos = origin
            + IVec3::new(
                (bits & 15) as i32,
                ((bits >> 4) & 15) as i32,
                ((bits >> 8) & 15) as i32,
            );
        if let Some(state) = world.block_at(pos) {
            random_tick(world, pos, state, rng);
        }
    }
}

/// The live world: random tick changes go through the chunk store with
/// block updates; pops break with drops.
struct Live<'w>(&'w mut bevy_ecs::prelude::World);

impl BlockSource for Live<'_> {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        self.0.block_at(pos)
    }

    fn registry(&self) -> &BlockRegistry {
        self.0.registry()
    }
}

impl UpdateTarget for Live<'_> {
    fn write(&mut self, pos: IVec3, state: u32) {
        super::blocks::write_block(self.0, pos, state, false);
    }

    fn pop(&mut self, pos: IVec3, state: u32) {
        super::blocks::pop_block(self.0, pos, state);
    }
}

/// Give every non-empty sub-chunk of every ticking chunk its random ticks
/// (exclusive: edits chunks). Runs once per game tick.
pub(crate) fn tick_random_blocks(world: &mut bevy_ecs::prelude::World) {
    use crate::world::ecs::{ChunkData, ChunkFlags, ChunkPosition, ChunkStateFlags};
    use unastar_data::game_rules::GameRuleValue;

    let Some(registry) = world.get_resource::<super::types::BlockRegistryResource>() else {
        return;
    };
    if rules(&registry.0).is_none() {
        // Fail fast once: the rules only know the vanilla registry.
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            tracing::warn!("random ticks are off: the block registry is not the vanilla one");
        });
        return;
    }
    let speed = world
        .get_resource::<crate::world::rules::GameRules>()
        .and_then(|rules| rules.value("randomtickspeed"))
        .map_or(1, |(_, value)| match value {
            GameRuleValue::Int(speed) => speed.max(0) as u32,
            _ => 1,
        });
    if speed == 0 {
        return;
    }
    let mut origins = Vec::new();
    let mut chunks = world.query::<(&ChunkPosition, &ChunkStateFlags, &ChunkData)>();
    for (position, flags, data) in chunks.iter(world) {
        if !flags.flags.contains(ChunkFlags::TICKING) {
            continue;
        }
        // `highest_subchunk` counts sub-chunks from the bottom of the world.
        let count = i32::from(data.inner.highest_subchunk());
        for index in 0..count {
            let bottom = crate::world::chunk::MIN_Y + index * 16;
            if data.inner.is_subchunk_empty(bottom >> 4) {
                continue;
            }
            origins.push(IVec3::new(position.x * 16, bottom, position.z * 16));
        }
    }
    if origins.is_empty() {
        return;
    }
    world.init_resource::<RandomTickRandom>();
    world.resource_scope(|world, mut rng: bevy_ecs::prelude::Mut<RandomTickRandom>| {
        for origin in origins {
            tick_subchunk(&mut Live(world), origin, speed, &mut rng.0);
        }
    });
}

/// The random source of random block ticks, its own: drawing the picks from
/// [`crate::entity::EntityRandom`] moved every item drop's offset and
/// velocity whenever a ticking sub-chunk existed. Seeded from entropy.
#[derive(bevy_ecs::prelude::Resource)]
pub(crate) struct RandomTickRandom(pub(crate) rand::rngs::StdRng);

impl Default for RandomTickRandom {
    fn default() -> Self {
        use rand::SeedableRng;
        Self(rand::rngs::StdRng::from_entropy())
    }
}

#[cfg(test)]
mod tests;
