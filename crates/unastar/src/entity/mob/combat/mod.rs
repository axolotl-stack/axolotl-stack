//! Mob combat: hits and the hurt rule ([`hit`]), damage sensors
//! ([`damage_sensor`]), fall damage ([`fall`]), fire ([`fire`]), mob melee
//! hits ([`melee`]), contact damage ([`area_attack`]), ranged shots
//! ([`shoot`]), death ([`death`]) and fuses ([`explode`]).
//!
//! Every damage source goes through [`hurt_mob`]; the component
//! configuration these systems read lives in `mob::components`.

pub mod area_attack;
pub mod damage_sensor;
pub mod death;
pub mod explode;
pub mod fall;
pub mod fire;
pub mod hit;
pub mod melee;
pub mod shoot;

pub use hit::{
    Hit, HurtTimer, hurt_mob, hurt_on_condition, knockback_power_of, knockback_velocity,
    player_attack, send_actor_event,
};
