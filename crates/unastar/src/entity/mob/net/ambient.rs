//! Ambient sound event names (`minecraft:ambient_sound_interval`, see
//! `mob::components::ambient`).
//!
//! BDS never sends ambient sounds: the client times and plays them from the
//! actor data the component writes. The server's only ongoing work is
//! choosing the event name: every tick the first `event_names` entry whose
//! Molang condition holds wins (the chicken's `ambient.baby` while it is a
//! baby), else `event_name`.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::types::DataItemEntryPayload;
use rand::SeedableRng;
use rand::rngs::StdRng;
use unastar_rules::molang::eval::evaluate_num;

use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::combat::death::{RewardMolang, last_hit_by_player};
use crate::entity::mob::components::AmbientSoundConfig;
use crate::entity::mob::state::Dead;

/// System (exclusive, `EntityLogicSet`): keep each mob's ambient sound event
/// name on the first conditional name whose condition holds.
pub fn update_ambient_sound_names(
    world: &mut World,
    mobs: &mut QueryState<(Entity, &AmbientSoundConfig), Without<Dead>>,
    mut changes: Local<Vec<(Entity, Arc<str>)>>,
) {
    let seed = {
        use rand::Rng;
        world
            .get_resource_or_init::<crate::entity::EntityRandom>()
            .0
            .r#gen::<u64>()
    };
    for (mob, config) in mobs.iter(world) {
        if config.event_names.is_empty() {
            continue;
        }
        let mut ctx = RewardMolang {
            world,
            mob,
            last_hit_by_player: last_hit_by_player(world, mob),
            rng: StdRng::seed_from_u64(seed ^ mob.to_bits()),
        };
        let name = config
            .event_names
            .iter()
            .find(|(condition, _)| {
                evaluate_num(condition, &mut ctx).is_ok_and(|value| value != 0.0)
            })
            .map_or(&config.event_name, |(_, name)| name);
        let current = world.get::<EntityMetadata>(mob).and_then(|metadata| {
            match metadata.get(ids::AMBIENT_SOUND_EVENT_NAME) {
                Some(DataItemEntryPayload::DataItemStringPayload(value)) => {
                    Some(*value.value == **name)
                }
                _ => None,
            }
        });
        if current != Some(true) {
            changes.push((mob, Arc::clone(name)));
        }
    }
    for (mob, name) in changes.drain(..) {
        if let Some(mut metadata) = world.get_mut::<EntityMetadata>(mob) {
            metadata.set_string(ids::AMBIENT_SOUND_EVENT_NAME, &*name);
        }
    }
}
