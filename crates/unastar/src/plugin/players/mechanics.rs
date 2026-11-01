//! Bounded callback staging over native mechanics; no component borrow survives an import.

use super::*;
use crate::entity::components::{
    ActiveEffect, EffectType, Effects, GameMode, PlayerSession, PlayerState, RuntimeEntityId,
    Velocity,
};
use crate::plugin::manifest::PluginCapability;
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
    application: wit::EffectApplication,
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
}

#[derive(Resource, Default)]
struct Pending(Vec<MechanicsRequest>);

/// Discard failed/vetoed mechanics independently of message, command and task semantics.
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
            || !ready(world, request.player)
            || !super::super::readiness::admission_allowed(world)
        {
            continue;
        }
        let player = request.player;
        match request.mutation {
            Mutation::Apply(intent) => {
                let EffectIntent {
                    application,
                    source,
                } = *intent;
                if !source.as_ref().is_none_or(|(reference, entity)| {
                    reference.matches(world, *entity) && ready(world, *entity)
                }) || !valid_application(&application)
                {
                    continue;
                }
                effects::add_effect_state_from(
                    world,
                    player,
                    source.as_ref().map(|(_, entity)| *entity),
                    native_kind(application.kind),
                    ActiveEffect {
                        level: application.amplifier,
                        duration_ticks: application.duration_ticks,
                        ambient: application.ambient,
                        show_particles: application.show_particles,
                        infinite: false,
                    },
                    |world| {
                        request.owner.upgrade().is_some()
                            && request.scope.world.matches(world)
                            && request.reference.matches(world, player)
                            && ready(world, player)
                            && super::super::readiness::admission_allowed(world)
                            && source.as_ref().is_none_or(|(reference, entity)| {
                                reference.matches(world, *entity) && ready(world, *entity)
                            })
                    },
                );
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
            Mutation::Velocity(_) => {}
        }
    }
}

/// Mutations require a complete, published, live connection; retained actors remain readable.
fn ready(world: &World, player: Entity) -> bool {
    matches!(availability(world, player), wit::PlayerAvailability::Ready)
        && world.get::<PlayerSession>(player).is_some()
}

/// Restrict durations to the signed native packet range; instant effects use at least one tick.
fn valid_application(effect: &wit::EffectApplication) -> bool {
    (1..=i32::MAX as u32).contains(&effect.duration_ticks)
}

/// Motion uses finite wire-representable values within one native movement distance bound.
fn valid_velocity(value: DVec3) -> bool {
    value.is_finite()
        && value.as_vec3().is_finite()
        && value.length_squared() <= MAX_SPEED * MAX_SPEED
}

impl HostContext {
    /// Capture owned current mechanics without using an observation from a previous tick.
    pub(in crate::plugin::manager) fn mechanics_state(
        &mut self,
        rep: u32,
    ) -> Result<wit::PlayerMechanicsState, wit::AccessError> {
        let (_, player) = self.live_player(rep)?;
        let world = unsafe { self.world() };
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
        if !ready(unsafe { self.world() }, player) {
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
            .is_some_and(|(_, entity)| !ready(unsafe { self.world() }, *entity))
        {
            return Err(wit::AccessError::Unavailable);
        }
        self.stage_mechanics(
            rep,
            Mutation::Apply(Box::new(EffectIntent {
                application,
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
fn native_kind(kind: wit::EffectKind) -> EffectType {
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
fn kind(effect: EffectType) -> Result<wit::EffectKind, wit::AccessError> {
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
