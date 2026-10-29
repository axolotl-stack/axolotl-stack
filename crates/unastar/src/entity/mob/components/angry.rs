//! `minecraft:angry` and `minecraft:on_target_acquired` (run by
//! `mob::life::angry`): a timed angry state started when the component is
//! added, which shows as the `ANGRY` actor flag while it lasts, and the
//! event a mob fires on itself when it acquires a target.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use jolyne::valentine::actor_flags;
use rand::Rng;
use thiserror::Error;
use unastar_data::actor_components::body::{self, EventTrigger, FilterSubject};
use unastar_rules::filter::Filter;

use super::common::{FilterError, SelfEvent, SelfEventError, filter, insert, self_event};
use crate::ecs::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::definition::{
    ComponentCompileError, ComponentError, ComponentKindsBuilder, DefinitionError,
};
use crate::entity::mob::ai::set_flag;
use crate::entity::mob::constants::TICKS_PER_SECOND;

/// Why an anger body cannot be compiled.
#[derive(Debug, Error)]
pub enum AngryError {
    #[error("angry {0} is not implemented")]
    Unsupported(&'static str),
    #[error("on_target_acquired targets {0:?}; only self is supported")]
    OtherTarget(FilterSubject),
    #[error(transparent)]
    Event(#[from] SelfEventError),
    #[error(transparent)]
    Filter(#[from] FilterError),
}

impl ComponentError for AngryError {}

/// `minecraft:angry`, its seconds converted to ticks.
#[derive(Debug, Clone, PartialEq)]
pub struct AngryConfig {
    /// How long the anger lasts; `None` (a negative `duration`) never calms.
    pub duration_ticks: Option<i32>,
    /// The anger lasts up to this much shorter or longer, uniformly.
    pub delta_ticks: i32,
    pub calm_event: Option<SelfEvent>,
}

/// A mob's running `minecraft:angry`: the definition plus the deadline
/// drawn when the component was added.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Angry {
    pub config: Arc<AngryConfig>,
    /// The server tick the anger ends at; `None` never ends.
    pub until: Option<u64>,
}

impl AngryConfig {
    /// The anger's length, drawn uniformly in
    /// `[duration - delta, duration + delta]` ticks.
    pub fn draw(&self, rng: &mut impl Rng) -> Option<u64> {
        let duration = self.duration_ticks?;
        let delta = if self.delta_ticks > 0 {
            rng.gen_range(-self.delta_ticks..=self.delta_ticks)
        } else {
            0
        };
        Some((duration + delta).max(0) as u64)
    }
}

/// `minecraft:on_target_acquired`: `event` fires on the mob whenever it
/// gains a target (from none) that passes `filter` (`other` = the target).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct OnTargetAcquiredConfig {
    pub event: SelfEvent,
    pub filter: Filter,
}

/// Register the anger component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::Angry| compile_angry(b).map_err(ComponentCompileError::from),
        start_anger,
        |entity| {
            entity.remove::<Angry>();
            set_flag(entity, actor_flags::ANGRY, false);
        },
    )?;
    insert(builder, |b: body::OnTargetAcquired| {
        if b.target != FilterSubject::SelfValue {
            return Err(AngryError::OtherTarget(b.target));
        }
        Ok(OnTargetAcquiredConfig {
            event: self_event(&EventTrigger::Event(b.event))?,
            filter: match &b.filters {
                Some(json) => filter(json)?,
                None => Filter::always(),
            },
        })
    })
}

fn compile_angry(b: body::Angry) -> Result<Arc<AngryConfig>, AngryError> {
    if b.broadcast_anger || b.broadcast_anger_on_attack || b.broadcast_anger_on_being_attacked {
        return Err(AngryError::Unsupported("anger broadcasting"));
    }
    if b.filters.is_some() {
        return Err(AngryError::Unsupported("filters"));
    }
    if b.angry_sound.is_some() {
        return Err(AngryError::Unsupported("angry_sound"));
    }
    let ticks = |seconds: i32| seconds.saturating_mul(TICKS_PER_SECOND as i32);
    Ok(Arc::new(AngryConfig {
        duration_ticks: (b.duration >= 0).then(|| ticks(b.duration)),
        delta_ticks: ticks(b.duration_delta.max(0)),
        calm_event: b
            .calm_event
            .map(|trigger| self_event(&EventTrigger::Trigger(trigger)))
            .transpose()?,
    }))
}

/// The anger starts when the component is added.
fn start_anger(config: &Arc<AngryConfig>, entity: &mut EntityWorldMut<'_>) {
    let until = entity.world_scope(|world| {
        let now = world.resource::<TickCounter>().current;
        config
            .draw(&mut world.get_resource_or_init::<EntityRandom>().0)
            .map(|ticks| now + ticks)
    });
    entity.insert(Angry {
        config: Arc::clone(config),
        until,
    });
    set_flag(entity, actor_flags::ANGRY, true);
}
