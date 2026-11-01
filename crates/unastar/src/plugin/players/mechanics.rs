//! Bounded callback staging over native mechanics; no component borrow survives an import.

use super::*;
use crate::entity::components::{
    ActiveEffect, EffectType, Effects, GameMode, Health, Hunger, PlayerSession, PlayerState,
    RuntimeEntityId, Velocity,
};
use crate::plugin::manifest::PluginCapability;
use crate::server::game::hunger::PlayerFoodPolicy;
use crate::server::game::{effects, flight};
use glam::DVec3;
use std::sync::Weak;

const MAX_CALLBACK_MUTATIONS: usize = 64;
const MAX_QUEUED_MUTATIONS: usize = 1024;
const MAX_SPEED: f64 = 64.0;

/// An owned intent, bound to one original actor, world and component instance.
pub(in crate::plugin::manager) struct MechanicsRequest {
    scope: Scope,
    reference: PlayerLineage,
    player: Entity,
    owner: Weak<()>,
    mutation: Mutation,
}

struct EffectIntent {
    effect: EffectType,
    state: ActiveEffect,
    source: Option<(PlayerLineage, Entity)>,
}

enum Mutation {
    Apply(Box<EffectIntent>),
    Remove(wit::EffectKind),
    Clear,
    Velocity(DVec3),
    Mode(wit::GameMode),
    ModeOverride(Option<wit::GameMode>),
    Flight(Option<bool>),
    Heal(f32),
    FoodPolicy(Option<PlayerFoodPolicy>),
}

#[derive(Resource, Default)]
struct Pending(Vec<MechanicsRequest>);

/// Commit successful explicit intents independently of the default action's cancellation.
pub(in crate::plugin::manager) fn finish(
    context: &mut HostContext,
    world: &mut World,
    accepted: bool,
) {
    let pending = std::mem::take(&mut context.pending_mechanics);
    if accepted && !pending.is_empty() {
        world.get_resource_or_init::<Pending>().0.extend(pending);
    }
}

/// Drain one bounded generation after simulation, with no component-manager lock held.
/// Native effect hooks may queue a later generation, never recursively drain this one.
pub(crate) fn apply_pending(world: &mut World) {
    let Some(mut pending) = world.get_resource_mut::<Pending>() else {
        return;
    };
    let requests = std::mem::take(&mut pending.0);
    for request in requests {
        if request.owner.upgrade().is_none()
            || !request.scope.world.matches(world)
            || !request.reference.matches(world, request.player)
            || !writable(world, request.player)
            || !super::super::readiness::admission_allowed(world)
        {
            continue;
        }
        let player = request.player;
        match request.mutation {
            Mutation::Apply(intent) => {
                let EffectIntent {
                    effect,
                    state,
                    source,
                } = *intent;
                if !source.as_ref().is_none_or(|(reference, entity)| {
                    reference.matches(world, *entity) && writable(world, *entity)
                }) {
                    continue;
                }
                let valid = |world: &World| {
                    request.owner.upgrade().is_some()
                        && request.scope.world.matches(world)
                        && request.reference.matches(world, player)
                        && writable(world, player)
                        && super::super::readiness::admission_allowed(world)
                        && source.as_ref().is_none_or(|(reference, entity)| {
                            reference.matches(world, *entity) && writable(world, *entity)
                        })
                };
                if state.infinite {
                    effects::add_infinite_effect_state_from(
                        world,
                        player,
                        source.as_ref().map(|(_, entity)| *entity),
                        effect,
                        state,
                        valid,
                    );
                } else {
                    effects::add_effect_state_from(
                        world,
                        player,
                        source.as_ref().map(|(_, entity)| *entity),
                        effect,
                        state,
                        valid,
                    );
                }
            }
            Mutation::Remove(kind) => effects::remove_effect(world, player, native_kind(kind)),
            Mutation::Clear => effects::clear_effects(world, player),
            Mutation::Velocity(value) if valid_velocity(value) => {
                let value = value.as_vec3().as_dvec3();
                if world
                    .get::<Velocity>(player)
                    .is_some_and(|current| current.0 == value)
                {
                    continue;
                }
                let Some(runtime) = world.get::<RuntimeEntityId>(player).copied() else {
                    continue;
                };
                world.entity_mut(player).insert(Velocity(value));
                let packet = jolyne::valentine::SetActorMotionPacket {
                    target_runtime_id: jolyne::valentine::types::ActorRuntimeId {
                        actor_runtime_id: runtime.0 as u64,
                    },
                    motion: jolyne::valentine::types::Vec3 {
                        x: value.x as f32,
                        y: value.y as f32,
                        z: value.z as f32,
                    },
                    tick: crate::entity::components::input_tick_of(world, player),
                };
                crate::entity::player_life::send_to_player_and_viewers(
                    world,
                    player,
                    &packet.into(),
                );
            }
            Mutation::Mode(mode) => {
                let mode = native_mode(mode);
                if crate::command::exec::base_game_mode(world, player) != Some(mode) {
                    crate::command::exec::set_game_mode(world, player, mode);
                }
            }
            Mutation::ModeOverride(mode) => {
                crate::command::exec::set_mode_override(world, player, mode.map(native_mode));
            }
            Mutation::Flight(value) => {
                flight::set_override(world, player, value);
            }
            Mutation::Heal(amount) => {
                if let Some(mut health) = world.get_mut::<Health>(player) {
                    health.heal(amount);
                }
            }
            Mutation::FoodPolicy(Some(policy)) => {
                world.entity_mut(player).insert(policy);
            }
            Mutation::FoodPolicy(None) => {
                world.entity_mut(player).remove::<PlayerFoodPolicy>();
            }
            Mutation::Velocity(_) => {}
        }
    }
}

