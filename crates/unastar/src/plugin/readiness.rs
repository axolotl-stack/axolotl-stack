//! Component readiness and capture demand shared by every world of one runtime.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};

use bevy_ecs::prelude::*;

use super::PluginManager;

const MAX_CONFIG_BYTES: u64 = 1_048_576;

#[derive(Default)]
struct State {
    death_captures: Vec<Weak<()>>,
    required: BTreeSet<String>,
    initialized: BTreeSet<String>,
    failure: Option<String>,
}

/// The same admission latch follows the manager into every dimension world.
#[derive(Resource, Clone, Default)]
pub(crate) struct PluginReadiness(Arc<Mutex<State>>);
impl PluginReadiness {
    /// A live subscribed instance requests capture in every world; unload expires its lease.
    pub(super) fn request_death_capture(&self, owner: &Arc<()>) {
        let mut state = self.0.lock().expect("component readiness lock");
        state
            .death_captures
            .retain(|owner| owner.strong_count() > 0);
        state.death_captures.push(Arc::downgrade(owner));
    }
    /// Require an ID before discovery or instantiation can expose player state.
    pub(super) fn require(&self, id: &str) {
        self.0
            .lock()
            .expect("component readiness lock")
            .required
            .insert(id.into());
    }
    /// Successful initialization is recorded only after on-load returns.
    pub(super) fn initialized(&self, id: &str) {
        self.0
            .lock()
            .expect("component readiness lock")
            .initialized
            .insert(id.into());
    }
    /// Read whether this identity is an essential runtime component.
    pub(super) fn is_required(&self, id: &str) -> bool {
        self.0.lock().is_ok_and(|state| state.required.contains(id))
    }
    /// Latch the first essential failure; another tick cannot clear it.
    pub(super) fn fail(&self, id: &str, reason: &str) -> bool {
        let Ok(mut state) = self.0.lock() else {
            return true;
        };
        if !state.required.contains(id) {
            return false;
        }
        if state.failure.is_none() {
            state.failure = Some(format!(
                "required component {id} failed: {}",
                reason.chars().take(4096).collect::<String>()
            ));
        }
        true
    }
    /// Fail startup when an explicitly required manifest never appeared.
    pub(super) fn discovered(&self, present: &BTreeSet<String>) -> Result<(), String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "component readiness lock is poisoned".to_owned())?;
        if let Some(id) = state.required.difference(present).next().cloned()
            && state.failure.is_none()
        {
            state.failure = Some(format!("required component {id} was not discovered"));
        }
        state.failure.clone().map_or(Ok(()), Err)
    }
    /// Describe a pending initialization or sticky runtime failure.
    pub(crate) fn check(&self) -> Result<(), String> {
        let state = self
            .0
            .lock()
            .map_err(|_| "component readiness lock is poisoned".to_owned())?;
        if let Some(error) = &state.failure {
            return Err(error.clone());
        }
        if let Some(id) = state.required.difference(&state.initialized).next() {
            return Err(format!("required component {id} has not initialized"));
        }
        Ok(())
    }
}

/// Read runtime-wide archive demand without acquiring the component manager during simulation.
pub(crate) fn death_capture_requested(world: &World) -> bool {
    world
        .get_resource::<PluginReadiness>()
        .is_some_and(|readiness| {
            readiness.0.lock().is_ok_and(|state| {
                state
                    .death_captures
                    .iter()
                    .any(|owner| owner.strong_count() > 0)
            })
        })
}

/// Native admission remains closed until every declared component is ready.
pub(crate) fn admission_allowed(world: &World) -> bool {
    world
        .get_resource::<PluginReadiness>()
        .is_none_or(|state| state.check().is_ok())
}

/// A failed required callback cannot leak queued actions or later player saves.
pub(crate) fn fence_world(world: &mut World) {
    use crate::entity::components::{Player, PlayerSession};
    use crate::server::game::inventory_effects::{RecoverySource, fence_for_recovery};
    let mut query = world.query_filtered::<Entity, (With<Player>, With<PlayerSession>)>();
    let players: Vec<_> = query.iter(world).collect();
    for player in players {
        if let Ok(source) = RecoverySource::capture(world, player) {
            let _ = fence_for_recovery(world, &source);
        }
    }
    if let Some(mut actions) = world.get_resource_mut::<crate::ecs::events::ActionQueue>() {
        actions.drain();
    }
}

/// Load bounded opaque configuration without interpreting application data.
pub(super) fn configuration(directory: &Path, requested: Option<&Path>) -> anyhow::Result<Vec<u8>> {
    let Some(requested) = requested else {
        return Ok(Vec::new());
    };
    anyhow::ensure!(
        !requested.as_os_str().is_empty() && !requested.is_absolute(),
        "component config must be a relative file path"
    );
    let directory = directory.canonicalize()?;
    let path = directory.join(requested).canonicalize()?;
    anyhow::ensure!(
        path.starts_with(&directory),
        "component config escapes its directory"
    );
    anyhow::ensure!(
        std::fs::metadata(&path)?.is_file(),
        "component config is not a file"
    );
    let file = std::fs::File::open(path)?;
    anyhow::ensure!(file.metadata()?.is_file(), "component config is not a file");
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_CONFIG_BYTES,
        "component config exceeds 1 MiB"
    );
    Ok(bytes)
}

impl PluginManager {
    /// Install the host's explicit expected identities before scanning files.
    pub(crate) fn require_plugins(&mut self, ids: &[String]) {
        for id in ids {
            self.readiness.require(id);
        }
    }
    /// Initialize components before listeners are bound, then check readiness.
    pub(crate) fn initialize_startup(&mut self, world: &mut World) -> Result<(), String> {
        self.initialize_pending(world);
        self.readiness.check()
    }
    /// Convert essential callback failures to a sticky admission and save fence.
    pub(super) fn callback_failed(
        &mut self,
        world: &mut World,
        id: &crate::plugin::manifest::PluginId,
        reason: &str,
    ) {
        // A failed policy callback cannot leave an earlier queued item authorization alive.
        crate::server::game::item_actions::clear_blocks(world);
        if self.readiness.fail(&id.0, reason) {
            self.tick_budget_valid = false;
            fence_world(world);
        }
    }
    /// Fail a router cycle instead of silently running without required policy.
    pub(crate) fn check_required(&self) -> Result<(), String> {
        self.readiness.check()
    }
}
