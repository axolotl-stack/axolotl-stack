//! Native authorization and committed notifications for held-item actions.
//!
//! Callbacks run from an ordered snapshot without borrowing the registry. The
//! actor and held stack are revalidated after all handlers and observers. Custom
//! handling asks the engine to consume a validated count; callbacks apply their
//! resulting effects from the committed notification rather than modifying the
//! inventory during authorization.

use crate::entity::components::{GameMode, Health, PlayerSession, PlayerUuid, Position};
use crate::item::ItemStack;
use bevy_ecs::prelude::*;
use std::sync::Arc;

/// The authoritative stage an extension is authorizing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemActionKind {
    Use,
    Release,
    Consume,
    Launch,
}

/// A custom action selected by an extension, with engine-owned item consumption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandledItemAction {
    /// Opaque bounded action identifier echoed in the committed notification.
    pub action: String,
    /// Number removed from the original held stack outside creative mode.
    pub consume: u8,
}

/// A proposed held-item action; actor, slot and stack are immutable.
#[derive(Event, Clone, Debug)]
pub struct PlayerItemAction {
    player: Entity,
    slot: u8,
    stack: ItemStack,
    kind: ItemActionKind,
    cancelled: bool,
    handled: Option<HandledItemAction>,
}
impl PlayerItemAction {
    /// Read the actor producing this action.
    pub fn player(&self) -> Entity {
        self.player
    }
    /// Read the original selected inventory slot.
    pub fn slot(&self) -> u8 {
        self.slot
    }
    /// Read the authoritative held stack before effects or costs.
    pub fn stack(&self) -> &ItemStack {
        &self.stack
    }
    /// Read the operation stage.
    pub fn kind(&self) -> ItemActionKind {
        self.kind
    }
    /// Reject ordinary and custom handling without consuming items.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    /// Whether an earlier callback rejected the action.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
    /// Replace ordinary behavior with a bounded engine-owned consumption.
    /// Conflicting handlers fail closed instead of replacing another handler.
    pub fn handle(&mut self, action: HandledItemAction) {
        if self
            .handled
            .as_ref()
            .is_some_and(|existing| existing != &action)
        {
            self.cancel();
        } else {
            self.handled = Some(action);
        }
    }
}

/// Ordinary or custom state that actually committed.
#[derive(Clone, Debug, PartialEq)]
pub enum ItemActionOutcome {
    /// Eating, drinking or drawing began; no consumable was used yet.
    Started,
    /// A consumable finished, with its actual removed held count.
    Consumed { count: u8 },
    /// An actual projectile was spawned and its item/ammunition cost committed.
    Launched { projectile: Entity },
    /// An extension's action consumed the requested validated count.
    Custom { action: String, count: u8 },
}

/// Immutable connection facts retained for accepted-action accounting.
#[derive(Clone, Debug)]
pub struct ItemActorIdentity {
    session_id: Option<u64>,
    xuid: Option<String>,
    uuid: Option<uuid::Uuid>,
}
impl ItemActorIdentity {
    /// Read the source transport identifier, when connected.
    pub fn session_id(&self) -> Option<u64> {
        self.session_id
    }
    /// Read the source account identifier supplied by its connection.
    pub fn xuid(&self) -> Option<&str> {
        self.xuid.as_deref()
    }
    /// Read the source actor's persistent UUID, when present.
    pub fn uuid(&self) -> Option<uuid::Uuid> {
        self.uuid
    }
}

/// A committed action, distinct from an attempt or an input packet.
#[derive(Event, Clone, Debug)]
pub struct PlayerItemActionCommitted {
    identity: ItemActorIdentity,
    pub player: Entity,
    pub slot: u8,
    pub stack: ItemStack,
    pub kind: ItemActionKind,
    pub outcome: ItemActionOutcome,
}

impl PlayerItemActionCommitted {
    /// Read immutable source facts even if a later callback removed its actor.
    pub fn identity(&self) -> &ItemActorIdentity {
        &self.identity
    }
}

/// Native item callbacks; extension state must not assume attempts committed.
pub trait ItemActionHandler: Send + Sync + 'static {
    /// Authorize, cancel or handle an action before its ordinary costs/effects.
    fn on_item_action(&self, _world: &mut World, _event: &mut PlayerItemAction) {}
    /// Observe ordinary/custom behavior only after its effects and costs commit.
    fn on_item_committed(&self, _world: &mut World, _event: &PlayerItemActionCommitted) {}
}

/// Ordered item handlers installed by an embedding application.
#[derive(Resource, Default, Clone)]
pub struct ItemActionHooks {
    handlers: Vec<Arc<dyn ItemActionHandler>>,
}
impl ItemActionHooks {
    /// Install a handler for subsequent snapshots.
    pub fn register(&mut self, handler: impl ItemActionHandler) {
        self.handlers.push(Arc::new(handler));
    }
}

/// Account and outbound transport facts retained across an extension callback.
type SessionSnapshot = (
    u64,
    Option<String>,
    Option<String>,
    tokio::sync::mpsc::Sender<jolyne::valentine::McpePacket>,
);

