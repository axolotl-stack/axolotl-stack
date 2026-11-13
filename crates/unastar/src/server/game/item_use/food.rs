//! Eating and drinking.
//!
//! Foods are the items whose behaviour-pack document has `minecraft:food`
//! (nutrition, saturation modifier, `can_always_eat`, effects, removed
//! effects, `using_converts_to`, `on_use_action`). The milk bucket is a
//! built-in item class without pack data: drinking it clears every effect
//! and leaves a bucket.
//!
//! Potions and ominous bottles are built-in drinks too, measured on BDS
//! 1.26.51 (item-use capture, `tools/bds-sweep/scripts/use-items.txt`,
//! replayed by `item_use/drink_tests.rs`). Each potion data value gives the
//! effects of [`potion_effects`]; the potion leaves a glass bottle and plays
//! `bottle.empty`. An ominous bottle gives Bad Omen at its data value as
//! amplifier (at most 4) for 120000 ticks without particles, plays
//! `apply_effect.bad_omen` and `ominous_bottle.end_use`, and leaves nothing.
//! Both finish with `CompletedUsingItem` method 3 and play their sounds 0.2
//! below the eyes and 0.2 ahead of them in the facing direction, whatever
//! the pitch.
//!
//! The use duration is `minecraft:use_modifiers.use_duration` (seconds) or
//! the legacy `minecraft:use_duration` (ticks), else the native
//! `maxUseDuration`; they agree for every vanilla food.

use bevy_ecs::prelude::*;
use glam::DVec3;
use rand::Rng;

use super::UseMethod;
use crate::entity::components::{GameMode, Hunger, Position};
use crate::entity::item_drop::EntityRandom;
use crate::item::ItemStack;
use crate::registry::item::ItemEntry;
use crate::server::game::effects;

/// A consumable item: how long it takes and what kind it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Consumable {
    pub duration_ticks: u32,
    pub kind: ConsumeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumeKind {
    /// `minecraft:food` from the item's pack document.
    Food,
    /// The milk bucket.
    Milk,
    /// A potion: the effects of its data value.
    Potion,
    /// An ominous bottle: Bad Omen.
    OminousBottle,
}

const MILK_BUCKET: &str = "minecraft:milk_bucket";
const BUCKET: &str = "minecraft:bucket";
const POTION: &str = "minecraft:potion";
const OMINOUS_BOTTLE: &str = "minecraft:ominous_bottle";
const GLASS_BOTTLE: &str = "minecraft:glass_bottle";
/// Bad Omen from an ominous bottle: 100 minutes.
const BAD_OMEN_TICKS: u32 = 120_000;

/// The use duration of `entry` in ticks.
pub fn use_duration(entry: &ItemEntry) -> u32 {
    let components = entry.components();
    if let Some(modifiers) = &components.use_modifiers {
        return (modifiers.use_duration * 20.0).round().max(0.0) as u32;
    }
    if let Some(ticks) = components.use_duration.and_then(|t| u32::try_from(t).ok()) {
        return ticks;
    }
    u32::try_from(entry.data().max_use_duration).unwrap_or(0)
}

pub fn consumable(entry: &ItemEntry) -> Option<Consumable> {
    let kind = if let Some(food) = &entry.components().food {
        // A food whose saturation modifier names no known level cannot be
        // eaten (BDS rejects such documents; vanilla has none).
        food.saturation_modifier.value()?;
        ConsumeKind::Food
    } else if entry.identifier() == MILK_BUCKET {
        ConsumeKind::Milk
    } else if entry.identifier() == POTION {
        ConsumeKind::Potion
    } else if entry.identifier() == OMINOUS_BOTTLE {
        ConsumeKind::OminousBottle
    } else {
        return None;
    };
    Some(Consumable {
        duration_ticks: use_duration(entry),
        kind,
    })
}

/// Whether the player may start eating: hungry, a food that can always be
/// eaten, a drink, creative, or peaceful.
fn may_start(world: &World, player: Entity, entry: &ItemEntry, kind: ConsumeKind) -> bool {
    if kind != ConsumeKind::Food {
        return true;
    }
    let always = entry
        .components()
        .food
        .as_ref()
        .is_some_and(|food| food.can_always_eat);
    let hungry = world
        .get::<Hunger>(player)
        .is_some_and(|hunger| hunger.food_level < Hunger::MAX_FOOD);
    let creative = world.get::<GameMode>(player) == Some(&GameMode::Creative);
    let peaceful = crate::server::game::hunger::FoodRules::of(world).difficulty
        == crate::server::game::hunger::Difficulty::Peaceful;
    always || hungry || creative || peaceful
}

