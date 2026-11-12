//! `minecraft:environment_sensor` and `minecraft:timer` (the components are
//! compiled in `mob::components::sensors`): events fired from a mob's
//! surroundings and from a countdown.
//!
//! Native rules:
//! - Environment sensor: every tick, for every actor that is not removed
//!   (a dying mob still senses), each trigger is checked in authored order
//!   and every one whose filter passes fires its event on the mob; there is
//!   no "first match" stop. A trigger's filter sees what the earlier
//!   triggers' events already changed (property assignments apply at once;
//!   group changes wait for the definition apply barrier). The filter's
//!   `other` is the mob's vehicle, which never exists (riding is not
//!   implemented).
//! - Target-nearby sensor: every tick, for every living mob with a target,
//!   the target's zone is found from the gap between the two collision
//!   boxes (and, when the sensor must see, the line from the mob's eye to the
//!   target's); the event of the zone it moved into fires on the mob:
//!   `on_inside_range` on entering the inside range in sight,
//!   `on_vision_lost_inside_range` on losing sight inside it, and
//!   `on_outside_range` on leaving past the outside range. Staying in a
//!   zone fires nothing; a mob without a target forgets the zone (BDS
//!   1.26.51 creeper sweeps: a creeper lit once per approach, its fuse
//!   cancelled one tick after the player left past 6 blocks or hid behind
//!   a wall, and not cancelled with the player 5.8 blocks away between the
//!   boxes, 6.4 between their centres).
//! - Timer: the deadline is drawn when the component is added
//!   (`components::sensors::start_timer`). A timer does nothing while the mob
//!   is dead or after it fired once without `looping`. At the deadline it
//!   marks itself fired, redraws its span when `random_interval` (otherwise
//!   the span stays), schedules the next deadline that span later, and only
//!   then fires `time_down_event`. Nothing is written while it waits.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use tracing::warn;

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::Health;
use crate::entity::mob::MobKind;
use crate::entity::mob::ai::Candidate;
use crate::entity::mob::ai::sensing::can_see;
use crate::entity::mob::components::sensors::EnvironmentTrigger;
use crate::entity::mob::components::{
    EnvironmentSensorConfig, SelfEvent, TargetNearbySensorConfig, TargetZone, Timer,
};
use crate::entity::mob::events::{MobFacts, fire_event};
use crate::entity::mob::state::{AttackTarget, Dead};

/// System (exclusive, `EntityLogicSet`): fire the events of every passing
/// environment sensor trigger.
pub fn tick_environment_sensors(
    world: &mut World,
    mobs: &mut QueryState<(Entity, &EnvironmentSensorConfig), With<MobKind>>,
    mut sensing: Local<Vec<(Entity, Arc<[EnvironmentTrigger]>)>>,
) {
    sensing.extend(
        mobs.iter(world)
            .map(|(mob, config)| (mob, Arc::clone(&config.triggers))),
    );
    for (mob, triggers) in sensing.drain(..) {
        for trigger in triggers.iter() {
            let passes = trigger.filter.evaluate(&MobFacts {
                world,
                entity: mob,
                other: None,
            });
            if passes {
                fire(world, mob, &trigger.event, "environment_sensor");
            }
        }
    }
}

type SensorQuery = (
    Entity,
    &'static TargetNearbySensorConfig,
    Option<&'static TargetZone>,
    Option<&'static AttackTarget>,
);

/// A mob with a target-nearby sensor, as read before its events fire.
pub struct SensingMob {
    mob: Entity,
    config: TargetNearbySensorConfig,
    previous: Option<TargetZone>,
    target: Option<Entity>,
}

/// System (exclusive, `EntityLogicSet`): fire the target-nearby sensor
/// events of every mob whose target moved into another zone.
pub fn tick_target_nearby_sensors(
    world: &mut World,
    mobs: &mut QueryState<SensorQuery, (With<MobKind>, Without<Dead>)>,
    mut sensing: Local<Vec<SensingMob>>,
) {
    sensing.extend(
        mobs.iter(world)
            .map(|(mob, config, zone, target)| SensingMob {
                mob,
                config: config.clone(),
                previous: zone.copied(),
                target: target.map(|t| t.0),
            }),
    );
    for SensingMob {
        mob,
        config,
        previous,
        target,
    } in sensing.drain(..)
    {
        let Some(zone) = target.and_then(|target| target_zone(world, mob, target, &config)) else {
            if previous.is_some() {
                world.entity_mut(mob).remove::<TargetZone>();
            }
            continue;
        };
        if previous != Some(zone) {
            world.entity_mut(mob).insert(zone);
        }
        if let Some(event) = config.crossing(previous, zone) {
            fire(world, mob, event, "target_nearby_sensor");
        }
    }
}

/// The zone `target` is in for `mob`'s sensor; `None` when either is gone.
fn target_zone(
    world: &World,
    mob: Entity,
    target: Entity,
    config: &TargetNearbySensorConfig,
) -> Option<TargetZone> {
    let own = Candidate::of(world, mob)?;
    let other = Candidate::of(world, target)?;
    let gap = (own.aabb.min - other.aabb.max)
        .max(other.aabb.min - own.aabb.max)
        .max(DVec3::ZERO)
        .length();
    let seen = !config.must_see || can_see(world, own.eye, other.eye);
    Some(config.zone(gap, seen))
}

/// System (exclusive, `EntityLogicSet`): fire the timers whose deadline came.
#[allow(clippy::type_complexity)]
pub fn tick_timers(
    world: &mut World,
    timers: &mut QueryState<(Entity, &Timer, Option<&Health>), (With<MobKind>, Without<Dead>)>,
    mut due: Local<Vec<Entity>>,
) {
    let now = world.resource::<TickCounter>().current;
    due.extend(
        timers
            .iter(world)
            .filter(|(_, timer, health)| {
                (timer.config.looping || !timer.fired)
                    && now >= timer.deadline
                    && health.is_none_or(|health| !health.is_dead())
            })
            .map(|(mob, _, _)| mob),
    );
    for mob in due.drain(..) {
        let Some(event) = reschedule(world, mob, now) else {
            continue;
        };
        fire(world, mob, &event, "timer");
    }
}

/// Mark `mob`'s timer fired and schedule its next deadline; the event to
/// fire. `None` when an earlier event this tick removed the timer.
fn reschedule(world: &mut World, mob: Entity, now: u64) -> Option<SelfEvent> {
    let mut timer = world.get::<Timer>(mob)?.clone();
    timer.fired = true;
    if timer.config.random_interval {
        timer.span_ticks = timer
            .config
            .span
            .draw(&mut world.get_resource_or_init::<EntityRandom>().0);
    }
    timer.deadline = now + timer.span_ticks.max(0) as u64;
    let event = timer.config.event.clone();
    *world.get_mut::<Timer>(mob)? = timer;
    Some(event)
}

fn fire(world: &mut World, mob: Entity, event: &SelfEvent, source: &'static str) {
    let Some(handle) = world
        .get::<MobKind>(mob)
        .and_then(|kind| kind.0.events.event(&event.0))
    else {
        warn!(event = %event.0, source, "sensor names an event the mob does not define");
        return;
    };
    if let Err(error) = fire_event(world, mob, &handle, None) {
        warn!(event = %event.0, source, %error, "sensor event failed");
    }
}
