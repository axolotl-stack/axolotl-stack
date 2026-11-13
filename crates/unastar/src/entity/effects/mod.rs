//! Status effects on mobs and players, one implementation for both: giving,
//! taking and clearing effects, the per-tick effect step, and what the
//! effects change and show. Measured on BDS 1.26.51 by `sweep.py effects`
//! (`testdata/bds-1.26.51-effects.json`, replayed in [`tests`]).
//!
//! **Giving** ([`give`]) follows the effect rule of [`Effects::apply`]: a
//! weaker effect leaves the active one alone, but the target's client still
//! gets a `MobEffect` `Update` with the active effect's state. Undead mobs
//! ignore poison and regeneration altogether, and `mob_effect_immunity` keeps
//! its effects away. What an effect changes is applied at once: absorption
//! adds `4 × (level + 1)` to what is left, health boost raises the maximum
//! health by the same (current health unchanged), speed and slowness put a
//! `MovementSpeed` (+0.2 per level) or `MovementSlowdown` (−0.15 per level)
//! total multiplier on the movement attribute, fire resistance sets the
//! `FIRE_IMMUNE` flag (so fire, fire tick, lava and magma are ignored) and
//! invisibility the `INVISIBLE` flag (with its `disappeared` sound; a
//! player only hears the sound). An item ([`give_from_item`]) gives instant
//! health and instant damage at once instead: they act straight away and
//! the client sees them added for 1 tick and removed.
//!
//! **The effect step** ([`tick_mob_effects`], [`tick_player_effects`]) runs
//! once per tick for every actor with effects, in effect id order:
//! - first, if an effect ran out in the previous step, everything derived
//!   from the effects is recomputed: an effect's removal reaches the client
//!   one tick after its `MobEffect` `Remove`;
//! - each effect with `r` ticks left (counting from its full duration on the
//!   tick it was given) acts: poison deals 1 magic damage while health is
//!   above 1 when `r` is a multiple of `25 >> level`, wither 1 wither damage
//!   every `40 >> level`, regeneration heals 1 every `50 >> level`, hunger
//!   adds `0.005 × (level + 1)` exhaustion every tick, saturation adds
//!   `level + 1` food every step it is listed; instant health heals
//!   `4 << level` (undead: takes it as damage, without the hurt animation)
//!   and instant damage deals `6 << level` magic damage (undead: heals it),
//!   on the first step after they were given only (they stay listed for
//!   their duration: `/effect … instant_damage 1` lasts one tick, the
//!   default 30);
//! - then it counts down and, having run out, is removed with a `MobEffect`
//!   `Remove` (duration 0).
//!
//! Levitation (a pull toward `0.05 × (level + 1)` blocks per tick instead of
//! gravity) and slow falling (gravity 0.01) change the living physics step
//! from the moment they are given until the step they run out in (BDS
//! 1.26.51: a cow given levitation II rises 0.0196 on the next tick).
//!
//! **Showing them**: a player's client gets `MobEffect` for each of its
//! effects (`Add`, `Update`, `Remove`, the player's input tick for the
//! effects that change movement); a mob's viewers get `MobEffect` only for
//! those movement effects. A mob's visible effects are the
//! `VISIBLE_MOB_EFFECTS` actor data: every effect shown with particles packed
//! as `(id << 1) | ambient`, 7 bits each, lowest id first. BDS sends no
//! effect colour or ambience actor data. (A player's visible effects come
//! from `server::game::effects`.)
//!
//! **Damage** ([`resisted`], [`absorbed`]): resistance takes `0.2 × (level +
//! 1)` of a hit (a level 5 resistance all of it, though the hit still
//! hurts), and absorption takes what is left before health does.

#[cfg(test)]
mod tests;

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{ActorRuntimeId, EnumsMobEffectPacketPayloadEvent, PlayerInputTick};
use jolyne::valentine::{McpePacket, MobEffectPacket, actor_flags};
use unastar_data::actor_components::body::ActorDamageCause;

use crate::entity::components::{
    ActiveEffect, ActorViewers, EffectType, Effects, GameMode, Health, Hunger, Player,
    PlayerSession, RuntimeEntityId, RuntimeId,
};
use crate::entity::definition::attributes::{ActorAttributes, WireModifier};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::MobKind;
use crate::entity::mob::combat::Hit;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::state::Dead;
use crate::server::game::attributes::{Absorption, MAX_ABSORPTION};

