//! `minecraft:ambient_sound_interval` (the event-name system is
//! `mob::net::ambient`).
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
use thiserror::Error;
use unastar_data::actor_components::body;
use unastar_rules::molang::{Expr, QueryContext, QueryRegistry, parse};

use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Why a `minecraft:ambient_sound_interval` body cannot be compiled.
#[derive(Debug, Error)]
pub enum AmbientSoundError {
    #[error("molang condition {source_text:?}: {message}")]
    Condition {
        source_text: String,
        message: String,
    },
}

impl ComponentError for AmbientSoundError {}

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

pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::AmbientSoundInterval| Ok(compile(b)?),
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