/// Start eating or drinking the held `stack`, if the player may.
pub fn start(
    world: &mut World,
    player: Entity,
    slot: u8,
    stack: &ItemStack,
    consumable: Consumable,
) {
    let Some(entry) = stack.item() else {
        return;
    };
    if may_start(world, player, entry, consumable.kind) {
        super::start_using(world, player, slot, entry.id);
    }
}

/// Finish eating or drinking `stack` from hotbar `slot`.
pub fn finish(world: &mut World, player: Entity, slot: u8, stack: &ItemStack, kind: ConsumeKind) {
    let Some(entry) = stack.item().cloned() else {
        return;
    };
    let components = entry.components().clone();
    let converts_to = match (kind, components.food.as_ref()) {
        (ConsumeKind::Food, Some(food)) => {
            if let Some(mut hunger) = world.get_mut::<Hunger>(player) {
                hunger.eat(
                    food.nutrition,
                    food.saturation_modifier
                        .value()
                        .expect("consumable() checked the saturation modifier"),
                );
            }
            for effect in &food.effects {
                let roll: f32 = world.get_resource_or_init::<EntityRandom>().0.r#gen();
                if roll >= effect.chance {
                    continue;
                }
                let Some(effect_type) =
                    crate::entity::components::EffectType::from_name(&effect.name)
                else {
                    tracing::warn!(item = entry.identifier(), effect = %effect.name, "Food effect is not a known effect");
                    continue;
                };
                effects::add_effect(
                    world,
                    player,
                    effect_type,
                    u8::try_from(effect.amplifier).unwrap_or(u8::MAX),
                    u32::try_from(effect.duration).unwrap_or(0) * 20,
                );
            }
            for name in &food.remove_effects {
                if let Some(effect_type) = crate::entity::components::EffectType::from_name(name) {
                    effects::remove_effect(world, player, effect_type);
                }
            }
            match food.on_use_action.as_deref() {
                Some("chorus_teleport") => {
                    let range = food.on_use_range.unwrap_or([8.0, 8.0, 8.0]);
                    chorus_teleport(world, player, range);
                }
                Some("suspicious_stew_effect") => {
                    if let Some(effect) = stew_effect(stack.damage) {
                        effects::add_effect(world, player, effect.0, 0, effect.1);
                    }
                }
                Some(other) => {
                    tracing::warn!(
                        item = entry.identifier(),
                        action = other,
                        "Unsupported food on_use_action"
                    );
                }
                None => {}
            }
            food.using_converts_to.clone()
        }
        (ConsumeKind::Milk, _) => {
            effects::clear_effects(world, player);
            Some(BUCKET.to_owned())
        }
        (ConsumeKind::Potion, _) => {
            for &(effect, amplifier, duration) in potion_effects(stack.damage) {
                effects::add_effect(world, player, effect, amplifier, duration);
            }
            Some(GLASS_BOTTLE.to_owned())
        }
        (ConsumeKind::OminousBottle, _) => {
            let mut omen = crate::entity::components::ActiveEffect::new(
                u8::try_from(stack.damage.clamp(0, 4)).unwrap_or(0),
                BAD_OMEN_TICKS,
            );
            omen.show_particles = false;
            effects::add_effect_with(
                world,
                player,
                crate::entity::components::EffectType(28),
                omen,
            );
            None
        }
        (ConsumeKind::Food, None) => return,
    };
    let creative = world.get::<GameMode>(player) == Some(&GameMode::Creative);
    if !creative {
        let left = stack.grow(-1);
        let converted = converts_to.and_then(|name| {
            let name = if name.contains(':') {
                name
            } else {
                format!("minecraft:{name}")
            };
            world
                .resource::<crate::server::game::types::ItemRegistryResource>()
                .0
                .stack(&name, 1)
        });
        match (left.is_empty(), converted) {
            (true, Some(converted)) => super::set_slot(world, player, slot, converted),
            (_, converted) => {
                super::set_slot(world, player, slot, left);
                if let Some(converted) = converted {
                    super::give_or_drop(world, player, converted);
                }
            }
        }
    }
    let method = match kind {
        ConsumeKind::Food => UseMethod::Eat,
        ConsumeKind::Milk => UseMethod::None,
        ConsumeKind::Potion | ConsumeKind::OminousBottle => UseMethod::Drink,
    };
    super::completed(world, player, entry.network_id(), method);
    super::stop_using(world, player);
    let sounds: &[&str] = match kind {
        ConsumeKind::Potion => &["bottle.empty"],
        ConsumeKind::OminousBottle => &["apply_effect.bad_omen", "ominous_bottle.end_use"],
        _ => &[],
    };
    if let Some(at) = drink_sound_at(world, player) {
        for sound in sounds {
            crate::entity::mob::net::sound::play_level_sound(world, None, at, sound, -1);
        }
    }
    // Food ends with a burp 0.1 below the eyes; drinking milk does not.
    if kind == ConsumeKind::Food
        && let Some(eye) = world.get::<Position>(player).map(|p| p.0)
    {
        crate::entity::mob::net::sound::play_level_sound(
            world,
            Some(player),
            eye - DVec3::new(0.0, 0.1, 0.0),
            "burp",
            -1,
        );
    }
}

