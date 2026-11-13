//! Players acting on mobs: packet ingress and dispatch ([`actions`]) and
//! item interactions ([`interact`]: dyeing, spawn eggs, feeding, authored
//! `minecraft:interact` rows).

pub mod actions;
#[cfg(test)]
mod bds_mooshroom;
pub mod interact;
