//! Synchronous authorization for player block operations before side effects.
//!
//! Register native handlers in [`BlockInteractionHooks`] or observe
//! [`BlockInteraction`] with Bevy. Handlers run in registration order from a
//! snapshot, followed by observers. No registry borrow is held during callbacks.
//! Registry changes affect the next dispatch, and cancellation cannot be undone.
//! Callbacks may access the world; the operation revalidates its inputs afterward.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::IVec3;

/// The kind of proposed operation, independent of its packet representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockInteractionKind {
    Break,
    Place,
    Use,
}

/// A block operation whose mutations, item costs and normal effects have not run.
///
/// Positions include every block affected by the operation, including upper
/// halves and adjacent targets. Use operations also include the clicked block.
/// Player, kind and positions are immutable; extensions authorize or cancel the
/// actual operation rather than redirecting it to unvalidated targets.
#[derive(Event, Clone, Debug)]
pub struct BlockInteraction {
    player: Entity,
    kind: BlockInteractionKind,
    positions: Vec<IVec3>,
    cancelled: bool,
}

impl BlockInteraction {
    /// Identify the player attempting this operation.
    pub fn player(&self) -> Entity {
        self.player
    }

    /// Read the semantic operation being authorized.
    pub fn kind(&self) -> BlockInteractionKind {
        self.kind
    }

    /// Read the exact affected block coordinates in this world.
    pub fn positions(&self) -> &[IVec3] {
        &self.positions
    }

    /// Prevent the operation and its normal mutations and item costs.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Whether an earlier handler or observer has vetoed this operation.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Native synchronous callbacks with no engine lock or borrowed registry held.
///
/// Callbacks may cause nested operations. Handlers with internal locks must allow
/// reentrant calls or avoid initiating nested operations while holding a lock.
pub trait BlockInteractionHandler: Send + Sync + 'static {
    /// Inspect or cancel one proposed block operation before any ordinary effects.
    fn on_interaction(&self, _world: &mut World, _event: &mut BlockInteraction) {}
}

/// Ordered native block authorization callbacks for an embedding application.
#[derive(Resource, Default, Clone)]
pub struct BlockInteractionHooks {
    handlers: Vec<Arc<dyn BlockInteractionHandler>>,
}

impl BlockInteractionHooks {
    /// Add a handler to subsequent dispatch snapshots.
    pub fn register(&mut self, handler: impl BlockInteractionHandler) {
        self.handlers.push(Arc::new(handler));
    }
}

/// Dispatch a decision without holding the callback registry resource borrowed.
pub(crate) fn authorize(
    world: &mut World,
    player: Entity,
    kind: BlockInteractionKind,
    positions: Vec<IVec3>,
) -> bool {
    let handlers = world
        .get_resource::<BlockInteractionHooks>()
        .map_or_else(Vec::new, |hooks| hooks.handlers.clone());
    let mut event = BlockInteraction {
        player,
        kind,
        positions,
        cancelled: false,
    };
    for handler in handlers {
        handler.on_interaction(world, &mut event);
    }
    world.trigger_ref(&mut event);
    !event.is_cancelled()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Record callback order while trying to add another handler during dispatch.
    struct RegisterDuringDispatch(Arc<Mutex<Vec<u8>>>);

    impl BlockInteractionHandler for RegisterDuringDispatch {
        fn on_interaction(&self, world: &mut World, event: &mut BlockInteraction) {
            self.0.lock().unwrap().push(1);
            world
                .resource_mut::<BlockInteractionHooks>()
                .register(Observe(self.0.clone()));
            event.cancel();
        }
    }

    /// Record a later callback and verify cancellation remains visible.
    struct Observe(Arc<Mutex<Vec<u8>>>);

    impl BlockInteractionHandler for Observe {
        fn on_interaction(&self, _world: &mut World, event: &mut BlockInteraction) {
            assert!(event.is_cancelled());
            self.0.lock().unwrap().push(2);
        }
    }

    #[test]
    fn callback_snapshot_allows_registry_mutation_and_sticky_cancellation() {
        let mut world = World::new();
        world.init_resource::<BlockInteractionHooks>();
        let calls = Arc::new(Mutex::new(Vec::new()));
        world
            .resource_mut::<BlockInteractionHooks>()
            .register(RegisterDuringDispatch(calls.clone()));
        let player = world.spawn_empty().id();
        assert!(!authorize(
            &mut world,
            player,
            BlockInteractionKind::Use,
            vec![IVec3::ZERO]
        ));
        assert_eq!(*calls.lock().unwrap(), [1]);
        assert!(!authorize(
            &mut world,
            player,
            BlockInteractionKind::Use,
            vec![IVec3::ZERO]
        ));
        assert_eq!(*calls.lock().unwrap(), [1, 1, 2]);
    }
}
