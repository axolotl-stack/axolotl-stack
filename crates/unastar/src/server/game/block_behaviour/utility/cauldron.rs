//! Cauldrons (`fill_level` 0..=6, `cauldron_liquid` water/lava/
//! powder_snow; the `Cauldron` block actor keeps `PotionId` (−1 none),
//! `PotionType` (0 drinkable, 1 splash, 2 lingering) and `CustomColor`
//! (ARGB, written only for dyed water)).
//!
//! Use (handled for every held item; an empty hand does nothing):
//!
//! - **Filled buckets** fill to 6: water into an empty cauldron or water
//!   without a potion (any dye is washed out, LevelEvent 3506), lava into an
//!   empty or lava cauldron (3512), powder snow into an empty or powder
//!   snow cauldron (3514). Any other contents are destroyed ([`explode`]):
//!   the bucket still empties and the cauldron is left empty.
//! - **An empty bucket** takes a full (6), potion-free cauldron: a water
//!   (also dyed water, 3507), lava (3513) or powder snow (3515) bucket.
//! - **A glass bottle** takes two levels from a cauldron holding at least 2:
//!   the potion inside (3505) or, from undyed water, a water bottle (3507),
//!   with `bottle.fill` at the centre. Taking the last levels clears the
//!   potion and colour.
//! - **Potions** (aux = potion id): a water bottle adds two levels of water
//!   to an empty or water cauldron (3506); an effect potion (id above
//!   awkward's, 4) adds two levels to an empty cauldron or one holding the
//!   same potion at level 5 or less (3504, `bottle.empty`); either gives
//!   the bottle back. A water bottle into a potion, lava or powder snow,
//!   or an effect potion into water or another potion destroys the
//!   contents; mundane, thick and awkward potions do nothing.
//! - **Arrows** (plain) dipped into a potion with an effect take
//!   `min(held, level ≤ 2 ? 16 : 16 × level − 32)` tipped arrows (aux =
//!   potion id + 1); the level drops by `(count + 15) / 16` and the potion
//!   clears below level 3.
//! - **Dye** (`<colour>_dye`) into water without a potion colours it (the
//!   average with the colour already there), LevelEvent 3508, one dye used.
//! - **Leather armour** in dyed water takes its colour (item NBT
//!   `customColor`, 3502); undyed water washes a dyed piece (3503). Both
//!   take one level, and the colour clears when the water is gone.
//! - **Dyed shulker boxes** in undyed water become undyed shulker boxes
//!   (NBT kept, 3503); **banners** with patterns lose their last pattern
//!   (3509). Both take one level.
//!
//! Items change only outside creative; a stack of several gets one less and
//! the result goes to the inventory. Every change resends the actor (with
//! an empty `Items` list; no potion is `PotionId` −1, `PotionType` −1).
//! LevelEvents play at `(x + 0.5, y + 0.375 + 0.125 × level, z + 0.5)`,
//! `level` the higher of the levels before and after, with the liquid's
//! colour as data; buckets play no LevelSoundEvent, bottles `bottle.fill`
//! (data −1). Comparator output ([`comparator_signal`]) is
//! `(fill_level + 1) / 2`.
//!
//! Provisional: the destroyed-contents effect (`fizz` LevelSoundEvent;
//! the server's own effect was not identified), the bucket and bottle
//! amounts (2 levels per bottle, 6 per bucket), the dye mixing average and
//! the dye colours (Minecraft Wiki). Not modelled: rain and snow filling
//! (no weather), dripstone drips.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use zuri_nbt::{NBTTag, tag};

use super::super::{Place, actors, centre, int, with};
use crate::entity::components::GameMode;
use crate::entity::systems::BlockSource;
use crate::item::ItemStack;
use crate::item::enchant::{item_tag, with_item_tag};
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::item_use::{give_or_drop, held, set_slot};

/// State properties.
pub const LEVEL: &str = "fill_level";
pub const LIQUID: &str = "cauldron_liquid";
/// The cauldron's block actor id.
pub const ACTOR: &str = "Cauldron";
/// A full cauldron.
pub const FULL: i32 = 6;
/// The highest potion id without effects (awkward).
pub const AWKWARD: i16 = 4;
/// LevelEvent data of a cauldron without potion or dye (the plain liquid's
/// colour as the server sends it).
pub const PLAIN_EVENT_DATA: i32 = 0xBF34_5159_u32 as i32;

/// Cauldron LevelEvents.
pub const DYE_ARMOUR: i32 = 3502;
pub const CLEAN_ARMOUR: i32 = 3503;
pub const FILL_POTION: i32 = 3504;
pub const TAKE_POTION: i32 = 3505;
pub const FILL_WATER: i32 = 3506;
pub const TAKE_WATER: i32 = 3507;
pub const ADD_DYE: i32 = 3508;
pub const CLEAN_BANNER: i32 = 3509;
pub const FILL_LAVA: i32 = 3512;
pub const TAKE_LAVA: i32 = 3513;
pub const FILL_POWDER_SNOW: i32 = 3514;
pub const TAKE_POWDER_SNOW: i32 = 3515;

/// The liquid in a cauldron.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liquid {
    Water,
    Lava,
    PowderSnow,
}

impl Liquid {
    fn name(self) -> &'static str {
        match self {
            Self::Water => "water",
            Self::Lava => "lava",
            Self::PowderSnow => "powder_snow",
        }
    }
}

/// What a cauldron holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contents {
    pub level: i32,
    pub liquid: Liquid,
    /// Potion id and type (0 drinkable, 1 splash, 2 lingering).
    pub potion: Option<(i16, i16)>,
    /// ARGB colour of dyed water.
    pub colour: Option<i32>,
}

impl Contents {
    const EMPTY: Self = Self {
        level: 0,
        liquid: Liquid::Water,
        potion: None,
        colour: None,
    };

