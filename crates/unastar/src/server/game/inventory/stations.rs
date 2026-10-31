//! Anvil, grindstone, smithing table, enchanting table, stonecutter, loom
//! and cartography table: what they make from their input slots (pure
//! functions over stacks; the request engine owns slots, consumption and
//! the response) and the world side of a committed use (window opening,
//! XP, anvil wear, events, enchanting offers).
//!
//! Measured on BDS 1.26.51 (client-diff `blocks-20261029`: scenarios
//! `anvil`, `anvil-wear`, `smithing-table`, `grindstone-probe`):
//! - a click opens `ContainerOpen` type 5 (anvil), 33 (smithing table) or
//!   26 (grindstone) at the block with actor -1, followed by an empty
//!   three-slot `InventoryContent` for the window;
//! - inputs live in the UI container (see `containers.rs`); an anvil craft
//!   is `CraftRecipeOptional` naming the request's filtered rename string, a
//!   smithing craft `CraftRecipe` with the advertised smithing recipe, a
//!   grindstone craft `CraftRepairAndDisenchant`; inputs that make nothing
//!   answer `Invalidcraftresult`;
//! - anvil costs: rename 1 (and a rename alone keeps the penalty, writing
//!   `RepairCost` 0 when absent), unit repair 1 per material (a quarter of
//!   the maximum durability each), combining a damaged item 2, enchantments
//!   gained the level increase times their item multiplier, plus both
//!   inputs' `RepairCost`; the result's `RepairCost` is
//!   `2 × max(penalties) + 1`. A sacrifice whose only enchantment conflicts
//!   with the target makes nothing;
//! - every anvil use sends `LevelEvent` 1021 at the block before the
//!   response (1020 when the use destroys it), then the block change and an
//!   `UpdateAttributes` with only `player.level`; the window stays open
//!   after the anvil breaks and later uses still succeed;
//! - a smithing transform's result carries the input's user data verbatim;
//!   a trim adds `Trim {Material, Pattern}`; the template is consumed; no
//!   event or sound;
//! - grindstone XP spawns as orbs at the player's position after the
//!   response (16 XP for Sharpness III).
//!
//! Public references (minecraft.wiki "Anvil mechanics" and "Grindstone",
//! not measured): the enchantment table, unit-repair materials and trim ids
//! from `unastar-data`; the 12 % durability bonus of combining, the 40-level
//! "Too Expensive!" limit (creative exempt; measured: refused at 63 + 1),
//! book multipliers, the 12 % wear chance per use (not in creative), the
//! two-item grindstone bonus (5 %, keeping the first's name) and its XP
//! (`ceil(v / 2)` plus a uniform draw below that, `v` the stripped
//! enchantments' minimum enchanting levels; measured totals 6, 14, 16, 17
//! and 18 fit). Measured grindstone results: enchantments removed,
//! `RepairCost` 0, name and damage kept, an enchanted book becomes a plain
//! book, any curse refuses the craft, anvil inputs stay in their slots
//! after a close.
//!
//! Enchanting table, stonecutter, loom and cartography table, measured on
//! BDS 1.26.51 (client-diff items3-20261030 `enchanting`, `stations-2`,
//! `cartography`): windows `ContainerOpen` type 3, 29, 24 and 30 with 2,
//! 2, 4 and 3 slots; the enchanting table sends `PlayerEnchantOptions`
//! (before the response) when an enchantable item is put in, and empty
//! ones before its window opens and whenever the item slot changes to
//! anything else; an enchant
//! charges the offer slot's number of levels and lapis and replaces the
//! player's seed (see [`crate::item::enchanting`] for the offers);
//! stonecutter and cartography crafts are the advertised recipes tagged
//! `stonecutter` / `cartography_table`; the loom adds one pattern in the
//! dye's colour to a banner's `Patterns` and keeps the pattern item.
//! Filled maps do not exist on this server: the cartography table's map
//! multi recipes (clone, extend, lock) answer `Invalidcraftresult`, as BDS
//! did for the request shapes the bot sent. Public reference: bookshelves
//! count two blocks out on the table's level and the one above, each
//! column only when both blocks between it and the table are air
//! (minecraft.wiki "Enchanting table mechanics", Bedrock; the measured
//! counts with a blocking stone fit).

use bevy_ecs::prelude::*;
use glam::IVec3;
use jolyne::valentine::{
    ActorRuntimeId, ActorUniqueId, BlockPos, ContainerOpenPacket, EnchantmentInstance,
    EnumsEnchantType, InventoryContentPacket, ItemEnchantOption, ItemEnchants, McpePacket,
    PlayerEnchantOptionsPacket, TypedServerNetIdstructRecipeNetIdTag, UpdateAttributesPacket,
};
use rand::Rng;
use unastar_data::{banner_patterns, enchantments};

use super::engine::StationUse;
use crate::entity::EntityRandom;
use crate::entity::components::{
    ContainerWindows, Experience, PlayerSession, Position, RuntimeEntityId, ScreenKind,
    StationKind, StationScreen,
};

use crate::item::ItemStack;
use crate::item::enchant::{
    custom_name, durability_damage, enchantments as enchantment_list, has_repair_cost, item_tag,
    repair_cost, with_custom_name, with_durability_damage, with_enchantment_list, with_item_tag,
    with_repair_cost,
};
use crate::item::enchanting::{BOOK, ENCHANTED_BOOK, accepts};
use crate::registry::item::ItemEntry;
use crate::server::game::durability::max_durability;

/// Survival anvil operations must cost less than this many levels
/// ("Too Expensive!").
pub const ANVIL_COST_LIMIT: i32 = 40;

/// An anvil operation the inputs allow.
#[derive(Debug, Clone, PartialEq)]
pub struct AnvilResult {
    pub output: ItemStack,
    /// Total level cost, penalties included.
    pub cost: i32,
    /// How many of the target are used (the whole stack).
    pub input_used: u8,
    /// How many of the material slot are used.
    pub material_used: u8,
}

