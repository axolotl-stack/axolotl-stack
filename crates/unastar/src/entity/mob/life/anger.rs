//! Anger (`minecraft:angry`; the configuration is
//! `mob::components::angry::AngryConfig`).
//!
//! Applying the component starts the anger: [`Angry`] holds the tick it runs
//! out at (none for a duration of -1) and the mob shows the `ANGRY` actor
//! flag while the component stays. [`tick_anger`] then, in order, spreads
//! new anger to the mob's kin (`broadcast_anger`: every other living mob of
//! the same definition within `broadcast_range` that passes
//! `broadcast_filters` takes the angry mob's target as its own, which fires
//! its `on_target_acquired`), and fires `calm_event` on mobs whose anger ran
//! out; a target the mob only had through its kin's anger goes then too
//! (unmeasured: no target goal holds such a target). Every attack of an
//! angry mob restarts its anger span ([`restart_on_attack`]). Anger a mob
//! received from its kin does not spread again. Removing the component drops
//! the anger without the calm event.
//!
//! Measured on BDS 1.26.51 (zombie pigmen, 2026-11-10 captures): one tick
//! after the hit the hit pigman and the pigmen 6 and 12 blocks away target
//! the attacker and show `ANGRY`; a pigman 15 blocks from a receiver but 27
//! from the hit one stays calm; each pigman calms 25 s after its own last
//! swing (`testdata/bds-1.26.51-pigman-anger.json`).

use bevy_ecs::prelude::*;
use bevy_ecs::world::EntityWorldMut;
use rand::Rng;
use tracing::warn;

use crate::ecs::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::Position;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::angry::AngryConfig;
use crate::entity::mob::events::{MobFacts, fire_event};
use crate::entity::mob::state::{AttackTarget, Dead};

/// An angry mob: its anger runs out at `until_tick` (never when `None`),
/// `span_ticks` after it started or the mob last attacked.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct Angry {
    pub until_tick: Option<u64>,
    pub span_ticks: Option<u64>,
}

/// The angry `mob` attacked: its anger lasts its full span again from now.
/// Measured: zombie pigmen calmed 25 s after their own last swing, also when
/// the swing did no damage.
pub fn restart_on_attack(world: &mut World, mob: Entity) {
    let Some(span) = world.get::<Angry>(mob).and_then(|angry| angry.span_ticks) else {
        return;
    };
    let now = world.resource::<TickCounter>().current;
    if let Some(mut angry) = world.get_mut::<Angry>(mob) {
        angry.until_tick = Some(now + span);
    }
}

/// The anger started since the last [`tick_anger`]: it still has to spread.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct AngerToSpread;

/// The mob was provoked by an angry kin: the anger it starts next does not
/// spread.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct AngerReceived;

/// Start `config`'s anger on the mob (component apply).
pub(crate) fn start(entity: &mut EntityWorldMut<'_>, config: &AngryConfig) {
    let (now, span_ticks) = entity.world_scope(|world| {
        let now = world.resource::<TickCounter>().current;
        let span = config.duration_ticks.map(|ticks| {
            let delta = i64::from(config.duration_delta_ticks);
            let drawn = if delta > 0 {
                world
                    .get_resource_or_init::<EntityRandom>()
                    .0
                    .gen_range(-delta..=delta)
            } else {
                0
            };
            (i64::from(ticks) + drawn).max(0) as u64
        });
        (now, span)
    });
    entity.insert((
        config.clone(),
        Angry {
            until_tick: span_ticks.map(|span| now + span),
            span_ticks,
        },
    ));
    if entity.take::<AngerReceived>().is_none() {
        entity.insert(AngerToSpread);
    }
    set_angry_flag(entity, true);
}

/// End the anger without the calm event (component removal).
pub(crate) fn stop(entity: &mut EntityWorldMut<'_>) {
    entity.remove::<(AngryConfig, Angry, AngerToSpread, AngerReceived)>();
    set_angry_flag(entity, false);
}