/// Poison acts every `25 >> level` ticks.
pub const POISON_INTERVAL: u32 = 25;
/// Regeneration acts every `50 >> level` ticks.
pub const REGENERATION_INTERVAL: u32 = 50;
/// Wither acts every `40 >> level` ticks.
pub const WITHER_INTERVAL: u32 = 40;
/// Hunger's exhaustion per tick and level.
pub const HUNGER_EXHAUSTION: f32 = 0.005;
/// Health boost's maximum health and absorption's extra health per level.
pub const HEALTH_PER_LEVEL: f32 = 4.0;
/// Instant health heals `4 << level`, instant damage deals `6 << level`.
pub const INSTANT_HEALTH: f32 = 4.0;
pub const INSTANT_DAMAGE: f32 = 6.0;
/// Resistance takes this share of a hit per level.
pub const RESISTANCE_PER_LEVEL: f32 = 0.2;
/// Speed and slowness multiply the movement speed by `1 + this × (level + 1)`.
pub const SPEED_PER_LEVEL: f32 = 0.2;
pub const SLOWNESS_PER_LEVEL: f32 = -0.15;
/// Levitation lifts toward `this × (level + 1)` blocks per tick.
pub const LEVITATION_SPEED: f64 = 0.05;
/// Levitation closes this share of the gap to its speed each tick.
pub const LEVITATION_ACCELERATION: f64 = 0.2;
/// Gravity of a mob slow falling.
pub const SLOW_FALLING_GRAVITY: f64 = 0.01;

/// The movement modifiers of speed and slowness and the maximum-health
/// modifier of health boost, with the ids BDS sends them with.
pub const MOVEMENT_SPEED_MODIFIER: (&str, &str) =
    ("MovementSpeed", "91aeaa56-376b-4498-935b-2f7f68070635");
pub const MOVEMENT_SLOWDOWN_MODIFIER: (&str, &str) =
    ("MovementSlowdown", "7107de5e-7ce8-4030-940e-514c1f160890");
pub const HEALTH_BOOST_MODIFIER: (&str, &str) =
    ("HealthBoost", "5d6f0ba2-1186-46ac-b896-c61c5cee99cc");

/// What [`give`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Given {
    /// The target had no such effect.
    Added,
    /// It replaced the active one.
    Replaced,
    /// The active one is stronger or lasts longer and stays.
    Kept,
}

/// The modifiers the effects put on an actor's attributes (read whenever
/// the movement or health attribute is derived).
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct EffectModifiers {
    /// Speed and slowness on `minecraft:movement`, in effect id order.
    pub movement: Vec<WireModifier>,
    /// Health boost's extra maximum health (0 without it).
    pub health_boost: f32,
}

impl EffectModifiers {
    /// `base` with the movement modifiers applied (total multipliers), never
    /// below 0.
    pub fn movement(&self, base: f32) -> f32 {
        self.movement
            .iter()
            .fold(base, |speed, modifier| speed * (1.0 + modifier.amount))
            .max(0.0)
    }

    /// The `HealthBoost` modifier, if health boost is active.
    pub fn health_modifiers(&self) -> Vec<WireModifier> {
        if self.health_boost == 0.0 {
            return Vec::new();
        }
        let (name, id) = HEALTH_BOOST_MODIFIER;
        vec![WireModifier {
            id,
            name,
            amount: self.health_boost,
            operation: 0,
            operand: 1,
        }]
    }
}

/// The physics the effects change, read by the living physics step; it
/// follows the effects as they are given and run out.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct EffectPhysics {
    /// Levitation's target vertical speed.
    pub levitation: Option<f64>,
    pub slow_falling: bool,
}

/// An effect ran out in the last effect step; what the effects change is
/// recomputed at the start of the next.
#[derive(Component, Debug, Clone, Copy)]
#[component(storage = "SparseSet")]
pub struct EffectsChanged;

/// Instant effects given since the last effect step, which acts on them
/// once.
#[derive(Component, Debug, Clone, Default)]
#[component(storage = "SparseSet")]
pub struct PendingInstant(pub Vec<EffectType>);

/// Flags an effect turned on, to turn off again when it ends (and only
/// then: a fire-immune mob stays fire immune).
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct EffectFlags {
    pub fire_immune: bool,
    pub invisible: bool,
}

