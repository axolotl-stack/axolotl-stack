//! Anger (`minecraft:angry`; the configuration is
//! `mob::components::angry::AngryConfig`).
//!
//! Applying the component starts the anger: [`Angry`] holds the tick it runs
//! out at (none for a negative draw). Every tick, [`tick_anger`] then runs
//! each angry mob, dying ones included, in the native order:
//!
//! 1. A mob that can no longer be angry at its target ([`can_stay_angry`]:
//!    the target is gone, dead, fails `filters`, or is a creative or
//!    spectator player or any player on Peaceful; a mob without a target
//!    stays angry only without `filters`) calms at once: the `ANGRY` flag
//!    clears, the target is dropped and `calm_event` fires.
//! 2. Otherwise it shows `ANGRY` and spreads its anger ([`broadcast`]) the
//!    first time (`broadcast_anger`) and, with `broadcast_anger_on_attack` /
//!    `_on_being_attacked`, on every hit it dealt / took from an actor this
//!    tick or the one before. A dying mob spreads only with
//!    `broadcast_anger_when_dying`.
//! 3. When the anger runs out the mob calms the same way as in 1.
//! 4. Every `sound_interval` it plays `angry_sound` at its head.
//!
//! Every swing of an angry mob draws its span again ([`restart_on_attack`]).
//! Removing the component drops the anger without the calm event.
//!
//! Measured on BDS 1.26.51 (zombie pigmen and a zoglin, 2026-11-10 captures,
//! `testdata/bds-1.26.51-pigman-anger.json` and
//! `testdata/bds-1.26.51-anger-chain.json`): anger spreads to pigmen within
//! 20 blocks one tick after the spreading pigman turned angry, and on from
//! each receiver (a chain 15 blocks apart reached the fourth pigman three
//! ticks after the first); each pigman calms 499 ticks after its anger
//! started or its last swing and then drops its target, the hurt one too;
//! the zoglin's angry sound plays on its first angry tick and then every
//! 41 to 101 ticks while it stays angry, target or not.

use bevy_ecs::prelude::*;
use bevy_ecs::world::EntityWorldMut;
use glam::DVec3;
use rand::Rng;
use tracing::warn;

use crate::ecs::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{GameMode, Health, PhysicsBody, Player, Position};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::OnFriendlyAnger;
use crate::entity::mob::components::angry::AngryConfig;
use crate::entity::mob::constants::{MOB_EYE_HEIGHT_RATIO, TICKS_PER_SECOND};
use crate::entity::mob::events::{MobFacts, fire_event, is_family};
use crate::entity::mob::state::{AttackTarget, Dead, LastAttack, LastHurt};
use crate::registry::collision::Aabb;
use crate::world::rules::Difficulty;
use jolyne::valentine::actor_flags;

/// An angry mob's running state.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct Angry {
    /// The tick the anger runs out on; never when `None`.
    pub until_tick: Option<u64>,
    /// `broadcast_anger` spread it already.
    pub broadcast_done: bool,
    /// The angry sound plays on the first tick after this one.
    pub next_sound_tick: u64,
}

/// The tick an anger drawn now runs out on: a draw of `duration - delta +
/// random(2 × delta + 1)` seconds counted down once per tick from the tick
/// it starts on (the anger tick runs after the start in the same tick), so
/// a span of `n` ticks ends on tick `now + n - 1`; a negative draw never
/// ends. Every start and every restart draws again.
fn draw_until(world: &mut World, config: &AngryConfig, now: u64) -> Option<u64> {
    let delta = config.duration_delta;
    let drawn = if delta < 0 {
        0
    } else {
        world
            .get_resource_or_init::<EntityRandom>()
            .0
            .gen_range(0..=2 * i64::from(delta))
    };
    let seconds = i64::from(config.duration) - i64::from(delta) + drawn;
    let ticks = seconds * TICKS_PER_SECOND as i64;
    (ticks >= 0).then(|| now + (ticks.max(1) - 1) as u64)
}

/// Start `config`'s anger on the mob (component apply).
pub(crate) fn start(entity: &mut EntityWorldMut<'_>, config: &AngryConfig) {
    let until_tick = entity.world_scope(|world| {
        let now = world.resource::<TickCounter>().current;
        draw_until(world, config, now)
    });
    entity.insert((
        config.clone(),
        Angry {
            until_tick,
            broadcast_done: false,
            next_sound_tick: 0,
        },
    ));
}

/// End the anger without the calm event (component removal).
pub(crate) fn stop(entity: &mut EntityWorldMut<'_>) {
    entity.remove::<(AngryConfig, Angry)>();
    set_angry_flag(entity, false);
}

/// The angry `mob` swung at its target: its anger lasts a fresh draw of its
/// span from now. Measured: zombie pigmen calmed 499 ticks after their own
/// last swing, also when the swing did no damage.
pub fn restart_on_attack(world: &mut World, mob: Entity) {
    let Some(config) = world
        .get::<AngryConfig>(mob)
        .filter(|_| world.get::<Angry>(mob).is_some())
        .cloned()
    else {
        return;
    };
    let now = world.resource::<TickCounter>().current;
    let until = draw_until(world, &config, now);
    if let Some(mut angry) = world.get_mut::<Angry>(mob) {
        angry.until_tick = until;
    }
}

