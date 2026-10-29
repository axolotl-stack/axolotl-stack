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
//! - Timer: the deadline is drawn when the component is added
//!   (`components::sensors::start_timer`). A timer does nothing while the mob
//!   is dead or after it fired once without `looping`. At the deadline it
//!   marks itself fired, redraws its span when `random_interval` (otherwise
//!   the span stays), schedules the next deadline that span later, and only
//!   then fires `time_down_event`. Nothing is written while it waits.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use tracing::warn;

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::Health;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::sensors::EnvironmentTrigger;
use crate::entity::mob::components::{EnvironmentSensorConfig, SelfEvent, Timer};
use crate::entity::mob::events::{MobFacts, fire_event};
use crate::entity::mob::state::Dead;

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
