//! A mob's life cycle besides combat: growing up ([`ageing`]), mating
//! ([`breeding`]), egg laying ([`spawn_entity`]), environment sensors and
//! timers ([`sensors`]), anger and target acquisition ([`angry`]), being
//! looked at ([`looked_at`]), teleporting ([`teleport`]) and natural
//! despawning ([`despawn`]).

pub mod angry;
pub mod ageing;
pub mod breeding;
pub mod despawn;
pub mod sensors;
pub mod spawn_entity;
// Enderman lane.
pub mod looked_at;
pub mod teleport;
