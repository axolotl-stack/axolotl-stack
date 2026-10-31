//! Native connection, movement and chat events at their authoritative boundaries.
//!
//! Ordered handler snapshots run before Bevy observers, without borrowing the
//! registry during callbacks. Proposed movement and chat are cancellable;
//! committed notifications are separate. Callbacks may access the world, so
//! operations revalidate connection and subject state before applying effects.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;

use crate::entity::components::{PlayerSession, PlayerUuid};

/// Immutable identity of the connection that produced an event.
#[derive(Clone, Debug)]
pub struct ConnectionIdentity {
    session_id: u64,
    name: String,
    xuid: Option<String>,
    uuid: Option<String>,
    player_uuid: Option<uuid::Uuid>,
    outbound: tokio::sync::mpsc::Sender<jolyne::valentine::McpePacket>,
}

impl ConnectionIdentity {
    /// Read the transport session identifier.
    pub fn session_id(&self) -> u64 {
        self.session_id
    }
    /// Read the authenticated connection's supplied display name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Read its account identifier, when authentication supplied one.
    pub fn xuid(&self) -> Option<&str> {
        self.xuid.as_deref()
    }
    /// Read its protocol UUID, when supplied.
    pub fn uuid(&self) -> Option<&str> {
        self.uuid.as_deref()
    }
    /// Snapshot identity without retaining an ECS component borrow.
    pub(crate) fn capture(world: &World, player: Entity) -> Option<Self> {
        let session = world.get::<PlayerSession>(player)?;
        Some(Self {
            session_id: session.session_id,
            name: session.display_name.clone(),
            xuid: session.xuid.clone(),
            uuid: session.uuid.clone(),
            player_uuid: world.get::<PlayerUuid>(player).map(|id| id.0),
            outbound: session.outbound_tx.clone(),
        })
    }
    /// Require the same account and transport after extension callbacks.
    pub(crate) fn matches(&self, world: &World, player: Entity) -> bool {
        world.get::<PlayerSession>(player).is_some_and(|session| {
            session.session_id == self.session_id
                && session.display_name == self.name
                && session.xuid == self.xuid
                && session.uuid == self.uuid
                && session.outbound_tx.same_channel(&self.outbound)
                && world.get::<PlayerUuid>(player).map(|id| id.0) == self.player_uuid
        })
    }
}

/// A player whose connection joined; its live entity is fully initialized
/// before initial packets are produced.
#[derive(Event, Clone, Debug)]
pub struct PlayerJoined {
    player: Entity,
    identity: ConnectionIdentity,
}
impl PlayerJoined {
    /// Read the initialized player entity.
    pub fn player(&self) -> Entity {
        self.player
    }
    /// Read the immutable connection identity.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
}

/// The engine's entity disposition after its network session is removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DisconnectDisposition {
    /// Remove the ordinary player entity.
    #[default]
    Despawn,
    /// Keep an attackable actor but detach all network and input ownership.
    RetainEntity,
}

/// A departing connection, before its player entity is removed or detached.
#[derive(Event, Clone, Debug)]
pub struct PlayerQuit {
    player: Entity,
    identity: ConnectionIdentity,
    disposition: DisconnectDisposition,
}
impl PlayerQuit {
    /// Read the departing entity while its authoritative state is still present.
    pub fn player(&self) -> Entity {
        self.player
    }
    /// Read the departing connection's immutable identity.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
    /// Keep this entity as a sessionless actor managed by the embedding application.
    pub fn retain_entity(&mut self) {
        self.disposition = DisconnectDisposition::RetainEntity;
    }
    /// Read the current entity disposition.
    pub fn disposition(&self) -> DisconnectDisposition {
        self.disposition
    }
}

/// A retained player actor with no connected session or client input ownership.
/// Embedding applications own its lifetime and persistence after detachment.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct DetachedPlayer;