/// Whether `material` unit-repairs `target`.
fn repairs(target: &ItemEntry, material: &ItemEntry) -> bool {
    unastar_data::repair_materials::rows().iter().any(|row| {
        (row.tags.iter().any(|tag| target.has_tag(tag))
            || row.items.iter().any(|id| id == target.identifier()))
            && (row.material_tags.iter().any(|tag| material.has_tag(tag))
                || row.materials.iter().any(|id| id == material.identifier()))
    })
}

/// Damage after combining two items: remaining durability of both plus
/// `bonus_percent` of the maximum (rounded down), capped at full.
fn combined_damage(max: i32, a: i32, b: i32, bonus_percent: i32) -> i32 {
    (max - ((max - a) + (max - b) + max * bonus_percent / 100)).max(0)
}

/// The anvil result for `target` and `material` renamed to `name`
/// (`None`: the rename field was not sent). `None` when nothing would change.
pub fn anvil(target: &ItemStack, material: &ItemStack, name: Option<&str>) -> Option<AnvilResult> {
    let item = target.item()?;
    let mut output = target.clone();
    let mut cost = 0;
    let mut material_used = 0;
    let mut worked = false;
    let penalty = repair_cost(target) + repair_cost(material);
    if let Some(sacrifice) = material.item() {
        let max = max_durability(item);
        let damage = durability_damage(target);
        if max > 0 && repairs(item, sacrifice) {
            let quarter = max / 4;
            if damage == 0 || quarter == 0 {
                return None;
            }
            let mut left = damage;
            while left > 0 && material_used < material.count {
                left = (left - quarter).max(0);
                material_used += 1;
            }
            cost += i32::from(material_used);
            output = with_durability_damage(&output, left);
            worked = true;
        } else {
            let book = sacrifice.identifier() == ENCHANTED_BOOK;
            if !book && (sacrifice.identifier() != item.identifier() || max == 0) {
                return None;
            }
            if !book && damage > 0 {
                let repaired = combined_damage(max, damage, durability_damage(material), 12);
                output = with_durability_damage(&output, repaired);
                cost += 2;
                worked = true;
            }
            let mut levels = enchantment_list(&output);
            for (id, level) in enchantment_list(material) {
                let Some(enchantment) = enchantments::get(id) else {
                    continue;
                };
                if !accepts(&output, id)
                    || levels
                        .iter()
                        .any(|(other, _)| enchantments::incompatible(*other, id))
                {
                    continue;
                }
                let current = levels
                    .iter()
                    .find(|(other, _)| *other == id)
                    .map_or(0, |(_, level)| *level);
                let new = if level == current {
                    level + 1
                } else {
                    level.max(current)
                }
                .min(enchantment.max_level);
                if new <= current {
                    continue;
                }
                let multiplier = if book {
                    enchantment.book_multiplier
                } else {
                    enchantment.item_multiplier
                };
                cost += i32::from(new - current) * multiplier;
                match levels.iter_mut().find(|(other, _)| *other == id) {
                    Some(entry) => entry.1 = new,
                    None => levels.push((id, new)),
                }
                worked = true;
            }
            if !worked {
                return None;
            }
            output = with_enchantment_list(&output, &levels);
            material_used = 1;
        }
    }
    let current_name = custom_name(target);
    let renamed = match name {
        Some("") => current_name.is_some(),
        Some(name) => current_name.as_deref() != Some(name),
        None => false,
    };
    if renamed {
        let name = name.filter(|name| !name.is_empty());
        output = with_custom_name(&output, name);
        cost += 1;
    }
    if !worked && !renamed {
        return None;
    }
    let next_penalty = if worked {
        repair_cost(target).max(repair_cost(material)) * 2 + 1
    } else {
        repair_cost(target)
    };
    output = with_repair_cost(&output, Some(next_penalty));
    Some(AnvilResult {
        output,
        cost: cost + penalty,
        input_used: target.count,
        material_used,
    })
}

/// A grindstone operation the inputs allow.
#[derive(Debug, Clone, PartialEq)]
pub struct GrindstoneResult {
    pub output: ItemStack,
    /// Sum of the stripped enchantments' minimum enchanting levels.
    pub enchantment_value: i32,
}

/// Split `stack`'s enchantments into kept curses and the stripped value.
fn strip(stack: &ItemStack) -> (bool, i32) {
    let mut cursed = false;
    let mut value = 0;
    for (id, level) in enchantment_list(stack) {
        match enchantments::get(id) {
            Some(enchantment) if enchantment.curse => cursed = true,
            Some(enchantment) => value += enchantment.min_cost(level).max(0),
            None => {}
        }
    }
    (cursed, value)
}

/// The grindstone result for its two input slots, `None` when it makes
/// nothing. `items` supplies the plain book a disenchanted book becomes.
pub fn grindstone(
    input: &ItemStack,
    additional: &ItemStack,
    items: &crate::registry::ItemRegistry,
) -> Option<GrindstoneResult> {
    let (first, second) = match (input.is_empty(), additional.is_empty()) {
        (false, false) => (input, Some(additional)),
        (false, true) => (input, None),
        (true, false) => (additional, None),
        (true, true) => return None,
    };
    let item = first.item()?;
    let mut value = 0;
    for stack in std::iter::once(first).chain(second) {
        // Measured (grindstone-curse): a cursed input makes nothing.
        let (cursed, stripped) = strip(stack);
        if cursed {
            return None;
        }
        value += stripped;
    }
    let mut output = first.clone();
    if let Some(second) = second {
        let max = max_durability(item);
        if second.item_id() != first.item_id() || max == 0 || first.count != 1 || second.count != 1
        {
            return None;
        }
        let damage = combined_damage(max, durability_damage(first), durability_damage(second), 5);
        output = with_durability_damage(&output, damage);
    } else if value == 0 && !has_repair_cost(first) {
        return None;
    }
    if item.identifier() == ENCHANTED_BOOK {
        // Measured (grindstone-book): a plain book without NBT.
        let book = items.stack(BOOK, first.count)?;
        output = match custom_name(first) {
            Some(name) => with_custom_name(&book, Some(&name)),
            None => book,
        };
    } else {
        // Measured (grindstone, grindstone-named): RepairCost 0, name and
        // damage kept.
        output = with_repair_cost(&with_enchantment_list(&output, &[]), Some(0));
    }
    Some(GrindstoneResult {
        output,
        enchantment_value: value,
    })
}

