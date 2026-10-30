//! Players acting on mobs: packet ingress and dispatch ([`actions`]) and
//! item interactions ([`interact`]: dyeing, spawn eggs, feeding, authored
//! `minecraft:interact` rows; [`leash`]: leads; [`naming`]: name tags).

pub mod actions;
pub mod interact;
pub mod leash;
pub mod naming;
