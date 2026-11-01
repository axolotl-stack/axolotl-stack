//! Component-owned references and callback-scoped access to native player state.

use super::{HostContext, wit_types as wit};
use bevy_ecs::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use wasmtime::component::Resource;

#[path = "players/identity.rs"]
mod identity;
pub(super) use identity::WorldReference;
pub(crate) use identity::{ComponentDirectory, PlayerLineage};
use identity::{availability, observe, snapshot};
pub(super) use identity::{ensure_identity, resolve};
#[path = "players/admission.rs"]
mod admission;
#[path = "players/archive.rs"]
mod archive;
#[path = "players/connection_control.rs"]
mod connection_control;
#[path = "players/host.rs"]
mod host;
#[path = "players/inventory.rs"]
mod inventory;
#[path = "players/inventory_transactions.rs"]
mod inventory_transactions;
pub(super) use inventory_transactions::Requests as InventoryRequests;
pub(super) use inventory_transactions::finish as finish_inventory_invocation;
#[path = "players/items.rs"]
mod items;
#[path = "players/mechanics.rs"]
mod mechanics;
#[path = "players/travel.rs"]
pub(super) mod travel;
#[path = "players/values.rs"]
mod values;
#[path = "players/world_blocks.rs"]
mod world_blocks;
pub(crate) use mechanics::apply_pending as apply_mechanics;
pub(super) use mechanics::{MechanicsRequest, finish as finish_mechanics};
pub(crate) use world_blocks::apply_pending as apply_block_edits;
pub(super) use world_blocks::{Request as BlockEditRequest, finish as finish_block_edits};

/// One callback's authority; tokens are invalidated before another callback can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Scope {
    epoch: u64,
    world: WorldReference,
}

/// Resource representations are monotonic within one component instance, never ECS IDs.
enum Entry {
    Player {
        reference: PlayerLineage,
        scope: Scope,
        transient: bool,
    },
    PlayerHandle(PlayerLineage),
    WorldTx {
        scope: Scope,
        transient: bool,
    },
    WorldHandle(WorldReference),
    Task(Arc<super::tasks::TaskState>),
    Item(Arc<values::Value<crate::item::ItemStack>>),
    AdmissionHold(super::sessions::OwnedHold),
    InventoryImage(Arc<values::Value<inventory::Image>>),
    ArchivedState(Arc<values::Value<archive::Archived>>),
    InventoryOperation(Arc<inventory_transactions::Operation>),
    ConnectionClose(Arc<super::connection_control::RequestState>),
    BlockCell(Arc<values::Value<world_blocks::Cell>>),
    BlockEdit(world_blocks::Edit),
    BlockEditRequest(Arc<world_blocks::RequestState>),
}

/// Per-component resource ownership and bounded per-tick host operation counters.
#[derive(Default)]
pub(super) struct PlayerResources {
    next: u32,
    epoch: u64,
    scope: Option<Scope>,
    entries: HashMap<u32, Entry>,
    pub(super) operations: u32,
    pub(super) messages: u32,
    pub(super) connection_closes: u32,
    pub(super) block_edits: u32,
    pub(super) tasks: u32,
    pub(super) value_budget: values::Budget,
}
impl PlayerResources {
    /// Attach a fresh scope without invalidating storable owned handles.
    pub(super) fn begin_scope(&mut self, world: &World) {
        let Some(epoch) = self.epoch.checked_add(1) else {
            self.scope = None;
            return;
        };
        self.epoch = epoch;
        self.scope = WorldReference::of(world).ok().map(|world| Scope {
            epoch: self.epoch,
            world,
        });
    }
    /// Borrowed arguments disappear; retained owned live capabilities become expired tokens.
    pub(super) fn end_scope(&mut self) {
        self.entries.retain(|_, entry| {
            !matches!(
                entry,
                Entry::Player {
                    transient: true,
                    ..
                } | Entry::WorldTx {
                    transient: true,
                    ..
                }
            )
        });
        self.scope = None;
    }
    /// Check scope before accessing any world pointer or native component.
    fn scope(&self, scope: Scope) -> Result<(), wit::AccessError> {
        (self.scope == Some(scope))
            .then_some(())
            .ok_or(wit::AccessError::ExpiredScope)
    }
    /// Allocate atomically within the configured host resource budget.
    fn allocate(&mut self, entry: Entry, maximum: u32) -> Result<u32, wit::AccessError> {
        if self.entries.len() >= maximum as usize {
            return Err(wit::AccessError::LimitExceeded);
        }
        let rep = self
            .next
            .checked_add(1)
            .ok_or(wit::AccessError::LimitExceeded)?;
        self.next = rep;
        self.entries.insert(rep, entry);
        Ok(rep)
    }
}

