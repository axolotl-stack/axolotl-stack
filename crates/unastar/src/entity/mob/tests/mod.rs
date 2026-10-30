//! End-to-end mob tests through a real `GameServer`: spawning and the
//! AddActor wire form ([`spawn`]), the chicken and its egg ([`chicken`]),
//! player interactions ([`interaction`]), leads and name tags ([`leads`])
//! and the other passive mobs ([`passive`]).

mod chicken;
mod interaction;
mod leads;
mod passive;
mod spawn;
