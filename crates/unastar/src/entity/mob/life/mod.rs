//! A mob's life cycle besides combat: growing up ([`ageing`]), mating
//! ([`breeding`]), egg laying ([`spawn_entity`]), environment sensors and
//! timers ([`sensors`]) and natural despawning ([`despawn`]).

pub mod ageing;
pub mod breeding;
pub mod despawn;
pub mod sensors;
pub mod spawn_entity;
pub mod triggers;
