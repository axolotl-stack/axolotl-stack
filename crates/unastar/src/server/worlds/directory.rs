//! Immutable, transport-bound facts for cross-world native requests.

use std::sync::Arc;

use bevy_ecs::prelude::*;

use super::DimensionRouter;
use crate::entity::components::{Player, PlayerSession};
use crate::server::game::player_events::ConnectionIdentity;
use crate::server::game::player_state::PlayerSnapshot;
use crate::server::game::{SessionEntityMap, inventory_effects, player_events, player_state};
use crate::world::ecs::ChunkManager;

/// Native lifecycle state, independent of application permissions or game rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerAvailability {
    Ready,
    Initializing,
    Quarantined,
    Recovering,
    Loading,
    Incomplete,
}

/// A player handle is scoped to one ECS world and one exact authenticated
/// connection. Numeric session/entity reuse cannot authorize a stale handle.
#[derive(Clone, Debug)]
pub struct WorldPlayer {
    dimension: i32,
    player: Entity,
    identity: ConnectionIdentity,
    pub availability: PlayerAvailability,
    pub snapshot: Option<PlayerSnapshot>,
}
impl WorldPlayer {
    /// Read the ECS world's protocol dimension.
    pub fn dimension(&self) -> i32 {
        self.dimension
    }
    /// Read the actor only in that ECS world.
    pub fn player(&self) -> Entity {
        self.player
    }
    /// Read the original immutable account and transport identity.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
    /// Require the same world, actor, session-map ownership and outbound channel.
    pub fn matches(&self, world: &World) -> bool {
        world
            .get_resource::<ChunkManager>()
            .is_some_and(|manager| manager.dimension() == self.dimension)
            && world
                .get_resource::<SessionEntityMap>()
                .is_some_and(|map| map.get(self.identity.session_id()) == Some(self.player))
            && self.identity.matches(world, self.player)
    }
}

/// One owned snapshot across all worlds, refreshed between global callbacks.
/// Captured native state is factual; applications still authorize access and
/// revalidate the handle before applying effects in a mutable destination world.
#[derive(Resource, Clone, Default, Debug)]
pub struct WorldDirectory {
    players: Arc<Vec<WorldPlayer>>,
}
impl WorldDirectory {
    /// Enumerate scoped, immutable connection facts from every registered world.
    pub fn players(&self) -> impl Iterator<Item = &WorldPlayer> {
        self.players.iter()
    }
    /// Read a connection only when its session ID has exactly one world owner.
    pub fn session(&self, session_id: u64) -> Option<&WorldPlayer> {
        let mut players = self
            .players
            .iter()
            .filter(|player| player.identity.session_id() == session_id);
        let player = players.next()?;
        players.next().is_none().then_some(player)
    }
    /// Resolve the source actor of a local callback without comparing display names.
    pub fn actor(&self, dimension: i32, entity: Entity) -> Option<&WorldPlayer> {
        self.players
            .iter()
            .find(|player| player.dimension == dimension && player.player == entity)
    }
    /// Capture every world without retaining any ECS query borrow.
    pub(super) fn capture(router: &mut DimensionRouter) -> Self {
        let mut players = Vec::new();
        for (&dimension, server) in &mut router.worlds {
            let world = server.ecs.world_mut();
            let mut query = world.query_filtered::<Entity, (With<Player>, With<PlayerSession>)>();
            let entities: Vec<_> = query.iter(world).collect();
            for player in entities {
                let Some(identity) = ConnectionIdentity::capture(world, player) else {
                    continue;
                };
                let snapshot = player_state::capture(world, player).ok();
                let availability = if inventory_effects::is_recovery_fenced(world, player) {
                    PlayerAvailability::Recovering
                } else if inventory_effects::is_quarantined(world, player) {
                    PlayerAvailability::Quarantined
                } else if player_events::initial_state_pending(world, player) {
                    PlayerAvailability::Initializing
                } else if super::loading::is_loading(world, player) {
                    PlayerAvailability::Loading
                } else if snapshot.is_none() {
                    PlayerAvailability::Incomplete
                } else {
                    PlayerAvailability::Ready
                };
                players.push(WorldPlayer {
                    dimension,
                    player,
                    identity,
                    availability,
                    snapshot,
                });
            }
        }
        players.sort_by_key(|player| (player.dimension, player.identity.session_id()));
        Self {
            players: Arc::new(players),
        }
    }
}

/// Owned callbacks execute only after the local ECS system releases its world.
type Request = Box<dyn FnOnce(&mut DimensionRouter, &WorldDirectory) + Send + Sync>;
#[derive(Resource, Default)]
pub(super) struct RouterRequests(pub Vec<Request>);

/// Enqueue owned native work instead of reentering a mutably borrowed world.
/// This acknowledges queueing only; the callback owns actual authorization,
/// durable commits and success/failure publication. Each world holds at most 256.
pub fn queue_router_request(
    world: &mut World,
    request: impl FnOnce(&mut DimensionRouter, &WorldDirectory) + Send + Sync + 'static,
) -> bool {
    let mut queue = world.get_resource_or_init::<RouterRequests>();
    if queue.0.len() >= 256 {
        return false;
    }
    queue.0.push(Box::new(request));
    true
}

/// A global application tick runs once after all dimension ticks and transfers.
pub trait RouterTickHandler: Send + Sync + 'static {
    /// Read aggregate facts and route accepted effects to their actual world.
    fn on_tick(&self, router: &mut DimensionRouter, directory: &WorldDirectory);
}