/// Scoped messages carry their original actor and transport binding until callback commit.
pub(super) struct BoundMessage {
    reference: PlayerLineage,
    scope: Scope,
    text: String,
}

/// Preserve actor references through legitimate native world migration.
pub(crate) fn register_transfer_components(
    components: &mut crate::server::worlds::TransferComponents,
) {
    components.preserve::<PlayerLineage>();
}

impl HostContext {
    /// Bound all host calls even when a component loops through cheap imports.
    pub(super) fn operation(&mut self) -> Result<(), wit::AccessError> {
        self.resources.operations = self
            .resources
            .operations
            .checked_add(1)
            .ok_or(wit::AccessError::LimitExceeded)?;
        if self.resources.operations > self.api_limits.max_host_operations_per_tick {
            return Err(wit::AccessError::LimitExceeded);
        }
        Ok(())
    }
    /// Capabilities authorize an import independently of the component's instruction budget.
    pub(super) fn require(
        &self,
        capability: crate::plugin::manifest::PluginCapability,
    ) -> Result<(), wit::AccessError> {
        self.capabilities
            .contains(&capability)
            .then_some(())
            .ok_or(wit::AccessError::PermissionDenied)
    }
    /// Resolve a live capability only inside its original callback and exact world.
    pub(super) fn live_player(
        &mut self,
        rep: u32,
    ) -> Result<(PlayerLineage, Entity), wit::AccessError> {
        self.live_player_for(rep, crate::plugin::manifest::PluginCapability::ReadPlayers)
    }
    /// Validate the same live capability while authorizing a separate narrowly granted import.
    fn live_player_for(
        &mut self,
        rep: u32,
        capability: crate::plugin::manifest::PluginCapability,
    ) -> Result<(PlayerLineage, Entity), wit::AccessError> {
        self.operation()?;
        self.require(capability)?;
        let (reference, scope) = match self.resources.entries.get(&rep) {
            Some(Entry::Player {
                reference, scope, ..
            }) => (reference.clone(), *scope),
            _ => return Err(wit::AccessError::StalePlayer),
        };
        self.resources.scope(scope)?;
        let world = self.world_mut();
        if !scope.world.matches(world) {
            return Err(wit::AccessError::WrongWorld);
        }
        let entity = resolve(world, &reference)?;
        Ok((reference, entity))
    }
    /// Require a live callback scope for imports that have no transaction argument.
    pub(super) fn validate_callback(&self) -> Result<(), wit::AccessError> {
        let scope = self.resources.scope.ok_or(wit::AccessError::ExpiredScope)?;
        if self.world_ptr.is_null() || !scope.world.matches(unsafe { self.world() }) {
            return Err(wit::AccessError::ExpiredScope);
        }
        Ok(())
    }
    /// Validate a callback transaction for additional capability interfaces.
    pub(super) fn validate_transaction(&mut self, rep: u32) -> Result<(), wit::AccessError> {
        self.transaction(rep).map(|_| ())
    }
    /// Validate a transaction token before returning any current-world facts.
    fn transaction(&mut self, rep: u32) -> Result<Scope, wit::AccessError> {
        self.operation()?;
        let scope = match self.resources.entries.get(&rep) {
            Some(Entry::WorldTx { scope, .. }) => *scope,
            _ => return Err(wit::AccessError::ExpiredScope),
        };
        self.resources.scope(scope)?;
        if !scope.world.matches(unsafe { self.world() }) {
            return Err(wit::AccessError::WrongWorld);
        }
        Ok(scope)
    }
    /// Resolve owned player references without manufacturing live player resources.
    fn stored_player(&self, rep: u32) -> Result<PlayerLineage, wit::AccessError> {
        match self.resources.entries.get(&rep) {
            Some(Entry::PlayerHandle(reference)) => Ok(reference.clone()),
            _ => Err(wit::AccessError::StalePlayer),
        }
    }
    /// Validate an owned world reference independently of the current callback's world.
    fn stored_world(&self, rep: u32) -> Result<WorldReference, wit::AccessError> {
        match self.resources.entries.get(&rep) {
            Some(Entry::WorldHandle(reference)) => Ok(*reference),
            _ => Err(wit::AccessError::WorldClosed),
        }
    }
    /// Issue a storable reference while charging the resource budget before exposure.
    fn own_player(
        &mut self,
        reference: PlayerLineage,
    ) -> Result<Resource<wit::PlayerHandle>, wit::AccessError> {
        self.resources
            .allocate(
                Entry::PlayerHandle(reference),
                self.api_limits.max_owned_resources,
            )
            .map(Resource::new_own)
    }
    /// Issue an owned world identity without creating a transaction or live borrow.
    fn own_world(
        &mut self,
        reference: WorldReference,
    ) -> Result<Resource<wit::WorldHandle>, wit::AccessError> {
        self.resources
            .allocate(
                Entry::WorldHandle(reference),
                self.api_limits.max_owned_resources,
            )
            .map(Resource::new_own)
    }
    /// Read the full current player observation after validating the scope and actor.
    fn live_snapshot(&mut self, rep: u32) -> Result<wit::PlayerSnapshot, wit::AccessError> {
        let (reference, player) = self.live_player(rep)?;
        snapshot(unsafe { self.world() }, player, &reference)
    }
    /// Bind borrowed callback arguments to the current scope without allowing resource escape.
    pub(super) fn borrow_player(
        &mut self,
        entity: Entity,
    ) -> Result<Resource<wit::Player>, wit::AccessError> {
        let reference = ensure_identity(self.world_mut(), entity)?;
        let scope = self.resources.scope.ok_or(wit::AccessError::ExpiredScope)?;
        self.resources
            .allocate(
                Entry::Player {
                    reference,
                    scope,
                    transient: true,
                },
                self.api_limits.max_owned_resources,
            )
            .map(Resource::new_own)
    }
    /// Supply exactly the transaction currently owned by the callback dispatcher.
    pub(super) fn borrow_transaction(
        &mut self,
    ) -> Result<Resource<wit::WorldTx>, wit::AccessError> {
        let scope = self.resources.scope.ok_or(wit::AccessError::ExpiredScope)?;
        self.resources
            .allocate(
                Entry::WorldTx {
                    scope,
                    transient: true,
                },
                self.api_limits.max_owned_resources,
            )
            .map(Resource::new_own)
    }
}