/// Where BDS plays a drink's sounds: 0.2 below the eyes and 0.2 ahead of
/// them horizontally, along the player's yaw (the pitch does not move it).
fn drink_sound_at(world: &World, player: Entity) -> Option<DVec3> {
    let eye = world.get::<Position>(player)?.0;
    let yaw = f64::from(
        world
            .get::<crate::entity::components::Rotation>(player)?
            .yaw,
    )
    .to_radians();
    Some(eye + DVec3::new(-yaw.sin() * 0.2, -0.2, yaw.cos() * 0.2))
}

/// The effects of the potion with data value `data`: (effect, amplifier,
/// duration in ticks), measured on BDS 1.26.51 by drinking data 0-46
/// (item-use capture, `MobEffect` adds). Water, mundane, thick and awkward
/// potions (0-4) give nothing; instant effects (21-24) last 1 tick.
pub fn potion_effects(data: i16) -> &'static [(crate::entity::components::EffectType, u8, u32)] {
    use crate::entity::components::EffectType as E;
    match data {
        5 => &[(E::NIGHT_VISION, 0, 3600)],
        6 => &[(E::NIGHT_VISION, 0, 9600)],
        7 => &[(E::INVISIBILITY, 0, 3600)],
        8 => &[(E::INVISIBILITY, 0, 9600)],
        9 => &[(E::JUMP_BOOST, 0, 3600)],
        10 => &[(E::JUMP_BOOST, 0, 9600)],
        11 => &[(E::JUMP_BOOST, 1, 1800)],
        12 => &[(E::FIRE_RESISTANCE, 0, 3600)],
        13 => &[(E::FIRE_RESISTANCE, 0, 9600)],
        14 => &[(E::SPEED, 0, 3600)],
        15 => &[(E::SPEED, 0, 9600)],
        16 => &[(E::SPEED, 1, 1800)],
        17 => &[(E::SLOWNESS, 0, 1800)],
        18 => &[(E::SLOWNESS, 0, 4800)],
        19 => &[(E::WATER_BREATHING, 0, 3600)],
        20 => &[(E::WATER_BREATHING, 0, 9600)],
        21 => &[(E::INSTANT_HEALTH, 0, 1)],
        22 => &[(E::INSTANT_HEALTH, 1, 1)],
        23 => &[(E::INSTANT_DAMAGE, 0, 1)],
        24 => &[(E::INSTANT_DAMAGE, 1, 1)],
        25 => &[(E::POISON, 0, 900)],
        26 => &[(E::POISON, 0, 2400)],
        27 => &[(E::POISON, 1, 450)],
        28 => &[(E::REGENERATION, 0, 900)],
        29 => &[(E::REGENERATION, 0, 2400)],
        30 => &[(E::REGENERATION, 1, 450)],
        31 => &[(E::STRENGTH, 0, 3600)],
        32 => &[(E::STRENGTH, 0, 9600)],
        33 => &[(E::STRENGTH, 1, 1800)],
        34 => &[(E::WEAKNESS, 0, 1800)],
        35 => &[(E::WEAKNESS, 0, 4800)],
        36 => &[(E::WITHER, 1, 800)],
        37 => &[(E::SLOWNESS, 3, 400), (E::RESISTANCE, 2, 400)],
        38 => &[(E::SLOWNESS, 3, 800), (E::RESISTANCE, 2, 800)],
        39 => &[(E::SLOWNESS, 5, 400), (E::RESISTANCE, 3, 400)],
        40 => &[(E::SLOW_FALLING, 0, 1800)],
        41 => &[(E::SLOW_FALLING, 0, 4800)],
        42 => &[(E::SLOWNESS, 3, 400)],
        43 => &[(E(32), 0, 3600)],
        44 => &[(E::WEAVING, 0, 3600)],
        45 => &[(E(34), 0, 3600)],
        46 => &[(E(35), 0, 3600)],
        _ => &[],
    }
}