/// Only complete living actors with their original connection or retained lineage may mutate.
fn writable(world: &World, player: Entity) -> bool {
    let admitted = match availability(world, player) {
        wit::PlayerAvailability::Ready => world.get::<PlayerSession>(player).is_some(),
        wit::PlayerAvailability::Retained => {
            world
                .get::<crate::server::game::player_events::DetachedPlayer>(player)
                .is_some()
                && world.get::<PlayerSession>(player).is_none()
        }
        _ => false,
    };
    admitted
        && world.get::<PlayerLineage>(player).is_some_and(|lineage| {
            identity::snapshot(world, player, lineage)
                .is_ok_and(|snapshot| snapshot.health > 0.0 && !snapshot.flags.dead)
        })
}

/// Restrict durations to the signed native packet range; instant effects use at least one tick.
fn valid_application(effect: &wit::EffectApplication) -> bool {
    (1..=i32::MAX as u32).contains(&effect.duration_ticks)
}

/// Instantaneous kinds cannot acquire an indefinite lifetime or periodic pulse history.
fn valid_infinite(effect: EffectType) -> bool {
    !matches!(
        effect,
        EffectType::INSTANT_HEALTH | EffectType::INSTANT_DAMAGE | EffectType::SATURATION
    )
}

/// Embedded player-input history is authoritative when present; absence remains explicit.
fn fall_distance(world: &World, player: Entity) -> Result<Option<f64>, wit::AccessError> {
    let distance = world
        .get::<crate::entity::player_life::environment::PlayerFall>(player)
        .map(|fall| fall.fall.distance)
        .or_else(|| {
            world
                .get::<crate::entity::components::FallDistance>(player)
                .map(|fall| fall.distance)
        });
    if distance.is_some_and(|distance| !distance.is_finite() || distance < 0.0) {
        return Err(wit::AccessError::Unavailable);
    }
    Ok(distance)
}

/// Motion uses finite wire-representable values within one native movement distance bound.
fn valid_velocity(value: DVec3) -> bool {
    value.is_finite()
        && value.as_vec3().is_finite()
        && value.length_squared() <= MAX_SPEED * MAX_SPEED
}