/// Set or clear `ANGRY`, mirroring the flag words into the actor data the
/// viewers receive.
fn set_angry_flag(entity: &mut EntityWorldMut<'_>, on: bool) {
    use jolyne::valentine::actor_data_i_ds as ids;
    let Some(mut flags) = entity.get_mut::<ActorFlags>() else {
        return;
    };
    if !flags
        .update(actor_flags::ANGRY, on)
        .expect("catalogue flag index")
    {
        return;
    }
    let words = *flags.snapshot().words();
    if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
        metadata.set_long(ids::RESERVED_0, words[0] as i64);
        metadata.set_long(ids::RESERVED_092, words[1] as i64);
    }
}

/// The mobs [`tick_anger`] runs: angry ones, dying ones included.
type AngryMobs = (With<Angry>, With<AngryConfig>, With<MobKind>);

/// System (exclusive, `EntityLogicSet`): run every angry mob's anger.
pub fn tick_anger(
    world: &mut World,
    angry: &mut QueryState<Entity, AngryMobs>,
    mut mobs: Local<Vec<Entity>>,
    mut receivers: Local<Vec<Entity>>,
) {
    mobs.clear();
    mobs.extend(angry.iter(world));
    let now = world.resource::<TickCounter>().current;
    for &mob in mobs.iter() {
        // An earlier mob's events may have ended this one's anger.
        let (Some(config), Some(state)) = (
            world.get::<AngryConfig>(mob).cloned(),
            world.get::<Angry>(mob).copied(),
        ) else {
            continue;
        };
        let target = world.get::<AttackTarget>(mob).map(|t| t.0);
        if !can_stay_angry(world, mob, &config, target) {
            calm(world, mob, &config);
            continue;
        }
        set_angry_flag(&mut world.entity_mut(mob), true);
        if wants_broadcast(world, mob, &config, &state, now) {
            if let Some(mut angry) = world.get_mut::<Angry>(mob) {
                angry.broadcast_done = true;
            }
            broadcast(world, mob, &config, &mut receivers);
        }
        if state.until_tick.is_some_and(|until| now >= until) {
            calm(world, mob, &config);
            continue;
        }
        if state.next_sound_tick < now {
            if let Some(sound) = &config.sound
                && let Some(head) = head(world, mob)
            {
                crate::entity::mob::net::sound::queue_synced_sound(
                    world,
                    Some(mob),
                    head,
                    sound,
                    -1,
                );
            }
            let (min, max) = config.sound_interval;
            let u: f32 = world.resource_mut::<EntityRandom>().0.r#gen();
            let wait = ((min + (max - min) * u) * TICKS_PER_SECOND) as u64;
            if let Some(mut angry) = world.get_mut::<Angry>(mob) {
                angry.next_sound_tick = now + wait;
            }
        }
    }
}

/// The mob's head: its feet plus [`MOB_EYE_HEIGHT_RATIO`] of its height
/// (where BDS plays its angry and warning sounds).
pub fn head(world: &World, mob: Entity) -> Option<DVec3> {
    let feet = world.get::<Position>(mob)?.0;
    let height = world
        .get::<PhysicsBody>(mob)
        .map_or(0.0, |body| body.height);
    Some(feet + DVec3::Y * height * MOB_EYE_HEIGHT_RATIO)
}

fn alive(world: &World, actor: Entity) -> bool {
    world.get::<Dead>(actor).is_none() && world.get::<Health>(actor).is_some_and(|h| !h.is_dead())
}

/// Whether `mob` can stay angry at `target` (the native per-tick check).
fn can_stay_angry(
    world: &World,
    mob: Entity,
    config: &AngryConfig,
    target: Option<Entity>,
) -> bool {
    let Some(target) = target else {
        return config.filter.is_none();
    };
    if target == mob || world.get_entity(target).is_err() {
        return false;
    }
    if !passes_filters(world, mob, config, target) || !alive(world, target) {
        return false;
    }
    if world.get::<Player>(target).is_some() {
        let invulnerable = matches!(
            world.get::<GameMode>(target),
            Some(GameMode::Creative | GameMode::Spectator)
        );
        let peaceful = world
            .get_resource::<Difficulty>()
            .is_some_and(|difficulty| *difficulty == Difficulty::Peaceful);
        if invulnerable || peaceful {
            return false;
        }
    }
    true
}

/// `filters`: `target` passes them with the angry mob as `self`.
fn passes_filters(world: &World, mob: Entity, config: &AngryConfig, target: Entity) -> bool {
    config.filter.as_ref().is_none_or(|filter| {
        filter.evaluate(&MobFacts {
            world,
            entity: mob,
            other: Some(target),
        })
    })
}

