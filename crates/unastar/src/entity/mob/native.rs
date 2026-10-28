//! Behaviour of native actor classes that behaviour-pack definitions do not
//! describe, keyed by [`ActorType`] and resolved once per archetype.

use unastar_data::actor_identifiers::ActorType;

use super::constants::{CHICKEN_FALL_DAMAGE_MULTIPLIER, WING_FLAP_VERTICAL_DRAG};

/// The native traits of one actor class.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeTraits {
    /// Wing-flap vertical drag of the living physics step (the chicken
    /// glides down instead of falling); see `PhysicsBody::wing_flap_drag`.
    /// The flap animation is client-side: no actor data carries it.
    pub wing_flap_drag: Option<f64>,
    /// The `FALL_DAMAGE_MULTIPLIER` actor data the class starts with (the
    /// fall damage formula's multiplier).
    pub fall_damage_multiplier: f32,
}

impl NativeTraits {
    pub const fn of(actor_type: ActorType) -> Self {
        match actor_type {
            ActorType::Chicken => Self {
                wing_flap_drag: Some(WING_FLAP_VERTICAL_DRAG),
                fall_damage_multiplier: CHICKEN_FALL_DAMAGE_MULTIPLIER,
            },
            _ => Self {
                wing_flap_drag: None,
                fall_damage_multiplier: 1.0,
            },
        }
    }
}
