//! A mob's life cycle besides combat: growing up ([`ageing`]), mating
//! ([`breeding`]), egg laying ([`spawn_entity`]), environment sensors and
//! timers ([`sensors`]), anger ([`anger`]), natural despawning
//! ([`despawn`]) and transformations ([`transformation`]).

pub mod ageing;
pub mod anger;
pub mod breeding;
pub mod despawn;
pub mod sensors;
pub mod spawn_entity;
pub mod transformation;
