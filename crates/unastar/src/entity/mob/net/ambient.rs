//! Ambient sound event names (`minecraft:ambient_sound_interval`, see
//! `mob::components::ambient`).
//!
//! BDS never sends ambient sounds: the client times and plays them from the
//! actor data the component writes. The server's only ongoing work is
//! choosing the event name: every tick the first `event_names` entry whose
//! Molang condition holds wins (the chicken's `ambient.baby` while it is a
//! baby), else `event_name`. A class with an angry ambient sound
//! (`NativeTraits::angry_ambient_sound`, the enderman's `mad`) uses it
//! instead while its `ANGRY` flag is set, and else its usual name (the
//! component's, or [`DEFAULT_AMBIENT_SOUND`] without one).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::DataItemEntryPayload;
use rand::SeedableRng;
use rand::rngs::StdRng;
use unastar_rules::molang::eval::evaluate_num;

use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::MobKind;
use crate::entity::mob::combat::death::{RewardMolang, last_hit_by_player};
use crate::entity::mob::components::AmbientSoundConfig;
use crate::entity::mob::constants::DEFAULT_AMBIENT_SOUND;
use crate::entity::mob::state::Dead;

/// A mob's ambient sound event name: borrowed from its definition, or a
/// native name.
enum Name<'a> {
    Authored(&'a Arc<str>),
    Native(&'static str),
}

impl Name<'_> {
    fn as_str(&self) -> &str {
        match self {
            Self::Authored(name) => name,
            Self::Native(name) => name,
        }
    }
}

/// System (exclusive, `EntityLogicSet`): keep each mob's ambient sound event
/// name on the first conditional name whose condition holds, or on its
/// class's angry name while it is angry.
#[allow(clippy::type_complexity)]
pub fn update_ambient_sound_names(
    world: &mut World,
    mobs: &mut QueryState<
        (Entity, &MobKind, Option<&AmbientSoundConfig>, &ActorFlags),
        Without<Dead>,
    >,
    mut changes: Local<Vec<(Entity, Arc<str>)>>,
) {
    let seed = {
        use rand::Rng;
        world
            .get_resource_or_init::<crate::entity::EntityRandom>()
            .0
            .r#gen::<u64>()
    };
    for (mob, kind, config, flags) in mobs.iter(world) {
        let angry_name = kind.0.native.angry_ambient_sound;
        let has_names = config.is_some_and(|config| !config.event_names.is_empty());
        if angry_name.is_none() && !has_names {
            continue;
        }
        let angry = angry_name
            .filter(|_| flags.get(actor_flags::ANGRY).unwrap_or(false))
            .map(Name::Native);
        let name = angry.unwrap_or_else(|| match config {
            Some(config) => {
                let mut ctx = RewardMolang {
                    world,
                    mob,
                    last_hit_by_player: last_hit_by_player(world, mob),
                    rng: StdRng::seed_from_u64(seed ^ mob.to_bits()),
                };
                Name::Authored(
                    config
                        .event_names
                        .iter()
                        .find(|(condition, _)| {
                            evaluate_num(condition, &mut ctx).is_ok_and(|value| value != 0.0)
                        })
                        .map_or(&config.event_name, |(_, name)| name),
                )
            }
            None => Name::Native(DEFAULT_AMBIENT_SOUND),
        });
        let current = world.get::<EntityMetadata>(mob).and_then(|metadata| {
            match metadata.get(ids::AMBIENT_SOUND_EVENT_NAME) {
                Some(DataItemEntryPayload::DataItemStringPayload(value)) => {
                    Some(*value.value == *name.as_str())
                }
                _ => None,
            }
        });
        if current != Some(true) {
            changes.push((mob, Arc::from(name.as_str())));
        }
    }
    for (mob, name) in changes.drain(..) {
        if let Some(mut metadata) = world.get_mut::<EntityMetadata>(mob) {
            metadata.set_string(ids::AMBIENT_SOUND_EVENT_NAME, &*name);
        }
    }
}