    fn is_empty(self) -> bool {
        self.level == 0
    }

    /// Water without a potion (dyed or not).
    fn is_water(self) -> bool {
        self.level > 0 && self.liquid == Liquid::Water && self.potion.is_none()
    }

    /// Water without a potion or dye.
    fn is_plain_water(self) -> bool {
        self.is_water() && self.colour.is_none()
    }

    fn filled(liquid: Liquid, level: i32) -> Self {
        Self {
            level,
            liquid,
            potion: None,
            colour: None,
        }
    }

    /// These contents with `level`; an emptied cauldron forgets its potion
    /// and colour.
    fn at_level(self, level: i32) -> Self {
        let level = level.clamp(0, FULL);
        if level == 0 {
            Self::EMPTY
        } else {
            Self { level, ..self }
        }
    }
}

fn short(compound: &tag::Compound, key: &str) -> Option<i16> {
    match compound.0.get(key)? {
        NBTTag::Short(value) => Some(value.0),
        NBTTag::Int(value) => i16::try_from(value.0).ok(),
        _ => None,
    }
}

/// The contents of the cauldron `state` at `pos`.
pub fn contents(world: &World, pos: IVec3, state: u32) -> Contents {
    let Some(entry) = world.registry().get_by_runtime_id(state) else {
        return Contents::EMPTY;
    };
    let level = int(entry, state, LEVEL).unwrap_or(0);
    let liquid = match entry.state_value(state, LIQUID) {
        Some(StateValue::Str("lava")) => Liquid::Lava,
        Some(StateValue::Str("powder_snow")) => Liquid::PowderSnow,
        _ => Liquid::Water,
    };
    let actor = actors::read_kind(world, pos, ACTOR);
    let potion = actor.as_ref().and_then(|actor| {
        let id = short(actor, "PotionId").filter(|id| *id >= 0)?;
        Some((id, short(actor, "PotionType").unwrap_or(0)))
    });
    let colour = actor
        .as_ref()
        .and_then(|actor| match actor.0.get("CustomColor") {
            Some(NBTTag::Int(colour)) => Some(colour.0),
            _ => None,
        });
    Contents {
        level,
        liquid,
        potion,
        colour,
    }
    .at_level(level)
}

/// Write `contents` into the cauldron at `pos`: its state and its actor.
pub fn set_contents(world: &mut World, pos: IVec3, contents: Contents) {
    let Some(state) = world.block_at(pos) else {
        return;
    };
    let Some(entry) = world.registry().get_by_runtime_id(state).cloned() else {
        return;
    };
    let mut actor = actors::new_compound(ACTOR, pos);
    let (id, kind) = contents.potion.unwrap_or((-1, -1));
    actor
        .0
        .insert("Items".into(), NBTTag::List(tag::List(Vec::new())));
    actor
        .0
        .insert("PotionId".into(), NBTTag::Short(tag::Short(id)));
    actor
        .0
        .insert("PotionType".into(), NBTTag::Short(tag::Short(kind)));
    if let Some(colour) = contents.colour {
        actor
            .0
            .insert("CustomColor".into(), NBTTag::Int(tag::Int(colour)));
    }
    actors::store(world, pos, actor);
    let next = with(&entry, state, LEVEL, StateValue::Int(contents.level));
    let next = with(
        &entry,
        next,
        LIQUID,
        StateValue::Str(contents.liquid.name()),
    );
    if next != state {
        set_block(world, pos, next, false);
    }
}

/// Turn the contents `before` of the cauldron at `pos` into `after` and
/// send LevelEvent `id` at `(x + 0.5, y + 0.375 + 0.125 × level, z + 0.5)`,
/// `level` the higher of the two levels, with the liquid's colour as data
/// (the dye, else the potion's colour, else [`PLAIN_EVENT_DATA`]).
fn change(world: &mut World, pos: IVec3, before: Contents, after: Contents, id: i32) {
    set_contents(world, pos, after);
    let level = before.level.max(after.level);
    let at = pos.as_dvec3() + DVec3::new(0.5, 0.375 + 0.125 * f64::from(level), 0.5);
    let data = match (
        after.colour.or(before.colour),
        after.potion.or(before.potion),
    ) {
        (Some(colour), _) => colour,
        (None, Some((potion, _))) => crate::server::game::effects::potion_color(potion),
        (None, None) => PLAIN_EVENT_DATA,
    };
    crate::entity::mob::net::sound::level_event(world, at, id, data);
}

/// Play the LevelSoundEvent `name` at the centre of `pos` (data −1).
fn sound(world: &mut World, pos: IVec3, name: &str) {
    crate::server::game::blocks::queue_level_sound(world, None, centre(pos), name, -1);
}

/// Destroy the contents of the cauldron at `pos` (mixing what does not
/// mix): it is left empty with a `fizz`.
pub fn explode(world: &mut World, pos: IVec3) {
    set_contents(world, pos, Contents::EMPTY);
    sound(world, pos, "fizz");
}

/// The held stack after using one item: outside creative, `result` takes
/// its place (or joins the inventory when the stack held more than one).
fn exchange(
    world: &mut World,
    player: Entity,
    slot: u8,
    stack: &ItemStack,
    result: Option<ItemStack>,
) {
    if world.get::<GameMode>(player) == Some(&GameMode::Creative) {
        return;
    }
    let rest = stack.grow(-1);
    match result {
        Some(result) if rest.is_empty() => set_slot(world, player, slot, result),
        result => {
            set_slot(world, player, slot, rest);
            if let Some(result) = result {
                give_or_drop(world, player, result);
            }
        }
    }
}

fn stack(world: &World, id: &str, damage: i16) -> Option<ItemStack> {
    crate::server::game::world_stack(world, id, 1).map(|stack| stack.with_damage(damage))
}