/// Whether `effect` acts on a tick with `remaining` ticks left at `level`,
/// for the effects that act every `base >> level` ticks.
pub fn pulses(base: u32, level: u8, remaining: u32) -> bool {
    let interval = base >> level.min(31);
    interval == 0 || remaining.is_multiple_of(interval)
}

/// The `ActorEvent` hurt data for a hit of `cause`: the cause's number
/// (none -1, magic 14, wither 15, as BDS 1.26.51 sends them).
pub fn hurt_event_data(cause: ActorDamageCause) -> i32 {
    cause as i32 - 1
}

/// The `VISIBLE_MOB_EFFECTS` actor data of `effects`: each effect shown
/// with particles as `(id << 1) | ambient`, 7 bits each, the lowest id in
/// the highest bits (BDS 1.26.51: speed, weakness, poison and absorption
/// pack to 4789036), at most eight.
pub fn visible_effects(effects: &Effects) -> i64 {
    pack_visible_effects(effects.active.iter())
}

/// The `VISIBLE_MOB_EFFECTS` packing of [`visible_effects`] over `effects`
/// (the ones without particles left out).
pub fn pack_visible_effects<'a>(
    effects: impl Iterator<Item = (&'a EffectType, &'a ActiveEffect)>,
) -> i64 {
    let mut visible: Vec<(u8, bool)> = effects
        .filter(|(_, state)| state.show_particles)
        .map(|(effect, state)| (effect.0, state.ambient))
        .collect();
    visible.sort_unstable();
    visible.iter().take(8).fold(0, |packed, &(id, ambient)| {
        (packed << 7) | (i64::from(id) << 1) | i64::from(ambient)
    })
}

/// Whether `entity` is in the `undead` family.
pub fn is_undead(world: &World, entity: Entity) -> bool {
    let (Some(families), Some(registry)) = (
        world.get::<crate::entity::definition::families::ActorFamilies>(entity),
        world.get_resource::<crate::entity::mob::MobRegistry>(),
    ) else {
        return false;
    };
    registry
        .families()
        .resolve("undead")
        .is_some_and(|id| families.contains(&id).unwrap_or(false))
}

/// Whether `effect` can affect `entity`: undead mobs take no poison or
/// regeneration (BDS 1.26.51: neither shows on a zombie), and a mob's
/// `mob_effect_immunity` keeps its effects away.
pub fn affects(world: &World, entity: Entity, effect: EffectType) -> bool {
    if matches!(effect, EffectType::POISON | EffectType::REGENERATION) && is_undead(world, entity) {
        return false;
    }
    !crate::entity::mob::components::stats::immune_to_effect(world, entity, effect)
}

/// Give `entity` an effect; `None` when it cannot affect it.
pub fn give(
    world: &mut World,
    entity: Entity,
    effect: EffectType,
    active: ActiveEffect,
) -> Option<Given> {
    if !affects(world, entity, effect) {
        return None;
    }
    let (given, current) = {
        let mut target = world.entity_mut(entity);
        let mut entry = target.entry::<Effects>().or_default();
        let mut effects = entry.get_mut();
        let existed = effects.has(effect);
        let given = match (effects.apply(effect, active), existed) {
            (true, false) => Given::Added,
            (true, true) => Given::Replaced,
            (false, _) => Given::Kept,
        };
        (given, effects.get(effect).cloned().expect("given or kept"))
    };
    let event = if given == Given::Added {
        EnumsMobEffectPacketPayloadEvent::Add
    } else {
        EnumsMobEffectPacketPayloadEvent::Update
    };
    send_mob_effect(world, entity, event, effect, &current);
    if given != Given::Kept {
        if effect.is_instant() {
            world
                .entity_mut(entity)
                .entry::<PendingInstant>()
                .or_default()
                .get_mut()
                .0
                .push(effect);
        }
        if effect == EffectType::ABSORPTION {
            add_absorption(
                world,
                entity,
                HEALTH_PER_LEVEL * (f32::from(current.level) + 1.0),
                given == Given::Added,
            );
        }
        refresh(world, entity);
    }
    Some(given)
}

