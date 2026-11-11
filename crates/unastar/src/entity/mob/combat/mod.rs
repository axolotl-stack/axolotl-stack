//! Mob combat: hits and the hurt rule ([`hit`]), knockback resistance
//! ([`knockback`]), damage sensors ([`damage_sensor`]), fall damage
//! ([`fall`]), fire ([`fire`]), mob melee hits ([`melee`]) and death
//! ([`death`]).
//!
//! Every damage source goes through [`hurt_mob`]; the component
//! configuration these systems read lives in `mob::components`.

pub mod damage_sensor;
pub mod death;
pub mod fall;
pub mod fire;
pub mod hit;
pub mod knockback;
pub mod melee;

#[cfg(test)]
mod bds_knockback;

pub use hit::{
    Hit, HurtTimer, hurt_mob, hurt_on_condition, knockback_power_of, knockback_velocity,
    player_attack, send_actor_event, weapon_damage,
};