/// Snapshot identity without retaining a borrowed actor or registry.
struct ActorState {
    session: Option<SessionSnapshot>,
    uuid: Option<uuid::Uuid>,
    mode: Option<GameMode>,
    health: Option<(f32, f32)>,
    position: Option<glam::DVec3>,
}
impl ActorState {
    /// Retain the state used to authorize an item action.
    fn capture(world: &World, player: Entity) -> Self {
        Self {
            session: world.get::<PlayerSession>(player).map(|s| {
                (
                    s.session_id,
                    s.xuid.clone(),
                    s.uuid.clone(),
                    s.outbound_tx.clone(),
                )
            }),
            uuid: world.get::<PlayerUuid>(player).map(|id| id.0),
            mode: world.get::<GameMode>(player).copied(),
            health: world.get::<Health>(player).map(|h| (h.current, h.max)),
            position: world.get::<Position>(player).map(|p| p.0),
        }
    }
    /// Prevent stale item use after a callback changed the actor or connection.
    fn matches(&self, world: &World, player: Entity) -> bool {
        world.get_entity(player).is_ok()
            && world.get::<PlayerUuid>(player).map(|id| id.0) == self.uuid
            && world.get::<GameMode>(player).copied() == self.mode
            && world.get::<Health>(player).map(|h| (h.current, h.max)) == self.health
            && world.get::<Position>(player).map(|p| p.0) == self.position
            && match (&self.session, world.get::<PlayerSession>(player)) {
                (Some((id, xuid, uuid, sender)), Some(current)) => {
                    *id == current.session_id
                        && *xuid == current.xuid
                        && *uuid == current.uuid
                        && sender.same_channel(&current.outbound_tx)
                }
                (None, None) => true,
                _ => false,
            }
    }
}

/// A validated action retained until its actual committed outcome is known.
pub(crate) struct ItemPermit {
    event: PlayerItemAction,
    actor: ActorState,
}
impl ItemPermit {
    /// Check that spawning or other intermediate work did not change its source.
    pub(crate) fn is_current(&self, world: &World) -> bool {
        self.actor.matches(world, self.event.player)
            && super::item_use::held(world, self.event.player)
                .is_some_and(|(slot, stack)| slot == self.event.slot && stack == self.event.stack)
    }
    /// Notify callbacks after the ordinary action's effects and costs committed.
    pub(crate) fn commit(self, world: &mut World, outcome: ItemActionOutcome) {
        committed(
            world,
            PlayerItemActionCommitted {
                identity: ItemActorIdentity {
                    session_id: self.actor.session.as_ref().map(|s| s.0),
                    xuid: self.actor.session.as_ref().and_then(|s| s.1.clone()),
                    uuid: self.actor.uuid,
                },
                player: self.event.player,
                slot: self.event.slot,
                stack: self.event.stack,
                kind: self.event.kind,
                outcome,
            },
        );
    }
}

/// Snapshot the registry without borrowing it during callbacks or observers.
fn handlers(world: &World) -> Vec<Arc<dyn ItemActionHandler>> {
    world
        .get_resource::<ItemActionHooks>()
        .map_or_else(Vec::new, |hooks| hooks.handlers.clone())
}

/// Authorize one action; custom handling commits here and suppresses the default.
pub(crate) fn authorize(
    world: &mut World,
    player: Entity,
    kind: ItemActionKind,
) -> Option<ItemPermit> {
    let (slot, stack) = super::item_use::held(world, player)?;
    let actor = ActorState::capture(world, player);
    if actor.mode == Some(GameMode::Spectator)
        || actor
            .health
            .is_some_and(|(health, max)| !health.is_finite() || !max.is_finite() || health <= 0.0)
    {
        return None;
    }
    let mut event = PlayerItemAction {
        player,
        slot,
        stack,
        kind,
        cancelled: false,
        handled: None,
    };
    for handler in handlers(world) {
        handler.on_item_action(world, &mut event);
    }
    world.trigger_ref(&mut event);
    // Observer Commands must finish before stale-state revalidation and costs.
    world.flush();
    let permit = ItemPermit { event, actor };
    if permit.event.cancelled || !permit.is_current(world) {
        return None;
    }
    let Some(custom) = permit.event.handled.clone() else {
        return Some(permit);
    };
    if custom.action.trim().is_empty()
        || custom.action.len() > 128
        || custom.action.chars().any(char::is_control)
        || custom.consume > permit.event.stack.count
    {
        return None;
    }
    let count = if permit.actor.mode == Some(GameMode::Creative) {
        0
    } else {
        custom.consume
    };
    if count > 0 {
        super::item_use::set_slot(
            world,
            player,
            slot,
            permit.event.stack.grow(-i16::from(count)),
        );
    }
    permit.commit(
        world,
        ItemActionOutcome::Custom {
            action: custom.action,
            count,
        },
    );
    None
}

/// Emit a committed event only after the caller's actual successful mutation.
fn committed(world: &mut World, event: PlayerItemActionCommitted) {
    for handler in handlers(world) {
        handler.on_item_committed(world, &event);
    }
    world.trigger(event);
}

#[cfg(test)]
mod tests;
