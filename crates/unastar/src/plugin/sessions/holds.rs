//! Component-owned admission holds bound to one native connection and world.

use bevy_ecs::prelude::*;
use bevy_ecs::world::WorldId;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::entity::components::RuntimeEntityId;
use crate::server::game::SessionEntityMap;
use crate::server::game::player_events::{self, ConnectionIdentity};

/// Runtime-only tokens never contain an entity, account or transport number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AdmissionId(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HoldId(u64);

#[derive(Clone)]
struct Binding {
    world: WorldId,
    player: Entity,
    connection: ConnectionIdentity,
    runtime: i64,
}
impl Binding {
    /// A replaced channel or actor generation cannot acknowledge an older hold.
    fn matches(&self, world: &World) -> bool {
        world.id() == self.world
            && self.connection.matches(world, self.player)
            && world.get::<RuntimeEntityId>(self.player).map(|id| id.0) == Some(self.runtime)
            && world
                .get_resource::<SessionEntityMap>()
                .is_some_and(|map| map.get(self.connection.session_id()) == Some(self.player))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HoldState {
    Provisional,
    Held,
    Allowed,
    Abandoned,
}
struct Hold {
    admission: AdmissionId,
    owner: String,
    state: HoldState,
}
struct Admission {
    binding: Binding,
    evaluating: bool,
    decided: HashSet<String>,
    rejected: Option<String>,
    holds: Vec<HoldId>,
}
#[derive(Default)]
struct Book {
    next: u64,
    admissions: HashMap<AdmissionId, Admission>,
    holds: HashMap<HoldId, Hold>,
}

/// Every world of one component manager shares the same hold ownership map.
#[derive(Resource, Clone, Default)]
pub(crate) struct AdmissionBook(Arc<Mutex<Book>>);

/// The native publication consumer consults this independently of its ready bit.
#[derive(Component)]
pub(crate) struct AdmissionGuard {
    pub(crate) id: AdmissionId,
    pub(crate) book: AdmissionBook,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Stale,
    WrongOwner,
    InvalidDecision,
    Limit,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Allow,
    Reject(String),
    Deferred,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Waiting,
    Ready,
    Rejected(String),
    Stale,
}

/// Validate wire-visible reason text without changing its meaning through truncation.
fn reason(text: &str) -> bool {
    !text.is_empty() && text.len() <= 1024 && !text.chars().any(|ch| ch.is_control() && ch != '\n')
}
impl AdmissionBook {
    /// Close publication before invoking any component, with no lock across host calls.
    pub(crate) fn begin(&self, world: &mut World, player: Entity) -> Result<AdmissionId, Error> {
        if world.get::<AdmissionGuard>(player).is_some() {
            return Err(Error::Stale);
        }
        let binding = Binding {
            world: world.id(),
            player,
            connection: ConnectionIdentity::capture(world, player).ok_or(Error::Stale)?,
            runtime: world.get::<RuntimeEntityId>(player).ok_or(Error::Stale)?.0,
        };
        if !binding.matches(world) || !player_events::defer_initial_state(world, player) {
            return Err(Error::Stale);
        }
        let id = {
            let mut book = self.0.lock().map_err(|_| Error::Unavailable)?;
            let next = book.next.checked_add(1).ok_or(Error::Limit)?;
            book.next = next;
            let id = AdmissionId(next);
            book.admissions.insert(
                id,
                Admission {
                    binding,
                    evaluating: true,
                    decided: HashSet::new(),
                    rejected: None,
                    holds: Vec::new(),
                },
            );
            id
        };
        world.entity_mut(player).insert(AdmissionGuard {
            id,
            book: self.clone(),
        });
        Ok(id)
    }

    /// Only the current admission callback can create its provisional owned resources.
    pub(crate) fn hold(&self, id: AdmissionId, owner: &str) -> Result<HoldId, Error> {
        let mut book = self.0.lock().map_err(|_| Error::Unavailable)?;
        let admission = book.admissions.get(&id).ok_or(Error::Stale)?;
        if !admission.evaluating
            || admission.rejected.is_some()
            || admission.decided.contains(owner)
        {
            return Err(Error::Stale);
        }
        if admission.holds.len() >= 32 {
            return Err(Error::Limit);
        }
        let next = book.next.checked_add(1).ok_or(Error::Limit)?;
        book.next = next;
        let hold = HoldId(next);
        book.holds.insert(
            hold,
            Hold {
                admission: id,
                owner: owner.into(),
                state: HoldState::Provisional,
            },
        );
        book.admissions
            .get_mut(&id)
            .expect("checked admission")
            .holds
            .push(hold);
        Ok(hold)
    }

    /// Finalize a component's response only after its callback returned successfully.
    pub(crate) fn decide(
        &self,
        id: AdmissionId,
        owner: &str,
        decision: Decision,
    ) -> Result<(), Error> {
        let mut book = self.0.lock().map_err(|_| Error::Unavailable)?;
        let admission = book.admissions.get(&id).ok_or(Error::Stale)?;
        if !admission.evaluating || admission.decided.contains(owner) {
            return Err(Error::InvalidDecision);
        }
        let owned: Vec<_> = admission
            .holds
            .iter()
            .copied()
            .filter(|hold| book.holds.get(hold).is_some_and(|hold| hold.owner == owner))
            .collect();
        match decision {
            Decision::Allow => {
                // Returning Allow does not depend on retaining provisional resources.
                for id in owned {
                    book.holds.remove(&id);
                }
            }
            Decision::Reject(text) => {
                if !reason(&text) {
                    return Err(Error::InvalidDecision);
                }
                book.admissions
                    .get_mut(&id)
                    .expect("checked admission")
                    .rejected
                    .get_or_insert(text);
            }
            Decision::Deferred => {
                if owned.is_empty()
                    || owned
                        .iter()
                        .any(|id| book.holds[id].state != HoldState::Provisional)
                {
                    return Err(Error::InvalidDecision);
                }
                for id in owned {
                    book.holds.get_mut(&id).expect("checked hold").state = HoldState::Held;
                }
            }
        }
        book.admissions
            .get_mut(&id)
            .expect("checked admission")
            .decided
            .insert(owner.into());
        Ok(())
    }

    /// End aggregate proposals after all subscribed components have decided.
    pub(crate) fn evaluated(&self, id: AdmissionId) -> Result<(), Error> {
        self.0
            .lock()
            .map_err(|_| Error::Unavailable)?
            .admissions
            .get_mut(&id)
            .ok_or(Error::Stale)?
            .evaluating = false;
        Ok(())
    }

    /// Initialization authority belongs to an unresolved token and its exact original player.
    pub(crate) fn validate_initialization(
        &self,
        world: &World,
        hold: HoldId,
        owner: &str,
        player: Entity,
        active: Option<AdmissionId>,
    ) -> Result<(), Error> {
        let book = self.0.lock().map_err(|_| Error::Unavailable)?;
        let entry = book.holds.get(&hold).ok_or(Error::Stale)?;
        if entry.owner != owner {
            return Err(Error::WrongOwner);
        }
        let admission = book.admissions.get(&entry.admission).ok_or(Error::Stale)?;
        if admission.binding.player != player
            || !admission.binding.matches(world)
            || admission.rejected.is_some()
        {
            return Err(Error::Stale);
        }
        match entry.state {
            HoldState::Provisional if admission.evaluating && active == Some(entry.admission) => {
                Ok(())
            }
            HoldState::Held if !admission.evaluating => Ok(()),
            _ => Err(Error::Stale),
        }
    }

    /// Component callbacks cannot borrow another world's provisional authority.
    pub(crate) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Observe the aggregate book through one exact component-owned hold.
    pub(crate) fn status_for_hold(&self, world: &World, hold: HoldId, owner: &str) -> Status {
        let admission = {
            let Ok(book) = self.0.lock() else {
                return Status::Stale;
            };
            let Some(entry) = book.holds.get(&hold) else {
                return Status::Stale;
            };
            if entry.owner != owner {
                return Status::Stale;
            }
            entry.admission
        };
        self.status(world, admission)
    }

    /// Native plan failure may close its original admission after a publication vote.
    pub(crate) fn reject_for_hold(&self, hold: HoldId, owner: &str, text: &str) -> bool {
        let admission = {
            let Ok(book) = self.0.lock() else {
                return false;
            };
            let Some(entry) = book.holds.get(&hold) else {
                return false;
            };
            if entry.owner != owner {
                return false;
            }
            entry.admission
        };
        self.reject(admission, text);
        true
    }

    /// Preflight a token without changing its pending state before callback commit.
    pub(crate) fn validate_resolution(
        &self,
        world: &World,
        hold: HoldId,
        owner: &str,
    ) -> Result<(), Error> {
        let book = self.0.lock().map_err(|_| Error::Unavailable)?;
        let entry = book.holds.get(&hold).ok_or(Error::Stale)?;
        if entry.owner != owner {
            return Err(Error::WrongOwner);
        }
        if entry.state != HoldState::Held {
            return Err(Error::Stale);
        }
        if !book
            .admissions
            .get(&entry.admission)
            .is_some_and(|admission| {
                admission.binding.matches(world)
                    && !admission.evaluating
                    && admission.rejected.is_none()
            })
        {
            return Err(Error::Stale);
        }
        Ok(())
    }

    /// Apply an acknowledgement from a successful fresh transaction callback.
    pub(crate) fn resolve(
        &self,
        world: &World,
        hold: HoldId,
        owner: &str,
        decision: Decision,
    ) -> Result<(), Error> {
        let mut book = self.0.lock().map_err(|_| Error::Unavailable)?;
        let entry = book.holds.get(&hold).ok_or(Error::Stale)?;
        if entry.owner != owner {
            return Err(Error::WrongOwner);
        }
        if entry.state != HoldState::Held {
            return Err(Error::Stale);
        }
        let id = entry.admission;
        if !book
            .admissions
            .get(&id)
            .is_some_and(|admission| admission.binding.matches(world))
        {
            return Err(Error::Stale);
        }
        match decision {
            Decision::Allow => {
                book.holds.get_mut(&hold).expect("checked hold").state = HoldState::Allowed
            }
            Decision::Reject(text) => {
                if !reason(&text) {
                    return Err(Error::InvalidDecision);
                }
                book.admissions
                    .get_mut(&id)
                    .expect("checked admission")
                    .rejected
                    .get_or_insert(text);
            }
            Decision::Deferred => return Err(Error::InvalidDecision),
        }
        Ok(())
    }

    /// Dropping a retained unresolved hold can never authorize publication.
    pub(crate) fn abandoned(&self, hold: HoldId, owner: &str) -> Result<(), Error> {
        let mut book = self.0.lock().map_err(|_| Error::Unavailable)?;
        let entry = book.holds.get_mut(&hold).ok_or(Error::Stale)?;
        if entry.owner != owner {
            return Err(Error::WrongOwner);
        }
        if entry.state != HoldState::Allowed {
            entry.state = HoldState::Abandoned;
        }
        Ok(())
    }

    /// A component trap cannot leave its retained admission tokens pending forever.
    pub(crate) fn owner_failed(&self, owner: &str) {
        let Ok(mut book) = self.0.lock() else {
            return;
        };
        let affected: Vec<_> = book
            .holds
            .values()
            .filter(|hold| hold.owner == owner && hold.state != HoldState::Allowed)
            .map(|hold| hold.admission)
            .collect();
        for id in affected {
            if let Some(admission) = book.admissions.get_mut(&id) {
                admission
                    .rejected
                    .get_or_insert("Unable to complete connection setup.".into());
            }
        }
    }

    /// Observe aggregate acceptance without returning borrowed state across callbacks.
    pub(crate) fn status(&self, world: &World, id: AdmissionId) -> Status {
        let Ok(book) = self.0.lock() else {
            return Status::Waiting;
        };
        let Some(admission) = book.admissions.get(&id) else {
            return Status::Stale;
        };
        if !admission.binding.matches(world) {
            return Status::Stale;
        }
        if let Some(text) = &admission.rejected {
            return Status::Rejected(text.clone());
        }
        if admission.evaluating {
            return Status::Waiting;
        }
        if admission.holds.iter().any(|id| {
            book.holds
                .get(id)
                .is_some_and(|hold| hold.state == HoldState::Abandoned)
        }) {
            return Status::Rejected("Unable to complete connection setup.".into());
        }
        if admission.holds.iter().any(|id| {
            book.holds
                .get(id)
                .is_some_and(|hold| hold.state != HoldState::Allowed)
        }) {
            return Status::Waiting;
        }
        Status::Ready
    }

    /// Reject a proposal that could not produce a valid typed decision.
    pub(crate) fn reject(&self, id: AdmissionId, text: &str) {
        let Ok(mut book) = self.0.lock() else {
            return;
        };
        if let Some(admission) = book.admissions.get_mut(&id) {
            admission.rejected.get_or_insert(if reason(text) {
                text.into()
            } else {
                "Unable to complete connection setup.".into()
            });
        }
    }

    /// Resolve the original native source without returning any borrowed book state.
    pub(crate) fn player(&self, world: &World, id: AdmissionId) -> Option<Entity> {
        let book = self.0.lock().ok()?;
        let admission = book.admissions.get(&id)?;
        admission
            .binding
            .matches(world)
            .then_some(admission.binding.player)
    }

    /// Late native observers may remove a join source without another departure callback.
    pub(crate) fn prune(&self, world: &World) {
        let Ok(mut book) = self.0.lock() else {
            return;
        };
        let stale: Vec<_> = book
            .admissions
            .iter()
            .filter(|(_, admission)| !admission.binding.matches(world))
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            if let Some(admission) = book.admissions.remove(&id) {
                for hold in admission.holds {
                    book.holds.remove(&hold);
                }
            }
        }
    }

    /// Remove terminal bookkeeping on publication, rejection or exact-session departure.
    pub(crate) fn remove(&self, id: AdmissionId) {
        let Ok(mut book) = self.0.lock() else {
            return;
        };
        if let Some(admission) = book.admissions.remove(&id) {
            for id in admission.holds {
                book.holds.remove(&id);
            }
        }
    }
}

/// Unresolved component holds remain authoritative even if another caller sets ready.
pub(crate) fn blocks_publication(world: &World, player: Entity) -> bool {
    world
        .get::<AdmissionGuard>(player)
        .is_some_and(|guard| guard.book.status(world, guard.id) != Status::Ready)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
