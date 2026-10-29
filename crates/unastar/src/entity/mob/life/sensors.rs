//! `minecraft:environment_sensor`, `minecraft:timer` and
//! `minecraft:target_nearby_sensor` (the components are compiled in
//! `mob::components::sensors`): events fired from a mob's surroundings, a
//! countdown and its target's distance.
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
//! - Target nearby sensor: see [`tick_target_nearby_sensors`].

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use tracing::warn;

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{Health, PhysicsBody, Position};
use crate::entity::mob::MobKind;
use crate::entity::mob::ai::Candidate;
use crate::entity::mob::ai::sensing::can_see;
use crate::entity::mob::components::sensors::{
    EnvironmentTrigger, TargetNearbySensorConfig, TargetNearbyState,
};
use crate::entity::mob::components::{EnvironmentSensorConfig, SelfEvent, Timer};
use crate::entity::mob::constants::MOB_HEAD_ATTACH_RATIO;
use crate::entity::mob::events::{MobFacts, fire_event};
use crate::entity::mob::state::{AttackTarget, Dead};
use crate::registry::collision::Aabb;

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

/// Fire `event` on `mob`; a missing or failing event is logged with
/// `source`.
pub(crate) fn fire(world: &mut World, mob: Entity, event: &SelfEvent, source: &'static str) {
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

/// The mobs [`tick_target_nearby_sensors`] reads.
pub type TargetNearbyQuery = (
    Entity,
    &'static TargetNearbySensorConfig,
    &'static Position,
    &'static PhysicsBody,
    Option<&'static AttackTarget>,
    Option<&'static TargetNearbyState>,
);

/// What one `minecraft:target_nearby_sensor` saw this tick.
pub struct TargetNearbySight {
    mob: Entity,
    config: TargetNearbySensorConfig,
    previous: TargetNearbyState,
    /// The box gap to the target and whether the mob sees it; `None`
    /// without a living target.
    target: Option<(f64, bool)>,
}

/// System (exclusive, `EntityLogicSet`, after the fuse tick):
/// `minecraft:target_nearby_sensor`.
///
/// Native rules: every tick, for every mob with the sensor, `d` is the gap
/// between the mob's box and its current target's box (0 when they
/// overlap). Without a target the state resets and nothing fires. The
/// target is inside when `d < inside_range` and outside when
/// `outside_range < d`. Without `must_see`, `on_inside_range` fires when it
/// enters the inside range. With `must_see` it counts as inside only while
/// the mob also sees it (head to head): it fires when the target is first
/// seen inside, and `on_vision_lost_inside_range` fires on the tick the
/// mob stops seeing a target that is not outside. `on_outside_range` fires
/// when it leaves the outside range. All three fire on transitions only, in
/// that order.
pub fn tick_target_nearby_sensors(
    world: &mut World,
    mobs: &mut QueryState<TargetNearbyQuery, With<MobKind>>,
    mut sensing: Local<Vec<TargetNearbySight>>,
) {
    sensing.extend(
        mobs.iter(world)
            .map(|(mob, config, position, body, target, previous)| {
                let target = target
                    .and_then(|target| Candidate::of(world, target.0))
                    .map(|target| {
                        let own = Aabb::from_feet(position.0, body.width, body.height);
                        let gap = own.gap(&target.aabb);
                        let seen = config.must_see && {
                            let head = position.0 + DVec3::Y * body.height * MOB_HEAD_ATTACH_RATIO;
                            can_see(world, head, target.eye)
                        };
                        (gap, seen)
                    });
                TargetNearbySight {
                    mob,
                    config: config.clone(),
                    previous: previous.copied().unwrap_or_default(),
                    target,
                }
            }),
    );
    for sight in sensing.drain(..) {
        let Some((gap, can_see_target)) = sight.target else {
            if sight.previous != TargetNearbyState::default() {
                world.entity_mut(sight.mob).remove::<TargetNearbyState>();
            }
            continue;
        };
        let config = &sight.config;
        let previous = sight.previous;
        let inside = gap < config.inside_range;
        let outside = config.outside_range < gap;
        let mut fire_inside = !previous.inside && inside;
        let mut fire_lost = false;
        let mut seen = false;
        if config.must_see {
            if inside && can_see_target {
                seen = true;
                fire_inside = !previous.inside || !previous.seen;
            } else {
                fire_inside = false;
                if outside {
                    seen = can_see_target;
                } else if can_see_target {
                    seen = true;
                } else {
                    fire_lost = previous.seen;
                }
            }
        }
        let state = TargetNearbyState {
            seen,
            inside,
            outside,
        };
        if state != previous {
            let mut entity = world.entity_mut(sight.mob);
            if state == TargetNearbyState::default() {
                entity.remove::<TargetNearbyState>();
            } else {
                entity.insert(state);
            }
        }
        let fired = [
            (fire_inside, &config.on_inside_range),
            (outside && !previous.outside, &config.on_outside_range),
            (fire_lost, &config.on_vision_lost_inside_range),
        ];
        for (fires, event) in fired {
            if fires && let Some(event) = event {
                fire(world, sight.mob, event, "target_nearby_sensor");
            }
        }
    }
}