/// Give `entity` an effect from an item it used (a potion, a food). Instant
/// health and instant damage act at once and are never listed: the target's
/// client sees `MobEffect` `Add` with a duration of 1 tick and `Remove` with
/// 0, then the health changes (BDS 1.26.51 potion capture: harming took the
/// player from 20 to 14 health). Every other effect is given as [`give`]
/// gives it.
pub fn give_from_item(world: &mut World, entity: Entity, effect: EffectType, active: ActiveEffect) {
    if !matches!(
        effect,
        EffectType::INSTANT_HEALTH | EffectType::INSTANT_DAMAGE
    ) {
        give(world, entity, effect, active);
        return;
    }
    if !affects(world, entity, effect)
        || world.get::<Dead>(entity).is_some()
        || world.get::<Health>(entity).is_none_or(Health::is_dead)
    {
        return;
    }
    let shown = ActiveEffect {
        duration_ticks: 1,
        infinite: false,
        ..active.clone()
    };
    send_mob_effect(
        world,
        entity,
        EnumsMobEffectPacketPayloadEvent::Add,
        effect,
        &shown,
    );
    let gone = ActiveEffect {
        duration_ticks: 0,
        ..shown
    };
    send_mob_effect(
        world,
        entity,
        EnumsMobEffectPacketPayloadEvent::Remove,
        effect,
        &gone,
    );
    act(world, entity, effect, &active);
}

/// Take one effect from `entity`, telling its client (`MobEffect` `Remove`
/// with the duration it had left); returns it.
pub fn take(world: &mut World, entity: Entity, effect: EffectType) -> Option<ActiveEffect> {
    let removed = world.get_mut::<Effects>(entity)?.active.remove(&effect)?;
    send_mob_effect(
        world,
        entity,
        EnumsMobEffectPacketPayloadEvent::Remove,
        effect,
        &removed,
    );
    refresh(world, entity);
    Some(removed)
}

/// Take every effect from `entity` (milk, `/effect clear`), in effect id
/// order; returns them.
pub fn clear(world: &mut World, entity: Entity) -> Vec<(EffectType, ActiveEffect)> {
    let Some(mut effects) = world.get_mut::<Effects>(entity) else {
        return Vec::new();
    };
    let mut removed: Vec<(EffectType, ActiveEffect)> = effects.active.drain().collect();
    removed.sort_by_key(|(effect, _)| effect.0);
    for (effect, state) in &removed {
        send_mob_effect(
            world,
            entity,
            EnumsMobEffectPacketPayloadEvent::Remove,
            *effect,
            state,
        );
    }
    if !removed.is_empty() {
        refresh(world, entity);
    }
    removed
}

/// `MobEffect` about `entity`: to a player itself (and the players that see
/// it); to a mob's viewers only for the effects that change movement.
fn send_mob_effect(
    world: &World,
    entity: Entity,
    event: EnumsMobEffectPacketPayloadEvent,
    effect: EffectType,
    state: &ActiveEffect,
) {
    let player = world.get::<Player>(entity).is_some();
    if !player && !effect.affects_movement() {
        return;
    }
    let runtime_id = if player {
        world.get::<RuntimeEntityId>(entity).map(|id| id.0)
    } else {
        world.get::<RuntimeId>(entity).map(|id| id.0)
    };
    let Some(runtime_id) = runtime_id else {
        return;
    };
    let packet = McpePacket::from(MobEffectPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        event_id: event,
        effect_id: i32::from(effect.0),
        effect_amplifier: i32::from(state.level),
        show_particles: state.show_particles,
        // An infinite effect is -1, also when it is taken away.
        effect_duration_ticks: if state.infinite {
            -1
        } else {
            state.duration_ticks as i32
        },
        // The player's input tick only for the effects that change
        // movement; 0 for every other effect and for mobs.
        tick: if player && effect.affects_movement() {
            crate::entity::components::input_tick_of(world, entity)
        } else {
            PlayerInputTick { inputtick: 0 }
        },
        ambient: state.ambient,
    });
    if player {
        crate::entity::player_life::send_to_player_and_viewers(world, entity, &packet);
    } else if let Some(viewers) = world.get::<ActorViewers>(entity) {
        for viewer in &viewers.viewers {
            if let Some(session) = world.get::<PlayerSession>(*viewer) {
                let _ = session.send(packet.clone());
            }
        }
    }
}

