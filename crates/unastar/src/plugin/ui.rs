//! Component-owned player widgets and typed, replay-checked form requests.

use super::players::PlayerLineage;
use super::wit_bindings::unastar::plugin::presentation as wit;
use super::{HostContext, PluginManager};
use bevy_ecs::prelude::*;
use parking_lot::Mutex;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Weak};

#[path = "ui/events.rs"]
mod events;
#[path = "ui/host.rs"]
mod host;
#[path = "ui/wire.rs"]
mod wire;

/// Ownership lasts for one component instance, independently of its display data.
#[derive(Clone)]
struct Owner {
    id: crate::plugin::manifest::PluginId,
    generation: uuid::Uuid,
    lease: Weak<()>,
}
impl Owner {
    /// A dropped or replaced store never inherits an earlier instance's widgets.
    fn matches(&self, context: &HostContext) -> bool {
        self.id == context.plugin_id && self.generation == context.generation && self.alive()
    }
    /// Weak leases permit cleanup without borrowing a removed component or world.
    fn alive(&self) -> bool {
        self.lease.upgrade().is_some()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Widget {
    Sidebar,
    ActionBar,
    Form,
}
type Key = (uuid::Uuid, Widget);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Sidebar {
    title: String,
    lines: Vec<(String, String)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Form {
    request: u64,
    wire: u32,
    callback: u64,
    payload: Arc<str>,
    buttons: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Sidebar(Option<Sidebar>),
    ActionBar(Option<String>),
    Form(Option<Form>),
}
impl Value {
    /// A clear releases ownership as soon as its successful callback commits.
    fn cleared(&self) -> bool {
        matches!(
            self,
            Self::Sidebar(None) | Self::ActionBar(None) | Self::Form(None)
        )
    }
}
struct Claim {
    owner: Owner,
    provisional: Option<uuid::Uuid>,
}
struct Desired {
    version: u64,
    value: Value,
}

/// One invocation's reversible widget intent; it contains owned identity only.
pub(super) struct Action {
    key: Key,
    target: PlayerLineage,
    owner: Owner,
    version: u64,
    reservation: Option<uuid::Uuid>,
    value: Value,
    replaced_forms: Vec<(Form, wit::FormCloseReason)>,
}
struct ActiveForm {
    target: PlayerLineage,
    owner: Owner,
    value: Form,
    sent_at: Option<std::time::Instant>,
}
/// Accepted form input is owned until a fresh callback scope can be supplied.
enum Notice {
    Response {
        target: Box<PlayerLineage>,
        owner: Owner,
        request: u64,
        callback: u64,
        response: wit::FormResponse,
    },
    Closed {
        owner: Owner,
        request: u64,
        callback: u64,
        reason: wit::FormCloseReason,
    },
}

struct Book {
    claims: BTreeMap<Key, Claim>,
    desired: BTreeMap<Key, Desired>,
    viewers: HashMap<uuid::Uuid, wire::Viewer>,
    forms: HashMap<u32, ActiveForm>,
    notices: VecDeque<Notice>,
    next_version: u64,
    next_request: u64,
    next_wire: u32,
    next_objective: u64,
}
impl Default for Book {
    fn default() -> Self {
        Self {
            claims: Default::default(),
            desired: Default::default(),
            viewers: Default::default(),
            forms: Default::default(),
            notices: Default::default(),
            next_version: 0,
            next_request: 0,
            next_wire: rand::random::<u32>() & 0x7fff_ffff,
            next_objective: 0,
        }
    }
}
impl Book {
    /// Allocate opaque monotonic versions without wrapping into a stale accepted intent.
    fn version(&mut self) -> Result<u64, wit::PresentationError> {
        self.next_version = self
            .next_version
            .checked_add(1)
            .ok_or(wit::PresentationError::LimitExceeded)?;
        Ok(self.next_version)
    }
    /// Close at most the exact current form, notifying its still-live owner once.
    fn retire_form(&mut self, viewer: uuid::Uuid, reason: wit::FormCloseReason) {
        let ids: Vec<_> = self
            .forms
            .iter()
            .filter(|(_, form)| form.target.key() == viewer)
            .map(|(&wire, _)| wire)
            .collect();
        for id in ids {
            if let Some(form) = self.forms.remove(&id)
                && form.owner.alive()
            {
                self.notices.push_back(Notice::Closed {
                    owner: form.owner,
                    request: form.value.request,
                    callback: form.value.callback,
                    reason,
                });
            }
        }
    }
    /// Owner cleanup clears desired widgets but keeps acknowledged state long enough to restore it.
    fn prune_owners(&mut self) {
        let dead: Vec<_> = self
            .claims
            .iter()
            .filter(|(_, claim)| !claim.owner.alive())
            .map(|(&key, _)| key)
            .collect();
        for key in dead {
            self.claims.remove(&key);
            self.desired.remove(&key);
            if key.1 == Widget::Form {
                self.retire_form(key.0, wit::FormCloseReason::Cancelled);
            }
        }
    }
    /// Reject another live owner before reserving bounded per-instance widget state.
    fn reserve(
        &mut self,
        context: &HostContext,
        key: Key,
        target: &PlayerLineage,
    ) -> Result<Option<uuid::Uuid>, wit::PresentationError> {
        self.prune_owners();
        if let Some(claim) = self.claims.get(&key) {
            return if claim.owner.matches(context) {
                Ok(claim.provisional)
            } else {
                Err(wit::PresentationError::Conflict)
            };
        }
        let count = self
            .claims
            .values()
            .filter(|claim| claim.owner.matches(context))
            .count();
        if count >= context.api_limits.max_ui_widgets as usize || self.claims.len() >= 16_384 {
            return Err(wit::PresentationError::LimitExceeded);
        }
        if !self.viewers.contains_key(&key.0) {
            self.next_objective = self
                .next_objective
                .checked_add(1)
                .filter(|value| *value <= 0x0fff_ffff_ffff_ffff)
                .ok_or(wit::PresentationError::LimitExceeded)?;
            self.viewers.insert(
                key.0,
                wire::Viewer::new(target.clone(), format!("u{:015x}", self.next_objective)),
            );
        }
        let token = uuid::Uuid::new_v4();
        self.claims.insert(
            key,
            Claim {
                owner: context.ui_owner(),
                provisional: Some(token),
            },
        );
        Ok(Some(token))
    }
}

/// Shared presentation ownership follows exact connections through real world migration.
#[derive(Resource, Clone, Default)]
pub(super) struct Registry(Arc<Mutex<Book>>);

impl HostContext {
    /// Bind every widget intent to this particular component instance.
    fn ui_owner(&self) -> Owner {
        Owner {
            id: self.plugin_id.clone(),
            generation: self.generation,
            lease: Arc::downgrade(&self.ui_lease),
        }
    }
    /// Bound host allocations independently of guest fuel and committed widget count.
    fn stage_ui(
        &mut self,
        target: PlayerLineage,
        widget: Widget,
        value: Value,
    ) -> Result<(), wit::PresentationError> {
        if self.ui_updates >= self.api_limits.max_ui_updates_per_tick {
            return Err(wit::PresentationError::LimitExceeded);
        }
        let registry = self.ui_registry.clone();
        let mut book = registry.0.lock();
        let key = (target.key(), widget);
        if value.cleared()
            && !book.claims.contains_key(&key)
            && !self.pending_ui.iter().any(|action| action.key == key)
        {
            return Ok(());
        }
        let version = book.version()?;
        let reservation = book.reserve(self, key, &target)?;
        self.ui_updates += 1;
        let mut action = Action {
            key,
            target,
            owner: self.ui_owner(),
            version,
            reservation,
            value,
            replaced_forms: Vec::new(),
        };
        if let Some(previous) = self
            .pending_ui
            .iter_mut()
            .find(|previous| previous.key == key)
        {
            action.replaced_forms = std::mem::take(&mut previous.replaced_forms);
            if let Value::Form(Some(old)) = &previous.value {
                action.replaced_forms.push((
                    old.clone(),
                    if action.value.cleared() {
                        wit::FormCloseReason::Cancelled
                    } else {
                        wit::FormCloseReason::Replaced
                    },
                ));
            }
            *previous = action;
        } else {
            self.pending_ui.push(action);
        }
        Ok(())
    }
}

/// Commit once after successful callbacks; traps preserve previous committed widgets.
pub(super) fn finish_invocation(context: &mut HostContext, world: &mut World, succeeded: bool) {
    let actions = std::mem::take(&mut context.pending_ui);
    if actions.is_empty() {
        return;
    }
    let registry = context.ui_registry.clone();
    let mut book = registry.0.lock();
    for action in actions {
        let valid = book.claims.get(&action.key).is_some_and(|claim| {
            claim.owner.matches(context) && claim.provisional == action.reservation
        });
        if !valid {
            continue;
        }
        let current = super::players::resolve(world, &action.target).is_ok();
        if !succeeded || !current {
            if action.reservation.is_some() {
                book.claims.remove(&action.key);
            }
            continue;
        }
        for (form, reason) in &action.replaced_forms {
            book.notices.push_back(Notice::Closed {
                owner: action.owner.clone(),
                request: form.request,
                callback: form.callback,
                reason: *reason,
            });
        }
        if action.value.cleared() {
            book.claims.remove(&action.key);
            book.desired.remove(&action.key);
            if action.key.1 == Widget::Form {
                book.retire_form(action.key.0, wit::FormCloseReason::Cancelled);
            }
            continue;
        }
        book.claims
            .get_mut(&action.key)
            .expect("validated widget owner")
            .provisional = None;
        if book
            .desired
            .get(&action.key)
            .is_some_and(|old| old.value == action.value)
        {
            continue;
        }
        if let Value::Form(Some(form)) = &action.value {
            book.retire_form(action.key.0, wit::FormCloseReason::Replaced);
            book.forms.insert(
                form.wire,
                ActiveForm {
                    target: action.target.clone(),
                    owner: action.owner,
                    value: form.clone(),
                    sent_at: None,
                },
            );
        }
        book.desired.insert(
            action.key,
            Desired {
                version: action.version,
                value: action.value,
            },
        );
    }
}

/// Install native protocol intake without exposing an embedding or packet interface to guests.
pub(super) fn install(registry: Registry, world: &mut World) {
    world.insert_resource(registry.clone());
    world.init_resource::<crate::server::game::player_events::PlayerEventHooks>();
    world
        .resource_mut::<crate::server::game::player_events::PlayerEventHooks>()
        .register(events::Intake(registry));
}

impl PluginManager {
    /// Render only after the component cycle has passed its required readiness checks.
    pub(crate) fn publish_ui(&self, world: &mut World) {
        if self.readiness.check().is_ok() {
            wire::reconcile(world);
        }
    }
}
