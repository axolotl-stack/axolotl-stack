//! `minecraft:conditional_bandwidth_optimization`: how often a viewer far
//! from the actor gets its position updates (applied by
//! `mob::net::network::sync_mob_viewers`).
//!
//! A viewer `d` blocks from the actor (eye to the actor's position) may go
//! `⌊min(d, max_optimized_distance) / max_optimized_distance ×
//! max_dropped_ticks⌋` ticks without a `MoveActorDelta`, measured on BDS
//! 1.26.51 for the empty component (defaults 10 / 80), 7 / 100 (snowball),
//! 7 / 80 (egg, arrow), 10 / 80 (xp orb, enderman), beyond the 80-block
//! maximum and with no component at all (every tick; see
//! `docs/quirks/bds.md`, "Entities / movement replication"). No
//! `MotionPredictionHints` packet was seen from any actor with
//! `use_motion_prediction_hints`, so the field is kept but sends nothing.
//!
//! `conditional_values` replace the defaults while their filter holds; the
//! first entry that holds wins. The vanilla entries (minecarts and boats:
//! 0 / 0 while `is_moving`) test the actor itself, answered by whether it
//! moved in the previous tick: on BDS a minecart's and a boat's first update
//! after resting uses the defaults, and every later one while it falls is
//! sent every tick with `ticks` 0. A filter testing anything else is
//! rejected, as the network sync cannot answer it.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body;
use unastar_rules::filter::{
    BuiltinTest, Fact, Filter, FilterContext, FilterRegistry, FilterTest, Subject,
};

use super::common::insert;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Why a `minecraft:conditional_bandwidth_optimization` body cannot be
/// compiled.
#[derive(Debug, Error)]
pub enum BandwidthError {
    #[error("max_dropped_ticks {0} is negative")]
    NegativeDroppedTicks(i32),
    #[error("a conditional bandwidth entry has no filter")]
    MissingFilter,
    #[error("conditional bandwidth filter: {0}")]
    Filter(#[from] unastar_rules::ParseError),
    #[error("conditional bandwidth filter diagnostics {0:?}")]
    Diagnostics(Vec<unastar_rules::Diagnostic>),
    #[error(
        "conditional bandwidth filter test {test} on {subject} is not answerable (only is_moving \
         on the actor itself is)"
    )]
    Unanswerable { test: String, subject: &'static str },
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

    fn new(
        max_dropped_ticks: i32,
        max_optimized_distance: f32,
        use_motion_prediction_hints: bool,
    ) -> Result<Self, BandwidthError> {
        Ok(Self {
            max_dropped_ticks: u32::try_from(max_dropped_ticks)
                .map_err(|_| BandwidthError::NegativeDroppedTicks(max_dropped_ticks))?,
            max_optimized_distance,
            use_motion_prediction_hints,
        })
    }
}

impl TryFrom<body::BandwidthValues> for BandwidthValues {
    type Error = BandwidthError;

    fn try_from(values: body::BandwidthValues) -> Result<Self, BandwidthError> {
        Self::new(
            values.max_dropped_ticks,
            values.max_optimized_distance,
            values.use_motion_prediction_hints,
        )
    }
}

/// What the network sync knows about an actor when it picks its values.
struct NetFacts {
    /// The actor moved in the previous tick.
    moving: bool,
}

impl FilterContext for NetFacts {
    fn fact(&self, subject: Subject, test: &FilterTest) -> Option<Fact> {
        match (test.builtin()?, subject) {
            (BuiltinTest::IsMoving, Subject::Self_) => Some(Fact::Bool(self.moving)),
            _ => None,
        }
    }
}

/// `minecraft:conditional_bandwidth_optimization`: the values every viewer's
/// position updates follow. An actor without it sends every update to every
/// viewer.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BandwidthOptimization {
    /// `default_values`, or the published defaults (10 ticks over 80 blocks,
    /// no hints) when absent.
    pub default: BandwidthValues,
    /// `conditional_values` in authored order: each filter selects its
    /// values instead of [`Self::default`] while it holds.
    pub conditional: Arc<[(Filter, BandwidthValues)]>,
}

impl BandwidthOptimization {
    /// Unconditional values.
    pub fn constant(values: BandwidthValues) -> Self {
        Self {
            default: values,
            conditional: Arc::from([]),
        }
    }

