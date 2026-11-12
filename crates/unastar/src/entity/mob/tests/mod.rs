//! End-to-end mob tests through a real `GameServer`: spawning and the
//! AddActor wire form ([`spawn`]), the chicken and its egg ([`chicken`]),
//! the creeper's fuse sensor replayed against BDS sweeps ([`creeper`]),
//! player interactions ([`interaction`]), the other passive mobs
//! ([`passive`]), the zombie ([`zombie`]), the other hostile mobs
//! ([`hostile`]), door breaking ([`doors`]), environment sensors and
//! timers ([`sensors`]), anger replayed against BDS captures ([`anger`]),
//! projectile actors replayed against BDS captures ([`projectiles`]), the
//! silverfish replayed against BDS sweeps ([`silverfish`]), and
//! shared behaviour checked mob by mob for every spawnable mob that uses it
//! ([`facets`]).

mod anger;
mod bds_mob;
mod chicken;
mod creeper;
mod doors;
mod facets;
mod hostile;
mod interaction;
mod passive;
mod projectiles;
mod sensors;
mod silverfish;
mod spawn;
mod spider;
mod zombie;
