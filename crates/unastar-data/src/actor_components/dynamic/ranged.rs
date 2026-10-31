//! Closed bodies for authored projectile selection and target range events.

use serde::Deserialize;

use super::{EventTrigger, FilterJson};

/// A conditional projectile choice, evaluated before the default projectile.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShooterProjectile {
    pub def: String,
    pub filters: Option<FilterJson>,
}

/// `minecraft:shooter`: projectile selection and launch customization.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shooter {
    #[serde(default)]
    pub def: String,
    #[serde(default = "no_aux")]
    pub aux_val: i32,
    #[serde(default)]
    pub power: f32,
    #[serde(default)]
    pub magic: bool,
    #[serde(default)]
    pub sound: String,
    #[serde(default)]
    pub projectiles: Vec<ShooterProjectile>,
}

/// Plain projectiles have no potion effect override.
fn no_aux() -> i32 {
    -1
}

/// `minecraft:target_nearby_sensor`: hysteresis around the current target.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetNearbySensor {
    #[serde(default = "inside")]
    pub inside_range: f32,
    #[serde(default = "outside")]
    pub outside_range: f32,
    #[serde(default)]
    pub must_see: bool,
    pub on_inside_range: Option<EventTrigger>,
    pub on_outside_range: Option<EventTrigger>,
    pub on_vision_lost_inside_range: Option<EventTrigger>,
}

/// Default inner boundary, in blocks.
fn inside() -> f32 {
    1.0
}

/// Default outer boundary, in blocks.
fn outside() -> f32 {
    5.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_components::{ActorComponent, decode, decode_component};

    #[test]
    fn shooter_defaults_and_conditional_choices_decode_without_open_fields() {
        let body = serde_json::json!({"def":"minecraft:small_fireball"});
        let shooter = decode::<Shooter>(&body).unwrap();
        assert_eq!(shooter.aux_val, -1);
        assert_eq!(shooter.power, 0.0);
        assert!(!shooter.magic);
        assert!(decode_component(ActorComponent::Shooter, &body).unwrap());
        assert!(
            decode::<Shooter>(
                &serde_json::json!({"def":"minecraft:small_fireball","unimplemented":true})
            )
            .is_err()
        );
        let conditional=decode::<Shooter>(&serde_json::json!({"projectiles":[{"def":"minecraft:small_fireball","filters":{"test":"is_family","subject":"other","value":"player"}}]})).unwrap();
        assert_eq!(conditional.projectiles.len(), 1);
    }

    #[test]
    fn target_sensor_preserves_string_and_object_event_references() {
        let sensor=decode::<TargetNearbySensor>(&serde_json::json!({"on_inside_range":"inside","on_outside_range":{"event":"outside","target":"self"}})).unwrap();
        assert_eq!(sensor.inside_range, 1.0);
        assert_eq!(sensor.outside_range, 5.0);
        assert!(matches!(
            sensor.on_inside_range,
            Some(EventTrigger::Event(_))
        ));
        assert!(matches!(
            sensor.on_outside_range,
            Some(EventTrigger::Trigger(_))
        ));
        assert!(decode::<TargetNearbySensor>(&serde_json::json!({"unknown":1})).is_err());
    }
}
