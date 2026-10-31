//! Status effects on players: adding and clearing them from item use, the
//! per-tick effects that change player state, and the `MobEffect` packets.
//!
//! Effect durations tick in the shared `tick_effects` system (the
//! [`Effects`] component, also used by mobs). This module only adds the
//! player side that system does not cover: what regeneration, poison,
//! wither, hunger, saturation, absorption and instant health/damage do to a
//! player each tick, and telling the player's client about every effect
//! change ([`sync_player_effects`]).

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{ActorRuntimeId, EnumsMobEffectPacketPayloadEvent, PlayerInputTick};
use jolyne::valentine::{McpePacket, MobEffectPacket};

use super::attributes::{Absorption, MAX_ABSORPTION};
use crate::entity::components::{
    EffectType, Effects, GameMode, Health, Hunger, PlayerInput, PlayerSession, RuntimeEntityId,
};

/// Bedrock effect identifiers and their protocol ids.
const EFFECT_NAMES: [(&str, u8); 36] = [
    ("speed", 1),
    ("slowness", 2),
    ("haste", 3),
    ("mining_fatigue", 4),
    ("strength", 5),
    ("instant_health", 6),
    ("instant_damage", 7),
    ("jump_boost", 8),
    ("nausea", 9),
    ("regeneration", 10),
    ("resistance", 11),
    ("fire_resistance", 12),
    ("water_breathing", 13),
    ("invisibility", 14),
    ("blindness", 15),
    ("night_vision", 16),
    ("hunger", 17),
    ("weakness", 18),
    ("poison", 19),
    ("wither", 20),
    ("health_boost", 21),
    ("absorption", 22),
    ("saturation", 23),
    ("levitation", 24),
    ("fatal_poison", 25),
    ("conduit_power", 26),
    ("slow_falling", 27),
    ("bad_omen", 28),
    ("village_hero", 29),
    ("darkness", 30),
    ("trial_omen", 31),
    ("wind_charged", 32),
    ("weaving", 33),
    ("oozing", 34),
    ("infested", 35),
    ("raid_omen", 36),
];

/// The effect named `name` (`minecraft:` prefix optional, case-insensitive
/// as pack JSON is lenient here).
pub fn effect_by_name(name: &str) -> Option<EffectType> {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    EFFECT_NAMES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, id)| EffectType(*id))
}

/// Give `player` an effect (amplifier 0-based). Instant effects apply at
/// once and are not stored.
pub fn add_effect(
    world: &mut World,
    player: Entity,
    effect: EffectType,
    amplifier: u8,
    duration_ticks: u32,
) {
    add_effect_from(world, player, None, effect, amplifier, duration_ticks);
}

/// Give an actor an effect after authorizing its actual source and recipient.
/// Returns false when a native handler, observer or stale actor rejects it.
pub fn add_effect_from(
    world: &mut World,
    player: Entity,
    source: Option<Entity>,
    effect: EffectType,
    amplifier: u8,
    duration_ticks: u32,
) -> bool {
    add_effect_scaled_from(
        world,
        player,
        source,
        effect,
        amplifier,
        duration_ticks,
        1.0,
    )
}

