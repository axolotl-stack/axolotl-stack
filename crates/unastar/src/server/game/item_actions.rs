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
    /// Loading ammunition into a weapon.
    Charge,
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
    target: ItemTarget,
    action_id: u64,
    owners: Vec<std::sync::Weak<()>>,
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
    /// Read actual native target facts, never a client-selected item declaration.
    pub(crate) fn target(&self) -> &ItemTarget {
        &self.target
    }
    /// Correlate this physical action within its world lifetime only.
    pub(crate) fn action_id(&self) -> u64 {
        self.action_id
    }
    /// Keep queued native work bound to every approving component instance.
    pub(crate) fn require_owner(&mut self, owner: std::sync::Weak<()>) {
        self.owners.push(owner);
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
    /// A crossbow actually stored its ammunition.
    Charged,
    /// A native actor interaction actually changed state.
    Interacted,
    /// An authorized block operation actually completed.
    Block { operation_id: u64 },
}

/// Immutable connection facts retained for accepted-action accounting.
#[derive(Clone, Debug)]
pub struct ItemActorIdentity {
    name: Option<String>,
    session_id: Option<u64>,
    xuid: Option<String>,
    uuid: Option<uuid::Uuid>,
}
impl ItemActorIdentity {
    /// Read the original connection name without resolving a replacement actor.
    pub(crate) fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
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
    pub(crate) action_id: u64,
    pub(crate) dimension: Option<i32>,
    pub(crate) target: ItemTarget,
    pub(crate) after: Option<ItemStack>,
    pub(crate) ammo: Option<AmmoSnapshot>,
    pub(crate) ammo_after: Option<ItemStack>,
    pub(crate) projectile: Option<(String, glam::DVec3)>,
    pub(crate) inline: bool,
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
    world: bevy_ecs::world::WorldId,
    dimension: Option<i32>,
    runtime: Option<i64>,
    mob_runtime: Option<i64>,
    is_player: bool,
    mob_identifier: Option<String>,
    name: Option<String>,
    dead: bool,
}
impl ActorState {
    /// Retain the state used to authorize an item action.
    fn capture(world: &World, player: Entity) -> Self {
        Self {
            is_player: world
                .get::<crate::entity::components::Player>(player)
                .is_some(),
            mob_runtime: world
                .get::<crate::entity::components::RuntimeId>(player)
                .map(|id| id.0),
            mob_identifier: world
                .get::<crate::entity::mob::MobKind>(player)
                .map(|kind| kind.0.identifier.to_string()),
            dead: world
                .get::<crate::entity::mob::state::Dead>(player)
                .is_some(),
            world: world.id(),
            dimension: world
                .get_resource::<crate::world::ChunkManager>()
                .map(|m| m.dimension()),
            runtime: world
                .get::<crate::entity::components::RuntimeEntityId>(player)
                .map(|id| id.0),
            name: world
                .get::<PlayerSession>(player)
                .map(|s| s.display_name.clone()),
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
    /// Compare identity independently of legitimate food, movement and inventory effects.
    fn same_owner(&self, world: &World, player: Entity) -> bool {
        self.world == world.id()
            && self.is_player
                == world
                    .get::<crate::entity::components::Player>(player)
                    .is_some()
            && self.mob_runtime
                == world
                    .get::<crate::entity::components::RuntimeId>(player)
                    .map(|id| id.0)
            && self.dimension
                == world
                    .get_resource::<crate::world::ChunkManager>()
                    .map(|m| m.dimension())
            && self.runtime
                == world
                    .get::<crate::entity::components::RuntimeEntityId>(player)
                    .map(|id| id.0)
            && self.name
                == world
                    .get::<PlayerSession>(player)
                    .map(|s| s.display_name.clone())
            && world.get_entity(player).is_ok()
            && world.get::<PlayerUuid>(player).map(|id| id.0) == self.uuid
            && match (&self.session, world.get::<PlayerSession>(player)) {
                (Some((id, xuid, uuid, sender)), Some(now)) => {
                    *id == now.session_id
                        && *xuid == now.xuid
                        && *uuid == now.uuid
                        && sender.same_channel(&now.outbound_tx)
                }
                (None, None) => true,
                _ => false,
            }
    }
    /// Prevent stale item use after a callback changed the actor or connection.
    fn matches(&self, world: &World, player: Entity) -> bool {
        self.same_owner(world, player)
            && self.mob_identifier
                == world
                    .get::<crate::entity::mob::MobKind>(player)
                    .map(|kind| kind.0.identifier.to_string())
            && self.dead
                == world
                    .get::<crate::entity::mob::state::Dead>(player)
                    .is_some()
            && world.get_entity(player).is_ok()
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
    ammo: Option<AmmoSnapshot>,
    target_state: Option<ActorState>,
}
impl ItemPermit {
    /// Check that spawning or other intermediate work did not change its source.
    pub(crate) fn is_current(&self, world: &World) -> bool {
        self.event
            .owners
            .iter()
            .all(|owner| owner.upgrade().is_some())
            && self.actor.matches(world, self.event.player)
            && selected(world, self.event.player)
                .is_some_and(|(slot, stack)| slot == self.event.slot && stack == self.event.stack)
            && self
                .ammo
                .as_ref()
                .is_none_or(|ammo| ammo.current(world, self.event.player))
            && match (&self.event.target, &self.target_state) {
                (ItemTarget::Actor(target), Some(state)) => state.matches(world, target.entity),
                (ItemTarget::Block(click), _) => {
                    super::blocks::block_at(world, click.position.into()) == click.state
                }
                _ => true,
            }
    }
    /// Notify callbacks after the ordinary action's effects and costs committed.
    pub(crate) fn commit(self, world: &mut World, outcome: ItemActionOutcome) {
        let event = self.into_event(world, outcome);
        committed(world, event);
    }
    /// Capture owned post-action facts before any completion observer can mutate them.
    fn into_event(self, world: &World, outcome: ItemActionOutcome) -> PlayerItemActionCommitted {
        let after = self
            .actor
            .same_owner(world, self.event.player)
            .then(|| selected_slot(world, self.event.player, self.event.slot))
            .flatten();
        let ammo_after = self
            .ammo
            .as_ref()
            .filter(|_| self.actor.same_owner(world, self.event.player))
            .and_then(|ammo| ammo.stack_at(world, self.event.player).cloned());
        let projectile = match &outcome {
            ItemActionOutcome::Launched { projectile } => world
                .get::<crate::entity::mob::MobKind>(*projectile)
                .zip(world.get::<Position>(*projectile))
                .map(|(kind, pos)| (kind.0.identifier.to_string(), pos.0)),
            _ => None,
        };
        PlayerItemActionCommitted {
            action_id: self.event.action_id,
            dimension: self.actor.dimension,
            target: self.event.target,
            after,
            ammo: self.ammo,
            ammo_after,
            projectile,
            inline: false,
            identity: ItemActorIdentity {
                name: self.actor.name,
                session_id: self.actor.session.as_ref().map(|s| s.0),
                xuid: self.actor.session.as_ref().and_then(|s| s.1.clone()),
                uuid: self.actor.uuid,
            },
            player: self.event.player,
            slot: self.event.slot,
            stack: self.event.stack,
            kind: self.event.kind,
            outcome,
        }
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
    authorize_target(world, player, kind, ItemTarget::Air, None)
}

/// Authorize immutable actual target and cost witnesses before ordinary native effects.
pub(crate) fn authorize_target(
    world: &mut World,
    player: Entity,
    kind: ItemActionKind,
    target: ItemTarget,
    ammo: Option<AmmoSnapshot>,
) -> Option<ItemPermit> {
    let (slot, stack) = selected(world, player)?;
    let target_state = match target {
        ItemTarget::Actor(ref actor) => Some(ActorState::capture(world, actor.entity)),
        _ => None,
    };
    let sequence = &mut world.get_resource_or_init::<ActionSequence>().0;
    *sequence = sequence.checked_add(1)?;
    let action_id = *sequence;
    let actor = ActorState::capture(world, player);
    if actor.dead
        || actor.mode == Some(GameMode::Spectator)
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
        target,
        action_id,
        owners: Vec::new(),
        cancelled: false,
        handled: None,
    };
    for handler in handlers(world) {
        handler.on_item_action(world, &mut event);
    }
    world.trigger_ref(&mut event);
    // Observer Commands must finish before stale-state revalidation and costs.
    world.flush();
    let permit = ItemPermit {
        event,
        actor,
        ammo,
        target_state,
    };
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
    if matches!(kind, ItemActionKind::Consume | ItemActionKind::Release) {
        super::item_use::stop_using(world, player);
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

/// A native target observed before any default action; packet actor IDs never cross the API.
#[derive(Clone, Debug)]
pub(crate) enum ItemTarget {
    Air,
    Block(ItemBlockTarget),
    Actor(ItemActorTarget),
}
/// Owned target observations survive removal after actual native effects.
#[derive(Clone, Debug)]
pub(crate) struct ItemActorTarget {
    pub entity: Entity,
    pub identifier: String,
    pub feet: glam::DVec3,
    pub identity: Option<ItemActorIdentity>,
}
impl ItemActorTarget {
    /// Capture only an actual unique resolved live actor; no departed owner becomes a player.
    pub(crate) fn capture(world: &World, entity: Entity) -> Option<Self> {
        let position = world.get::<Position>(entity)?.0;
        let player = world
            .get::<crate::entity::components::Player>(entity)
            .is_some();
        let identifier = if player {
            "minecraft:player".to_owned()
        } else {
            world
                .get::<crate::entity::mob::MobKind>(entity)?
                .0
                .identifier
                .to_string()
        };
        let feet = if player {
            position - glam::DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT
        } else {
            position
        };
        if !feet.is_finite() {
            return None;
        }
        let state = ActorState::capture(world, entity);
        let identity = player.then(|| ItemActorIdentity {
            name: state.name,
            session_id: state.session.as_ref().map(|s| s.0),
            xuid: state.session.as_ref().and_then(|s| s.1.clone()),
            uuid: state.uuid,
        });
        Some(Self {
            entity,
            identifier,
            feet,
            identity,
        })
    }
}
/// A loaded clicked block and its actual native hit facts.
#[derive(Clone, Debug)]
pub(crate) struct ItemBlockTarget {
    pub position: glam::IVec3,
    pub face: u8,
    pub offset: Option<glam::DVec3>,
    pub state: Option<u32>,
}
#[derive(Resource, Default)]
struct ActionSequence(u64);
/// One known ammunition cost cell. All fields are authoritative server values.
#[derive(Clone, Debug)]
pub(crate) struct AmmoSnapshot {
    pub slot: Option<usize>,
    pub before: ItemStack,
}
impl AmmoSnapshot {
    /// Capture a main-inventory or offhand ammunition cell before callbacks.
    pub(crate) fn capture(world: &World, player: Entity, slot: Option<usize>) -> Option<Self> {
        let before = match slot {
            Some(slot) => world
                .get::<crate::entity::components::MainInventory>(player)?
                .0
                .item(slot)?,
            None => {
                &world
                    .get::<crate::entity::components::OffhandSlot>(player)?
                    .0
            }
        }
        .clone();
        Some(Self { slot, before })
    }
    /// Read the same cell, without selecting substitute ammunition after authorization.
    fn stack_at<'a>(&self, world: &'a World, player: Entity) -> Option<&'a ItemStack> {
        match self.slot {
            Some(slot) => world
                .get::<crate::entity::components::MainInventory>(player)?
                .0
                .item(slot),
            None => Some(
                &world
                    .get::<crate::entity::components::OffhandSlot>(player)?
                    .0,
            ),
        }
    }
    /// A full stack fingerprint includes auxiliary values and owned user data.
    fn current(&self, world: &World, player: Entity) -> bool {
        self.stack_at(world, player) == Some(&self.before)
    }
}
/// Capture an actual cell even when the hand is empty.
fn selected_slot(world: &World, player: Entity, slot: u8) -> Option<ItemStack> {
    world
        .get::<crate::entity::components::MainInventory>(player)?
        .0
        .item(usize::from(slot))
        .cloned()
}
/// Do not invent a stack when the native inventory is withheld or missing.
pub(crate) fn selected(world: &World, player: Entity) -> Option<(u8, ItemStack)> {
    let slot = world.get::<crate::entity::components::HeldSlot>(player)?.0;
    Some((slot, selected_slot(world, player, slot)?))
}

/// Transient block correlations live only through their native queued batch.
#[derive(Resource, Default)]
struct PendingBlocks {
    active: Option<u64>,
    entries: std::collections::BTreeMap<u64, (ItemPermit, bool)>,
}
/// Bound retained stack bytes and count independently of guest resource quotas.
pub(crate) fn begin_block(world: &mut World, permit: ItemPermit) -> Option<u64> {
    let id = permit.event.action_id;
    let bytes = permit.event.stack.nbt.as_ref().map_or(0, |nbt| nbt.len());
    let mut pending = world.get_resource_or_init::<PendingBlocks>();
    if pending.entries.len() >= 128
        || bytes > 1024 * 1024
        || pending
            .entries
            .values()
            .map(|(p, _)| p.event.stack.nbt.as_ref().map_or(0, |nbt| nbt.len()))
            .sum::<usize>()
            .saturating_add(bytes)
            > 16 * 1024 * 1024
    {
        return None;
    }
    pending.entries.insert(id, (permit, false));
    pending.active = Some(id);
    Some(id)
}
/// Trusted synchronous default actions inherit the current raw click correlation.
pub(crate) fn active_block(world: &World) -> Option<u64> {
    world.get_resource::<PendingBlocks>().and_then(|p| p.active)
}
/// Retain only a proposal actually handed to the native queued-placement path.
pub(crate) fn queue_block(world: &mut World, id: Option<u64>) {
    if let Some(id) = id
        && let Some(mut p) = world.get_resource_mut::<PendingBlocks>()
        && let Some((_, queued)) = p.entries.get_mut(&id)
    {
        *queued = true;
    }
}
/// Inert/default-veto paths discard their correlation without any completion.
pub(crate) fn end_block(world: &mut World, id: u64) {
    if let Some(mut p) = world.get_resource_mut::<PendingBlocks>() {
        p.active = None;
        if p.entries.get(&id).is_some_and(|(_, queued)| !queued) {
            p.entries.remove(&id);
        }
    }
}
/// Remove one completed or rejected queued action without affecting later proposals.
pub(crate) fn discard_block(world: &mut World, id: Option<u64>) {
    if let Some(id) = id
        && let Some(mut pending) = world.get_resource_mut::<PendingBlocks>()
    {
        pending.entries.remove(&id);
    }
}
/// Finalize every queued batch, including required traps and rejected scalar compatibility events.
pub(crate) fn clear_blocks(world: &mut World) {
    if let Some(mut pending) = world.get_resource_mut::<PendingBlocks>() {
        pending.entries.clear();
        pending.active = None;
    }
}
/// Source closure cannot retain snapshots or grant a replacement connection permission.
pub(crate) fn discard_player(world: &mut World, player: Entity) {
    if let Some(mut pending) = world.get_resource_mut::<PendingBlocks>() {
        pending
            .entries
            .retain(|_, (permit, _)| permit.event.player != player);
    }
}
/// Recheck identity, selected cell and clicked before-state immediately before the actual block plan.
pub(crate) fn block_current(world: &World, player: Entity, id: Option<u64>) -> bool {
    id.is_none_or(|id| {
        world
            .get_resource::<PendingBlocks>()
            .and_then(|p| p.entries.get(&id))
            .is_some_and(|(permit, _)| {
                permit.event.player == player
                    && permit
                        .event
                        .owners
                        .iter()
                        .all(|owner| owner.upgrade().is_some())
                    && permit.actor.same_owner(world, player)
                    && permit.actor.mode == world.get::<GameMode>(player).copied()
                    && world.get::<Health>(player).is_none_or(|health| {
                        health.current.is_finite() && health.max.is_finite() && health.current > 0.0
                    })
                    && selected(world, player).is_some_and(|(slot, stack)| {
                        slot == permit.event.slot && stack == permit.event.stack
                    })
                    && match &permit.event.target {
                        ItemTarget::Block(block) => {
                            super::blocks::block_at(world, block.position.into()) == block.state
                        }
                        _ => false,
                    }
            })
    })
}
/// Capture the real correlated physical completion before any observer changes the resulting cells.
pub(crate) fn take_block_completion(
    world: &mut World,
    event: &super::block_interaction::PlayerBlockCommitted,
) -> Option<PlayerItemActionCommitted> {
    let id = event.click()?.item_action?;
    let (permit, _) = world
        .get_resource_mut::<PendingBlocks>()?
        .entries
        .remove(&id)?;
    let mut completed = permit.into_event(
        world,
        ItemActionOutcome::Block {
            operation_id: event.operation_id(),
        },
    );
    completed.inline = event.inline();
    Some(completed)
}
/// Publish through native listeners; manager-owned callers provide their own dispatcher separately.
pub(crate) fn publish_completion(world: &mut World, event: PlayerItemActionCommitted) {
    committed(world, event);
}

#[cfg(test)]
mod tests;
