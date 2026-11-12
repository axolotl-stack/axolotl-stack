//! `minecraft:conditional_bandwidth_optimization`: how often a viewer far
//! from the mob gets its position updates (applied by
//! `mob::net::network::sync_mob_viewers`).
//!
//! A viewer `d` blocks from the mob (eye to the mob's position) may go
//! `⌊min(d, max_optimized_distance) / max_optimized_distance ×
//! max_dropped_ticks⌋` ticks without a `MoveActorDelta`, measured on BDS
//! 1.26.51 for the empty component (defaults 10 / 80), 7 / 100 (snowball),
//! 7 / 80 (egg, arrow), 10 / 80 (xp orb, enderman), beyond the 80-block
//! maximum and with no component at all (every tick; see
//! `docs/quirks/bds.md`, "Entities / movement replication"). No
//! `MotionPredictionHints` packet was seen from any actor with
//! `use_motion_prediction_hints`, so the field is kept but sends nothing.
//! `conditional_values` select other values by filters; the vanilla ones (the
//! minecarts' and boats' `is_moving`, which BDS answers with 0 / 0 while the
//! cart moves) ask a test the mob runtime cannot answer, so they are rejected.

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body;

use super::common::insert;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Why a `minecraft:conditional_bandwidth_optimization` body cannot be
/// compiled.
#[derive(Debug, Error)]
pub enum BandwidthError {
    #[error(
        "conditional bandwidth values are not implemented (their filters, such as is_moving, \
         are not answerable)"
    )]
    ConditionalValues,
    #[error("max_dropped_ticks {0} is negative")]
    NegativeDroppedTicks(i32),
}

impl ComponentError for BandwidthError {}

/// One set of bandwidth values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandwidthValues {
    /// Most ticks a viewer may go without a position update.
    pub max_dropped_ticks: u32,
    /// Distance (blocks) at which [`Self::max_dropped_ticks`] is reached.
    pub max_optimized_distance: f32,
    /// Authored, but BDS 1.26.51 sends no `MotionPredictionHints` for it.
    pub use_motion_prediction_hints: bool,
}

impl BandwidthValues {
    /// Ticks a viewer `distance` blocks away may go without a position
    /// update: the distance's share of the maximum distance (capped at 1)
    /// times the maximum dropped ticks, rounded down.
    pub fn dropped_ticks(&self, distance: f64) -> u64 {
        let max_distance = f64::from(self.max_optimized_distance);
        if self.max_dropped_ticks == 0 || max_distance <= 0.0 {
            return 0;
        }
        let share = distance.min(max_distance) / max_distance;
        (share * f64::from(self.max_dropped_ticks)).floor() as u64
    }
}

impl TryFrom<body::BandwidthValues> for BandwidthValues {
    type Error = BandwidthError;

    fn try_from(values: body::BandwidthValues) -> Result<Self, BandwidthError> {
        Ok(Self {
            max_dropped_ticks: u32::try_from(values.max_dropped_ticks)
                .map_err(|_| BandwidthError::NegativeDroppedTicks(values.max_dropped_ticks))?,
            max_optimized_distance: values.max_optimized_distance,
            use_motion_prediction_hints: values.use_motion_prediction_hints,
        })
    }
}

/// `minecraft:conditional_bandwidth_optimization`: the values every viewer's
/// position updates follow. A mob without it sends every update to every
/// viewer.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BandwidthOptimization(pub BandwidthValues);

impl BandwidthOptimization {
    /// The component's values: `default_values`, or the published defaults
    /// (10 ticks over 80 blocks, no hints) when it is empty.
    pub fn compile(body: body::ConditionalBandwidthOptimization) -> Result<Self, BandwidthError> {
        if body
            .conditional_values
            .is_some_and(|values| !values.is_empty())
        {
            return Err(BandwidthError::ConditionalValues);
        }
        let values = body.default_values.unwrap_or(body::BandwidthValues {
            max_dropped_ticks:
                unastar_data::actor_components::provisional::BANDWIDTH_MAX_DROPPED_TICKS,
            max_optimized_distance:
                unastar_data::actor_components::provisional::BANDWIDTH_MAX_OPTIMIZED_DISTANCE,
            use_motion_prediction_hints:
                unastar_data::actor_components::provisional::BANDWIDTH_USE_MOTION_PREDICTION_HINTS,
        });
        Ok(Self(values.try_into()?))
    }
}

/// Register the `minecraft:conditional_bandwidth_optimization` kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, BandwidthOptimization::compile)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(max_dropped_ticks: u32, max_optimized_distance: f32) -> BandwidthValues {
        BandwidthValues {
            max_dropped_ticks,
            max_optimized_distance,
            use_motion_prediction_hints: false,
        }
    }

    #[test]
    fn dropped_ticks_grow_with_distance_and_stop_at_the_maximum() {
        let defaults = values(10, 80.0);
        assert_eq!(defaults.dropped_ticks(0.0), 0);
        assert_eq!(defaults.dropped_ticks(7.98), 0);
        assert_eq!(defaults.dropped_ticks(8.56), 1);
        assert_eq!(defaults.dropped_ticks(15.56), 1);
        assert_eq!(defaults.dropped_ticks(16.19), 2);
        assert_eq!(defaults.dropped_ticks(79.9), 9);
        assert_eq!(defaults.dropped_ticks(102.78), 10);
        assert_eq!(defaults.dropped_ticks(1.0e6), 10);
        let snowball = values(7, 100.0);
        assert_eq!(snowball.dropped_ticks(57.01), 3);
        assert_eq!(snowball.dropped_ticks(57.37), 4);
        assert_eq!(values(0, 80.0).dropped_ticks(50.0), 0);
        assert_eq!(values(20, 0.0).dropped_ticks(50.0), 0);
    }
}
