//! The flying mobs' components: `minecraft:navigation.float` (the float
//! control, run by `mob::ai::fly`) and `minecraft:behavior.float_wander`,
//! the goal that picks the points it flies to.

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body;

use super::common::{insert, insert_infallible};
use super::hostile::controls;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::goal_selector::GoalControls;
use crate::entity::mob::constants::TICKS_PER_SECOND;

/// Why a flying component cannot be compiled.
#[derive(Debug, Error)]
pub enum FlightError {
    #[error("behavior.float_wander float_duration {min}..{max} is not an ordered range of seconds")]
    Duration { min: f32, max: f32 },
    /// The published schema requires at least 1 block.
    #[error("behavior.float_wander {field} {value} is below 1")]
    Distance { field: &'static str, value: i32 },
    /// A field whose behaviour no measured flyer shows.
    #[error("behavior.float_wander {0} is not implemented")]
    Unmeasured(&'static str),
}

impl ComponentError for FlightError {}

/// `minecraft:navigation.float`: the mob flies straight at the point its
/// goal wants, with no path search, and floats without gravity (BDS
/// 1.26.51's bat accelerated along the exact line from its feet to its
/// wander point, walls and floors included, and never fell). The other
/// path permissions have nothing to restrict, as no path is searched.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatNavigation {
    /// `can_walk`: the `CANWALK` actor flag (the bat's summon carries it,
    /// from the field's default).
    pub can_walk: bool,
}

/// `minecraft:behavior.float_wander`: fly to random points around the mob.
///
/// Each wander lasts a duration drawn from `float_duration`; on each of its
/// ticks without a point the goal draws one within `±xz_dist`
/// horizontally and `y_offset ± y_dist` vertically of the feet, and keeps
/// none (an idle tick) when the point's block is solid. When the duration
/// runs out a new wander begins at once.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct FloatWanderGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub xz_dist: f64,
    pub y_dist: f64,
    pub y_offset: f64,
    /// Ticks in `float_duration`, the bounds of the uniform draw.
    pub duration_ticks: (f32, f32),
}

impl FloatWanderGoal {
    /// A wander's length in whole ticks for a uniform draw `unit` in
    /// `0..1`: the drawn tick count cut to whole ticks, plus the tick the
    /// wander begins on (BDS 1.26.51's bats wandered 3 to 7 ticks, about
    /// equally often, for `[0.1, 0.35]` seconds).
    pub fn duration(&self, unit: f32) -> u32 {
        let (min, max) = self.duration_ticks;
        (min + unit * (max - min)).max(0.0) as u32 + 1
    }
}

fn compile_float_wander(b: body::BehaviorFloatWander) -> Result<FloatWanderGoal, FlightError> {
    let (min, max) = (b.float_duration.min, b.float_duration.max);
    if !(min >= 0.0 && max >= min) {
        return Err(FlightError::Duration { min, max });
    }
    // The bat's values are the measured ones; anything else changes how
    // points are chosen in ways no capture shows yet.
    // `use_home_position_restriction` only restricts a mob with a
    // `minecraft:home`, which no spawnable mob has (the component has no
    // runtime kind).
    for (unmeasured, field) in [
        (b.must_reach, "must_reach"),
        (b.navigate_around_surface, "navigate_around_surface"),
        (b.additional_collision_buffer, "additional_collision_buffer"),
        (
            b.allow_navigating_through_liquids,
            "allow_navigating_through_liquids",
        ),
        (!b.random_reselect, "random_reselect false"),
    ] {
        if unmeasured {
            return Err(FlightError::Unmeasured(field));
        }
    }
    for (value, field) in [(b.xz_dist, "xz_dist"), (b.y_dist, "y_dist")] {
        if value < 1 {
            return Err(FlightError::Distance { field, value });
        }
    }
    let native = if b.float_wander_has_move_control {
        GoalControls::MOVE
    } else {
        GoalControls::NONE
    };
    Ok(FloatWanderGoal {
        priority: b.priority,
        controls: controls(&b.control_flags, native),
        xz_dist: f64::from(b.xz_dist),
        y_dist: f64::from(b.y_dist),
        y_offset: f64::from(b.y_offset),
        duration_ticks: (min * TICKS_PER_SECOND, max * TICKS_PER_SECOND),
    })
}

/// Register the flying component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert_infallible(builder, |b: body::NavigationFloat| FloatNavigation {
        can_walk: b.0.can_walk,
    })?;
    insert(builder, compile_float_wander)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(body: serde_json::Value) -> Result<FloatWanderGoal, FlightError> {
        compile_float_wander(
            unastar_data::actor_components::decode(&body).expect("float_wander body decodes"),
        )
    }

    #[test]
    fn float_wander_rejects_distances_below_one_and_unmeasured_options() {
        let bat = serde_json::json!({"float_duration": [0.1, 0.35], "random_reselect": true});
        assert!(compile(bat.clone()).is_ok());
        let mut flat = bat.clone();
        flat["y_dist"] = 0.into();
        assert!(matches!(
            compile(flat),
            Err(FlightError::Distance {
                field: "y_dist",
                value: 0
            })
        ));
        let mut reaching = bat;
        reaching["must_reach"] = true.into();
        assert!(matches!(
            compile(reaching),
            Err(FlightError::Unmeasured("must_reach"))
        ));
    }
}
