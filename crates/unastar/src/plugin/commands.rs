//! Owned command declarations and exact-connection callbacks before native execution.

use super::wit_bindings::unastar::plugin::commands as wit;
use super::{HostContext, PluginManager, make_player_resource, make_world_tx};
use crate::plugin::manifest::{PluginCapability, PluginId};
use crate::server::game::player_events::ConnectionIdentity;
use bevy_ecs::prelude::*;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

#[path = "commands/publication.rs"]
mod publication;
pub(crate) use publication::{initial_packet, note_initial_packet};

const MAX_ROOTS: usize = 4096;
const MAX_LINE_BYTES: usize = 16_384;
const MAX_REQUEST_ID_BYTES: usize = 1024;

/// Owned display syntax, separate from the immutable native parser definitions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Root {
    name: String,
    description: String,
}

struct Entry {
    root: Root,
    owner: PluginId,
    generation: uuid::Uuid,
    lease: Weak<()>,
    provisional: Option<uuid::Uuid>,
}
#[derive(Default)]
struct State {
    roots: BTreeMap<String, Entry>,
    published: Arc<Vec<Root>>,
    dirty: bool,
}
impl State {
    /// Expired owners cannot retain declarations across replacement or unload.
    fn prune(&mut self) {
        self.roots.retain(|_, entry| {
            let alive = entry.lease.upgrade().is_some();
            self.dirty |= !alive && entry.provisional.is_none();
            alive
        });
    }
}

/// One namespace shared by a runtime's component stores and real dimension worlds.
#[derive(Resource, Clone, Default)]
pub(super) struct CommandRoots(Arc<Mutex<State>>);
impl CommandRoots {
    /// Capture a cheap identity-stable catalogue; provisional declarations stay invisible.
    fn snapshot(&self) -> Arc<Vec<Root>> {
        let mut state = self.0.lock();
        state.prune();
        if state.dirty {
            let published: Vec<_> = state
                .roots
                .values()
                .filter(|entry| entry.provisional.is_none())
                .map(|entry| entry.root.clone())
                .collect();
            if published != *state.published {
                state.published = Arc::new(published);
            }
            state.dirty = false;
        }
        state.published.clone()
    }
}

/// A callback's provisional ownership reservation, independently reversible on a trap.
pub(super) struct Reservation {
    name: String,
    token: uuid::Uuid,
}

impl super::wit_bindings::unastar::plugin::commands::Host for HostContext {
    fn register_root(&mut self, root: wit::Root) -> Result<wit::Registration, wit::CommandError> {
        self.operation()
            .map_err(|_| wit::CommandError::LimitExceeded)?;
        self.require(PluginCapability::Commands)
            .map_err(|_| wit::CommandError::PermissionDenied)?;
        self.validate_callback()
            .map_err(|_| wit::CommandError::Unavailable)?;
        if root.name.is_empty()
            || root.name.len() > 32
            || root.description.len() > 256
            || root.description.chars().any(char::is_control)
        {
            return Err(wit::CommandError::InvalidRoot);
        }
        let registry = unsafe { self.world() }
            .get_resource::<crate::command::CommandRegistry>()
            .ok_or(wit::CommandError::Unavailable)?;
        if registry.schema().command(&root.name).is_some() {
            return Ok(wit::Registration::ExistingBuiltin);
        }
        if !root.name.as_bytes()[0].is_ascii_lowercase()
            || !root.name.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            })
        {
            return Err(wit::CommandError::InvalidRoot);
        }
        let mut state = self.command_roots.0.lock();
        state.prune();
        if let Some(entry) = state.roots.get(&root.name) {
            return if entry.owner == self.plugin_id && entry.generation == self.generation {
                Ok(wit::Registration::ExistingOwned)
            } else {
                Err(wit::CommandError::Conflict)
            };
        }
        let owned = state
            .roots
            .values()
            .filter(|entry| entry.generation == self.generation && entry.owner == self.plugin_id)
            .count();
        if owned >= self.api_limits.max_command_roots as usize || state.roots.len() >= MAX_ROOTS {
            return Err(wit::CommandError::LimitExceeded);
        }
        let token = uuid::Uuid::new_v4();
        self.pending_command_roots.push(Reservation {
            name: root.name.clone(),
            token,
        });
        state.roots.insert(
            root.name.clone(),
            Entry {
                root: Root {
                    name: root.name,
                    description: root.description,
                },
                owner: self.plugin_id.clone(),
                generation: self.generation,
                lease: Arc::downgrade(&self.command_owner),
                provisional: Some(token),
            },
        );
        Ok(wit::Registration::Created)
    }
}

