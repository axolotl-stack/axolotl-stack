//! End-to-end mob tests through a real `GameServer`: spawning and the
//! AddActor wire form ([`spawn`]), the chicken and its egg ([`chicken`]),
//! player interactions ([`interaction`]), the other passive mobs
//! ([`passive`]), the zombie ([`zombie`]), door breaking ([`doors`]) and
//! environment sensors and timers ([`sensors`]).

mod chicken;
mod doors;
mod interaction;
mod passive;
mod sensors;
mod spawn;
mod zombie;