/// Apply a finite instantaneous potency after authorizing the actual recipient.
/// Lasting durations are supplied by the caller, independently of potency.
pub fn add_effect_scaled_from(
    world: &mut World,
    player: Entity,
    source: Option<Entity>,
    effect: EffectType,
    amplifier: u8,
    duration_ticks: u32,
    potency: f32,
) -> bool {
    if !potency.is_finite() || !(0.0..=1.0).contains(&potency) {
        return false;
    }
    let Some((amplifier, duration_ticks)) = crate::entity::projectile::hooks::effect(
        world,
        player,
        source,
        effect,
        amplifier,
        duration_ticks,
    ) else {
        return false;
    };
    match effect {
        EffectType::INSTANT_HEALTH => {
            heal(world, player, (4u32 << amplifier.min(28)) as f32 * potency);
            return true;
        }
        EffectType::INSTANT_DAMAGE => {
            let attacker = source
                .and_then(|source| {
                    if let Some(flight) =
                        world.get::<crate::entity::projectile::ProjectileFlight>(source)
                    {
                        flight.owner
                    } else if let Some(cloud) =
                        world.get::<crate::entity::projectile::cloud::AreaEffectCloud>(source)
                    {
                        cloud.owner()
                    } else {
                        Some(source)
                    }
                })
                .filter(|actor| {
                    world
                        .get::<crate::entity::components::Player>(*actor)
                        .is_some()
                        || world.get::<crate::entity::mob::MobKind>(*actor).is_some()
                });
            let hit = crate::entity::mob::combat::Hit {
                damage: (6u32 << amplifier.min(28)) as f32 * potency,
                cause: unastar_data::actor_components::body::ActorDamageCause::Magic,
                attacker,
                by_player: attacker.is_some_and(|actor| {
                    world
                        .get::<crate::entity::components::Player>(actor)
                        .is_some()
                }),
                knockback_from: None,
                knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
            };
            if world
                .get::<crate::entity::components::Player>(player)
                .is_some()
            {
                crate::entity::player_life::hurt_player(world, player, &hit);
            } else if world.get::<crate::entity::mob::MobKind>(player).is_some() {
                crate::entity::mob::combat::hurt_mob(world, player, hit);
            }
            return true;
        }
        EffectType::SATURATION if duration_ticks <= 1 => {
            saturate(world, player, amplifier);
            return true;
        }
        _ => {}
    }
    let mut entity = world.entity_mut(player);
    if !entity.contains::<Effects>() {
        entity.insert(Effects::default());
    }
    entity
        .get_mut::<Effects>()
        .expect("inserted above")
        .add(effect, amplifier, duration_ticks);
    // Absorption grants its extra health when it starts or is renewed.
    if effect == EffectType::ABSORPTION {
        let amount = (4.0 * (f32::from(amplifier) + 1.0)).min(MAX_ABSORPTION);
        match entity.get_mut::<Absorption>() {
            Some(mut absorption) => absorption.0 = absorption.0.max(amount),
            None => {
                entity.insert(Absorption(amount));
            }
        }
    }
    true
}

/// Remove one effect from `player`.
pub fn remove_effect(world: &mut World, player: Entity, effect: EffectType) {
    if let Some(mut effects) = world.get_mut::<Effects>(player) {
        effects.remove(effect);
    }
}

/// Remove every effect (milk).
pub fn clear_effects(world: &mut World, player: Entity) {
    if let Some(mut effects) = world.get_mut::<Effects>(player) {
        effects.active.clear();
    }
}

fn heal(world: &mut World, player: Entity, amount: f32) {
    if let Some(mut health) = world.get_mut::<Health>(player)
        && !health.is_dead()
    {
        health.heal(amount);
    }
}

/// Saturation: `amplifier + 1` food points and twice that saturation.
fn saturate(world: &mut World, player: Entity, amplifier: u8) {
    if let Some(mut hunger) = world.get_mut::<Hunger>(player) {
        hunger.eat(i32::from(amplifier) + 1, 1.0);
    }
}

/// Whether an effect with `remaining` ticks left at `amplifier` acts this
/// tick, for effects that act every `base >> amplifier` ticks.
fn pulses(base: u32, amplifier: u8, remaining: u32) -> bool {
    let interval = base >> amplifier.min(31);
    interval == 0 || remaining.is_multiple_of(interval)
}

/// System: what the active effects do to each player this tick. Runs after
/// `tick_effects` has counted the durations down.
#[allow(clippy::type_complexity)]
pub fn apply_player_effects(
    mut commands: Commands,
    mut players: Query<(
        Entity,
        &Effects,
        &mut Health,
        &mut Hunger,
        &GameMode,
        Option<&mut Absorption>,
        Option<&super::hunger::PlayerFoodPolicy>,
    )>,
    mut absent: Query<&mut Absorption, Without<Effects>>,
) {
    for (player, effects, mut health, mut hunger, mode, absorption, food_policy) in &mut players {
        if health.is_dead() {
            continue;
        }
        let survival = !matches!(mode, GameMode::Creative | GameMode::Spectator);
        if let Some(e) = effects.get(EffectType::REGENERATION)
            && pulses(50, e.level, e.duration_ticks)
        {
            health.heal(1.0);
        }
        if survival
            && let Some(e) = effects.get(EffectType::POISON)
            && pulses(25, e.level, e.duration_ticks)
            && health.current > 1.0
        {
            health.current = (health.current - 1.0).max(1.0);
        }
        if survival
            && let Some(e) = effects.get(EffectType::WITHER)
            && pulses(40, e.level, e.duration_ticks)
        {
            // Wither can kill, so it takes the player damage path (hurt
            // timer, death), applied when the commands run.
            commands.queue(move |world: &mut World| {
                crate::entity::player_life::hurt_player(
                    world,
                    player,
                    &crate::entity::mob::combat::Hit {
                        damage: 1.0,
                        cause: unastar_data::actor_components::body::ActorDamageCause::Wither,
                        attacker: None,
                        by_player: false,
                        knockback_from: None,
                        knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
                    },
                );
            });
        }
        if survival
            && food_policy.is_none_or(|policy| policy.exhaustion)
            && let Some(e) = effects.get(EffectType::HUNGER)
        {
            hunger.exhaust(0.005 * (f32::from(e.level) + 1.0));
        }
        if let Some(e) = effects.get(EffectType::SATURATION) {
            hunger.eat(i32::from(e.level) + 1, 1.0);
        }
        // Absorption hearts last while the effect lasts; damage taken from
        // them only lowers the amount.
        let wanted = effects
            .get(EffectType::ABSORPTION)
            .map(|e| (4.0 * (f32::from(e.level) + 1.0)).min(MAX_ABSORPTION));
        if let Some(mut absorption) = absorption {
            match wanted {
                Some(max) => absorption.0 = absorption.0.min(max),
                None => absorption.0 = 0.0,
            }
        }
    }
    for mut absorption in &mut absent {
        absorption.0 = 0.0;
    }
}