/// The ARGB colour of a dye item (`<colour>_dye`).
pub fn dye_colour(item: &str) -> Option<i32> {
    let rgb: u32 = match item.strip_prefix("minecraft:")?.strip_suffix("_dye")? {
        "white" => 0xF9FFFE,
        "orange" => 0xF9801D,
        "magenta" => 0xC74EBD,
        "light_blue" => 0x3AB3DA,
        "yellow" => 0xFED83D,
        "lime" => 0x80C71F,
        "pink" => 0xF38BAA,
        "gray" => 0x474F52,
        "light_gray" => 0x9D9D97,
        "cyan" => 0x169C9C,
        "purple" => 0x8932B8,
        "blue" => 0x3C44AA,
        "brown" => 0x835432,
        "green" => 0x5E7C16,
        "red" => 0xB02E26,
        "black" => 0x1D1D21,
        _ => return None,
    };
    Some((0xFF00_0000 | rgb) as i32)
}

/// The per-channel average of two ARGB colours (opaque).
fn mix(a: i32, b: i32) -> i32 {
    let (a, b) = (a as u32, b as u32);
    let channel = |shift: u32| ((((a >> shift) & 0xFF) + ((b >> shift) & 0xFF)) / 2) << shift;
    (0xFF00_0000 | channel(16) | channel(8) | channel(0)) as i32
}

fn is_leather_armour(item: &str) -> bool {
    matches!(
        item,
        "minecraft:leather_helmet"
            | "minecraft:leather_chestplate"
            | "minecraft:leather_leggings"
            | "minecraft:leather_boots"
    )
}

/// The `customColor` of a dyed item.
pub fn item_colour(stack: &ItemStack) -> Option<i32> {
    match item_tag(stack)?.0.get("customColor")? {
        NBTTag::Int(colour) => Some(colour.0),
        _ => None,
    }
}

/// `stack` with its `customColor` set (or removed).
fn with_colour(stack: &ItemStack, colour: Option<i32>) -> ItemStack {
    let mut compound = item_tag(stack).unwrap_or_default();
    match colour {
        Some(colour) => {
            compound
                .0
                .insert("customColor".into(), NBTTag::Int(tag::Int(colour)));
        }
        None => {
            compound.0.remove("customColor");
        }
    }
    with_item_tag(stack, compound)
}

/// Whether a player's `use` on `state` is a block interaction: always.
pub fn is_usable(_entry: &BlockEntry, _state: u32) -> bool {
    true
}

/// A player's `use` with the held item (see the module docs).
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, state: u32) -> bool {
    let Some((slot, held)) = held(world, player) else {
        return true;
    };
    let now = contents(world, pos, state);
    let item = held.item_id().to_owned();
    match item.as_str() {
        "minecraft:water_bucket" | "minecraft:lava_bucket" | "minecraft:powder_snow_bucket" => {
            let (liquid, fill) = match item.as_str() {
                "minecraft:water_bucket" => (Liquid::Water, FILL_WATER),
                "minecraft:lava_bucket" => (Liquid::Lava, FILL_LAVA),
                _ => (Liquid::PowderSnow, FILL_POWDER_SNOW),
            };
            let fills = now.is_empty()
                || match liquid {
                    Liquid::Water => now.is_water(),
                    _ => now.liquid == liquid && now.potion.is_none() && now.colour.is_none(),
                };
            let bucket = stack(world, "minecraft:bucket", 0);
            exchange(world, player, slot, &held, bucket);
            if fills {
                change(world, pos, now, Contents::filled(liquid, FULL), fill);
            } else {
                explode(world, pos);
            }
        }
        "minecraft:bucket" => {
            if now.level < FULL || now.potion.is_some() {
                return true;
            }
            let (bucket, take) = match now.liquid {
                Liquid::Water => ("minecraft:water_bucket", TAKE_WATER),
                Liquid::Lava => ("minecraft:lava_bucket", TAKE_LAVA),
                Liquid::PowderSnow => ("minecraft:powder_snow_bucket", TAKE_POWDER_SNOW),
            };
            let filled = stack(world, bucket, 0);
            exchange(world, player, slot, &held, filled);
            change(world, pos, now, Contents::EMPTY, take);
        }
        "minecraft:glass_bottle" => {
            if now.level < 2 {
                return true;
            }
            let (bottle, take) = match now.potion {
                Some((id, kind)) => {
                    let item = match kind {
                        1 => "minecraft:splash_potion",
                        2 => "minecraft:lingering_potion",
                        _ => "minecraft:potion",
                    };
                    (stack(world, item, id), TAKE_POTION)
                }
                None if now.is_plain_water() => (stack(world, "minecraft:potion", 0), TAKE_WATER),
                None => return true,
            };
            exchange(world, player, slot, &held, bottle);
            change(world, pos, now, now.at_level(now.level - 2), take);
            sound(world, pos, "bottle.fill");
        }
        "minecraft:potion" | "minecraft:splash_potion" | "minecraft:lingering_potion" => {
            pour(world, player, pos, slot, &held, now);
        }
        "minecraft:arrow" if held.damage == 0 => tip_arrows(world, player, pos, slot, &held, now),
        _ => wash_or_dye(world, player, pos, slot, &held, now),
    }
    true
}

/// A potion poured into the cauldron.
fn pour(world: &mut World, player: Entity, pos: IVec3, slot: u8, held: &ItemStack, now: Contents) {
    let id = held.damage;
    let kind = match held.item_id() {
        "minecraft:splash_potion" => 1,
        "minecraft:lingering_potion" => 2,
        _ => 0,
    };
    if id > 0 && id <= AWKWARD {
        return;
    }
    if now.level >= FULL {
        return;
    }
    let bottle = stack(world, "minecraft:glass_bottle", 0);
    exchange(world, player, slot, held, bottle);
    if id == 0 {
        if now.is_empty() || now.is_water() {
            let base = if now.is_empty() {
                Contents::filled(Liquid::Water, 0)
            } else {
                now
            };
            let after = Contents {
                level: (now.level + 2).min(FULL),
                ..base
            };
            change(world, pos, now, after, FILL_WATER);
        } else {
            explode(world, pos);
        }
        return;
    }
    let same = now.potion == Some((id, kind)) && now.level <= 5;
    if now.is_empty() || same {
        let after = Contents {
            level: (now.level + 2).min(FULL),
            liquid: Liquid::Water,
            potion: Some((id, kind)),
            colour: None,
        };
        change(world, pos, now, after, FILL_POTION);
        sound(world, pos, "bottle.empty");
    } else {
        explode(world, pos);
    }
}