/// Recompute everything the active effects change: modifiers, maximum
/// health, absorption, flags and (mobs) the visible effects.
fn refresh(world: &mut World, entity: Entity) {
    let Some(effects) = world.get::<Effects>(entity) else {
        return;
    };
    let level = |effect| {
        effects
            .get(effect)
            .map(|state| f32::from(state.level) + 1.0)
    };
    let mut movement = Vec::new();
    if let Some(levels) = level(EffectType::SPEED) {
        let (name, id) = MOVEMENT_SPEED_MODIFIER;
        movement.push(WireModifier {
            id,
            name,
            amount: SPEED_PER_LEVEL * levels,
            operation: 2,
            operand: 2,
        });
    }
    if let Some(levels) = level(EffectType::SLOWNESS) {
        let (name, id) = MOVEMENT_SLOWDOWN_MODIFIER;
        movement.push(WireModifier {
            id,
            name,
            amount: SLOWNESS_PER_LEVEL * levels,
            operation: 2,
            operand: 2,
        });
    }
    let modifiers = EffectModifiers {
        movement,
        health_boost: level(EffectType::HEALTH_BOOST)
            .map_or(0.0, |levels| HEALTH_PER_LEVEL * levels),
    };
    let absorbing = effects.has(EffectType::ABSORPTION);
    let fire_immune = effects.has(EffectType::FIRE_RESISTANCE);
    let invisible = effects.has(EffectType::INVISIBILITY);
    let visible = visible_effects(effects);
    let mob = world.get::<MobKind>(entity).is_some();
    update_physics(world, entity);

    let before = world
        .get::<EffectModifiers>(entity)
        .cloned()
        .unwrap_or_default();
    if before != modifiers {
        apply_health_boost(world, entity, before.health_boost, modifiers.health_boost);
        world.entity_mut(entity).insert(modifiers.clone());
        if mob {
            derive_effect_attributes(world, entity, &before, &modifiers);
        }
    }
    if !absorbing {
        set_absorption(world, entity, 0.0);
    }
    if mob {
        if let Some(mut metadata) = world.get_mut::<EntityMetadata>(entity) {
            metadata.set_long(
                jolyne::valentine::actor_data_i_ds::VISIBLE_MOB_EFFECTS,
                visible,
            );
        }
        set_effect_flags(world, entity, fire_immune, invisible);
    } else if world.get::<Player>(entity).is_some() {
        player_invisibility_sound(world, entity, invisible);
    }
}

/// A player's invisibility starting or ending plays `disappeared` or
/// `reappeared` at its feet + 0.9, as BDS 1.26.51 does (potion capture).
fn player_invisibility_sound(world: &mut World, player: Entity, invisible: bool) {
    let set = world
        .get::<EffectFlags>(player)
        .copied()
        .unwrap_or_default();
    if set.invisible == invisible {
        return;
    }
    world
        .entity_mut(player)
        .insert(EffectFlags { invisible, ..set });
    let sound = if invisible {
        crate::entity::mob::constants::DISAPPEARED_SOUND
    } else {
        crate::entity::mob::constants::REAPPEARED_SOUND
    };
    if let Some(at) = crate::server::game::item_use::swing_sound_at(world, player) {
        crate::entity::mob::net::sound::play_level_sound(world, Some(player), at, sound, -1);
    }
}

/// Raise or lower the maximum health by the change in health boost; current
/// health stays, but never above the new maximum.
fn apply_health_boost(world: &mut World, entity: Entity, before: f32, after: f32) {
    if before == after {
        return;
    }
    if let Some(mut health) = world.get_mut::<Health>(entity) {
        health.max = (health.max - before + after).max(1.0);
        health.current = health.current.min(health.max);
    }
}

/// A mob's movement and health attributes with the effect modifiers.
fn derive_effect_attributes(
    world: &mut World,
    entity: Entity,
    before: &EffectModifiers,
    after: &EffectModifiers,
) {
    let Some(archetype) = world.get::<MobKind>(entity).map(|kind| kind.0.clone()) else {
        return;
    };
    let base = world
        .get::<crate::entity::definition::components::MovementConfig>(entity)
        .map(|movement| movement.value);
    let health = world.get::<Health>(entity).map(|h| (h.current, h.max));
    let Some(mut attributes) = world.get_mut::<ActorAttributes>(entity) else {
        return;
    };
    // One `UpdateAttributes` entry per modifier added or removed, as BDS
    // sends them (clearing speed and slowness at once lists the movement
    // twice).
    if before.movement != after.movement
        && let (Some(id), Some(base)) = (&archetype.movement_attribute, base)
        && let Ok(mut value) = attributes.get(id)
    {
        value.current = after.movement(base);
        let entries = modifier_changes(&before.movement, &after.movement);
        let _ = attributes.set_modified(id, value, &after.movement, entries);
    }
    if before.health_boost != after.health_boost
        && let (Some(id), Some((current, max))) = (&archetype.health_attribute, health)
        && let Ok(mut value) = attributes.get(id)
    {
        value.current = current;
        value.max = max;
        let _ = attributes.set_modified(id, value, &after.health_modifiers(), 1);
    }
}

