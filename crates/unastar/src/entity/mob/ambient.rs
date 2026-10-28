//! `minecraft:ambient_sound_interval`.
//!
//! BDS never sends ambient sounds: the client times and plays them from the
//! actor data the component writes, the minimum delay
//! (`AMBIENT_SOUND_INTERVAL`, seconds), the random extra delay
//! (`AMBIENT_SOUND_INTERVAL_RANGE`, seconds) and the sound event name
//! (`AMBIENT_SOUND_EVENT_NAME`). The server's only ongoing work is choosing
//! the event name: every tick the first `event_names` entry whose Molang
//! condition holds wins (the chicken's `ambient.baby` while it is a baby),
//! else `event_name`.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::types::DataItemEntryPayload;
use rand::SeedableRng;
use rand::rngs::StdRng;
use thiserror::Error;
use unastar_data::actor_components::body;
use unastar_rules::molang::eval::evaluate_num;
use unastar_rules::molang::{Expr, QueryContext, QueryRegistry, parse};

use super::death::{RewardMolang, last_hit_by_player};
use super::state::Dead;
use crate::entity::definition::ComponentKindsBuilder;
use crate::entity::metadata::EntityMetadata;

/// Why a `minecraft:ambient_sound_interval` body cannot be compiled.
#[derive(Debug, Error)]
pub enum AmbientSoundError {
    #[error("molang condition {source_text:?}: {message}")]
    Condition {
        source_text: String,
        message: String,
    },
}

/// `minecraft:ambient_sound_interval`.
#[derive(Component, Debug, Clone)]
pub struct AmbientSoundConfig {
    /// Minimum seconds between ambient sounds.
    pub interval: f32,
    /// Maximum seconds randomly added to the interval.
    pub range: f32,
    pub event_name: Arc<str>,
    /// Conditional event names, first holding condition wins.
    pub event_names: Arc<[(Arc<Expr>, Arc<str>)]>,
}

pub fn register_component_kinds(builder: &mut ComponentKindsBuilder) -> Result<(), String> {
    builder.register(
        |b: body::AmbientSoundInterval| compile(b).map_err(|e| e.to_string()),
        |config: &AmbientSoundConfig, entity| {
            if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
                metadata.set_float(ids::AMBIENT_SOUND_INTERVAL, config.interval);
                metadata.set_float(ids::AMBIENT_SOUND_INTERVAL_RANGE, config.range);
                metadata.set_string(ids::AMBIENT_SOUND_EVENT_NAME, &*config.event_name);
            }
            entity.insert(config.clone());
        },
        |entity| {
            entity.remove::<AmbientSoundConfig>();
        },
    )
}

fn compile(b: body::AmbientSoundInterval) -> Result<AmbientSoundConfig, AmbientSoundError> {
    let event_names = b
        .event_names
        .unwrap_or_default()
        .into_iter()
        .map(|entry| {
            let fail = |message: String| AmbientSoundError::Condition {
                source_text: entry.condition.clone(),
                message,
            };
            let mut expr = parse(&entry.condition).map_err(|e| fail(e.to_string()))?;
            expr.bind(QueryRegistry::vanilla(), QueryContext::Runtime)
                .map_err(|e| fail(e.to_string()))?;
            Ok((Arc::new(expr), Arc::from(entry.event_name.as_str())))
        })
        .collect::<Result<Vec<_>, AmbientSoundError>>()?;
    Ok(AmbientSoundConfig {
        interval: b.value,
        range: b.range,
        event_name: Arc::from(b.event_name.as_str()),
        event_names: event_names.into(),
    })
}

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