fn set_angry_flag(entity: &mut EntityWorldMut<'_>, on: bool) {
    if let Some(mut flags) = entity.get_mut::<ActorFlags>() {
        flags
            .update(jolyne::valentine::actor_flags::ANGRY, on)
            .expect("catalogue flag index");
    }
}

/// System (exclusive, `EntityLogicSet`): spread new anger, then calm the
/// mobs whose anger ran out.
#[allow(clippy::type_complexity)]
pub fn tick_anger(
    world: &mut World,
    angry: &mut QueryState<(Entity, &Angry, Has<AngerToSpread>), (With<MobKind>, Without<Dead>)>,
    kin: &mut QueryState<(Entity, &MobKind, &Position), Without<Dead>>,
    provoked: &mut QueryState<Entity, (With<AngerReceived>, Without<Angry>)>,
    mut spread: Local<Vec<Entity>>,
    mut calm: Local<Vec<Entity>>,
    mut receivers: Local<Vec<Entity>>,
) {
    // A provoked mob's anger starts at the definition barrier after it was
    // provoked; one that did not become angry by now never will from it.
    receivers.extend(provoked.iter(world));
    for mob in receivers.drain(..) {
        world.entity_mut(mob).remove::<AngerReceived>();
    }
    let now = world.resource::<TickCounter>().current;
    for (mob, anger, to_spread) in angry.iter(world) {
        if to_spread {
            spread.push(mob);
        }
        if anger.until_tick.is_some_and(|until| now >= until) {
            calm.push(mob);
        }
    }
    for mob in spread.drain(..) {
        world.entity_mut(mob).remove::<AngerToSpread>();
        spread_anger(world, mob, kin, &mut receivers);
    }
    for mob in calm.drain(..) {
        let Some(config) = world.get::<AngryConfig>(mob).cloned() else {
            continue;
        };
        world.entity_mut(mob).remove::<Angry>();
        // A target the mob only had through its kin's anger goes with it.
        crate::entity::mob::ai::release_unheld_target(world, mob);
        let Some(event) = config.calm_event else {
            continue;
        };
        let Some(handle) = world
            .get::<MobKind>(mob)
            .and_then(|kind| kind.0.events.event(&event.0))
        else {
            warn!(event = %event.0, "calm event the mob does not define");
            continue;
        };
        if let Err(error) = fire_event(world, mob, &handle, None) {
            warn!(event = %event.0, %error, "calm event failed");
        }
    }
}

/// `broadcast_anger`: the angry mob's kin within range take its target.
fn spread_anger(
    world: &mut World,
    mob: Entity,
    kin: &mut QueryState<(Entity, &MobKind, &Position), Without<Dead>>,
    receivers: &mut Vec<Entity>,
) {
    let (Some(config), Some(target), Some(kind), Some(origin)) = (
        world.get::<AngryConfig>(mob).cloned(),
        world.get::<AttackTarget>(mob).map(|t| t.0),
        world
            .get::<MobKind>(mob)
            .map(|k| std::sync::Arc::clone(&k.0)),
        world.get::<Position>(mob).map(|p| p.0),
    ) else {
        return;
    };
    if !config.broadcast {
        return;
    }
    let range_sq = config.broadcast_range * config.broadcast_range;
    receivers.extend(
        kin.iter(world)
            .filter(|(other, other_kind, position)| {
                *other != mob
                    // A mob of the same kind that is the target (one pigman
                    // hit by another) is never provoked against itself.
                    && *other != target
                    && std::sync::Arc::ptr_eq(&other_kind.0, &kind)
                    && position.0.distance_squared(origin) <= range_sq
            })
            .map(|(other, _, _)| other),
    );
    for receiver in receivers.drain(..) {
        if world.get::<Angry>(receiver).is_some() {
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
        // Marked before the provocation fires the receiver's
        // `on_target_acquired`; its anger starts at the next barrier.
        world.entity_mut(receiver).insert(AngerReceived);
        if !crate::entity::mob::ai::provoke(world, receiver, target) {
            world.entity_mut(receiver).remove::<AngerReceived>();
        }
    }
}