/// An authoritative movement proposal, before position, input or food changes.
#[derive(Event, Clone, Debug)]
pub struct PlayerMove {
    player: Entity,
    identity: ConnectionIdentity,
    from: DVec3,
    /// Proposed destination, in player eye coordinates. Redirected positions
    /// must remain finite, network-representable and within the normal move bound.
    pub destination: DVec3,
    cancelled: bool,
}
impl PlayerMove {
    /// Read the player that sent this input.
    pub fn player(&self) -> Entity {
        self.player
    }
    /// Read its immutable connection identity.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
    /// Read the authoritative position before this input.
    pub fn from(&self) -> DVec3 {
        self.from
    }
    /// Reject this input and send authoritative position correction.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    /// Whether a preceding handler or observer rejected the proposal.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
    /// Construct a proposal after packet validation.
    pub(crate) fn new(
        player: Entity,
        identity: ConnectionIdentity,
        from: DVec3,
        destination: DVec3,
    ) -> Self {
        Self {
            player,
            identity,
            from,
            destination,
            cancelled: false,
        }
    }
}

/// A movement that actually committed, after all proposal handlers and observers.
#[derive(Event, Clone, Debug)]
pub struct PlayerMoved {
    pub player: Entity,
    pub from: DVec3,
    pub to: DVec3,
    identity: ConnectionIdentity,
}
impl PlayerMoved {
    /// Read the player that committed this movement.
    pub fn player(&self) -> Entity {
        self.player
    }
    /// Read the connection that committed this movement.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
}

