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
    ActiveEffect, EffectType, Effects, GameMode, Health, Hunger, PlayerInput, PlayerSession,
    RuntimeEntityId,
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
    add_effect_state_scaled_from(
        world,
        player,
        source,
        effect,
        ActiveEffect::new(amplifier, duration_ticks),
        potency,
        |_, _| true,
    )
}

/// Apply a complete finite effect through ordinary authorization and attributed instant mechanics.
pub(crate) fn add_effect_state_from(
    world: &mut World,
    player: Entity,
    source: Option<Entity>,
    effect: EffectType,
    state: ActiveEffect,
    valid: impl FnOnce(&World) -> bool,
) -> bool {
    add_effect_state_scaled_from(world, player, source, effect, state, 1.0, |world, state| {
        !state.infinite && (1..=i32::MAX as u32).contains(&state.duration_ticks) && valid(world)
    })
}

/// Apply explicit infinite native state, retaining ordinary authorization and pulse counters.
pub(crate) fn add_infinite_effect_state_from(
    world: &mut World,
    player: Entity,
    source: Option<Entity>,
    effect: EffectType,
    state: ActiveEffect,
    valid: impl FnOnce(&World) -> bool,
) -> bool {
    if matches!(
        effect,
        EffectType::INSTANT_HEALTH | EffectType::INSTANT_DAMAGE | EffectType::SATURATION
    ) {
        return false;
    }
    add_effect_state_scaled_from(world, player, source, effect, state, 1.0, |world, state| {
        state.infinite && state.duration_ticks == u32::MAX && valid(world)
    })
}