impl HostContext {
    /// Read actual complete state without projecting any queued healing or policy change.
    pub(in crate::plugin::manager) fn vitals_state(
        &mut self,
        rep: u32,
    ) -> Result<wit::PlayerVitals, wit::AccessError> {
        let (reference, player) = self.live_player(rep)?;
        let world = unsafe { self.world() };
        let snapshot = identity::snapshot(world, player, &reference)?;
        if matches!(
            availability(world, player),
            wit::PlayerAvailability::Quarantined
                | wit::PlayerAvailability::Recovering
                | wit::PlayerAvailability::Incomplete
        ) {
            return Err(wit::AccessError::Unavailable);
        }
        let hunger = world
            .get::<Hunger>(player)
            .ok_or(wit::AccessError::Unavailable)?;
        let absorption = world
            .get::<crate::server::game::attributes::Absorption>(player)
            .map_or(0.0, |value| value.0);
        if !(0..=Hunger::MAX_FOOD).contains(&hunger.food_level)
            || !hunger.saturation.is_finite()
            || !(0.0..=Hunger::MAX_FOOD as f32).contains(&hunger.saturation)
            || !hunger.exhaustion.is_finite()
            || !(0.0..=Hunger::MAX_EXHAUSTION).contains(&hunger.exhaustion)
            || !absorption.is_finite()
            || absorption < 0.0
        {
            return Err(wit::AccessError::Unavailable);
        }
        Ok(wit::PlayerVitals {
            health: snapshot.health,
            max_health: snapshot.max_health,
            absorption,
            food_level: hunger.food_level,
            saturation: hunger.saturation,
            exhaustion: hunger.exhaustion,
            food_policy_override: world.get::<PlayerFoodPolicy>(player).map(|policy| {
                wit::FoodPolicy {
                    exhaustion: policy.exhaustion,
                    food_drain: policy.food_drain,
                    natural_regeneration: policy.natural_regeneration,
                    starvation: policy.starvation,
                }
            }),
            captured_tick: world
                .get_resource::<crate::ecs::resources::TickCounter>()
                .ok_or(wit::AccessError::Unavailable)?
                .current,
        })
    }
    /// Keep arbitrary exact-amount healing on the existing bounded living-actor mechanics lane.
    pub(in crate::plugin::manager) fn stage_heal(
        &mut self,
        rep: u32,
        amount: f32,
    ) -> Result<wit::MechanicsAcknowledgement, wit::AccessError> {
        self.operation()?;
        if !amount.is_finite() || amount < 0.0 {
            return Err(wit::AccessError::InvalidArgument);
        }
        self.stage_mechanics(rep, Mutation::Heal(amount))
    }
    /// Change native processing gates without recreating hunger timers in the component.
    pub(in crate::plugin::manager) fn stage_food_policy(
        &mut self,
        rep: u32,
        policy: Option<wit::FoodPolicy>,
    ) -> Result<wit::MechanicsAcknowledgement, wit::AccessError> {
        let policy = policy.map(|policy| PlayerFoodPolicy {
            exhaustion: policy.exhaustion,
            food_drain: policy.food_drain,
            natural_regeneration: policy.natural_regeneration,
            starvation: policy.starvation,
        });
        self.stage_mechanics(rep, Mutation::FoodPolicy(policy))
    }
    /// Capture owned current mechanics without using an observation from a previous tick.
    pub(in crate::plugin::manager) fn mechanics_state(
        &mut self,
        rep: u32,
    ) -> Result<wit::PlayerMechanicsState, wit::AccessError> {
        let (reference, player) = self.live_player(rep)?;
        let world = unsafe { self.world() };
        identity::snapshot(world, player, &reference)?;
        if matches!(
            availability(world, player),
            wit::PlayerAvailability::Quarantined
                | wit::PlayerAvailability::Recovering
                | wit::PlayerAvailability::Incomplete
        ) {
            return Err(wit::AccessError::Unavailable);
        }
        let base = crate::command::exec::base_game_mode(world, player)
            .ok_or(wit::AccessError::Unavailable)?;
        let effective = *world
            .get::<GameMode>(player)
            .ok_or(wit::AccessError::Unavailable)?;
        let velocity = world.get::<Velocity>(player).map(|velocity| velocity.0);
        if velocity.is_some_and(|velocity| !velocity.is_finite()) {
            return Err(wit::AccessError::Unavailable);
        }
        let mut effects = world
            .get::<Effects>(player)
            .map(|effects| effects.active.iter().collect::<Vec<_>>())
            .unwrap_or_default();
        effects.sort_by_key(|(kind, _)| kind.0);
        let effects = effects
            .into_iter()
            .map(|(effect, state)| {
                Ok(wit::ActiveStatusEffect {
                    kind: kind(*effect)?,
                    amplifier: state.level,
                    duration_ticks: state.duration_ticks,
                    ambient: state.ambient,
                    show_particles: state.show_particles,
                    infinite: state.infinite,
                })
            })
            .collect::<Result<_, wit::AccessError>>()?;
        Ok(wit::PlayerMechanicsState {
            effects,
            fall_distance: fall_distance(world, player)?,
            velocity: velocity.map(|velocity| wit::Vec3 {
                x: velocity.x,
                y: velocity.y,
                z: velocity.z,
            }),
            base_game_mode: identity::mode(base),
            effective_game_mode: identity::mode(effective),
            mode_override: crate::command::exec::mode_override(world, player).map(identity::mode),
            flight_override: flight::override_value(world, player),
            flight_allowed: flight::allowed(world, player),
            flying: world
                .get::<PlayerState>(player)
                .ok_or(wit::AccessError::Unavailable)?
                .flying,
            captured_tick: world
                .get_resource::<crate::ecs::resources::TickCounter>()
                .map_or(0, |tick| tick.current),
        })
    }
    /// Validate every import before staging and retain owned bindings rather than live resources.
    fn stage_mechanics(
        &mut self,
        rep: u32,
        mutation: Mutation,
    ) -> Result<wit::MechanicsAcknowledgement, wit::AccessError> {
        let (reference, player) = self.live_player(rep)?;
        self.require(PluginCapability::PlayerMechanics)?;
        let scope = self.resources.scope.ok_or(wit::AccessError::ExpiredScope)?;
        if !writable(unsafe { self.world() }, player) {
            return Err(wit::AccessError::Unavailable);
        }
        let queued = unsafe { self.world() }
            .get_resource::<Pending>()
            .map_or(0, |pending| pending.0.len());
        if self.pending_mechanics.len() >= MAX_CALLBACK_MUTATIONS
            || queued + self.pending_mechanics.len() >= MAX_QUEUED_MUTATIONS
        {
            return Err(wit::AccessError::LimitExceeded);
        }
        self.pending_mechanics.push(MechanicsRequest {
            scope,
            reference,
            player,
            owner: Arc::downgrade(&self.mechanics_owner),
            mutation,
        });
        Ok(wit::MechanicsAcknowledgement::Staged)
    }
    /// Capture a real same-scope source before a later effect application can enter damage hooks.
    pub(in crate::plugin::manager) fn stage_effect(
        &mut self,
        rep: u32,
        application: wit::EffectApplication,
        source: Option<u32>,
    ) -> Result<wit::MechanicsAcknowledgement, wit::AccessError> {
        self.operation()?;
        if !valid_application(&application) {
            return Err(wit::AccessError::InvalidArgument);
        }
        let source = source.map(|rep| self.live_player(rep)).transpose()?;
        if source
            .as_ref()
            .is_some_and(|(_, entity)| !writable(unsafe { self.world() }, *entity))
        {
            return Err(wit::AccessError::Unavailable);
        }
        self.stage_mechanics(
            rep,
            Mutation::Apply(Box::new(EffectIntent {
                effect: native_kind(application.kind),
                state: ActiveEffect {
                    level: application.amplifier,
                    duration_ticks: application.duration_ticks,
                    ambient: application.ambient,
                    show_particles: application.show_particles,
                    infinite: false,
                },
                source,
            })),
        )
    }
    /// Stage genuine native infinite state without overloading the finite import contract.
    pub(in crate::plugin::manager) fn stage_infinite_effect(
        &mut self,
        rep: u32,
        application: wit::InfiniteEffectApplication,
        source: Option<u32>,
    ) -> Result<wit::MechanicsAcknowledgement, wit::AccessError> {
        self.operation()?;
        let effect = native_kind(application.kind);
        if !valid_infinite(effect) {
            return Err(wit::AccessError::InvalidArgument);
        }
        let source = source.map(|rep| self.live_player(rep)).transpose()?;
        if source
            .as_ref()
            .is_some_and(|(_, entity)| !writable(unsafe { self.world() }, *entity))
        {
            return Err(wit::AccessError::Unavailable);
        }
        self.stage_mechanics(
            rep,
            Mutation::Apply(Box::new(EffectIntent {
                effect,
                state: ActiveEffect {
                    level: application.amplifier,
                    duration_ticks: u32::MAX,
                    ambient: application.ambient,
                    show_particles: application.show_particles,
                    infinite: true,
                },
                source,
            })),
        )
    }
    /// Stage removal without altering any attributed damage or health state.
    pub(in crate::plugin::manager) fn stage_remove(
        &mut self,
        rep: u32,
        kind: wit::EffectKind,
    ) -> Result<wit::MechanicsAcknowledgement, wit::AccessError> {
        self.stage_mechanics(rep, Mutation::Remove(kind))
    }
    /// Stage milk-style removal through the normal native effect path.
    pub(in crate::plugin::manager) fn stage_clear(
        &mut self,
        rep: u32,
    ) -> Result<wit::MechanicsAcknowledgement, wit::AccessError> {
        self.stage_mechanics(rep, Mutation::Clear)
    }
    /// Reject invalid motion before it consumes queue space.
    pub(in crate::plugin::manager) fn stage_velocity(
        &mut self,
        rep: u32,
        value: wit::Vec3,
    ) -> Result<wit::MechanicsAcknowledgement, wit::AccessError> {
        self.operation()?;
        let value = DVec3::new(value.x, value.y, value.z);
        if !valid_velocity(value) {
            return Err(wit::AccessError::InvalidArgument);
        }
        self.stage_mechanics(rep, Mutation::Velocity(value))
    }
    /// Stage durable base mode independently of the transient effective overlay.
    pub(in crate::plugin::manager) fn stage_mode(
        &mut self,
        rep: u32,
        value: wit::GameMode,
    ) -> Result<wit::MechanicsAcknowledgement, wit::AccessError> {
        self.stage_mechanics(rep, Mutation::Mode(value))
    }
    /// Stage a temporary effective mode without changing ordinary persistence.
    pub(in crate::plugin::manager) fn stage_mode_override(
        &mut self,
        rep: u32,
        value: Option<wit::GameMode>,
    ) -> Result<wit::MechanicsAcknowledgement, wit::AccessError> {
        self.stage_mechanics(rep, Mutation::ModeOverride(value))
    }
    /// Stage an independent MayFly permission rather than a creative-mode substitution.
    pub(in crate::plugin::manager) fn stage_flight(
        &mut self,
        rep: u32,
        value: Option<bool>,
    ) -> Result<wit::MechanicsAcknowledgement, wit::AccessError> {
        self.stage_mechanics(rep, Mutation::Flight(value))
    }
}

