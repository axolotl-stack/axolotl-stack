//! Mob combat: hits and the hurt rule ([`hit`]), knockback resistance
//! ([`knockback`]), damage sensors ([`damage_sensor`]), fall damage
//! ([`fall`]), fire ([`fire`]), mob melee hits ([`melee`]), contact damage
//! ([`area_attack`]) and death ([`death`]).
//!
//! Every damage source goes through [`hurt_mob`]; the component
//! configuration these systems read lives in `mob::components`.

pub mod area_attack;
pub mod damage_sensor;
pub mod death;
pub mod fall;
pub mod fire;
pub mod hit;
pub mod melee;
pub mod ranged;

#[cfg(test)]
mod bds_knockback;
#[cfg(test)]
mod bds_ranged;

pub use hit::{
    DamageOverTimeClock, Hit, HurtTimer, damage_over_time, fire_immune, hurt_mob, hurt_mob_quietly,
    hurt_on_condition, knockback_power_of, knockback_velocity, player_attack, send_actor_event,
};