/// The effect (and its duration in ticks) of a suspicious stew with aux
/// `data`, measured from BDS 1.26.51 (capture `item-use-20261029`, stew
/// data 0-9; the food capture, `item_use/food_tests.rs`, data 0-13: 13 gives
/// none). Every stew effect has amplifier 0.
pub fn stew_effect(data: i16) -> Option<(crate::entity::components::EffectType, u32)> {
    use crate::entity::components::EffectType as E;
    Some(match data {
        0 => (E::NIGHT_VISION, 100),
        1 => (E::JUMP_BOOST, 100),
        2 => (E::WEAKNESS, 140),
        3 => (E::BLINDNESS, 140),
        4 => (E::POISON, 220),
        5 | 6 => (E::SATURATION, 6),
        7 => (E::FIRE_RESISTANCE, 60),
        8 => (E::REGENERATION, 140),
        9 => (E::WITHER, 140),
        10 => (E::NIGHT_VISION, 100),
        11 => (E::BLINDNESS, 140),
        12 => (E::NAUSEA, 140),
        _ => return None,
    })
}

/// Chorus fruit: up to 16 tries at a random spot within `range` blocks of
/// the feet. The spot keeps its x and z and drops onto the ground below it
/// (up to 16 blocks); it is taken where the player's box fits there. The
/// player and those near hear `teleport` where it stood, then where it lands
/// (BDS 1.26.51 food capture: from feet (0.5, -60, 0.5) to (6.88, -60, -1.03)).
pub fn chorus_teleport(world: &mut World, player: Entity, range: [f32; 3]) {
    let Some(eye) = world.get::<Position>(player).map(|p| p.0) else {
        return;
    };
    let eye_height = DVec3::new(0.0, crate::entity::item_drop::PLAYER_EYE_HEIGHT, 0.0);
    let feet = eye - eye_height;
    for _ in 0..16 {
        let offset = {
            let rng = &mut world.get_resource_or_init::<EntityRandom>().0;
            DVec3::new(
                (rng.r#gen::<f64>() - 0.5) * 2.0 * f64::from(range[0]),
                (rng.r#gen::<f64>() - 0.5) * 2.0 * f64::from(range[1]),
                (rng.r#gen::<f64>() - 0.5) * 2.0 * f64::from(range[2]),
            )
        };
        if let Some(landing) = landing(world, feet + offset) {
            use crate::entity::mob::net::sound::play_level_sound;
            play_level_sound(world, Some(player), eye, "teleport", -1);
            super::teleport(world, player, landing, super::TeleportCause::ChorusFruit);
            play_level_sound(world, Some(player), landing + eye_height, "teleport", -1);
            return;
        }
    }
}

/// `target`'s x and z on the ground at or below its y (down to 16 blocks),
/// if the player's box fits there.
fn landing(world: &World, target: DVec3) -> Option<DVec3> {
    use crate::entity::systems::collides_with_blocks;
    use crate::registry::collision::Aabb;
    let registry = &world
        .get_resource::<crate::server::game::types::BlockRegistryResource>()?
        .0;
    let solid = |x: i32, y: i32, z: i32| -> Option<bool> {
        let id = crate::server::game::blocks::block_at(world, (x, y, z))?;
        Some(registry.get_by_runtime_id(id)?.data.blocks_motion)
    };
    let (x, z) = (target.x.floor() as i32, target.z.floor() as i32);
    let mut y = target.y.floor() as i32;
    for _ in 0..16 {
        if solid(x, y - 1, z)? {
            let feet = DVec3::new(target.x, f64::from(y), target.z);
            let body = Aabb::new(
                feet + DVec3::new(-0.3, 0.0, -0.3),
                feet + DVec3::new(0.3, 1.8, 0.3),
            );
            return (!collides_with_blocks(world, body)).then_some(feet);
        }
        y -= 1;
    }
    None
}
