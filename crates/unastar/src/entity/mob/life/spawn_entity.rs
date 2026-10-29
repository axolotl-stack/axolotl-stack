//! Egg laying: the `minecraft:spawn_entity` system (the component is
//! compiled in `mob::components::spawn_entity`); periodically drop an item
//! (the chicken's egg).
//!
//! Native rule: every rule (the root one and each `entities` entry) has its
//! own timer, running from the first tick the component exists whatever its
//! filters say. An idle timer rolls `min + random(max - min)` ticks (upper
//! bound exclusive; `min_wait_time`/`max_wait_time` are seconds × 20) and
//! the rolling tick counts as the first, so a rule fires exactly that many
//! ticks after its previous fire. When it fires its filters are evaluated:
//! passing, the `spawn_sound` plays at the mob's mid-body and `num_to_spawn`
//! items drop at its feet with the ordinary item toss, then
//! `spawn_item_event` fires on the mob; failing, nothing happens. A
//! `single_use` rule stops after its first fire, even a filtered-out one.
//! There is no age gate: vanilla puts the component in the adult group.

use bevy_ecs::prelude::*;
use rand::Rng;
use tracing::warn;

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::Position;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::{SpawnEntityConfig, SpawnEntityTimers, SpawnRule, SpawnTimer};
use crate::entity::mob::events::{MobFacts, fire_event};
use crate::entity::mob::net::sound::{body_centre, play_level_sound};
use crate::entity::mob::state::Dead;

/// System (exclusive, `EntityLogicSet`): advance spawn timers and fire the
/// rules that are due.
#[allow(clippy::type_complexity)]
pub fn tick_spawn_entity(
    world: &mut World,
    mobs: &mut QueryState<
        (Entity, &SpawnEntityConfig, &mut SpawnEntityTimers),
        (With<MobKind>, Without<Dead>),
    >,
    mut due: Local<Vec<(Entity, SpawnRule)>>,
) {
    let now = world.resource::<TickCounter>().current;
    let mut rng = world.remove_resource::<EntityRandom>().unwrap_or_default();
    for (mob, config, mut timers) in mobs.iter_mut(world) {
        for (rule, timer) in config.rules.iter().zip(timers.0.iter_mut()) {
            if *timer == SpawnTimer::Idle {
                let wait = if rule.max_ticks > rule.min_ticks {
                    rule.min_ticks + rng.0.gen_range(0..rule.max_ticks - rule.min_ticks)
                } else {
                    rule.min_ticks
                };
                // The rolling tick counts as the first tick of the wait.
                *timer = SpawnTimer::Due(now + u64::from(wait.max(1)) - 1);
            }
            if let SpawnTimer::Due(tick) = *timer
                && now >= tick
            {
                *timer = if rule.single_use {
                    SpawnTimer::Finished
                } else {
                    SpawnTimer::Idle
                };
                due.push((mob, rule.clone()));
            }
        }
    }
    world.insert_resource(rng);
    for (mob, rule) in due.drain(..) {
        fire(world, mob, &rule);
    }
}

fn fire(world: &mut World, mob: Entity, rule: &SpawnRule) {
    if world.get::<Dead>(mob).is_some() {
        return;
    }
    let passes = rule.filter.as_ref().is_none_or(|filter| {
        filter.evaluate(&MobFacts {
            world,
            entity: mob,
            other: None,
        })
    });
    let (true, Some(feet), Some(centre)) = (
        passes,
        world.get::<Position>(mob).map(|p| p.0),
        body_centre(world, mob),
    ) else {
        return;
    };
    play_level_sound(world, Some(mob), centre, &rule.sound, -1);
    let Some(stack) = crate::server::game::world_stack(world, &rule.item, 1) else {
        tracing::warn!(item = %rule.item, "spawn_entity item is not in the item registry");
        return;
    };
    for _ in 0..rule.count {
        crate::entity::spawn_item_drop(world, feet, stack.clone(), None);
    }
    let Some(name) = &rule.item_event else {
        return;
    };
    let Some(handle) = world
        .get::<MobKind>(mob)
        .and_then(|kind| kind.0.events.event(name))
    else {
        warn!(event = %name, "spawn_item_event names an event the mob does not define");
        return;
    };
    if let Err(error) = fire_event(world, mob, &handle, None) {
        warn!(event = %name, %error, "spawn_item_event failed");
    }
}
