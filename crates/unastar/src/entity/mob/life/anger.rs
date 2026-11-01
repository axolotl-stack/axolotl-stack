//! Timed anger expires once, clears its combat target and dispatches the
//! authored calm event after the state change commits.

use bevy_ecs::prelude::*;
use rand::Rng;

use crate::ecs::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::Position;
use crate::entity::mob::MobKind;
use crate::entity::mob::ai::MobBrain;
use crate::entity::mob::components::reactive::{Angry, angry_flag};
use crate::entity::mob::events::MobFacts;
use crate::entity::mob::life::triggers;
use crate::entity::mob::state::{AttackTarget, Dead};

/// Clear both the public target and the brain's current goal target.
fn clear_target(world: &mut World, actor: Entity) {
    world.entity_mut(actor).remove::<AttackTarget>();
    if let Some(mut brain) = world.get_mut::<MobBrain>(actor) {
        brain.clear_combat_target();
    }
}

/// Process timers before AI so an expired or disallowed target is never hit
/// during the frame in which the angry state calms down.
pub fn tick_anger(world: &mut World) {
    let now = world.resource::<TickCounter>().current;
    let actors: Vec<Entity> = world
        .query_filtered::<Entity, (With<MobKind>, With<Angry>, Without<Dead>)>()
        .iter(world)
        .collect();
    for actor in actors {
        let Some(state) = world.get::<Angry>(actor).cloned() else {
            continue;
        };
        if state.calmed {
            continue;
        }
        if state.config.filter.as_ref().is_some_and(|filter| {
            world.get::<AttackTarget>(actor).is_some_and(|target| {
                !filter.evaluate(&MobFacts {
                    world,
                    entity: actor,
                    other: Some(target.0),
                })
            })
        }) {
            clear_target(world, actor);
        }
        if now >= state.until_tick {
            world.get_mut::<Angry>(actor).expect("current anger").calmed = true;
            angry_flag(&mut world.entity_mut(actor), false);
            clear_target(world, actor);
            if let Some(event) = &state.config.calm_event {
                triggers::fire(world, actor, event, None);
                crate::entity::definition::apply_pending_definitions(world);
            }
        } else if now >= state.next_sound_tick
            && let Some(sound) = &state.config.sound
        {
            let (min, max) = state.config.sound_interval;
            let delay = if max > min {
                world
                    .get_resource_or_init::<EntityRandom>()
                    .0
                    .gen_range(min..=max)
            } else {
                min
            };
            world
                .get_mut::<Angry>(actor)
                .expect("current anger")
                .next_sound_tick = now.saturating_add(delay.max(1));
            if let Some(position) = world.get::<Position>(actor) {
                crate::entity::mob::net::sound::play_level_sound(
                    world,
                    Some(actor),
                    position.0,
                    sound,
                    -1,
                );
            }
        }
    }
}
