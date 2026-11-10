//! End-to-end mob tests through a real `GameServer`: spawning and the
//! AddActor wire form ([`spawn`]), the chicken and its egg ([`chicken`]),
//! player interactions ([`interaction`]), the other passive mobs
//! ([`passive`]), the zombie ([`zombie`]), the other hostile mobs
//! ([`hostile`]), door breaking ([`doors`]), environment sensors and
//! timers ([`sensors`]), anger replayed against BDS captures ([`anger`]),
//! and shared behaviour checked mob by mob for every spawnable mob that uses
//! it ([`facets`]).

mod anger;
mod chicken;
mod doors;
mod facets;
mod hostile;
mod interaction;
mod passive;
mod sensors;
mod spawn;
mod zombie;