/// Grindstone XP for `value`: with `half = ceil(value / 2)` and a uniform
/// draw `roll(half)` in `0..half`, `half + roll`, so `half..=2 * half - 1`.
pub fn grindstone_xp(value: i32, roll: impl FnOnce(i32) -> i32) -> i32 {
    if value <= 0 {
        return 0;
    }
    let half = (value + 1) / 2;
    half + roll(half)
}

/// A smithing transform's output: the recipe's result item (one) carrying
/// the input's user data, so enchantments, damage, name and trim survive.
pub fn transformed(result: &ItemStack, base: &ItemStack) -> Option<ItemStack> {
    result.item()?;
    Some(result.clone().with_count(1).with_nbt(base.user_data()))
}

/// A smithing trim's output: the input with `Trim {Pattern, Material}` from
/// the template and the material item.
pub fn trimmed(base: &ItemStack, template: &ItemStack, material: &ItemStack) -> Option<ItemStack> {
    use zuri_nbt::{NBTTag, tag};
    let pattern = unastar_data::armor_trims::pattern(template.item_id())?;
    let material = unastar_data::armor_trims::material(material.item_id())?;
    let mut compound = crate::item::enchant::item_tag(base).unwrap_or_default();
    let trim = tag::Compound(
        [
            (
                "Material".to_string(),
                NBTTag::String(tag::String(material.to_string())),
            ),
            (
                "Pattern".to_string(),
                NBTTag::String(tag::String(pattern.to_string())),
            ),
        ]
        .into_iter()
        .collect(),
    );
    compound
        .0
        .insert("Trim".to_string(), NBTTag::Compound(trim));
    Some(crate::item::enchant::with_item_tag(base, compound).with_count(1))
}

/// The loom's output for the `banner`, `dye` and pattern-slot item: one of
/// the banner with `pattern` in the dye's colour appended to its
/// `Patterns` (and `Type` 0 when it had none). `None` for anything but a
/// banner and a dye, an unknown pattern, a pattern whose banner pattern
/// item is not in the pattern slot, or a banner already at the pattern
/// limit.
pub fn loom(
    banner: &ItemStack,
    dye: &ItemStack,
    pattern_item: &ItemStack,
    pattern: &str,
) -> Option<ItemStack> {
    use zuri_nbt::{NBTTag, tag};
    if banner.item_id() != "minecraft:banner" {
        return None;
    }
    let colour = banner_patterns::dye_colour(dye.item_id())?;
    if let Some(item) = banner_patterns::required_item(pattern)?
        && pattern_item.item_id() != item
    {
        return None;
    }
    let mut compound = item_tag(banner).unwrap_or_default();
    let mut patterns = match compound.0.remove("Patterns") {
        Some(NBTTag::List(list)) => list.0,
        _ => Vec::new(),
    };
    if patterns.len() >= banner_patterns::max_patterns() {
        return None;
    }
    patterns.push(NBTTag::Compound(tag::Compound(
        [
            ("Color".to_string(), NBTTag::Int(tag::Int(colour))),
            (
                "Pattern".to_string(),
                NBTTag::String(tag::String(pattern.to_string())),
            ),
        ]
        .into_iter()
        .collect(),
    )));
    compound
        .0
        .insert("Patterns".to_string(), NBTTag::List(tag::List(patterns)));
    compound
        .0
        .entry("Type".to_string())
        .or_insert(NBTTag::Int(tag::Int(0)));
    Some(with_item_tag(&banner.clone().with_count(1), compound))
}

// ── World side ──────────────────────────────────────────────────────────────

/// `LevelEvent` of an anvil use (measured).
pub const ANVIL_USE_EVENT: i32 = 1021;
/// `LevelEvent` of the use that destroys an anvil (measured).
pub const ANVIL_BREAK_EVENT: i32 = 1020;
/// Chance that a survival anvil use wears the anvil one stage (public
/// reference; the measured runs wore it on uses 6, 11 and 12 of 60).
pub const ANVIL_WEAR_CHANCE: f32 = 0.12;

/// Anvil wear stages; a damaged anvil breaks on its next wear.
const ANVIL_STAGES: [&str; 3] = [
    "minecraft:anvil",
    "minecraft:chipped_anvil",
    "minecraft:damaged_anvil",
];

/// Signed `ContainerType` of a station window (anvil, smithing table and
/// grindstone measured; the others the protocol's container types).
pub const fn wire_type(kind: StationKind) -> i8 {
    match kind {
        StationKind::Anvil => 5,
        StationKind::SmithingTable => 33,
        StationKind::Grindstone => 26,
        StationKind::EnchantingTable => 3,
        StationKind::Stonecutter => 29,
        StationKind::Loom => 24,
        StationKind::CartographyTable => 30,
    }
}

/// The station block `identifier` is, if any.
pub fn station_of(identifier: &str) -> Option<StationKind> {
    match identifier {
        "minecraft:smithing_table" => Some(StationKind::SmithingTable),
        "minecraft:grindstone" => Some(StationKind::Grindstone),
        "minecraft:enchanting_table" => Some(StationKind::EnchantingTable),
        "minecraft:stonecutter_block" => Some(StationKind::Stonecutter),
        "minecraft:loom" => Some(StationKind::Loom),
        "minecraft:cartography_table" => Some(StationKind::CartographyTable),
        _ if ANVIL_STAGES.contains(&identifier) => Some(StationKind::Anvil),
        _ => None,
    }
}