/// Plain arrows dipped into a potion with an effect become tipped arrows.
fn tip_arrows(
    world: &mut World,
    player: Entity,
    pos: IVec3,
    slot: u8,
    held: &ItemStack,
    now: Contents,
) {
    let Some((id, _)) = now.potion.filter(|(id, _)| *id > AWKWARD) else {
        return;
    };
    if now.level == 0 {
        return;
    }
    let most = if now.level <= 2 {
        16
    } else {
        now.level * 16 - 32
    };
    let count = i32::from(held.count).min(most);
    let Some(tipped) = stack(world, "minecraft:arrow", id + 1) else {
        return;
    };
    if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
        set_slot(world, player, slot, held.grow(-(count as i16)));
    }
    give_or_drop(world, player, tipped.with_count(count as u8));
    let level = now.level - ((count + 15) >> 4);
    let left = now.at_level(level);
    let left = if level < 3 {
        Contents {
            potion: None,
            ..left
        }
    } else {
        left
    };
    set_contents(world, pos, left);
}

/// Dye, leather armour, shulker boxes and banners in water.
fn wash_or_dye(
    world: &mut World,
    player: Entity,
    pos: IVec3,
    slot: u8,
    held: &ItemStack,
    now: Contents,
) {
    let item = held.item_id();
    if let Some(dye) = dye_colour(item) {
        if !now.is_water() {
            return;
        }
        let colour = now.colour.map_or(dye, |old| mix(old, dye));
        if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
            set_slot(world, player, slot, held.grow(-1));
        }
        change(
            world,
            pos,
            now,
            Contents {
                colour: Some(colour),
                ..now
            },
            ADD_DYE,
        );
        return;
    }
    if !now.is_water() {
        return;
    }
    let one_level_less = |now: Contents| {
        let left = now.at_level(now.level - 1);
        if now.level < 2 { Contents::EMPTY } else { left }
    };
    if is_leather_armour(item) {
        let (result, id) = match now.colour {
            Some(colour) => (with_colour(held, Some(colour)), DYE_ARMOUR),
            None if item_colour(held).is_some() => (with_colour(held, None), CLEAN_ARMOUR),
            None => return,
        };
        if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
            set_slot(world, player, slot, result);
        }
        change(world, pos, now, one_level_less(now), id);
        return;
    }
    if !now.is_plain_water() {
        return;
    }
    let shulker = item
        .strip_suffix("_shulker_box")
        .filter(|colour| *colour != "minecraft:undyed");
    if shulker.is_some() {
        let Some(undyed) = stack(world, "minecraft:undyed_shulker_box", 0) else {
            return;
        };
        let undyed = match item_tag(held) {
            Some(compound) => with_item_tag(&undyed.with_count(held.count), compound),
            None => undyed.with_count(held.count),
        };
        if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
            set_slot(world, player, slot, undyed);
        }
        change(world, pos, now, one_level_less(now), CLEAN_ARMOUR);
        return;
    }
    if item == "minecraft:banner" {
        let Some(mut compound) = item_tag(held) else {
            return;
        };
        let Some(NBTTag::List(patterns)) = compound.0.get_mut("Patterns") else {
            return;
        };
        if patterns.0.pop().is_none() {
            return;
        }
        if patterns.0.is_empty() {
            compound.0.remove("Patterns");
        }
        let cleaned = with_item_tag(&held.clone().with_count(1), compound);
        exchange(world, player, slot, held, Some(cleaned));
        change(world, pos, now, one_level_less(now), CLEAN_BANNER);
    }
}

/// Comparator output: `(fill_level + 1) / 2`.
pub fn comparator_signal(entry: &BlockEntry, state: u32) -> u8 {
    ((int(entry, state, LEVEL).unwrap_or(0).clamp(0, FULL) + 1) / 2) as u8
}

/// Placement keeps the generic state (an empty cauldron).
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    Some(place.state)
}

/// A cauldron stands anywhere.
pub fn can_survive(
    _blocks: &impl BlockSource,
    _pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    true
}