/// Whether the anger spreads this tick.
fn wants_broadcast(
    world: &World,
    mob: Entity,
    config: &AngryConfig,
    state: &Angry,
    now: u64,
) -> bool {
    let recently = |tick: u64| tick + 2 > now;
    let first = config.broadcast && !state.broadcast_done;
    let attacked = config.broadcast_on_attack
        && world
            .get::<LastAttack>(mob)
            .is_some_and(|attack| recently(attack.tick));
    let hurt = config.broadcast_on_being_attacked
        && world
            .get::<LastHurt>(mob)
            .is_some_and(|hurt| hurt.attacker.is_some() && recently(hurt.tick));
    (first || attacked || hurt) && (config.broadcast_when_dying || world.get::<Dead>(mob).is_none())
}

/// Spread `mob`'s anger at its target: every mob within `broadcast_range`
/// (on every axis, box to box) of its native actor type, or of a family in
/// `broadcast_targets`, that has no target, is not tamed, passes
/// `broadcast_filters`, and could be angry at the same target, takes it (its
/// `on_target_acquired` fires) and fires its `on_friendly_anger` with the
/// angry mob as `other`. A mob without a target spreads nothing, and its one
/// `broadcast_anger` spread is used up (measured: a pigman that lost its
/// target beyond its follow range on its first angry tick stayed angry
/// without taking a target again).
fn broadcast(world: &mut World, mob: Entity, config: &AngryConfig, receivers: &mut Vec<Entity>) {
    let Some(target) = world.get::<AttackTarget>(mob).map(|t| t.0) else {
        return;
    };
    let (Some(kind), Some(feet), Some(body)) = (
        world.get::<MobKind>(mob).map(|kind| kind.0.actor_type),
        world.get::<Position>(mob).map(|p| p.0),
        world
            .get::<PhysicsBody>(mob)
            .map(|body| (body.width, body.height)),
    ) else {
        return;
    };
    let area = Aabb::from_feet(feet, body.0, body.1).inflate(DVec3::splat(config.broadcast_range));
    receivers.clear();
    let mut query = world.query::<(Entity, &MobKind, &Position, &PhysicsBody)>();
    receivers.extend(
        query
            .iter(world)
            .filter(|(other, other_kind, position, other_body)| {
                *other != mob
                    && area.intersects(&Aabb::from_feet(
                        position.0,
                        other_body.width,
                        other_body.height,
                    ))
                    && (if config.broadcast_targets.is_empty() {
                        other_kind.0.actor_type == kind
                    } else {
                        true
                    })
            })
            .map(|(other, ..)| other),
    );
    if !config.broadcast_targets.is_empty() {
        receivers.retain(|&other| {
            config
                .broadcast_targets
                .iter()
                .any(|family| is_family(world, other, family))
        });
    }
    for receiver in receivers.drain(..) {
        if receiver == target
            || world.get::<AttackTarget>(receiver).is_some()
            || world.get::<Dead>(receiver).is_some()
            || !passes_filters(world, mob, config, target)
            || !alive(world, target)
        {
            continue;
        }
        let tamed = world
            .get::<ActorFlags>(receiver)
            .is_some_and(|flags| flags.get(actor_flags::TAMED).unwrap_or(false));
        if tamed {
            continue;
        }
        if let Some(filter) = &config.broadcast_filter
            && !filter.evaluate(&MobFacts {
                world,
                entity: receiver,
                other: Some(mob),
            })
        {
            continue;
        }
        if crate::entity::mob::ai::provoke(world, receiver, target) {
            friendly_anger(world, receiver, mob);
        }
    }
}

/// Fire `receiver`'s `on_friendly_anger` (`other` is the angry mob).
fn friendly_anger(world: &mut World, receiver: Entity, angry: Entity) {
    let Some(trigger) = world.get::<OnFriendlyAnger>(receiver).map(|t| t.0.clone()) else {
        return;
    };
    let Some(event) = trigger.event else {
        return;
    };
    if let Some(filter) = &trigger.filter
        && !filter.evaluate(&MobFacts {
            world,
            entity: receiver,
            other: Some(angry),
        })
    {
        return;
    }
    let Some(handle) = world
        .get::<MobKind>(receiver)
        .and_then(|kind| kind.0.events.event(&event.0))
    else {
        warn!(event = %event.0, "on_friendly_anger names an event the mob does not define");
        return;
    };
    if let Err(error) = fire_event(world, receiver, &handle, Some(angry)) {
        warn!(event = %event.0, %error, "on_friendly_anger event failed");
    }
}

/// The anger ends: `ANGRY` clears and the anger state goes (a later
/// component apply starts a new one), the target is dropped (its
/// `on_target_escape` fires) and `calm_event` fires.
fn calm(world: &mut World, mob: Entity, config: &AngryConfig) {
    stop(&mut world.entity_mut(mob));
    crate::entity::mob::ai::drop_target(world, mob);
    let Some(event) = &config.calm_event else {
        return;
    };
    let Some(handle) = world
        .get::<MobKind>(mob)
        .and_then(|kind| kind.0.events.event(&event.0))
    else {
        warn!(event = %event.0, "calm event the mob does not define");
        return;
    };
    if let Err(error) = fire_event(world, mob, &handle, None) {
        warn!(event = %event.0, %error, "calm event failed");
    }
}