/// A station window's content: its UI input slots from the first, then
/// empty preview slots up to the window size. Measured (BDS 1.26.51):
/// anvil 1..=3, smithing table 51..=53 and grindstone 16..=18 (three UI
/// slots each); enchanting table 2 slots, stonecutter 2, loom 4 and
/// cartography table 3 (client-diff items3-20261030), whose inputs are 1,
/// 1, 3 and 2 UI slots.
fn window_slots(kind: StationKind) -> (std::ops::Range<usize>, usize) {
    use super::containers::*;
    let (first, inputs, size) = match kind {
        StationKind::Anvil => (ANVIL_INPUT_SLOT, 3, 3),
        StationKind::SmithingTable => (SMITHING_INPUT_SLOT, 3, 3),
        StationKind::Grindstone => (GRINDSTONE_INPUT_SLOT, 3, 3),
        StationKind::EnchantingTable => (ENCHANTING_INPUT_SLOT, 2, 2),
        StationKind::Stonecutter => (STONECUTTER_INPUT_SLOT, 1, 2),
        StationKind::Loom => (LOOM_BANNER_SLOT, 3, 4),
        StationKind::CartographyTable => (CARTOGRAPHY_INPUT_SLOT, 2, 3),
    };
    (usize::from(first)..usize::from(first) + inputs, size)
}