/// Replenish component host-operation counters once with its shared tick instruction budget.
pub(super) fn begin_budget(context: &mut HostContext) {
    context.resources.operations = 0;
    context.resources.messages = 0;
    context.resources.connection_closes = 0;
    context.resources.block_edits = 0;
    context.resources.tasks = 0;
}

/// Commit only after a successful guest callback; failed callbacks retain no messages or tasks.
pub(super) fn finish_invocation(context: &mut HostContext, world: &mut World, succeeded: bool) {
    let messages = std::mem::take(&mut context.pending_messages);
    let tasks = std::mem::take(&mut context.pending_tasks);
    if !succeeded {
        for task in tasks {
            task.state.cancel();
        }
        return;
    }
    for message in messages {
        if !message.scope.world.matches(world) {
            continue;
        }
        let Ok(player) = resolve(world, &message.reference) else {
            continue;
        };
        if let Some(session) = world.get::<crate::entity::components::PlayerSession>(player) {
            let packet = crate::server::game::types::system_text(&message.text).into();
            let _ = session.send(packet);
        }
    }
    context.task_requests.extend(tasks);
}

impl HostContext {
    /// Stage scheduled work only after checking capability, resource and aggregate job budgets.
    fn schedule_task(
        &mut self,
        target: super::tasks::TaskTarget,
        callback: u64,
    ) -> Result<Resource<wit::Task>, wit::AccessError> {
        self.require(crate::plugin::manifest::PluginCapability::ScheduleTasks)?;
        self.resources.scope.ok_or(wit::AccessError::ExpiredScope)?;
        if self.resources.tasks >= self.api_limits.max_tasks_per_tick
            || self.pending_tasks.len() + self.task_requests.len() + self.task_failures.len()
                >= self.api_limits.max_pending_tasks as usize
        {
            return Err(wit::AccessError::LimitExceeded);
        }
        let id = self
            .task_sequence
            .checked_add(1)
            .ok_or(wit::AccessError::LimitExceeded)?;
        let state = Arc::new(super::tasks::TaskState::new(id));
        let rep = self.resources.allocate(
            Entry::Task(state.clone()),
            self.api_limits.max_owned_resources,
        )?;
        self.task_sequence = id;
        self.resources.tasks += 1;
        self.pending_tasks.push(super::tasks::TaskRequest {
            owner: self.plugin_id.clone(),
            generation: self.generation,
            callback,
            state,
            target,
        });
        Ok(Resource::new_own(rep))
    }
}