/// Nothing happens on placement.
pub fn placed(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Neighbours do not affect a cauldron.
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// Cauldrons schedule no ticks.
pub fn scheduled_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Random ticks do not reach cauldrons (no weather fills them).
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Redstone power does not act on cauldrons.
pub fn power_changed(_world: &mut World, _pos: IVec3, _state: u32, _powered: bool) {}

/// A cauldron's contents are not items: breaking drops only the cauldron.
pub fn breaking(_world: &mut World, _pos: IVec3, _state: u32) {}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};
    use jolyne::valentine::McpePacketData;

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::entity::components::MainInventory;

    const AT: IVec3 = IVec3::new(2, FLOOR + 1, 2);
    const TOP: DVec3 = DVec3::new(0.5, 1.0, 0.5);

    fn now(h: &Harness) -> Contents {
        contents(&h.world, AT, h.state(AT))
    }

    fn held_item(h: &Harness) -> Option<ItemStack> {
        crate::server::game::item_use::held(&h.world, h.player).map(|(_, stack)| stack)
    }

    fn hold_stack(h: &mut Harness, stack: ItemStack) {
        h.world
            .get_mut::<MainInventory>(h.player)
            .unwrap()
            .0
            .set_item(0, stack)
            .unwrap();
    }

    fn events(h: &mut Harness) -> Vec<i32> {
        h.packets()
            .into_iter()
            .filter_map(|packet| match packet {
                McpePacketData::LevelEventPacket(event) => Some(event.event_id),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn buckets_fill_and_empty_it() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:cauldron", &[]));
        h.hold("minecraft:water_bucket", 1);
        h.packets();
        h.click(AT, 1, TOP);
        assert_eq!(now(&h), Contents::filled(Liquid::Water, FULL));
        assert_eq!(held_item(&h).unwrap().item_id(), "minecraft:bucket");
        assert!(events(&mut h).contains(&FILL_WATER));
        h.click(AT, 1, TOP);
        assert_eq!(now(&h).level, 0);
        assert_eq!(held_item(&h).unwrap().item_id(), "minecraft:water_bucket");
        h.hold("minecraft:lava_bucket", 1);
        h.click(AT, 1, TOP);
        assert_eq!(now(&h), Contents::filled(Liquid::Lava, FULL));
        assert_eq!(h.value(AT, LIQUID), Some(StateValue::Str("lava")));
        h.hold("minecraft:bucket", 1);
        h.click(AT, 1, TOP);
        assert_eq!(held_item(&h).unwrap().item_id(), "minecraft:lava_bucket");
        assert_eq!(h.value(AT, LEVEL), Some(StateValue::Int(0)));
    }

    #[test]
    fn bottles_take_and_add_two_levels() {
        let mut h = Harness::new();
        h.put(
            AT,
            state("minecraft:cauldron", &[(LEVEL, StateValue::Int(FULL))]),
        );
        h.hold("minecraft:glass_bottle", 2);
        h.click(AT, 1, TOP);
        assert_eq!(now(&h).level, 4);
        assert_eq!(held_item(&h).unwrap().count, 1);
        let inventory = h.world.get::<MainInventory>(h.player).unwrap().0.clone();
        let water_bottle = inventory
            .slots()
            .iter()
            .find(|stack| stack.item_id() == "minecraft:potion")
            .cloned()
            .expect("a water bottle");
        assert_eq!(water_bottle.damage, 0);
        h.hold("minecraft:potion", 1);
        h.click(AT, 1, TOP);
        assert_eq!(now(&h).level, FULL);
        assert_eq!(held_item(&h).unwrap().item_id(), "minecraft:glass_bottle");
    }

    #[test]
    fn potions_fill_and_mixing_destroys_the_contents() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:cauldron", &[]));
        let healing = crate::server::game::world_stack(&h.world, "minecraft:potion", 1)
            .unwrap()
            .with_damage(21);
        hold_stack(&mut h, healing.clone());
        h.packets();
        h.click(AT, 1, TOP);
        assert_eq!(now(&h).level, 2);
        assert_eq!(now(&h).potion, Some((21, 0)));
        assert!(events(&mut h).contains(&FILL_POTION));
        hold_stack(&mut h, healing.with_damage(5));
        h.click(AT, 1, TOP);
        assert_eq!(now(&h), Contents::EMPTY);
        assert_eq!(held_item(&h).unwrap().item_id(), "minecraft:glass_bottle");
    }

    #[test]
    fn dye_colours_water_and_leather_takes_the_colour() {
        let mut h = Harness::new();
        h.put(
            AT,
            state("minecraft:cauldron", &[(LEVEL, StateValue::Int(3))]),
        );
        h.hold("minecraft:red_dye", 1);
        h.click(AT, 1, TOP);
        let red = dye_colour("minecraft:red_dye").unwrap();
        assert_eq!(now(&h).colour, Some(red));
        assert_eq!(now(&h).level, 3);
        h.hold("minecraft:leather_boots", 1);
        h.packets();
        h.click(AT, 1, TOP);
        assert_eq!(item_colour(&held_item(&h).unwrap()), Some(red));
        assert_eq!(now(&h).level, 2);
        assert!(events(&mut h).contains(&DYE_ARMOUR));
        // Dyed water gives no water bottle.
        h.hold("minecraft:glass_bottle", 1);
        h.click(AT, 1, TOP);
        assert_eq!(now(&h).level, 2);
    }

    #[test]
    fn comparator_reads_half_the_level() {
        let entry = crate::registry::BlockRegistry::vanilla()
            .get_by_name("minecraft:cauldron")
            .unwrap();
        let signals: Vec<u8> = (0..=FULL)
            .map(|level| {
                comparator_signal(
                    entry,
                    state("minecraft:cauldron", &[(LEVEL, StateValue::Int(level))]),
                )
            })
            .collect();
        assert_eq!(signals, vec![0, 1, 1, 2, 2, 3, 3]);
    }
}

/// Replays the BDS 1.26.51 utility capture
/// (`testdata/bds_1_26_51_utility.json`) for the composter, cauldron,
/// campfire, beehive, bell, jukebox, flower pot and cake scenarios and
/// compares the watched cells (final states and change timing relative to
/// the first change), LevelSoundEvents, LevelEvents, PlaySounds, popups,
/// dropped items (and when they appear), the held stack, the block actor
/// and the player's food.
///
/// The capture's arena floor is at y = −61, the harness floor at
/// [`FLOOR`](super::super::super::tests::FLOOR): positions shift by the
/// difference. A player's click lands about two ticks after the capture's
/// tick 0, so dropped items are compared at `capture tick − 2` (±1).
/// Composter rolls are random: before a roll the block random is seeded so
/// that the roll comes out as it did in the capture. The campfire's
/// ambient crackle is not modelled and not compared.
#[cfg(test)]
mod bds_capture {
    use glam::{DVec3, IVec3};
    use jolyne::valentine::McpePacketData;
    use rand::{Rng, SeedableRng};
    use serde_json::Value;
    use zuri_nbt::NBTTag;

