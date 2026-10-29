//! Baby growth (`minecraft:ageable`).
//!
//! The `minecraft:ageable` component kind inserts a [`BabyAge`] next to
//! [`AgeableConfig`]: the baby grows up `duration × 20` ticks later, when
//! [`tick_baby_growth`] fires the authored `grow_up` event (the cow's
//! `minecraft:ageable_grow_up` swaps the baby group for the adult one).
//! Feeding a `feed_items` entry brings the deadline forward by
//! `growth × duration`; `pause_growth_items` stop growth and
//! `reset_growth_items` let it continue (see `mob::interaction::interact`). A negative
//! `duration` never grows.

use bevy_ecs::prelude::*;
use tracing::warn;

use crate::ecs::resources::TickCounter;
use crate::entity::definition::apply_pending_definitions;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::AgeableConfig;
use crate::entity::mob::constants::TICKS_PER_SECOND;
use crate::entity::mob::state::Dead;

/// Growth state of a baby, as a deadline on `TickCounter::current`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub enum BabyAge {
    /// Grows up once the server tick reaches `grow_tick`.
    Growing { grow_tick: u64 },
    /// Stopped by a pause item with `remaining` ticks still to grow.
    Paused { remaining: u64 },
    /// The authored duration is negative: never grows up.
    Never,
}

impl BabyAge {
    /// A freshly composed baby at server tick `now`.
    pub fn new(config: &AgeableConfig, now: u64) -> Self {
        if config.duration < 0.0 {
            Self::Never
        } else {
            Self::Growing {
                grow_tick: now + duration_ticks(config),
            }
        }
    }

    /// Ticks left until adulthood (`None` when it never grows). BDS saves
    /// the negation of this as the actor's `Age`.
    pub fn remaining(&self, now: u64) -> Option<u64> {
        match *self {
            Self::Growing { grow_tick } => Some(grow_tick.saturating_sub(now)),
            Self::Paused { remaining } => Some(remaining),
            Self::Never => None,
        }
    }

    /// Bring adulthood forward by `growth` (a fraction of the full
    /// duration). Returns false when the baby never grows.
    pub fn feed(&mut self, config: &AgeableConfig, growth: f32) -> bool {
        let ticks = (growth.max(0.0) * duration_ticks(config) as f32) as u64;
        match self {
            Self::Growing { grow_tick } => *grow_tick = grow_tick.saturating_sub(ticks),
            Self::Paused { remaining } => *remaining = remaining.saturating_sub(ticks),
            Self::Never => return false,
        }
        true
    }

    /// Stop growing (pause item). Returns false unless it was growing.
    pub fn pause(&mut self, now: u64) -> bool {
        let Self::Growing { grow_tick } = *self else {
            return false;
        };
        *self = Self::Paused {
            remaining: grow_tick.saturating_sub(now),
        };
        true
    }

    /// Continue growing (reset item). Returns false unless it was paused.
    pub fn resume(&mut self, now: u64) -> bool {
        let Self::Paused { remaining } = *self else {
            return false;
        };
        *self = Self::Growing {
            grow_tick: now + remaining,
        };
        true
    }
}

/// `duration` seconds in ticks.
fn duration_ticks(config: &AgeableConfig) -> u64 {
    (config.duration.max(0.0) * TICKS_PER_SECOND) as u64
}

/// System (exclusive, `EntityLogicSet`): fire `grow_up` on babies whose
/// growth deadline has passed.
pub fn tick_baby_growth(world: &mut World) {
    let now = world.resource::<TickCounter>().current;
    let grown: Vec<(Entity, Option<crate::entity::mob::components::SelfEvent>)> = world
        .query_filtered::<(Entity, &BabyAge, &AgeableConfig), Without<Dead>>()
        .iter(world)
        .filter(|(_, age, _)| matches!(age, BabyAge::Growing { grow_tick } if *grow_tick <= now))
        .map(|(entity, _, config)| (entity, config.grow_up.clone()))
        .collect();
    if grown.is_empty() {
        return;
    }
    for (entity, grow_up) in grown {
        // Grown: nothing left to count even if the event keeps the component.
        world.entity_mut(entity).insert(BabyAge::Never);
        let Some(grow_up) = grow_up else {
            continue;
        };
        let handle = world
            .get::<MobKind>(entity)
            .and_then(|kind| kind.0.events.event(&grow_up.0));
        let Some(handle) = handle else {
            warn!(event = %grow_up.0, "grow_up names an event the mob does not define");
            continue;
        };
        if let Err(error) = crate::entity::mob::events::fire_event(world, entity, &handle, None) {
            warn!(event = %grow_up.0, %error, "grow_up event failed");
        }
    }
    apply_pending_definitions(world);
}