/// The effects a player's client last heard of.
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct SentEffects(HashMap<EffectType, (u8, u32)>);

impl SentEffects {
    /// Record an explicitly synchronized effect set after authoritative restoration.
    pub(super) fn sent(effects: &Effects) -> Self {
        Self(
            effects
                .active
                .iter()
                .map(|(kind, effect)| (*kind, (effect.level, effect.duration_ticks)))
                .collect(),
        )
    }

    /// Read the effect identifiers the client may still have before replacement.
    pub(super) fn effect_types(&self) -> impl Iterator<Item = EffectType> + '_ {
        self.0.keys().copied()
    }
}

/// System: send each player `MobEffect` add, modify and remove packets for
/// the changes since the last tick.
#[allow(clippy::type_complexity)]
pub fn sync_player_effects(
    mut players: Query<
        (
            &RuntimeEntityId,
            &PlayerSession,
            Option<&Effects>,
            &mut SentEffects,
            Option<&PlayerInput>,
        ),
        Without<super::player_events::PendingInitialState>,
    >,
) {
    for (runtime_id, session, effects, mut sent, input) in &mut players {
        let tick = input.map_or(PlayerInputTick { inputtick: 0 }, PlayerInput::wire_tick);
        let send =
            |event, effect: EffectType, amplifier: u8, duration: u32, show_particles, ambient| {
                let _ = session.send(McpePacket::from(MobEffectPacket {
                    target_runtime_id: ActorRuntimeId {
                        actor_runtime_id: runtime_id.0 as u64,
                    },
                    event_id: event,
                    effect_id: i32::from(effect.0),
                    effect_amplifier: i32::from(amplifier),
                    show_particles,
                    effect_duration_ticks: duration as i32,
                    tick: if effect.affects_movement() {
                        tick.clone()
                    } else {
                        PlayerInputTick { inputtick: 0 }
                    },
                    ambient,
                }));
            };
        let active = effects.map(|e| &e.active);
        let removed: Vec<_> = sent
            .0
            .keys()
            .filter(|effect| active.is_none_or(|a| !a.contains_key(effect)))
            .copied()
            .collect();
        for effect in removed {
            sent.0.remove(&effect);
            send(
                EnumsMobEffectPacketPayloadEvent::Remove,
                effect,
                0,
                0,
                false,
                false,
            );
        }
        for (effect, state) in active.into_iter().flatten() {
            // A duration that only counted down is not a change.
            let event = match sent.0.get(effect) {
                None => Some(EnumsMobEffectPacketPayloadEvent::Add),
                Some(&(level, last)) if level != state.level || state.duration_ticks > last => {
                    Some(EnumsMobEffectPacketPayloadEvent::Update)
                }
                Some(_) => None,
            };
            sent.0.insert(*effect, (state.level, state.duration_ticks));
            if let Some(event) = event {
                send(
                    event,
                    *effect,
                    state.level,
                    state.duration_ticks,
                    state.show_particles,
                    state.ambient,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_names_resolve_to_protocol_ids() {
        assert_eq!(
            effect_by_name("regeneration"),
            Some(EffectType::REGENERATION)
        );
        assert_eq!(
            effect_by_name("minecraft:absorption"),
            Some(EffectType::ABSORPTION)
        );
        assert_eq!(effect_by_name("fatal_poison"), Some(EffectType(25)));
        assert_eq!(effect_by_name("not_an_effect"), None);
    }

    #[test]
    fn pulse_interval_halves_per_amplifier() {
        // Regeneration II (amplifier 1) heals every 25 ticks.
        let heals = (1..=100).filter(|r| pulses(50, 1, *r)).count();
        assert_eq!(heals, 4);
        assert!(
            pulses(50, 6, 7),
            "an interval below one tick acts every tick"
        );
    }
}