    use super::super::super::tests::{FLOOR, Harness};
    use crate::entity::components::{Hunger, MainInventory};
    use crate::registry::block::StateValue;

    const FIXTURE: &str = include_str!("../testdata/bds_1_26_51_utility.json");
    const BDS_FLOOR: i32 = -61;
    const SHIFT: i32 = FLOOR - BDS_FLOOR;
    /// The tick a player's click lands on in the capture.
    const LANDS: i64 = 2;

    fn shift(pos: &Value) -> IVec3 {
        let at = |i: usize| pos[i].as_i64().unwrap() as i32;
        IVec3::new(at(0), at(1) + SHIFT, at(2))
    }

    fn shift_f(pos: &Value) -> DVec3 {
        let at = |i: usize| pos[i].as_f64().unwrap();
        DVec3::new(at(0), at(1) + f64::from(SHIFT), at(2))
    }

    fn face(name: &str) -> u8 {
        match name {
            "down" => 0,
            "up" => 1,
            "north" => 2,
            "south" => 3,
            "west" => 4,
            "east" => 5,
            other => panic!("face {other}"),
        }
    }

    /// `name[k=v,...]` (keys sorted, bytes as booleans, strings quoted), or
    /// the bare name without states.
    fn notation(state: u32) -> String {
        let entry = crate::registry::BlockRegistry::vanilla()
            .get_by_runtime_id(state)
            .unwrap();
        let mut states: Vec<String> = entry
            .data
            .properties
            .iter()
            .map(|property| {
                let value = match entry.state_value(state, property.name).unwrap() {
                    StateValue::Byte(b) => (b != 0).to_string(),
                    StateValue::Int(i) => i.to_string(),
                    StateValue::Str(s) => format!("\"{s}\""),
                };
                format!("{}={value}", property.name)
            })
            .collect();
        states.sort();
        if states.is_empty() {
            return entry.string_id.clone();
        }
        format!("{}[{}]", entry.string_id, states.join(","))
    }

    fn parse(notation_text: &str) -> u32 {
        let registry = crate::registry::BlockRegistry::vanilla();
        let name = notation_text.split('[').next().unwrap();
        let entry = registry.get_by_name(name).unwrap();
        (entry.min_state_id..=entry.max_state_id)
            .find(|state| notation(*state) == notation_text)
            .unwrap_or_else(|| panic!("no state {notation_text}"))
    }

    /// The stack each step starts with in the selected slot: `None` keeps
    /// what the previous step left, `Some(None)` is an empty hand.
    fn held_before(scenario: &str, step: &str) -> Option<Option<(&'static str, u8)>> {
        Some(Some(match (scenario, step) {
            ("composter", "seeds_1") => ("minecraft:wheat_seeds", 64),
            ("composter", "use_at_8") => return Some(None),
            ("composter", _) => return None,
            ("cauldron", "water_bucket") => ("minecraft:water_bucket", 1),
            ("cauldron", "glass_bottle_1") => ("minecraft:glass_bottle", 3),
            ("cauldron", "glass_bottle_2") => ("minecraft:glass_bottle", 2),
            ("cauldron", "glass_bottle_3") => ("minecraft:glass_bottle", 1),
            ("cauldron", "lava_bucket") => ("minecraft:lava_bucket", 1),
            ("cauldron", "empty_bucket") => ("minecraft:bucket", 1),
            ("campfire", "place") => ("minecraft:campfire", 64),
            ("campfire", "raw_beef") => ("minecraft:beef", 4),
            ("campfire", "wooden_shovel") => ("minecraft:wooden_shovel", 1),
            ("beehive", "shears") => ("minecraft:shears", 1),
            ("beehive", "glass_bottle") => ("minecraft:glass_bottle", 1),
            ("bell", "lever_place") => ("minecraft:lever", 64),
            ("jukebox", "insert_disc") => ("minecraft:music_disc_13", 1),
            ("flower_pot", "poppy") => ("minecraft:poppy", 1),
            _ => return Some(None),
        }))
    }

    fn hold(h: &mut Harness, held: Option<(&str, u8)>) {
        match held {
            Some((item, count)) => h.hold(item, count),
            None => {
                h.world
                    .get_mut::<MainInventory>(h.player)
                    .unwrap()
                    .0
                    .set_item(0, crate::item::ItemStack::empty())
                    .unwrap();
            }
        }
    }

    /// Whether our actor tag `ours` holds the capture's `theirs`.
    fn nbt_holds(ours: &NBTTag, theirs: &Value) -> bool {
        match (ours, theirs) {
            (NBTTag::Byte(v), Value::Number(n)) => i64::from(v.0) == n.as_i64().unwrap(),
            (NBTTag::Short(v), Value::Number(n)) => i64::from(v.0) == n.as_i64().unwrap(),
            (NBTTag::Int(v), Value::Number(n)) => i64::from(v.0) == n.as_i64().unwrap(),
            (NBTTag::Long(v), Value::Number(n)) => v.0 == n.as_i64().unwrap(),
            (NBTTag::String(v), Value::String(s)) => v.0 == *s,
            (NBTTag::List(v), Value::Array(a)) => {
                v.0.len() == a.len() && v.0.iter().zip(a).all(|(o, t)| nbt_holds(o, t))
            }
            (NBTTag::Compound(c), Value::Object(o)) => o.iter().all(|(key, value)| {
                matches!(key.as_str(), "BlockEntityVersion" | "TicksPlaying")
                    || c.0.get(key).is_some_and(|ours| nbt_holds(ours, value))
            }),
            _ => false,
        }
    }

    /// What one step sent and changed, in the capture's terms.
    #[derive(Default)]
    struct Seen {
        sounds: Vec<String>,
        events: Vec<String>,
        play_sounds: Vec<String>,
        messages: Vec<String>,
    }