/// Re-resolve work on the correct world owner; no cached snapshot authorizes a live effect.
pub(super) fn task_destination(
    router: &mut crate::server::worlds::DimensionRouter,
    target: &super::tasks::TaskTarget,
) -> Result<(i32, Option<Entity>), wit::AccessError> {
    match target {
        super::tasks::TaskTarget::World(reference) => {
            let world = router
                .world(reference.dimension)
                .ok_or(wit::AccessError::WorldClosed)?;
            reference
                .matches(world.ecs.world())
                .then_some((reference.dimension, None))
                .ok_or(wit::AccessError::WorldClosed)
        }
        super::tasks::TaskTarget::Player(reference) => {
            let mut matches = Vec::new();
            for (dimension, server) in router.iter_mut() {
                match resolve(server.ecs.world_mut(), reference) {
                    Ok(player) => matches.push((dimension, Some(player))),
                    Err(wit::AccessError::InvalidIdentity) => {
                        return Err(wit::AccessError::InvalidIdentity);
                    }
                    Err(_) => {}
                }
            }
            match matches.len() {
                0 => Err(wit::AccessError::StalePlayer),
                1 => Ok(matches[0]),
                _ => Err(wit::AccessError::InvalidIdentity),
            }
        }
    }
}

/// Capture an owned event identity while the exact native actor is still present.
pub(super) fn owned_identity(
    world: &mut World,
    player: Entity,
) -> Result<wit::PlayerIdentity, wit::AccessError> {
    ensure_identity(world, player).map(|reference| reference.identity)
}
/// Capture immutable facts without manufacturing values for a withheld native source.
pub(super) fn owned_snapshot(
    world: &mut World,
    player: Entity,
) -> Result<wit::PlayerSnapshot, wit::AccessError> {
    let reference = ensure_identity(world, player)?;
    snapshot(world, player, &reference)
}

/// Finalize component-owned admission acknowledgements at the central callback boundary.
pub(super) fn finish_admission_invocation(
    context: &mut HostContext,
    world: &mut World,
    succeeded: bool,
) {
    admission::finish(context, world, succeeded);
}

#[cfg(test)]
#[path = "players/tests.rs"]
mod tests;