    pub fn compile(body: body::ConditionalBandwidthOptimization) -> Result<Self, BandwidthError> {
        let default = body.default_values.unwrap_or(body::BandwidthValues {
            max_dropped_ticks:
                unastar_data::actor_components::provisional::BANDWIDTH_MAX_DROPPED_TICKS,
            max_optimized_distance:
                unastar_data::actor_components::provisional::BANDWIDTH_MAX_OPTIMIZED_DISTANCE,
            use_motion_prediction_hints:
                unastar_data::actor_components::provisional::BANDWIDTH_USE_MOTION_PREDICTION_HINTS,
        });
        let conditional = body
            .conditional_values
            .unwrap_or_default()
            .into_iter()
            .map(|entry| {
                let json = entry
                    .conditional_values
                    .as_ref()
                    .ok_or(BandwidthError::MissingFilter)?;
                Ok((
                    answerable_filter(json)?,
                    BandwidthValues::new(
                        entry.max_dropped_ticks,
                        entry.max_optimized_distance,
                        entry.use_motion_prediction_hints,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, BandwidthError>>()?;
        Ok(Self {
            default: default.try_into()?,
            conditional: conditional.into(),
        })
    }

    /// The values in force for an actor that did (`moving`) or did not move
    /// in the previous tick.
    pub fn values(&self, moving: bool) -> BandwidthValues {
        let facts = NetFacts { moving };
        self.conditional
            .iter()
            .find(|(filter, _)| filter.evaluate(&facts))
            .map_or(self.default, |(_, values)| *values)
    }
}

/// Parse a conditional entry's filter; every test must be one
/// [`NetFacts`] answers.
fn answerable_filter(json: &serde_json::Value) -> Result<Filter, BandwidthError> {
    let mut diagnostics = unastar_rules::Diagnostics::default();
    let filter = Filter::parse(json, FilterRegistry::vanilla(), "filters", &mut diagnostics)?;
    if !diagnostics.items.is_empty() {
        return Err(BandwidthError::Diagnostics(diagnostics.items));
    }
    let mut failure = None;
    filter.for_each_test(&mut |test| {
        let answerable =
            test.builtin() == Some(BuiltinTest::IsMoving) && test.subject == Subject::Self_;
        if !answerable && failure.is_none() {
            failure = Some(BandwidthError::Unanswerable {
                test: test.name.clone(),
                subject: test.subject.name(),
            });
        }
    });
    failure.map_or(Ok(filter), Err)
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

    fn compiled(identifier: &str) -> BandwidthOptimization {
        let definition = unastar_data::actor_definitions::get(identifier).unwrap();
        BandwidthOptimization::compile(
            serde_json::from_value(
                definition.components["minecraft:conditional_bandwidth_optimization"].clone(),
            )
            .unwrap(),
        )
        .unwrap()
    }

    /// Every vanilla vehicle's `is_moving` entry compiles: 0 / 0 while it
    /// moves, its defaults (20 ticks over 60 blocks) at rest.
    #[test]
    fn vehicles_send_every_tick_only_while_moving() {
        for identifier in [
            "minecraft:minecart",
            "minecraft:boat",
            "minecraft:chest_boat",
            "minecraft:tnt_minecart",
            "minecraft:hopper_minecart",
            "minecraft:chest_minecart",
            "minecraft:command_block_minecart",
        ] {
            let bandwidth = compiled(identifier);
            let resting = bandwidth.values(false);
            assert_eq!(
                (resting.max_dropped_ticks, resting.max_optimized_distance),
                (20, 60.0),
                "{identifier}"
            );
            let moving = bandwidth.values(true);
            assert_eq!(moving.dropped_ticks(50.0), 0, "{identifier}");
            assert_eq!(resting.dropped_ticks(32.29), 10, "{identifier}");
        }
    }

    #[test]
    fn filters_the_sync_cannot_answer_are_rejected() {
        let body: body::ConditionalBandwidthOptimization =
            serde_json::from_value(serde_json::json!({
                "conditional_values": [{
                    "conditional_values": [{"test": "is_baby"}],
                    "max_dropped_ticks": 0
                }]
            }))
            .unwrap();
        assert!(matches!(
            BandwidthOptimization::compile(body),
            Err(BandwidthError::Unanswerable { .. })
        ));
    }
}