    fn collect(h: &mut Harness, seen: &mut Seen) {
        let ids = crate::server::game::types::block_network_ids(&h.world);
        for packet in h.packets() {
            match packet {
                McpePacketData::LevelSoundEventPacket(s) => {
                    let name = s.sound_event.to_string();
                    if name == "block.campfire.crackle" {
                        continue;
                    }
                    let data = if s.data == -1 {
                        "-1".to_string()
                    } else {
                        ids.from_wire(s.data as u32)
                            .map_or_else(|| s.data.to_string(), notation)
                    };
                    seen.sounds.push(format!(
                        "{name} ({:.2}, {:.2}, {:.2}) {data}",
                        s.position.x, s.position.y, s.position.z
                    ));
                }
                McpePacketData::LevelEventPacket(e) => seen.events.push(format!(
                    "{} ({:.3}, {:.3}, {:.3}) {}",
                    e.event_id, e.position.x, e.position.y, e.position.z, e.data
                )),
                McpePacketData::PlaySoundPacket(s) => seen.play_sounds.push(format!(
                    "{} ({:.2}, {:.2}, {:.2})",
                    s.name,
                    f64::from(s.position.x) / 8.0,
                    f64::from(s.position.y) / 8.0,
                    f64::from(s.position.z) / 8.0
                )),
                McpePacketData::TextPacket(t) => {
                    if let jolyne::valentine::types::TextPacketBody::MessageAndParams(m) = t.body {
                        let params: Vec<String> =
                            m.parameter_list.iter().map(ToString::to_string).collect();
                        seen.messages.push(format!("{} {:?}", m.message, params));
                    }
                }
                _ => {}
            }
        }
    }

