//! Native authorization and notifications for complete container slot changes.
//!
//! Proposal handlers run from an ordered snapshot, followed by Bevy observers,
//! without holding any world component or callback registry borrowed. A veto is
//! sticky. The engine revalidates all source state after callbacks and commits
//! player inventories, request assignments, automation and drops only on success.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::player_events::ConnectionIdentity;
use crate::block_entity::ContainerKind;
use crate::item::ItemStack;

/// The authoritative producer of a proposed container change.
#[derive(Clone, Debug)]
pub enum ContainerMutationCause {
    Player {
        player: Entity,
        identity: ConnectionIdentity,
    },
    Hopper {
        position: IVec3,
        item_entity: Option<Entity>,
    },
    Furnace {
        position: IVec3,
    },
}

/// One complete logical inventory, with canonical physical positions in slot
/// order. A reciprocal double chest lists lead then partner and includes all 54
/// slots. Unchanged participating containers may be included in automation.
#[derive(Clone, Debug, PartialEq)]
pub struct ContainerInventoryChange {
    pub positions: Vec<IVec3>,
    pub kind: ContainerKind,
    pub before: Vec<ItemStack>,
    pub after: Vec<ItemStack>,
}

/// A fully validated proposed slot change whose ordinary costs have not run.
/// Immutable full snapshots allow a handler to distinguish extraction, inserts,
/// replacements and internal reordering without trusting client action labels.
#[derive(Event, Clone, Debug)]
pub struct ContainerMutation {
    cause: ContainerMutationCause,
    changes: Vec<ContainerInventoryChange>,
    cancelled: bool,
}
impl ContainerMutation {
    /// Read the original authenticated player or automation producer.
    pub fn cause(&self) -> &ContainerMutationCause {
        &self.cause
    }
    /// Read every complete authoritative before/after inventory.
    pub fn changes(&self) -> &[ContainerInventoryChange] {
        &self.changes
    }
    /// Reject all ordinary costs and changes of this operation.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    /// Whether any earlier callback or observer vetoed this operation.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// An accepted operation after all ordinary inventory, cache and drop changes.
/// It cannot cancel a completed mutation, and owns its exact accepted slot data.
#[derive(Event, Clone, Debug)]
pub struct ContainerMutationCommitted {
    cause: ContainerMutationCause,
    changes: Vec<ContainerInventoryChange>,
}
impl ContainerMutationCommitted {
    /// Read the original producer, independent of later live component changes.
    pub fn cause(&self) -> &ContainerMutationCause {
        &self.cause
    }
    /// Read the complete owned inventories accepted by the engine.
    pub fn changes(&self) -> &[ContainerInventoryChange] {
        &self.changes
    }
}

/// Reentrant native callbacks for an embedding application's container policy.
pub trait ContainerMutationHandler: Send + Sync + 'static {
    /// Authorize a complete proposal before any ordinary operation costs.
    fn on_mutation(&self, _world: &mut World, _event: &mut ContainerMutation) {}
    /// Observe only an actually accepted and committed operation.
    fn on_committed(&self, _world: &mut World, _event: &ContainerMutationCommitted) {}
}

/// Ordered container policy callbacks; registry changes affect the next dispatch.
#[derive(Resource, Default, Clone)]
pub struct ContainerMutationHooks {
    handlers: Vec<Arc<dyn ContainerMutationHandler>>,
}
impl ContainerMutationHooks {
    /// Register a handler without coupling the engine to application policy.
    pub fn register(&mut self, handler: impl ContainerMutationHandler) {
        self.handlers.push(Arc::new(handler));
    }
}

/// Dispatch a proposal without borrowing the registry during callbacks.
pub(super) fn authorize(
    world: &mut World,
    cause: ContainerMutationCause,
    changes: Vec<ContainerInventoryChange>,
) -> Option<ContainerMutationCommitted> {
    let handlers = handlers(world);
    let mut event = ContainerMutation {
        cause,
        changes,
        cancelled: false,
    };
    for handler in handlers {
        handler.on_mutation(world, &mut event);
    }
    world.trigger_ref(&mut event);
    (!event.cancelled).then_some(ContainerMutationCommitted {
        cause: event.cause,
        changes: event.changes,
    })
}

/// Notify after engine state, request assignments and world drops have committed.
pub(super) fn committed(world: &mut World, mut event: ContainerMutationCommitted) {
    for handler in handlers(world) {
        handler.on_committed(world, &event);
    }
    world.trigger_ref(&mut event);
}

/// Snapshot callbacks once per dispatch, including nested dispatches.
fn handlers(world: &World) -> Vec<Arc<dyn ContainerMutationHandler>> {
    world
        .get_resource::<ContainerMutationHooks>()
        .map_or_else(Vec::new, |hooks| hooks.handlers.clone())
}