/// How many modifiers were added, removed or changed between two lists.
pub(crate) fn modifier_changes(before: &[WireModifier], after: &[WireModifier]) -> usize {
    let removed = before
        .iter()
        .filter(|modifier| !after.iter().any(|other| other.name == modifier.name))
        .count();
    let added_or_changed = after
        .iter()
        .filter(|modifier| !before.contains(modifier))
        .count();
    removed + added_or_changed
}

/// Fire resistance's `FIRE_IMMUNE` and invisibility's `INVISIBLE` on a mob.
fn set_effect_flags(world: &mut World, entity: Entity, fire_immune: bool, invisible: bool) {
    let set = world
        .get::<EffectFlags>(entity)
        .copied()
        .unwrap_or_default();
    let mut next = set;
    if fire_immune != set.fire_immune {
        let had = world
            .get::<ActorFlags>(entity)
            .is_some_and(|flags| flags.get(actor_flags::FIRE_IMMUNE).unwrap_or(false));
        if fire_immune && !had {
            update_flag(world, entity, actor_flags::FIRE_IMMUNE, true);
            next.fire_immune = true;
        } else if !fire_immune {
            update_flag(world, entity, actor_flags::FIRE_IMMUNE, false);
            next.fire_immune = false;
        }
    }
    if invisible != set.invisible {
        crate::entity::projectile::set_actor_invisible(world, entity, invisible);
        next.invisible = invisible;
    }
    if next != set {
        world.entity_mut(entity).insert(next);
    }
}

fn update_flag(world: &mut World, entity: Entity, flag: u32, on: bool) {
    let word = {
        let Some(mut flags) = world.get_mut::<ActorFlags>(entity) else {
            return;
        };
        flags.update(flag, on).expect("catalogue flag index");
        flags.snapshot().words()[0]
    };
    if let Some(mut metadata) = world.get_mut::<EntityMetadata>(entity) {
        metadata.set_long(jolyne::valentine::actor_data_i_ds::RESERVED_0, word as i64);
    }
}

/// The absorption an actor has (players and mobs alike).
fn absorption(world: &World, entity: Entity) -> f32 {
    world.get::<Absorption>(entity).map_or(0.0, |a| a.0)
}

/// Set an actor's absorption (and a mob's absorption attribute).
fn set_absorption(world: &mut World, entity: Entity, amount: f32) {
    if absorption(world, entity) == amount && world.get::<Absorption>(entity).is_some() {
        return;
    }
    if amount == 0.0 && world.get::<Absorption>(entity).is_none() {
        return;
    }
    world.entity_mut(entity).insert(Absorption(amount));
    set_absorption_attribute(world, entity, amount, false);
}

/// Absorption's extra health: added to what is left (BDS 1.26.51: 2 left
/// plus a level II absorption makes 10), up to the attribute's maximum. A
/// mob's viewers get the attribute twice when the effect is new, as BDS
/// sends it.
fn add_absorption(world: &mut World, entity: Entity, amount: f32, new: bool) {
    let total = (absorption(world, entity) + amount).min(MAX_ABSORPTION);
    world.entity_mut(entity).insert(Absorption(total));
    set_absorption_attribute(world, entity, total, new);
}

fn set_absorption_attribute(world: &mut World, entity: Entity, amount: f32, twice: bool) {
    let Some(id) = world
        .get::<ActorAttributes>(entity)
        .and_then(|attributes| attributes.layout().resolve("minecraft:absorption"))
    else {
        return;
    };
    let Some(mut attributes) = world.get_mut::<ActorAttributes>(entity) else {
        return;
    };
    if let Ok(mut value) = attributes.get(&id) {
        value.current = amount;
        let changed = attributes.update(&id, value).unwrap_or(false);
        if twice && changed {
            let _ = attributes.touch(&id);
        }
    }
}

