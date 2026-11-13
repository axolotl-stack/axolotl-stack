//! A mob's life cycle besides combat: growing up ([`ageing`]), mating
//! ([`breeding`]), egg laying ([`spawn_entity`]), environment sensors and
//! timers ([`sensors`]), anger ([`anger`]), natural despawning
//! ([`despawn`]), transformations ([`transformation`]), silverfish
//! hiding in and leaving infested blocks ([`infested`]), the enderman's
//! stare ([`looked_at`]) and teleports ([`teleport`]).

pub mod ageing;
pub mod anger;
pub mod breeding;
pub mod despawn;
pub mod infested;
pub mod looked_at;
pub mod sensors;
pub mod spawn_entity;
pub mod teleport;
pub mod transformation;