/// Semantic mappings do not expose native enum numbering as component authority.
fn native_mode(mode: wit::GameMode) -> GameMode {
    match mode {
        wit::GameMode::Survival => GameMode::Survival,
        wit::GameMode::Creative => GameMode::Creative,
        wit::GameMode::Adventure => GameMode::Adventure,
        wit::GameMode::Spectator => GameMode::Spectator,
    }
}
/// Map each semantic component effect to the native registry explicitly.
pub(super) fn native_kind(kind: wit::EffectKind) -> EffectType {
    EffectType(match kind {
        wit::EffectKind::Speed => 1,
        wit::EffectKind::Slowness => 2,
        wit::EffectKind::Haste => 3,
        wit::EffectKind::MiningFatigue => 4,
        wit::EffectKind::Strength => 5,
        wit::EffectKind::InstantHealth => 6,
        wit::EffectKind::InstantDamage => 7,
        wit::EffectKind::JumpBoost => 8,
        wit::EffectKind::Nausea => 9,
        wit::EffectKind::Regeneration => 10,
        wit::EffectKind::Resistance => 11,
        wit::EffectKind::FireResistance => 12,
        wit::EffectKind::WaterBreathing => 13,
        wit::EffectKind::Invisibility => 14,
        wit::EffectKind::Blindness => 15,
        wit::EffectKind::NightVision => 16,
        wit::EffectKind::Hunger => 17,
        wit::EffectKind::Weakness => 18,
        wit::EffectKind::Poison => 19,
        wit::EffectKind::Wither => 20,
        wit::EffectKind::HealthBoost => 21,
        wit::EffectKind::Absorption => 22,
        wit::EffectKind::Saturation => 23,
        wit::EffectKind::Levitation => 24,
        wit::EffectKind::FatalPoison => 25,
        wit::EffectKind::ConduitPower => 26,
        wit::EffectKind::SlowFalling => 27,
        wit::EffectKind::BadOmen => 28,
        wit::EffectKind::VillageHero => 29,
        wit::EffectKind::Darkness => 30,
        wit::EffectKind::TrialOmen => 31,
        wit::EffectKind::WindCharged => 32,
        wit::EffectKind::Weaving => 33,
        wit::EffectKind::Oozing => 34,
        wit::EffectKind::Infested => 35,
        wit::EffectKind::RaidOmen => 36,
    })
}
/// Reject unknown native effects rather than substituting a component kind.
pub(super) fn kind(effect: EffectType) -> Result<wit::EffectKind, wit::AccessError> {
    Ok(match effect.0 {
        1 => wit::EffectKind::Speed,
        2 => wit::EffectKind::Slowness,
        3 => wit::EffectKind::Haste,
        4 => wit::EffectKind::MiningFatigue,
        5 => wit::EffectKind::Strength,
        6 => wit::EffectKind::InstantHealth,
        7 => wit::EffectKind::InstantDamage,
        8 => wit::EffectKind::JumpBoost,
        9 => wit::EffectKind::Nausea,
        10 => wit::EffectKind::Regeneration,
        11 => wit::EffectKind::Resistance,
        12 => wit::EffectKind::FireResistance,
        13 => wit::EffectKind::WaterBreathing,
        14 => wit::EffectKind::Invisibility,
        15 => wit::EffectKind::Blindness,
        16 => wit::EffectKind::NightVision,
        17 => wit::EffectKind::Hunger,
        18 => wit::EffectKind::Weakness,
        19 => wit::EffectKind::Poison,
        20 => wit::EffectKind::Wither,
        21 => wit::EffectKind::HealthBoost,
        22 => wit::EffectKind::Absorption,
        23 => wit::EffectKind::Saturation,
        24 => wit::EffectKind::Levitation,
        25 => wit::EffectKind::FatalPoison,
        26 => wit::EffectKind::ConduitPower,
        27 => wit::EffectKind::SlowFalling,
        28 => wit::EffectKind::BadOmen,
        29 => wit::EffectKind::VillageHero,
        30 => wit::EffectKind::Darkness,
        31 => wit::EffectKind::TrialOmen,
        32 => wit::EffectKind::WindCharged,
        33 => wit::EffectKind::Weaving,
        34 => wit::EffectKind::Oozing,
        35 => wit::EffectKind::Infested,
        36 => wit::EffectKind::RaidOmen,
        _ => return Err(wit::AccessError::InvalidArgument),
    })
}