/// Station `use`: open its window at the block from the player's window
/// counter, unless a window is already open. The slots live in the
/// player's UI container, so the window content lists those slots.
pub(in crate::server::game) fn open(
    world: &mut World,
    player: Entity,
    at: IVec3,
    kind: StationKind,
) {
    let Some(mut windows) = world.get_mut::<ContainerWindows>(player) else {
        return;
    };
    if windows.open_id().is_some() {
        return;
    }
    let container_id = windows.open(ScreenKind::Station(StationScreen { kind, at }));
    let Some(session) = world.get::<PlayerSession>(player) else {
        return;
    };
    // Measured: an enchanting table window is preceded by empty offers.
    if kind == StationKind::EnchantingTable {
        let _ = session.send(McpePacket::from(PlayerEnchantOptionsPacket::default()));
    }
    let _ = session.send(McpePacket::from(ContainerOpenPacket {
        container_id,
        container_type: wire_type(kind) as u8,
        position: BlockPos {
            x: at.x,
            y: at.y,
            z: at.z,
        },
        target_actor_id: ActorUniqueId {
            actor_unique_id: -1,
        },
    }));
    // BDS lists the station's slots from index 0: empty for a fresh
    // window, the kept anvil inputs on a reopen.
    let block_ids = crate::server::game::types::block_network_ids(world);
    let (inputs, size) = window_slots(kind);
    let mut slots: Vec<_> = world
        .get::<crate::entity::components::PlayerUi>(player)
        .map(|ui| {
            inputs
                .map(|slot| {
                    ui.0.item(slot)
                        .and_then(|stack| super::wire_item(stack, block_ids))
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    slots.resize(size, Default::default());
    let _ = session.send(McpePacket::from(InventoryContentPacket {
        container_id: container_id.into(),
        slots,
        full_container_name: super::UNNAMED_CONTAINER,
        storage_item: Default::default(),
    }));
}

/// What a committed station use still does after its response is sent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum AfterResponse {
    Nothing,
    /// Charge `cost` levels and set the worn anvil block (`None`: unworn).
    Anvil {
        cost: i32,
        wear: Option<(IVec3, u32)>,
    },
    /// Spawn `xp` experience at the player.
    Grindstone {
        xp: u32,
    },
    /// Charge `levels` levels, replace the enchanting seed and resend the
    /// table's offers.
    Enchant {
        levels: i32,
    },
}

/// The anvil wear stage of the block at `at`, with its runtime state.
fn anvil_stage(world: &World, at: IVec3) -> Option<(usize, u32)> {
    let state = crate::server::game::blocks::block_at(world, (at.x, at.y, at.z))?;
    let registry = world.get_resource::<crate::server::game::types::BlockRegistryResource>()?;
    let entry = registry.0.get_by_runtime_id(state)?;
    let stage = ANVIL_STAGES.iter().position(|id| *id == entry.string_id)?;
    Some((stage, state))
}

/// The block an anvil in `state` at `stage` becomes after one wear: the
/// next stage facing the same way, or air.
fn worn_anvil(world: &World, stage: usize, state: u32) -> Option<u32> {
    let Some(next) = ANVIL_STAGES.get(stage + 1) else {
        return Some(*crate::world::chunk::blocks::AIR);
    };
    let registry = &world
        .get_resource::<crate::server::game::types::BlockRegistryResource>()?
        .0;
    let direction = registry
        .get_by_runtime_id(state)?
        .state_value(state, "minecraft:cardinal_direction")?;
    let next = registry.get_by_name(next)?;
    next.with_state_value(
        next.default_state_id,
        "minecraft:cardinal_direction",
        direction,
    )
}

/// The part of a committed station use that precedes its response: an
/// anvil rolls its wear and plays the use (or break) event.
pub(super) fn before_response(
    world: &mut World,
    screen: StationScreen,
    used: StationUse,
    creative: bool,
) -> AfterResponse {
    match used {
        StationUse::Smithing | StationUse::Crafted => AfterResponse::Nothing,
        StationUse::Enchant { levels } => AfterResponse::Enchant {
            levels: if creative { 0 } else { levels },
        },
        StationUse::Grindstone { enchantment_value } => {
            let xp = grindstone_xp(enchantment_value, |half| {
                world
                    .get_resource_or_init::<EntityRandom>()
                    .0
                    .gen_range(0..half)
            });
            AfterResponse::Grindstone {
                xp: u32::try_from(xp).unwrap_or(0),
            }
        }
        StationUse::Anvil { cost } => {
            let cost = if creative { 0 } else { cost };
            let Some((stage, state)) = anvil_stage(world, screen.at) else {
                // The anvil is gone: the open window still works, silently.
                return AfterResponse::Anvil { cost, wear: None };
            };
            let worn = !creative
                && world
                    .get_resource_or_init::<EntityRandom>()
                    .0
                    .r#gen::<f32>()
                    < ANVIL_WEAR_CHANCE;
            let wear = worn
                .then(|| worn_anvil(world, stage, state))
                .flatten()
                .map(|block| (screen.at, block));
            let event = if wear.is_some_and(|(_, block)| block == *crate::world::chunk::blocks::AIR)
            {
                ANVIL_BREAK_EVENT
            } else {
                ANVIL_USE_EVENT
            };
            crate::entity::mob::net::sound::level_event(world, screen.at.as_dvec3(), event, 0);
            AfterResponse::Anvil { cost, wear }
        }
    }
}

/// The part of a committed station use that follows its response.
pub(super) fn after_response(world: &mut World, player: Entity, after: AfterResponse) {
    match after {
        AfterResponse::Nothing => {}
        AfterResponse::Grindstone { xp } => {
            if xp > 0
                && let Some(position) = world.get::<Position>(player).map(|p| p.0)
            {
                crate::entity::xp_orb::spawn_experience(world, position, xp);
            }
        }
        AfterResponse::Anvil { cost, wear } => {
            if let Some((at, block)) = wear {
                crate::server::game::blocks::set_block(world, at, block, false);
            }
            if cost > 0 {
                charge_levels(world, player, cost);
            }
        }
        AfterResponse::Enchant { levels } => {
            if levels > 0 {
                charge_levels(world, player, levels);
            }
            let seed = crate::item::enchanting::new_seed(
                &mut world.get_resource_or_init::<EntityRandom>().0,
            );
            if let Some(mut experience) = world.get_mut::<Experience>(player) {
                experience.enchantment_seed = seed;
            }
        }
    }
}

/// Take `levels` XP levels and resend only the level attribute, as BDS
/// does after an anvil use (the progress within the level is kept).
fn charge_levels(world: &mut World, player: Entity, levels: i32) {
    let Some(mut experience) = world.get_mut::<Experience>(player) else {
        return;
    };
    experience.level = (experience.level - levels).max(0);
    let [level, _] = crate::entity::xp_orb::experience_attributes(&experience);
    let (Some(runtime_id), Some(session)) = (
        world.get::<RuntimeEntityId>(player).map(|id| id.0),
        world.get::<PlayerSession>(player),
    ) else {
        return;
    };
    let _ = session.send(McpePacket::from(UpdateAttributesPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        attribute_list: vec![level],
        tick: crate::entity::components::input_tick_of(world, player),
    }));
}

/// Bookshelves powering an enchanting table, at most
/// [`MAX_BOOKSHELVES`](crate::item::enchanting::MAX_BOOKSHELVES):
/// `bookshelf` and `air` tell whether the block at an offset from the table
/// is a bookshelf or air. Columns two blocks out (on the table's level and
/// one above) count when the two blocks between the column and the table,
/// next to the column, are both air.
pub fn count_bookshelves(
    mut bookshelf: impl FnMut(IVec3) -> bool,
    mut air: impl FnMut(IVec3) -> bool,
) -> u8 {
    let mut count = 0u8;
    for dx in -2..=2i32 {
        for dz in -2..=2i32 {
            if dx.abs() != 2 && dz.abs() != 2 {
                continue;
            }
            let gap = IVec3::new(dx.clamp(-1, 1), 0, dz.clamp(-1, 1));
            if !(air(gap) && air(gap + IVec3::Y)) {
                continue;
            }
            for dy in 0..=1 {
                count += u8::from(bookshelf(IVec3::new(dx, dy, dz)));
            }
        }
    }
    count.min(crate::item::enchanting::MAX_BOOKSHELVES)
}

/// Bookshelves powering the enchanting table at `at` in the world.
pub fn bookshelves(world: &World, at: IVec3) -> u8 {
    let Some(registry) = world.get_resource::<crate::server::game::types::BlockRegistryResource>()
    else {
        return 0;
    };
    let block = |offset: IVec3| {
        let p = at + offset;
        crate::server::game::blocks::block_at(world, (p.x, p.y, p.z))
    };
    count_bookshelves(
        |offset| {
            block(offset)
                .and_then(|state| registry.0.get_by_runtime_id(state))
                .is_some_and(|entry| entry.string_id == "minecraft:bookshelf")
        },
        |offset| block(offset) == Some(*crate::world::chunk::blocks::AIR),
    )
}

/// What the player's open enchanting table at `at` offers depend on, for
/// the request engine.
pub(in crate::server::game) fn enchanting_context(
    world: &World,
    player: Entity,
    at: IVec3,
) -> Option<super::engine::EnchantingContext> {
    Some(super::engine::EnchantingContext {
        seed: world.get::<Experience>(player)?.enchantment_seed,
        bookshelves: bookshelves(world, at),
        first_option_id: world.get::<EnchantOptionIds>(player)?.current?,
    })
}

/// Network IDs of a player's enchanting offers. Measured: the first
/// `PlayerEnchantOptions` of a session names its offers by the three IDs
/// after the last advertised recipe, every later one by the next three,
/// even for the same offers again.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnchantOptionIds {
    /// The first ID of the next options packet.
    next: u32,
    /// The first ID of the offers last sent.
    current: Option<u32>,
}

/// The `PlayerEnchantOptions` for `stack` and `table` (whose
/// `first_option_id` names the first offer): the three offers, each with
/// the item's enchant-slot bits; `None` for an item the table cannot
/// enchant.
pub fn enchant_options(
    stack: &ItemStack,
    table: super::engine::EnchantingContext,
) -> Option<PlayerEnchantOptionsPacket> {
    use jolyne::valentine::bedrock::codec::BedrockCodec;
    let offers = crate::item::enchanting::offers(stack, table.bookshelves, table.seed)?;
    let slot_mask = crate::item::enchanting::enchant_slot(stack);
    let options = offers
        .iter()
        .enumerate()
        .map(|(slot, offer)| {
            let mut lists: [Vec<EnchantmentInstance>; 3] = Default::default();
            for &(id, level) in &offer.enchantments {
                let enchantment = enchantments::get(id).expect("offers name known enchantments");
                let byte = [u8::try_from(id).expect("enchantment ids fit a byte")];
                lists[enchantment.activation.index()].push(EnchantmentInstance {
                    enchant_type: EnumsEnchantType::decode(&mut &byte[..], ())
                        .expect("a byte decodes as an enchantment type"),
                    enchant_level: u8::try_from(level).expect("levels fit a byte"),
                });
            }
            ItemEnchantOption {
                cost: u8::try_from(offer.cost).expect("offer costs are at most 30"),
                enchants: ItemEnchants {
                    slot: slot_mask,
                    item_enchants: lists,
                },
                enchant_name: offer.name.as_str().into(),
                enchant_net_id: TypedServerNetIdstructRecipeNetIdTag {
                    raw_id: table.first_option_id + slot as u32,
                },
            }
        })
        .collect();
    Some(PlayerEnchantOptionsPacket { options })
}

/// After a request changed the input slot of the player's enchanting table
/// at `at` from `before`: send the offers for the new item, or empty offers
/// when the table cannot enchant it. Measured: offers follow an item put
/// in (also the same item again); taking it out, an enchant and an
/// enchanted item send empty offers; lapis sends nothing; all precede the
/// request's response; only non-empty offers take network IDs.
pub(in crate::server::game) fn offer_on_change(
    world: &mut World,
    player: Entity,
    at: IVec3,
    before: &ItemStack,
) {
    let stack = enchanting_input(world, player);
    if same_slot(before, &stack) {
        return;
    }
    let packet = if crate::item::enchanting::can_enchant(&stack) {
        let Some(first_free) = world
            .get_resource::<crate::server::game::types::CraftingExecutorResource>()
            .map(|executor| executor.0.next_free_net_id())
        else {
            return;
        };
        let ids = world
            .get::<EnchantOptionIds>(player)
            .copied()
            .unwrap_or(EnchantOptionIds {
                next: first_free,
                current: None,
            });
        let first = ids.next;
        world.entity_mut(player).insert(EnchantOptionIds {
            next: first + 3,
            current: Some(first),
        });
        let Some(table) = enchanting_context(world, player, at) else {
            return;
        };
        enchant_options(&stack, table).expect("the table enchants the item")
    } else {
        PlayerEnchantOptionsPacket::default()
    };
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(McpePacket::from(packet));
    }
}