/// `damage` after the target's resistance: `0.2 × (level + 1)` of it is
/// taken away (all of it from level 5).
pub fn resisted(world: &World, entity: Entity, damage: f32) -> f32 {
    let Some(level) = world
        .get::<Effects>(entity)
        .and_then(|effects| effects.get(EffectType::RESISTANCE))
        .map(|state| f32::from(state.level) + 1.0)
    else {
        return damage;
    };
    (damage * (1.0 - RESISTANCE_PER_LEVEL * level)).max(0.0)
}

/// Take `damage` from the target's absorption first; returns what is left
/// for its health.
pub fn absorbed(world: &mut World, entity: Entity, damage: f32) -> f32 {
    let have = absorption(world, entity);
    if have <= 0.0 || damage <= 0.0 {
        return damage;
    }
    let taken = have.min(damage);
    set_absorption(world, entity, have - taken);
    damage - taken
}

/// System (exclusive): the effect step of every mob.
pub fn tick_mob_effects(
    world: &mut World,
    mobs: &mut QueryState<Entity, (With<Effects>, With<MobKind>)>,
    mut due: Local<Vec<Entity>>,
) {
    due.extend(mobs.iter(world));
    for mob in due.drain(..) {
        step(world, mob);
    }
}

/// System (exclusive): the effect step of every player. It runs after the
/// players' attributes were sent, so what it changes reaches them the next
/// tick, as BDS 1.26.51 sends a poisoned player's health a tick after the
/// hurt animation.
pub fn tick_player_effects(
    world: &mut World,
    players: &mut QueryState<Entity, (With<Effects>, With<Player>)>,
    mut due: Local<Vec<Entity>>,
) {
    due.extend(players.iter(world));
    for player in due.drain(..) {
        step(world, player);
    }
}

/// One effect step of one actor (see the module docs).
fn step(world: &mut World, entity: Entity) {
    if world.get::<EffectsChanged>(entity).is_some() {
        world.entity_mut(entity).remove::<EffectsChanged>();
        refresh(world, entity);
    }
    let Some(effects) = world.get::<Effects>(entity) else {
        return;
    };
    let mut active: Vec<(EffectType, ActiveEffect)> = effects
        .active
        .iter()
        .map(|(effect, state)| (*effect, state.clone()))
        .collect();
    active.sort_by_key(|(effect, _)| effect.0);
    let instant = world
        .entity_mut(entity)
        .take::<PendingInstant>()
        .map(|pending| pending.0)
        .unwrap_or_default();
    let mut expired = false;
    for (effect, state) in active {
        if world.get::<Dead>(entity).is_some()
            || world.get::<Health>(entity).is_none_or(Health::is_dead)
        {
            break;
        }
        // Saturation feeds on every step it is listed (BDS 1.26.51 food
        // capture: a suspicious stew's 6 ticks of it took food 15 to 20);
        // the other instant effects act on their first step only.
        if !effect.is_instant() || effect == EffectType::SATURATION || instant.contains(&effect) {
            act(world, entity, effect, &state);
        }
        let ran_out = {
            let Some(mut effects) = world.get_mut::<Effects>(entity) else {
                return;
            };
            let Some(current) = effects.active.get_mut(&effect) else {
                continue;
            };
            current.tick();
            if current.is_expired() {
                let mut gone = effects.active.remove(&effect).expect("present");
                gone.duration_ticks = 0;
                Some(gone)
            } else {
                None
            }
        };
        if let Some(gone) = ran_out {
            send_mob_effect(
                world,
                entity,
                EnumsMobEffectPacketPayloadEvent::Remove,
                effect,
                &gone,
            );
            expired = true;
        }
    }
    if expired {
        if world.get::<Player>(entity).is_some() {
            // A player's step runs after its attributes were sent, so the
            // change reaches it the next tick anyway.
            refresh(world, entity);
        } else {
            world.entity_mut(entity).insert(EffectsChanged);
            update_physics(world, entity);
        }
    }
}