    fn expected_sounds(step: &Value) -> Vec<String> {
        let mut out = Vec::new();
        for s in step["sounds"].as_array().into_iter().flatten() {
            let name = s["sound"].as_str().unwrap();
            if name == "block.campfire.crackle" {
                continue;
            }
            let at = shift_f(&s["position"]);
            let data = match &s["data"] {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            out.push(format!(
                "{name} ({:.2}, {:.2}, {:.2}) {data}",
                at.x, at.y, at.z
            ));
        }
        out
    }

    fn sorted(mut items: Vec<String>) -> Vec<String> {
        items.sort();
        items
    }

    /// Item stacks lying on the ground: (item, count).
    fn items_on_ground(h: &mut Harness) -> Vec<(String, u8)> {
        h.drops()
    }

    fn replay(h: &mut Harness, scenario: &str, step: &Value) {
        let name = step["step"].as_str().unwrap();
        let what = format!("{scenario}/{name}");
        let positions: Vec<(String, IVec3)> = step["positions"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(label, pos)| (label.clone(), shift(pos)))
            .collect();
        // Cells set up by commands between steps.
        for (label, pos) in &positions {
            let before = parse(step["before"][label].as_str().unwrap());
            if h.state(*pos) != before {
                h.put(*pos, before);
            }
        }
        // A step without a click (the hunger effect) leaves the player's
        // food where the capture's attribute updates ended.
        if step.get("use").is_none()
            && let Some(updates) = step["hunger"].as_array()
        {
            let mut hunger = h.world.get_mut::<Hunger>(h.player).unwrap();
            for update in updates {
                if let Some(food) = update["hunger"].as_f64() {
                    hunger.food_level = food as i32;
                }
                if let Some(saturation) = update["saturation"].as_f64() {
                    hunger.saturation = saturation as f32;
                }
            }
        }
        let Some(click) = step.get("use") else {
            return;
        };
        if let Some(held) = held_before(scenario, name) {
            hold(h, held);
        }
        let player = &click["player"];
        let feet = shift_f(&player["feet"]);
        h.stand(
            feet + DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0),
            player["yaw"].as_f64().unwrap() as f32,
            player["pitch"].as_f64().unwrap() as f32,
        );
        // A composter roll comes out as it did in the capture.
        if scenario == "composter" && name.contains("seeds") {
            let rises = !step["changes"].as_array().unwrap().is_empty();
            let seed = (0u64..)
                .find(|seed| {
                    let roll = rand::rngs::StdRng::seed_from_u64(*seed).gen_range(0..100);
                    (roll <= 30) == rises
                })
                .unwrap();
            h.world
                .insert_resource(super::super::super::BlockRandom::seeded(seed));
        }
        let drops_before = items_on_ground(h);
        let mut last: Vec<u32> = positions.iter().map(|(_, pos)| h.state(*pos)).collect();
        h.packets();
        let c = &click["click"];
        h.click(
            shift(&click["clicked"]),
            face(click["face"].as_str().unwrap()),
            DVec3::new(
                c[0].as_f64().unwrap(),
                c[1].as_f64().unwrap(),
                c[2].as_f64().unwrap(),
            ),
        );
        let mut seen = Seen::default();
        let mut changes: Vec<(i64, String, String)> = Vec::new();
        let mut appeared: Vec<(i64, String)> = Vec::new();
        let mut on_ground = drops_before.clone();
        let watched = step["watched_ticks"].as_i64().unwrap();
        for tick in 0..=watched {
            if tick > 0 {
                h.tick(1);
            }
            collect(h, &mut seen);
            for (index, (label, pos)) in positions.iter().enumerate() {
                let now = h.state(*pos);
                if now != last[index] {
                    changes.push((tick, label.clone(), notation(now)));
                    last[index] = now;
                }
            }
            let ground = items_on_ground(h);
            if ground != on_ground {
                let mut items: Vec<&String> = ground.iter().map(|(item, _)| item).collect();
                items.dedup();
                for item in items {
                    let had = on_ground.iter().filter(|(i, _)| i == item).count();
                    let has = ground.iter().filter(|(i, _)| i == item).count();
                    for _ in had..has {
                        appeared.push((tick, item.clone()));
                    }
                }
                on_ground = ground;
            }
        }
        // Cells: final states, and change timing from the first change.
        for (label, _) in &positions {
            let after = step["after"][label].as_str().unwrap();
            let index = positions.iter().position(|(l, _)| l == label).unwrap();
            assert_eq!(notation(last[index]), after, "{what}: {label} after");
        }
        let theirs: Vec<(i64, String, String)> = step["changes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|change| {
                let tick = change["tick"].as_i64().unwrap();
                change["cells"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(move |(label, cell)| {
                        (tick, label.clone(), cell.as_str().unwrap().to_owned())
                    })
            })
            .collect();
        let relative = |list: &[(i64, String, String)]| -> Vec<(i64, String, String)> {
            let first = list.first().map_or(0, |c| c.0);
            list.iter()
                .map(|(tick, label, cell)| (tick - first, label.clone(), cell.clone()))
                .collect()
        };
        assert_eq!(relative(&changes), relative(&theirs), "{what}: changes");
        // Sounds, LevelEvents, PlaySounds and popups.
        // The lever's own place sound belongs to the placement core.
        if name != "lever_place" {
            assert_eq!(
                sorted(seen.sounds),
                sorted(expected_sounds(step)),
                "{what}: sounds"
            );
        }
        let events: Vec<String> = step["level_events"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| {
                let at = shift_f(&e["position"]);
                format!(
                    "{} ({:.3}, {:.3}, {:.3}) {}",
                    e["event"], at.x, at.y, at.z, e["data"]
                )
            })
            .collect();
        assert_eq!(sorted(seen.events), sorted(events), "{what}: level events");
        let play_sounds: Vec<String> = step["play_sounds"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                let at = shift_f(&s["position"]);
                format!(
                    "{} ({:.2}, {:.2}, {:.2})",
                    s["name"].as_str().unwrap(),
                    at.x,
                    at.y,
                    at.z
                )
            })
            .collect();
        assert_eq!(seen.play_sounds, play_sounds, "{what}: play sounds");
        let messages: Vec<String> = step["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|m| {
                let params: Vec<String> = m["parameters"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| p.as_str().unwrap().to_owned())
                    .collect();
                format!("{} {:?}", m["message"].as_str().unwrap(), params)
            })
            .collect();
        assert_eq!(seen.messages, messages, "{what}: messages");
        // Dropped items and when they appear.
        let theirs: Vec<(i64, String)> = step["actors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|a| a["event"] == "appeared" && a["type"] == "minecraft:item")
            .map(|a| {
                (
                    a["tick"].as_i64().unwrap(),
                    a["item"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            appeared.len(),
            theirs.len(),
            "{what}: dropped items {appeared:?}"
        );
        for ((ours, item), (bds, wanted)) in appeared.iter().zip(&theirs) {
            assert_eq!(item, wanted, "{what}: dropped item");
            assert!(
                (ours + LANDS - bds).abs() <= 1,
                "{what}: {item} dropped at {ours}, captured at {bds}"
            );
        }
        // The held stack.
        let held = crate::server::game::item_use::held(&h.world, h.player).map(|(_, s)| {
            (
                s.item_id().to_owned(),
                i64::from(s.count),
                i64::from(s.damage),
            )
        });
        let wanted = step["held_after"].as_array().map(|held| {
            (
                held[0].as_str().unwrap().to_owned(),
                held[1].as_i64().unwrap(),
                held[2].as_i64().unwrap(),
            )
        });
        if name != "lever_place" {
            assert_eq!(held, wanted, "{what}: held after");
        }
        // The block actor, against the last one the capture saw.
        if let Some(actor) = step["block_actor_data"].as_array().and_then(|a| a.last()) {
            let pos = shift(&actor["position"]);
            let ours = crate::server::game::block_behaviour::actors::read(&h.world, pos)
                .unwrap_or_else(|| panic!("{what}: no actor"));
            let mut theirs = actor["nbt"].clone();
            let object = theirs.as_object_mut().unwrap();
            for key in ["x", "y", "z"] {
                object.remove(key);
            }
            assert!(
                nbt_holds(&NBTTag::Compound(ours.clone()), &theirs),
                "{what}: actor {ours:?} vs {theirs}"
            );
        }
        // Food.
        if let Some(last) = step["hunger"].as_array().and_then(|h| h.last())
            && step.get("use").is_some()
        {
            let hunger = h.world.get::<Hunger>(h.player).unwrap();
            assert_eq!(
                f64::from(hunger.food_level),
                last["hunger"].as_f64().unwrap()
            );
            assert!(
                (f64::from(hunger.saturation) - last["saturation"].as_f64().unwrap()).abs() < 1e-4
            );
        }
    }

    #[test]
    fn utility_blocks_match_the_bds_capture() {
        let fixture: Value = serde_json::from_str(FIXTURE).expect("fixture is JSON");
        for scenario in [
            "composter",
            "cauldron",
            "campfire",
            "beehive",
            "bell",
            "jukebox",
            "flower_pot",
            "cake",
        ] {
            let mut h = Harness::new();
            if scenario == "campfire" {
                let recipes = unastar_data::recipes::RecipeRegistry::vanilla().expect("recipes");
                h.world
                    .insert_resource(crate::server::game::types::RecipeRegistryResource(
                        std::sync::Arc::new(recipes),
                    ));
            }
            if scenario == "cake" {
                h.world.entity_mut(h.player).insert(Hunger::default());
            }
            if scenario == "bell" {
                // The lever's stone, west of the bell.
                let stone = crate::registry::BlockRegistry::vanilla()
                    .default_state_id_by_name("minecraft:stone")
                    .unwrap();
                h.put(IVec3::new(-1, FLOOR + 1, 0), stone);
            }
            for step in fixture[scenario].as_array().unwrap() {
                replay(&mut h, scenario, step);
            }
        }
    }
}
