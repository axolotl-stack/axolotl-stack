//! What clients see of mobs: replication to viewers ([`network`]), level
//! sounds and events ([`sound`]) and the ambient sound event name
//! ([`ambient`]).

pub mod ambient;
#[cfg(test)]
mod bds_replication;
pub mod network;
pub mod sound;
