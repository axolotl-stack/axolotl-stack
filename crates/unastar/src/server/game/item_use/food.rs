//! Eating and drinking.
//!
//! Foods are the items whose behaviour-pack document has `minecraft:food`
//! (nutrition, saturation modifier, `can_always_eat`, effects, removed
//! effects, `using_converts_to`, `on_use_action`). The milk bucket is a
//! built-in item class without pack data: drinking it clears every effect
//! and leaves a bucket.
//!
//! The use duration is `minecraft:use_modifiers.use_duration` (seconds) or
//! the legacy `minecraft:use_duration` (ticks), else the native
//! `maxUseDuration`; they agree for every vanilla food.

use bevy_ecs::prelude::*;
use glam::DVec3;
use rand::Rng;

use super::{ItemUsing, UseMethod};
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
}

const MILK_BUCKET: &str = "minecraft:milk_bucket";
const BUCKET: &str = "minecraft:bucket";

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
    if kind == ConsumeKind::Milk {
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

/// An air use of a consumable: start eating, or finish once the use
/// duration has passed (then keep eating, as holding the button does).
pub fn use_consumable(
    world: &mut World,
    player: Entity,
    slot: u8,
    stack: &ItemStack,
    consumable: Consumable,
    using: Option<ItemUsing>,
) {
    let Some(entry) = stack.item() else {
        return;
    };
    let Some(using) = using else {
        if may_start(world, player, entry, consumable.kind) {
            super::start_using(world, player, slot, entry.id);
        }
        return;
    };
    if super::used_ticks(world, player, &using) < i64::from(consumable.duration_ticks) {
        return;
    }
    consume(world, player, slot, stack, consumable.kind);
    // Holding the button eats the next one from the same stack.
    match super::held(world, player) {
        Some((_, next)) if next.item().map(|e| e.id) == Some(entry.id) => {
            let since = world
                .get::<crate::entity::components::PlayerInput>(player)
                .map_or(0, |input| input.tick);
            if let Some(mut using) = world.get_mut::<ItemUsing>(player) {
                using.since = since;
            }
        }
        _ => super::stop_using(world, player),
    }
}

/// Finish eating or drinking `stack` from hotbar `slot`.
pub(super) fn consume(
    world: &mut World,
    player: Entity,
    slot: u8,
    stack: &ItemStack,
    kind: ConsumeKind,
) {
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
                let Some(effect_type) = effects::effect_by_name(&effect.name) else {
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
                if let Some(effect_type) = effects::effect_by_name(name) {
                    effects::remove_effect(world, player, effect_type);
                }
            }
            match food.on_use_action.as_deref() {
                Some("chorus_teleport") => {
                    let range = food.on_use_range.unwrap_or([8.0, 8.0, 8.0]);
                    chorus_teleport(world, player, range);
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
        ConsumeKind::Milk => UseMethod::Consume,
    };
    super::completed(world, player, entry.network_id(), method);
}

/// Chorus fruit: up to 16 tries to teleport to a random spot within
/// `range` blocks (x/y/z) that has ground below and room for the player.
pub fn chorus_teleport(world: &mut World, player: Entity, range: [f32; 3]) {
    let Some(eye) = world.get::<Position>(player).map(|p| p.0) else {
        return;
    };
    let feet = eye - DVec3::new(0.0, crate::entity::item_drop::PLAYER_EYE_HEIGHT, 0.0);
    for _ in 0..16 {
        let offset = {
            let rng = &mut world.get_resource_or_init::<EntityRandom>().0;
            DVec3::new(
                (rng.r#gen::<f64>() - 0.5) * 2.0 * f64::from(range[0]),
                (rng.r#gen::<f64>() - 0.5) * 2.0 * f64::from(range[1]),
                (rng.r#gen::<f64>() - 0.5) * 2.0 * f64::from(range[2]),
            )
        };
        let target = feet + offset;
        if let Some(landing) = landing(world, target) {
            super::teleport(world, player, landing, super::TeleportCause::ChorusFruit);
            return;
        }
    }
}

/// The highest standable feet position at or below `target` (in the same
/// column, down to 16 blocks), if the player fits there.
fn landing(world: &World, target: DVec3) -> Option<DVec3> {
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
            let room = !solid(x, y, z)? && !solid(x, y + 1, z)?;
            return room.then(|| DVec3::new(f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5));
        }
        y -= 1;
    }
    None
}