/// Commit ownership before another component initializes, or remove only this invocation's reservations.
pub(super) fn finish_invocation(context: &mut HostContext, succeeded: bool) {
    let reservations = std::mem::take(&mut context.pending_command_roots);
    if reservations.is_empty() {
        return;
    }
    let mut state = context.command_roots.0.lock();
    for reservation in reservations {
        if !state.roots.get(&reservation.name).is_some_and(|entry| {
            entry.generation == context.generation && entry.provisional == Some(reservation.token)
        }) {
            continue;
        }
        if succeeded {
            state
                .roots
                .get_mut(&reservation.name)
                .expect("validated reservation")
                .provisional = None;
            state.dirty = true;
        } else {
            state.roots.remove(&reservation.name);
        }
    }
}

/// Internal callback bridge; the public extension contract remains the WASM handler.
#[derive(Resource, Clone)]
struct Bridge(Arc<std::sync::Mutex<PluginManager>>);

/// Install the runtime-owned namespace and bridge without holding its lock during callbacks.
pub(super) fn install(manager: Arc<std::sync::Mutex<PluginManager>>, world: &mut World) {
    let roots = manager
        .lock()
        .expect("component manager lock")
        .command_roots
        .clone();
    world.insert_resource(roots);
    world.init_resource::<publication::Publications>();
    world.insert_resource(Bridge(manager));
}

/// Whether this world has a component command boundary installed.
pub(crate) fn installed(world: &World) -> bool {
    world.contains_resource::<Bridge>()
}

/// Handle an authenticated player command before native parsing and permission checks.
/// A failed or busy component boundary never falls through into default execution.
pub(crate) fn dispatch(
    world: &mut World,
    player: Entity,
    line: &str,
    request_id: Option<&str>,
) -> bool {
    let Some(bridge) = world.get_resource::<Bridge>().cloned() else {
        return false;
    };
    if line.len() > MAX_LINE_BYTES
        || request_id.is_some_and(|id| id.len() > MAX_REQUEST_ID_BYTES)
        || crate::server::game::inventory_effects::is_quarantined(world, player)
        || crate::server::game::player_events::initial_state_pending(world, player)
        || crate::server::worlds::is_loading(world, player)
    {
        return true;
    }
    let Some(identity) = ConnectionIdentity::capture(world, player) else {
        return true;
    };
    let Ok(mut manager) = bridge.0.try_lock() else {
        tracing::error!("Component manager unavailable during command execution");
        return true;
    };
    manager.player_command(world, player, &identity, line, request_id)
}

impl PluginManager {
    /// Execute ordered scoped handlers; handled requests do not reach later/default executors.
    fn player_command(
        &mut self,
        world: &mut World,
        player: Entity,
        identity: &ConnectionIdentity,
        line: &str,
        request_id: Option<&str>,
    ) -> bool {
        if !self.begin_tick(world) {
            return true;
        }
        for id in self.subscribers(crate::ecs::events::EventKind::PlayerCommand) {
            if !identity.matches(world, player) {
                return true;
            }
            let plugin = self.plugins.get_mut(&id).expect("subscribed component");
            let result = super::lifecycle::invoke(plugin, world, |store, bindings| {
                let tx = make_world_tx(store)?;
                let player = make_player_resource(store, player)?;
                bindings.call_on_player_command(store, tx, player, line, request_id)
            });
            match result {
                Ok(true) if identity.matches(world, player) => {}
                Ok(_) => return true,
                Err(error) => {
                    tracing::error!(plugin = %id, %error, "Component command callback failed");
                    self.callback_failed(world, &id, &error.to_string());
                    return true;
                }
            }
        }
        !identity.matches(world, player)
    }
    /// Reconcile owned advertisements after global callbacks and deferred work.
    pub(crate) fn publish_command_roots(&self, world: &mut World) {
        if self.readiness.check().is_ok() {
            publication::reconcile(world);
        }
    }
}