/// Keep presentation flags in the same native precedence decision as amplifier and duration.
fn add_effect_state_scaled_from(
    world: &mut World,
    player: Entity,
    source: Option<Entity>,
    effect: EffectType,
    mut state: ActiveEffect,
    potency: f32,
    valid: impl FnOnce(&World, &ActiveEffect) -> bool,
) -> bool {
    if world
        .get::<crate::entity::components::Player>(player)
        .is_some()
        && super::inventory_effects::is_quarantined(world, player)
    {
        return false;
    }
    if !potency.is_finite() || !(0.0..=1.0).contains(&potency) {
        return false;
    }
    let attribution = crate::entity::damage::DamageAttribution::for_source(world, source);
    let authorized = if state.infinite {
        crate::entity::projectile::hooks::effect_state(world, player, source, effect, &state)
    } else {
        crate::entity::projectile::hooks::effect(
            world,
            player,
            source,
            effect,
            state.level,
            state.duration_ticks,
        )
    };
    let Some((amplifier, duration_ticks)) = authorized else {
        return false;
    };
    state.level = amplifier;
    state.duration_ticks = duration_ticks;
    if !valid(world, &state) {
        return false;
    }
    match effect {
        EffectType::INSTANT_HEALTH => {
            heal(world, player, (4u32 << amplifier.min(28)) as f32 * potency);
            return true;
        }
        EffectType::INSTANT_DAMAGE => {
            let attacker = attribution.live_owner(world).filter(|actor| {
                world
                    .get::<crate::entity::components::Player>(*actor)
                    .is_some()
                    || world.get::<crate::entity::mob::MobKind>(*actor).is_some()
            });
            let hit = crate::entity::mob::combat::Hit {
                damage: (6u32 << amplifier.min(28)) as f32 * potency,
                cause: unastar_data::actor_components::body::ActorDamageCause::Magic,
                attacker,
                by_player: attribution.owner().is_some_and(|owner| owner.is_player()),
                knockback_from: None,
                knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
            };
            if world
                .get::<crate::entity::components::Player>(player)
                .is_some()
            {
                crate::entity::player_life::hurt_player_attributed(
                    world,
                    player,
                    &hit,
                    attribution,
                );
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
        .apply(effect, state);
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
pub struct SentEffects(HashMap<EffectType, ActiveEffect>);

impl SentEffects {
    /// Record an explicitly synchronized effect set after authoritative restoration.
    pub(super) fn sent(effects: &Effects) -> Self {
        Self(
            effects
                .active
                .iter()
                .map(|(kind, effect)| (*kind, effect.clone()))
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
                session.send(McpePacket::from(MobEffectPacket {
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
                }))
            };
        let active = effects.map(|e| &e.active);
        let removed: Vec<_> = sent
            .0
            .keys()
            .filter(|effect| active.is_none_or(|a| !a.contains_key(effect)))
            .copied()
            .collect();
        for effect in removed {
            if send(
                EnumsMobEffectPacketPayloadEvent::Remove,
                effect,
                0,
                0,
                false,
                false,
            ) {
                sent.0.remove(&effect);
            }
        }
        for (effect, state) in active.into_iter().flatten() {
            // A duration that only counted down is not a change.
            let event = match sent.0.get(effect) {
                None => Some(EnumsMobEffectPacketPayloadEvent::Add),
                Some(previous)
                    if previous.level != state.level
                        || state.duration_ticks > previous.duration_ticks
                        || previous.ambient != state.ambient
                        || previous.show_particles != state.show_particles
                        || previous.infinite != state.infinite =>
                {
                    Some(EnumsMobEffectPacketPayloadEvent::Update)
                }
                Some(_) => None,
            };
            if let Some(event) = event {
                if send(
                    event,
                    *effect,
                    state.level,
                    if state.infinite {
                        u32::MAX
                    } else {
                        state.duration_ticks
                    },
                    state.show_particles,
                    state.ambient,
                ) {
                    sent.0.insert(*effect, state.clone());
                }
            } else {
                sent.0.insert(*effect, state.clone());
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

    /// Native packets are observed from a bounded real session, including retry behavior.
    fn sync_fixture() -> (
        crate::server::game::GameServer,
        Entity,
        tokio::sync::mpsc::Receiver<McpePacket>,
    ) {
        let mut server =
            crate::server::game::GameServer::with_config(crate::server::game::ServerConfig {
                world: crate::world::WorldConfig::void_spawn_platform(1),
                ..Default::default()
            });
        let (outbound_tx, mut packets) = tokio::sync::mpsc::channel(128);
        let player = server
            .spawn_player(crate::server::game::PlayerSpawnData {
                session_id: 91,
                runtime_id: 91,
                display_name: "Example".into(),
                xuid: Some("91".into()),
                uuid: Some(uuid::Uuid::from_u128(91).to_string()),
                position: glam::DVec3::new(0.5, 17.62, 0.5),
                saved_data: None,
                outbound_tx,
                chunk_radius: 0,
            })
            .unwrap();
        while packets.try_recv().is_ok() {}
        (server, player, packets)
    }

    /// Run only effect synchronization so duration changes are explicit in the test.
    fn synchronize(world: &mut World) {
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_player_effects);
        schedule.run(world);
    }

    #[test]
    fn ambient_and_particle_only_changes_emit_updates_without_duration_spam() {
        let (mut server, player, mut packets) = sync_fixture();
        add_effect(server.ecs.world_mut(), player, EffectType::SPEED, 1, 100);
        synchronize(server.ecs.world_mut());
        assert!(packets.try_recv().is_ok());
        let world = server.ecs.world_mut();
        let mut effects = world.get_mut::<Effects>(player).unwrap();
        let effect = effects.active.get_mut(&EffectType::SPEED).unwrap();
        effect.duration_ticks -= 1;
        effect.ambient = true;
        effect.show_particles = false;
        synchronize(server.ecs.world_mut());
        let packet = packets.try_recv().unwrap();
        let jolyne::valentine::McpePacketData::MobEffectPacket(packet) = packet.data else {
            panic!("effect update")
        };
        assert_eq!(packet.event_id, EnumsMobEffectPacketPayloadEvent::Update);
        assert!(packet.ambient);
        assert!(!packet.show_particles);
        server
            .ecs
            .world_mut()
            .get_mut::<Effects>(player)
            .unwrap()
            .active
            .get_mut(&EffectType::SPEED)
            .unwrap()
            .duration_ticks -= 1;
        synchronize(server.ecs.world_mut());
        assert!(packets.try_recv().is_err());
    }

    #[test]
    fn unsent_effect_add_update_and_removal_are_retried_without_cache_acknowledgement() {
        let (mut server, player, _old_packets) = sync_fixture();
        let (tx, mut packets) = tokio::sync::mpsc::channel(1);
        server
            .ecs
            .world_mut()
            .get_mut::<PlayerSession>(player)
            .unwrap()
            .outbound_tx = tx;
        let filler = super::super::types::system_text("queued");
        let world = server.ecs.world_mut();
        assert!(
            world
                .get::<PlayerSession>(player)
                .unwrap()
                .send(filler.clone().into())
        );
        add_effect(world, player, EffectType::SPEED, 0, 100);
        synchronize(world);
        assert!(world.get::<SentEffects>(player).unwrap().0.is_empty());
        packets.try_recv().unwrap();
        synchronize(world);
        assert!(matches!(
            packets.try_recv().unwrap().data,
            jolyne::valentine::McpePacketData::MobEffectPacket(_)
        ));
        assert!(
            world
                .get::<SentEffects>(player)
                .unwrap()
                .0
                .contains_key(&EffectType::SPEED)
        );
        assert!(
            world
                .get::<PlayerSession>(player)
                .unwrap()
                .send(filler.clone().into())
        );
        world
            .get_mut::<Effects>(player)
            .unwrap()
            .active
            .get_mut(&EffectType::SPEED)
            .unwrap()
            .ambient = true;
        synchronize(world);
        assert!(!world.get::<SentEffects>(player).unwrap().0[&EffectType::SPEED].ambient);
        packets.try_recv().unwrap();
        synchronize(world);
        packets.try_recv().unwrap();
        assert!(world.get::<SentEffects>(player).unwrap().0[&EffectType::SPEED].ambient);
        assert!(
            world
                .get::<PlayerSession>(player)
                .unwrap()
                .send(filler.into())
        );
        remove_effect(world, player, EffectType::SPEED);
        synchronize(world);
        assert!(
            world
                .get::<SentEffects>(player)
                .unwrap()
                .0
                .contains_key(&EffectType::SPEED)
        );
        packets.try_recv().unwrap();
        synchronize(world);
        let packet = packets.try_recv().unwrap();
        let jolyne::valentine::McpePacketData::MobEffectPacket(packet) = packet.data else {
            panic!("effect removal")
        };
        assert_eq!(packet.event_id, EnumsMobEffectPacketPayloadEvent::Remove);
        assert!(world.get::<SentEffects>(player).unwrap().0.is_empty());
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
