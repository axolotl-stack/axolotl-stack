//! Final destination guards run after native destination rewrites and observers.

use bevy_ecs::prelude::*;
use glam::DVec3;
use std::sync::{Arc, Weak};

use crate::entity::projectile::hooks::PlayerTeleport;
use crate::server::worlds::PlayerWorldTransfer;

/// An exact approval belongs to the component instance that actually made it.
#[derive(Clone, Debug)]
pub(crate) struct Approval {
    owner: Weak<()>,
    feet: DVec3,
}
impl Approval {
    /// Retain no live scope or ownership that could keep an unloaded instance authorized.
    pub(crate) fn new(owner: Weak<()>, feet: DVec3) -> Self {
        Self { owner, feet }
    }
    /// Later rewrites and component removal invalidate the original approval.
    pub(crate) fn matches(&self, feet: DVec3) -> bool {
        self.owner.upgrade().is_some() && self.feet == feet
    }
}

/// Internal runtime glue, separate from supported component-facing contracts.
pub(crate) trait Authorizer: Send + Sync + 'static {
    /// Validate the actual chosen same-world feet before movement or durable relocation.
    fn teleport(&self, _world: &mut World, _event: &mut PlayerTeleport) {}
    /// Validate the actual chosen cross-world feet before persistence or ownership movement.
    fn transfer(&self, _source: &mut World, _target: &mut World, _event: &mut PlayerWorldTransfer) {
    }
}

#[derive(Resource, Clone, Default)]
pub(crate) struct Authorizers(Vec<Arc<dyn Authorizer>>);
impl Authorizers {
    /// Register the runtime's final guard without exposing a second native extension tier.
    pub(crate) fn register(&mut self, authorizer: impl Authorizer) {
        self.0.push(Arc::new(authorizer));
    }
}

/// Snapshot guards so no registry borrow survives a callback.
fn handlers(world: &World) -> Vec<Arc<dyn Authorizer>> {
    world
        .get_resource::<Authorizers>()
        .map_or_else(Vec::new, |guards| guards.0.clone())
}

/// Authorize only an otherwise accepted final native proposal.
pub(crate) fn teleport(world: &mut World, event: &mut PlayerTeleport) {
    for handler in handlers(world) {
        if event.is_cancelled() {
            break;
        }
        handler.teleport(world, event);
    }
}

/// Keep both worlds on their actual owner while the guest sees only the source transaction.
pub(crate) fn transfer(source: &mut World, target: &mut World, event: &mut PlayerWorldTransfer) {
    for handler in handlers(source) {
        if event.is_cancelled() {
            break;
        }
        handler.transfer(source, target, event);
    }
}