/// A mob's [`EffectPhysics`] from its effects.
fn update_physics(world: &mut World, entity: Entity) {
    if world.get::<MobKind>(entity).is_none() {
        return;
    }
    let Some(effects) = world.get::<Effects>(entity) else {
        return;
    };
    let physics = EffectPhysics {
        levitation: effects
            .get(EffectType::LEVITATION)
            .map(|state| LEVITATION_SPEED * (f64::from(state.level) + 1.0)),
        slow_falling: effects.has(EffectType::SLOW_FALLING),
    };
    if world
        .get::<EffectPhysics>(entity)
        .copied()
        .unwrap_or_default()
        != physics
    {
        world.entity_mut(entity).insert(physics);
    }
}

/// What one effect does on one step, before it counts down.
fn act(world: &mut World, entity: Entity, effect: EffectType, state: &ActiveEffect) {
    let remaining = state.duration_ticks;
    let level = state.level;
    let scale = 2f32.powi(i32::from(level.min(30)));
    match effect {
        EffectType::POISON
            if pulses(POISON_INTERVAL, level, remaining)
                && world.get::<Health>(entity).is_some_and(|h| h.current > 1.0) =>
        {
            hurt(world, entity, 1.0, ActorDamageCause::Magic, true);
        }
        EffectType::WITHER if pulses(WITHER_INTERVAL, level, remaining) => {
            hurt(world, entity, 1.0, ActorDamageCause::Wither, true);
        }
        EffectType::REGENERATION if pulses(REGENERATION_INTERVAL, level, remaining) => {
            heal(world, entity, 1.0);
        }
        EffectType::HUNGER => {
            if survival(world, entity)
                && let Some(mut hunger) = world.get_mut::<Hunger>(entity)
            {
                hunger.exhaust(HUNGER_EXHAUSTION * (f32::from(level) + 1.0));
            }
        }
        // The food level rises by `level + 1` and saturation stays as it is
        // (BDS 1.26.51 food capture: amplifier 0 took food 11 to 12 and
        // amplifier 1 took 0 to 2, saturation 0 both times; a suspicious
        // stew's pulses kept saturation 7.2).
        EffectType::SATURATION => {
            if let Some(mut hunger) = world.get_mut::<Hunger>(entity) {
                hunger.food_level =
                    (hunger.food_level + i32::from(level) + 1).clamp(0, Hunger::MAX_FOOD);
            }
        }
        EffectType::INSTANT_HEALTH => {
            if is_undead(world, entity) {
                hurt(
                    world,
                    entity,
                    INSTANT_HEALTH * scale,
                    ActorDamageCause::Magic,
                    false,
                );
            } else {
                heal(world, entity, INSTANT_HEALTH * scale);
            }
        }
        EffectType::INSTANT_DAMAGE => {
            if is_undead(world, entity) {
                heal(world, entity, INSTANT_DAMAGE * scale);
            } else {
                hurt(
                    world,
                    entity,
                    INSTANT_DAMAGE * scale,
                    ActorDamageCause::Magic,
                    true,
                );
            }
        }
        _ => {}
    }
}

fn survival(world: &World, entity: Entity) -> bool {
    world
        .get::<GameMode>(entity)
        .is_none_or(|mode| mode.allows_damage())
}

/// Effect damage, through the hurt rules of a mob or a player.
fn hurt(world: &mut World, entity: Entity, damage: f32, cause: ActorDamageCause, animate: bool) {
    let hit = Hit {
        damage,
        cause,
        attacker: None,
        by_player: false,
        knockback_from: None,
        knockback_power: KnockbackPower::DEFAULT,
    };
    if world.get::<Player>(entity).is_some() {
        crate::entity::player_life::hurt_player(world, entity, &hit);
    } else if animate {
        crate::entity::mob::combat::hurt_mob(world, entity, hit);
    } else {
        crate::entity::mob::combat::hurt_mob_quietly(world, entity, hit);
    }
}

/// Effect healing; a mob's viewers hear of its health even when it was full.
fn heal(world: &mut World, entity: Entity, amount: f32) {
    let Some(mut health) = world.get_mut::<Health>(entity) else {
        return;
    };
    let before = health.current;
    health.heal(amount);
    if health.current == before
        && let Some(id) = world
            .get::<MobKind>(entity)
            .and_then(|kind| kind.0.health_attribute.clone())
        && let Some(mut attributes) = world.get_mut::<ActorAttributes>(entity)
    {
        let _ = attributes.touch(&id);
    }
}