/// The item in the player's enchanting-table input slot.
pub(in crate::server::game) fn enchanting_input(world: &World, player: Entity) -> ItemStack {
    world
        .get::<crate::entity::components::PlayerUi>(player)
        .and_then(|ui| {
            ui.0.item(usize::from(super::containers::ENCHANTING_INPUT_SLOT))
                .cloned()
        })
        .unwrap_or_else(ItemStack::empty)
}

/// Whether a slot still holds the same stack (kind, count and network ID).
fn same_slot(before: &ItemStack, after: &ItemStack) -> bool {
    before.is_empty() && after.is_empty()
        || (before.same_kind(after) && before.count == after.count && before.net_id == after.net_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::enchant::enchantments as levels;
    use crate::item::test_support::{VANILLA, stack};

    const SHARPNESS: i16 = 9;
    const SMITE: i16 = 10;
    const KNOCKBACK: i16 = 12;
    const FIRE_ASPECT: i16 = 13;
    const LOOTING: i16 = 14;
    const UNBREAKING: i16 = 17;
    const MENDING: i16 = 26;
    const VANISHING: i16 = 28;
    const PROTECTION: i16 = 0;

    fn enchanted(name: &str, list: &[(i16, i16)]) -> ItemStack {
        with_enchantment_list(&stack(name, 1), list)
    }

    fn damaged(name: &str, damage: i32) -> ItemStack {
        with_durability_damage(&stack(name, 1), damage)
    }

    #[test]
    fn rename_costs_one_and_keeps_the_penalty_at_zero() {
        // BDS (anvil-4): iron sword renamed "Named Once" then "Named Twice",
        // one level each; the result carries RepairCost 0.
        let once = anvil(
            &stack("minecraft:iron_sword", 1),
            &ItemStack::empty(),
            Some("Named Once"),
        )
        .unwrap();
        assert_eq!((once.cost, once.material_used), (1, 0));
        assert_eq!(custom_name(&once.output).as_deref(), Some("Named Once"));
        assert!(has_repair_cost(&once.output) && repair_cost(&once.output) == 0);
        let twice = anvil(&once.output, &ItemStack::empty(), Some("Named Twice")).unwrap();
        assert_eq!(twice.cost, 1);
        assert_eq!(repair_cost(&twice.output), 0);
        // Same name, empty name on an unnamed item, no name: nothing to do.
        assert!(anvil(&twice.output, &ItemStack::empty(), Some("Named Twice")).is_none());
        assert!(anvil(&stack("minecraft:stick", 1), &ItemStack::empty(), Some("")).is_none());
        assert!(anvil(&stack("minecraft:stick", 1), &ItemStack::empty(), None).is_none());
        // An empty name clears a custom name.
        let cleared = anvil(&twice.output, &ItemStack::empty(), Some("")).unwrap();
        assert_eq!(custom_name(&cleared.output), None);
        // A whole stack is renamed and consumed.
        let sticks = anvil(&stack("minecraft:stick", 5), &ItemStack::empty(), Some("S")).unwrap();
        assert_eq!((sticks.input_used, sticks.output.count), (5, 5));
    }

    #[test]
    fn unit_repair_uses_a_quarter_per_material() {
        // BDS (anvil-4): iron pickaxe at 200 damage with 1 ingot → 138 for
        // one level; with 3 ingots → 14 for three.
        let pick = damaged("minecraft:iron_pickaxe", 200);
        let one = anvil(&pick, &stack("minecraft:iron_ingot", 1), Some("")).unwrap();
        assert_eq!(
            (durability_damage(&one.output), one.cost, one.material_used),
            (138, 1, 1)
        );
        assert_eq!(repair_cost(&one.output), 1);
        let three = anvil(&pick, &stack("minecraft:iron_ingot", 3), Some("")).unwrap();
        assert_eq!(
            (
                durability_damage(&three.output),
                three.cost,
                three.material_used
            ),
            (14, 3, 3)
        );
        // Only as many units as the damage needs.
        let four = anvil(&pick, &stack("minecraft:iron_ingot", 64), None).unwrap();
        assert_eq!(
            (durability_damage(&four.output), four.material_used),
            (0, 4)
        );
        // Wrong material, or nothing to repair.
        assert!(anvil(&pick, &stack("minecraft:gold_ingot", 1), None).is_none());
        assert!(
            anvil(
                &stack("minecraft:iron_pickaxe", 1),
                &stack("minecraft:iron_ingot", 1),
                None
            )
            .is_none()
        );
        // Planks repair wooden tools by tag.
        assert!(
            anvil(
                &damaged("minecraft:wooden_sword", 30),
                &stack("minecraft:oak_planks", 1),
                None
            )
            .is_some()
        );
    }

    #[test]
    fn combining_two_items_repairs_with_a_bonus_for_two_levels() {
        // BDS (anvil-4): pickaxes at 100 and 150 damage → 0 for two levels.
        let result = anvil(
            &damaged("minecraft:iron_pickaxe", 100),
            &damaged("minecraft:iron_pickaxe", 150),
            Some(""),
        )
        .unwrap();
        assert_eq!((durability_damage(&result.output), result.cost), (0, 2));
        // 250 max: 50 + 30 remaining + 30 bonus → 140 damage.
        let partial = anvil(
            &damaged("minecraft:iron_pickaxe", 200),
            &damaged("minecraft:iron_pickaxe", 220),
            None,
        )
        .unwrap();
        assert_eq!(durability_damage(&partial.output), 140);
        // Different items do not combine.
        assert!(
            anvil(
                &damaged("minecraft:iron_pickaxe", 100),
                &stack("minecraft:iron_axe", 1),
                None
            )
            .is_none()
        );
    }

    #[test]
    fn enchantments_merge_with_the_bedrock_level_increase_cost() {
        // BDS (anvil-4): Sharpness II + Sharpness II → III for one level.
        let sword = "minecraft:iron_sword";
        let three = anvil(
            &enchanted(sword, &[(SHARPNESS, 2)]),
            &enchanted(sword, &[(SHARPNESS, 2)]),
            Some(""),
        )
        .unwrap();
        assert_eq!(levels(&three.output), [(SHARPNESS, 3)]);
        assert_eq!((three.cost, repair_cost(&three.output)), (1, 1));
        // Then + Unbreaking II: penalty 1 + 2 × 2 = 5, RepairCost 3.
        let next = anvil(
            &three.output,
            &enchanted(sword, &[(UNBREAKING, 2)]),
            Some(""),
        )
        .unwrap();
        assert_eq!(levels(&next.output), [(SHARPNESS, 3), (UNBREAKING, 2)]);
        assert_eq!((next.cost, repair_cost(&next.output)), (5, 3));
        // A sacrifice whose only enchantment conflicts makes nothing.
        assert!(
            anvil(
                &enchanted(sword, &[(SHARPNESS, 2)]),
                &enchanted(sword, &[(SMITE, 2)]),
                Some("")
            )
            .is_none()
        );
        // Levels never pass the maximum, and a lower level adds nothing.
        let max = anvil(
            &enchanted(sword, &[(SHARPNESS, 5)]),
            &enchanted(sword, &[(SHARPNESS, 5), (LOOTING, 1)]),
            None,
        )
        .unwrap();
        assert_eq!(levels(&max.output), [(SHARPNESS, 5), (LOOTING, 1)]);
        assert_eq!(max.cost, 4);
    }

    #[test]
    fn prior_work_penalty_doubles_until_too_expensive() {
        // BDS (anvil-4): a plain sword takes six one-enchantment swords in
        // turn for 1, 3, 7, 11, 17 and 35 levels.
        let sword = "minecraft:iron_sword";
        let mut target = stack(sword, 1);
        let mut costs = Vec::new();
        for id in [
            SHARPNESS,
            UNBREAKING,
            LOOTING,
            FIRE_ASPECT,
            KNOCKBACK,
            MENDING,
        ] {
            let result = anvil(&target, &enchanted(sword, &[(id, 1)]), Some("")).unwrap();
            costs.push(result.cost);
            target = result.output;
        }
        assert_eq!(costs, [1, 3, 7, 11, 17, 35]);
        assert_eq!(repair_cost(&target), 63);
        // The next operation costs at least the 63-level penalty.
        let next = anvil(&target, &ItemStack::empty(), Some("Too much")).unwrap();
        assert!(next.cost >= ANVIL_COST_LIMIT);
    }

    #[test]
    fn books_use_book_multipliers_and_ignore_what_the_target_cannot_take() {
        let book = enchanted("minecraft:enchanted_book", &[(PROTECTION, 3), (LOOTING, 2)]);
        let result = anvil(&stack("minecraft:iron_sword", 1), &book, None).unwrap();
        assert_eq!(levels(&result.output), [(LOOTING, 2)]);
        assert_eq!((result.cost, result.material_used), (4, 1));
        // Books combine into books and take every enchantment.
        let merged = anvil(
            &enchanted("minecraft:enchanted_book", &[(LOOTING, 2)]),
            &book,
            None,
        )
        .unwrap();
        assert_eq!(levels(&merged.output), [(LOOTING, 3), (PROTECTION, 3)]);
        // A book cannot take an item's enchantments.
        assert!(
            anvil(
                &enchanted("minecraft:enchanted_book", &[(LOOTING, 2)]),
                &enchanted("minecraft:iron_sword", &[(SHARPNESS, 1)]),
                None
            )
            .is_none()
        );
    }

    #[test]
    fn grindstone_strips_enchantments_resets_the_penalty_and_refuses_curses() {
        // BDS (grindstone-named): name and damage kept, RepairCost 0.
        let sword = with_repair_cost(
            &enchanted("minecraft:iron_sword", &[(SHARPNESS, 3)]),
            Some(3),
        );
        let sword = with_durability_damage(&with_custom_name(&sword, Some("Named")), 138);
        let result = grindstone(&sword, &ItemStack::empty(), &VANILLA).unwrap();
        assert!(levels(&result.output).is_empty());
        assert!(has_repair_cost(&result.output) && repair_cost(&result.output) == 0);
        assert_eq!(custom_name(&result.output).as_deref(), Some("Named"));
        assert_eq!(durability_damage(&result.output), 138);
        // BDS (grindstone-curse): a curse refuses the whole craft.
        let cursed = enchanted("minecraft:iron_sword", &[(SHARPNESS, 2), (VANISHING, 1)]);
        assert!(grindstone(&cursed, &ItemStack::empty(), &VANILLA).is_none());
        // Sharpness III: minimum enchanting level 23.
        assert_eq!(result.enchantment_value, 23);
        // BDS measured 16 XP for Sharpness III; the range is 12..=23.
        assert_eq!(grindstone_xp(23, |half| half - 8), 16);
        assert_eq!(grindstone_xp(23, |_| 0), 12);
        assert_eq!(grindstone_xp(23, |half| half - 1), 23);
        assert_eq!(grindstone_xp(0, |_| unreachable!()), 0);
        // An enchanted book with only removable enchantments becomes a book.
        let book = grindstone(
            &enchanted("minecraft:enchanted_book", &[(SHARPNESS, 1)]),
            &ItemStack::empty(),
            &VANILLA,
        )
        .unwrap();
        assert_eq!(book.output.item_id(), "minecraft:book");
        assert!(book.output.nbt.is_none());
        // A plain item without a penalty has nothing to grind.
        assert!(
            grindstone(
                &stack("minecraft:iron_sword", 1),
                &ItemStack::empty(),
                &VANILLA
            )
            .is_none()
        );
        // The additional slot alone works like the input slot.
        assert!(
            grindstone(
                &ItemStack::empty(),
                &enchanted("minecraft:iron_sword", &[(SHARPNESS, 1)]),
                &VANILLA
            )
            .is_some()
        );
    }

    #[test]
    fn grindstone_combines_two_items_with_five_percent() {
        let result = grindstone(
            &with_custom_name(&damaged("minecraft:iron_sword", 100), Some("Top")),
            &damaged("minecraft:iron_sword", 150),
            &VANILLA,
        )
        .unwrap();
        // BDS (grindstone): 100 + 150 damage → 0, RepairCost 0, no XP.
        assert_eq!(durability_damage(&result.output), 0);
        assert_eq!(repair_cost(&result.output), 0);
        assert_eq!(custom_name(&result.output).as_deref(), Some("Top"));
        assert_eq!(result.enchantment_value, 0);
        assert!(
            grindstone(
                &damaged("minecraft:iron_sword", 1),
                &with_enchantment_list(&damaged("minecraft:iron_sword", 1), &[(VANISHING, 1)]),
                &VANILLA
            )
            .is_none()
        );
        assert!(
            grindstone(
                &damaged("minecraft:iron_sword", 1),
                &damaged("minecraft:iron_axe", 1),
                &VANILLA
            )
            .is_none()
        );
    }

    #[test]
    fn smithing_keeps_user_data_and_trims_from_the_tables() {
        // BDS (smithing-table): the netherite sword carries the diamond
        // sword's Damage, RepairCost, name and enchantments verbatim.
        let diamond = with_custom_name(
            &with_repair_cost(
                &enchanted(
                    "minecraft:diamond_sword",
                    &[(SHARPNESS, 3), (UNBREAKING, 2)],
                ),
                Some(0),
            ),
            Some("Blade"),
        );
        let diamond = with_durability_damage(&diamond, 100);
        let netherite = transformed(&stack("minecraft:netherite_sword", 1), &diamond).unwrap();
        assert_eq!(netherite.item_id(), "minecraft:netherite_sword");
        assert_eq!(netherite.nbt, diamond.nbt);
        // BDS (smithing-table): coast template + amethyst shard.
        let trimmed = trimmed(
            &stack("minecraft:iron_chestplate", 1),
            &stack("minecraft:coast_armor_trim_smithing_template", 1),
            &stack("minecraft:amethyst_shard", 1),
        )
        .unwrap();
        let tag = crate::item::enchant::item_tag(&trimmed).unwrap();
        let zuri_nbt::NBTTag::Compound(trim) = &tag.0["Trim"] else {
            panic!("Trim compound");
        };
        let text = |key: &str| match &trim.0[key] {
            zuri_nbt::NBTTag::String(value) => value.0.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            (text("Pattern"), text("Material")),
            ("coast".into(), "amethyst".into())
        );
    }

    #[test]
    fn station_blocks_and_window_types() {
        assert_eq!(
            station_of("minecraft:damaged_anvil"),
            Some(StationKind::Anvil)
        );
        assert_eq!(
            station_of("minecraft:grindstone"),
            Some(StationKind::Grindstone)
        );
        assert_eq!(
            station_of("minecraft:smithing_table"),
            Some(StationKind::SmithingTable)
        );
        assert_eq!(station_of("minecraft:crafting_table"), None);
        assert_eq!(
            [
                "minecraft:enchanting_table",
                "minecraft:stonecutter_block",
                "minecraft:loom",
                "minecraft:cartography_table",
                "minecraft:stonecutter",
            ]
            .map(station_of),
            [
                Some(StationKind::EnchantingTable),
                Some(StationKind::Stonecutter),
                Some(StationKind::Loom),
                Some(StationKind::CartographyTable),
                None,
            ]
        );
        assert_eq!(
            [
                StationKind::EnchantingTable,
                StationKind::Stonecutter,
                StationKind::Loom,
                StationKind::CartographyTable,
            ]
            .map(|kind| (wire_type(kind), window_slots(kind))),
            [
                (3, (14..16, 2)),
                (29, (3..4, 2)),
                (24, (9..12, 4)),
                (30, (12..14, 3))
            ]
        );
        assert_eq!(
            [
                StationKind::Anvil,
                StationKind::SmithingTable,
                StationKind::Grindstone
            ]
            .map(wire_type),
            [5, 33, 26]
        );
    }
}