/// Chat about to enter the server's final broadcast, after plugin decisions.
#[derive(Event, Clone, Debug)]
pub struct PlayerChat {
    player: Entity,
    identity: ConnectionIdentity,
    /// Presentation name; editing it does not change the account identity.
    pub display_name: String,
    /// The broadcast text; the engine validates its size and control characters.
    pub message: String,
    cancelled: bool,
    handled: Option<String>,
}
impl PlayerChat {
    /// Read the player that sent this message.
    pub fn player(&self) -> Entity {
        self.player
    }
    /// Read the actual sending connection independent of presentation text.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
    /// Stop default chat delivery.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    /// Replace ordinary broadcast with an extension-owned accepted action.
    /// A later cancellation still suppresses acceptance; conflicting handlers
    /// fail closed instead of replacing another handler's action.
    pub fn handle(&mut self, action: String) {
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
    /// Read the extension action selected instead of ordinary broadcast.
    pub fn handled(&self) -> Option<&str> {
        self.handled.as_deref()
    }
    /// Whether default delivery was rejected.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Chat accepted after every proposal handler/observer and identity validation.
/// Ordinary messages are notified after default broadcast. A handled message
/// skips default broadcast and publishes its bounded extension action instead.
#[derive(Event, Clone, Debug)]
pub struct PlayerChatted {
    player: Entity,
    identity: ConnectionIdentity,
    pub display_name: String,
    pub message: String,
    pub handled: Option<String>,
}
impl PlayerChatted {
    /// Read the account whose message was accepted.
    pub fn player(&self) -> Entity {
        self.player
    }
    /// Read its immutable authenticated connection identity.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
}

/// An immutable form response from a still-authenticated live connection.
/// The embedding application owns form identifiers and response interpretation.
#[derive(Event, Clone, Debug)]
pub struct PlayerFormResponse {
    player: Entity,
    identity: ConnectionIdentity,
    pub form_id: u32,
    /// Optional raw response text, bounded to 65536 bytes; the host does not
    /// interpret application JSON or associate it with an outstanding request.
    pub data: Option<String>,
    /// A cancellation reason was present or no response payload was supplied.
    pub cancelled: bool,
}
impl PlayerFormResponse {
    /// Read the responding player.
    pub fn player(&self) -> Entity {
        self.player
    }
    /// Read the immutable authenticated source identity.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
}

/// Synchronous native callbacks; no registry borrow is held while they run.
pub trait PlayerEventHandler: Send + Sync + 'static {
    /// Observe a fully initialized connected player.
    fn on_joined(&self, _world: &mut World, _event: &PlayerJoined) {}
    /// Observe departure and optionally retain a sessionless actor.
    fn on_quit(&self, _world: &mut World, _event: &mut PlayerQuit) {}
    /// Authorize, redirect or veto an input before it mutates player state.
    fn on_move(&self, _world: &mut World, _event: &mut PlayerMove) {}
    /// Observe a movement that committed after all proposal decisions.
    fn on_moved(&self, _world: &mut World, _event: &PlayerMoved) {}
    /// Rewrite or reject final default chat delivery.
    fn on_chat(&self, _world: &mut World, _event: &mut PlayerChat) {}
    /// Observe accepted chat after all proposal vetoes and final validation.
    /// Apply extension-owned routing/accounting here, rather than in on_chat.
    fn on_chatted(&self, _world: &mut World, _event: &PlayerChatted) {}
    /// Observe a bounded response from the same live transport/account.
    fn on_form_response(&self, _world: &mut World, _event: &PlayerFormResponse) {}
}

/// Ordered native handler registry for connection, movement and chat events.
#[derive(Resource, Clone, Default)]
pub struct PlayerEventHooks {
    handlers: Vec<Arc<dyn PlayerEventHandler>>,
}
impl PlayerEventHooks {
    /// Register a handler for the next event snapshot.
    pub fn register(&mut self, handler: impl PlayerEventHandler) {
        self.handlers.push(Arc::new(handler));
    }
}

/// Snapshot handlers so nested dispatches and registration do not borrow a registry.
fn handlers(world: &World) -> Vec<Arc<dyn PlayerEventHandler>> {
    world
        .get_resource::<PlayerEventHooks>()
        .map_or_else(Vec::new, |hooks| hooks.handlers.clone())
}

/// Notify extensions after initialization and before initial join delivery.
pub(crate) fn joined(world: &mut World, player: Entity) {
    let Some(identity) = ConnectionIdentity::capture(world, player) else {
        return;
    };
    let event = PlayerJoined { player, identity };
    for handler in handlers(world) {
        handler.on_joined(world, &event);
    }
    world.trigger(event);
    world.flush();
}

/// Run departure callbacks while authoritative entity state is still available.
pub(crate) fn quit(world: &mut World, player: Entity) -> Option<DisconnectDisposition> {
    let identity = ConnectionIdentity::capture(world, player)?;
    let mut event = PlayerQuit {
        player,
        identity,
        disposition: DisconnectDisposition::Despawn,
    };
    for handler in handlers(world) {
        handler.on_quit(world, &mut event);
    }
    world.trigger_ref(&mut event);
    world.flush();
    event
        .identity
        .matches(world, player)
        .then_some(event.disposition)
}

/// Run movement proposals without holding state component borrows.
pub(crate) fn movement(world: &mut World, event: &mut PlayerMove) {
    for handler in handlers(world) {
        handler.on_move(world, event);
    }
    world.trigger_ref(event);
    world.flush();
}

/// Notify extensions only after a movement committed.
pub(crate) fn moved(
    world: &mut World,
    player: Entity,
    identity: ConnectionIdentity,
    from: DVec3,
    to: DVec3,
) {
    let event = PlayerMoved {
        player,
        identity,
        from,
        to,
    };
    for handler in handlers(world) {
        handler.on_moved(world, &event);
    }
    world.trigger(event);
}

/// Rewrite final default chat without consuming the shared plugin event stream.
pub(crate) fn chat(world: &mut World, player: Entity, message: String) -> Option<PlayerChat> {
    let identity = ConnectionIdentity::capture(world, player)?;
    let mut event = PlayerChat {
        player,
        display_name: identity.name.clone(),
        identity,
        message,
        cancelled: false,
        handled: None,
    };
    for handler in handlers(world) {
        handler.on_chat(world, &mut event);
    }
    world.trigger_ref(&mut event);
    world.flush();
    (!event.cancelled
        && event.identity.matches(world, player)
        && valid_chat(&event.message, 4096)
        && valid_chat(&event.display_name, 256)
        && event
            .handled
            .as_deref()
            .is_none_or(|action| valid_chat(action, 128)))
    .then_some(event)
}

/// Publish acceptance after default delivery, or in place of a handled broadcast.
pub(crate) fn chatted(world: &mut World, event: PlayerChat) {
    let event = PlayerChatted {
        player: event.player,
        identity: event.identity,
        display_name: event.display_name,
        message: event.message,
        handled: event.handled,
    };
    for handler in handlers(world) {
        handler.on_chatted(world, &event);
    }
    world.trigger(event);
}

/// Notify form responses without introducing host form or gameplay policy.
pub(crate) fn form_response(
    world: &mut World,
    player: Entity,
    expected_session: u64,
    form_id: u32,
    data: Option<String>,
    cancelled: bool,
) {
    if data.as_ref().is_some_and(|data| data.len() > 65536) {
        return;
    }
    let Some(identity) = ConnectionIdentity::capture(world, player) else {
        return;
    };
    if identity.session_id() != expected_session {
        return;
    }
    let event = PlayerFormResponse {
        player,
        identity,
        form_id,
        data,
        cancelled,
    };
    for handler in handlers(world) {
        if !event.identity.matches(world, player) {
            return;
        }
        handler.on_form_response(world, &event);
    }
    if event.identity.matches(world, player) {
        world.trigger(event);
    }
}

/// Keep rewritten chat bounded and free of control characters or blank text.
fn valid_chat(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

/// Remove departing connection input so retained actors cannot execute stale packets.
pub(crate) fn discard_input(world: &mut World, player: Entity) {
    use super::packet_queues::*;
    if let Some(mut queue) = world.get_resource_mut::<MovementPacketQueue>() {
        queue.0.retain(|(entity, _)| *entity != player);
    }
    if let Some(mut queue) = world.get_resource_mut::<BlockPacketQueue>() {
        queue.0.retain(|(entity, _)| *entity != player);
    }
    if let Some(mut queue) = world.get_resource_mut::<InventoryPacketQueue>() {
        queue.0.retain(|(entity, _)| *entity != player);
    }
    if let Some(mut queue) = world.get_resource_mut::<ChatPacketQueue>() {
        queue.0.retain(|(entity, _, _)| *entity != player);
    }
    if let Some(mut queue) = world.get_resource_mut::<ChunkPacketQueue>() {
        queue.0.retain(|(entity, _)| *entity != player);
    }
    if let Some(mut queue) = world.get_resource_mut::<super::commands::PendingCommands>() {
        queue.0.retain(|(entity, _)| *entity != player);
    }
    if let Some(mut queue) = world.get_resource_mut::<super::item_use::ItemUseQueue>() {
        queue.0.retain(|(entity, _)| *entity != player);
    }
    if let Some(mut queue) = world.get_resource_mut::<crate::entity::projectile::ThrowQueue>() {
        queue.0.retain(|entity| *entity != player);
    }
    if let Some(mut queue) =
        world.get_resource_mut::<crate::entity::mob::interaction::actions::MobActionQueue>()
    {
        queue.0.retain(|(entity, _)| *entity != player);
    }
    if let Some(mut queue) =
        world.get_resource_mut::<crate::entity::player_life::respawn::RespawnQueue>()
    {
        queue.0.retain(|(entity, _)| *entity != player);
    }
    if let Some(mut events) = world.get_resource_mut::<crate::ecs::events::EventBuffer>() {
        events.discard_player(player);
    }
    let screen = world
        .get::<crate::entity::components::ContainerWindows>(player)
        .map(|windows| windows.screen());
    if let Some(crate::entity::components::ScreenKind::Block(screen)) = screen {
        super::containers::stop_viewing(world, player, screen);
    }
    if let Some(mut windows) = world.get_mut::<crate::entity::components::ContainerWindows>(player)
    {
        windows.close();
    }
}

/// Tell current clients that a normally removed player actor is gone.
pub(crate) fn remove_actor(world: &mut World, player: Entity) {
    use crate::entity::components::{PlayerSession, RuntimeEntityId};
    let Some(id) = world.get::<RuntimeEntityId>(player).map(|id| id.0) else {
        return;
    };
    let packet = jolyne::valentine::McpePacket::from(jolyne::valentine::RemoveActorPacket {
        target_actor_id: jolyne::valentine::ActorUniqueId {
            actor_unique_id: id,
        },
    });
    let mut sessions = world.query::<(Entity, &PlayerSession)>();
    for (entity, session) in sessions.iter(world) {
        if entity != player {
            session.send(packet.clone());
        }
    }
}

#[cfg(test)]
mod tests;
