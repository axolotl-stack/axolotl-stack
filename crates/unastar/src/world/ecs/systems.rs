//! Chunk management systems for the ECS world.
//!
//! This module contains systems for:
//! - Processing chunk load queues (center-outward)
//! - Updating chunk viewers when players move
//! - Managing chunk unload with grace periods
//! - Sending chunk data to players
//! - Broadcasting block updates to viewers
//!
//! ## Performance Considerations
//! - Chunk ticking uses O(Players × SimDist²) not O(Chunks × Players)
//! - Player cleanup uses ChunkLoader's known set, not full world scan
//! - Publisher updates only sent on position/radius change, not per-chunk

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use bevy_ecs::prelude::*;
use tracing::{debug, error, trace, warn};

use crate::entity::components::{ChunkRadius, Player, PlayerSession, Position};
use crate::entity::mob::persistence::{restore_actors_of_chunk, snapshot_chunks};
use crate::registry::block::BlockNetworkIds;
use crate::storage::ChunkColumn;
use crate::world::ecs::manager::{chunk_entity_components, restore_held_columns};
use crate::world::ecs::streaming::{
    ChunkNetCache, ColumnLoads, EncodedColumn, LoadState, MAX_STORAGE_READS,
    PENDING_GENERATIONS_PER_WORKER, StreamStats, log_stream_stats,
};
use crate::world::ecs::{
    ChunkData, ChunkLoader, ChunkManager, ChunkPendingUnload, ChunkPosition, ChunkState,
    ChunkStateFlags, ChunkTickingState, ChunkViewers, PendingActorRestore,
};
use jolyne::valentine::types::BlockPos;
use jolyne::valentine::{
    McpePacket, NetworkChunkPublisherUpdatePacket, PlayStatusPacket, PlayStatusPacketStatus,
};

/// Configuration for chunk loading behavior.
#[derive(Resource, Debug, Clone)]
pub struct ChunkLoadConfig {
    /// Dimension ID for chunk packets.
    pub dimension: i32,
    /// Simulation distance in chunks.
    pub simulation_distance: i32,
    /// Grace period ticks before unloading chunks with no viewers.
    pub unload_grace_ticks: u32,
    /// Block state numbering of encoded sub-chunks.
    pub block_network_ids: BlockNetworkIds,
}

impl Default for ChunkLoadConfig {
    fn default() -> Self {
        Self {
            dimension: 0,
            simulation_distance: 6,
            unload_grace_ticks: 100,
            block_network_ids: BlockNetworkIds::Hashed,
        }
    }
}

impl ChunkLoadConfig {
    /// Create from ServerConfig.
    pub fn from_server_config(config: &crate::server::ServerConfig) -> Self {
        Self {
            dimension: config.world.dimension,
            simulation_distance: config.simulation_distance,
            unload_grace_ticks: config.chunk_unload_ticks,
            block_network_ids: config.block_network_ids,
        }
    }
}

// NOTE: PlayerDisconnecting marker has been removed.
// ChunkLoader now has an on_remove hook that automatically cleans up
// chunk viewers when the component is removed during entity despawn.
// See loader.rs for the implementation.

/// Component tracking the last publisher position sent to client.
/// Used to avoid spamming NetworkChunkPublisherUpdate.
#[derive(Component, Debug, Default)]
pub struct LastPublisherState {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub radius: i32,
}

impl LastPublisherState {
    /// Whether the player has been sent a view. Players join with an empty
    /// view (jolyne sends no `NetworkChunkPublisherUpdate`), so no chunk may
    /// be sent before this view.
    pub fn published(&self) -> bool {
        self.radius != 0
    }
}

/// Marks a player whose join still owes `PlayStatus(PlayerSpawn)`: the
/// streamed spawn hands the player over at its `RequestChunkRadius`, and the
/// spawn status follows the view and the chunks of the player's first tick
/// (BDS order) without waiting for chunk generation.
#[derive(Component, Debug, Default)]
pub struct AwaitingSpawnStatus;

/// System: Send `PlayStatus(PlayerSpawn)` once, after this tick's publisher
/// update and chunks. The client then loads the terrain still streaming in
/// and sends `SetLocalPlayerAsInitialized` when it is ready.
///
/// This is also where plugins learn of the join: `PlayerJoin` is queued
/// exactly once, when `AwaitingSpawnStatus` clears, so `on-player-join` never
/// runs while the client is still on the loading screen.
pub fn send_spawn_status(
    mut commands: Commands,
    players: Query<
        (
            Entity,
            &PlayerSession,
            &LastPublisherState,
            &crate::entity::components::PlayerName,
            &crate::entity::components::PlayerUuid,
        ),
        With<AwaitingSpawnStatus>,
    >,
    mut events: Option<ResMut<crate::ecs::events::EventBuffer>>,
) {
    for (player_entity, session, publisher_state, name, uuid) in players.iter() {
        if !publisher_state.published() {
            continue; // the view comes first
        }
        let spawned = session.send(McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Playerspawn,
        }));
        if spawned {
            debug!(player = ?player_entity, "Sent PlayStatus(PlayerSpawn)");
            commands
                .entity(player_entity)
                .remove::<AwaitingSpawnStatus>();
            if let Some(events) = events.as_mut() {
                events.push(crate::ecs::events::ServerEvent::PlayerJoin {
                    entity: player_entity,
                    player_id: uuid.0.to_string(),
                    username: name.0.clone(),
                });
            }
        } else {
            debug!(player = ?player_entity, "Failed to send PlayStatus(PlayerSpawn) (channel full or closed); retrying next tick");
        }
    }
}

/// System: Send `NetworkChunkPublisherUpdate` when a player's chunk or radius
/// changes, including the first tick after join. Runs before every system
/// that sends chunks, so the view always precedes its terrain.
pub fn send_chunk_publisher_updates(
    mut players: Query<
        (
            Entity,
            &Position,
            &PlayerSession,
            &ChunkLoader,
            &mut LastPublisherState,
        ),
        With<Player>,
    >,
) {
    for (player_entity, position, session, loader, mut publisher_state) in players.iter_mut() {
        let chunk_x = (position.0.x.floor() as i32) >> 4;
        let chunk_z = (position.0.z.floor() as i32) >> 4;
        let publisher_radius = loader.radius().saturating_mul(16);
        if publisher_state.chunk_x == chunk_x
            && publisher_state.chunk_z == chunk_z
            && publisher_state.radius == publisher_radius
        {
            continue;
        }
        debug!(player=?player_entity, old_pos = ?(publisher_state.chunk_x, publisher_state.chunk_z), new_pos = ?(chunk_x, chunk_z), "Player moved, sending NetworkChunkPublisherUpdate");
        let sent = session.send(McpePacket::from(NetworkChunkPublisherUpdatePacket {
            newpositionforview: BlockPos {
                x: position.0.x.floor() as i32,
                y: position.0.y.floor() as i32,
                z: position.0.z.floor() as i32,
            },
            newradiusforview: publisher_radius as u32,
            server_built_chunks_list: vec![],
        }));
        if !sent {
            debug!(player = ?player_entity, "Failed to send NetworkChunkPublisherUpdate (channel full or closed)");
            continue;
        }
        publisher_state.chunk_x = chunk_x;
        publisher_state.chunk_z = chunk_z;
        publisher_state.radius = publisher_radius;
    }
}

// NOTE: initialize_chunk_loaders has been removed.
//
// ChunkLoader and LastPublisherState are now included in PlayerBundle at spawn time.
// This eliminates archetype changes that occurred when these components were
// added by a system after the player entity was spawned.
//
// The initialization logic that was here is now in GameServer::spawn_player().

/// System: Update ChunkLoaders when players move.
/// Evicts old chunks and queues new ones.
pub fn update_chunk_loaders(
    chunk_manager: Res<ChunkManager>,
    mut players: Query<(Entity, &Position, &ChunkRadius, &mut ChunkLoader), With<Player>>,
    mut chunks: Query<&mut ChunkViewers>,
) {
    for (player_entity, position, radius, mut loader) in players.iter_mut() {
        loader.observe_position(position.0.x, position.0.z);
        let chunk_x = (position.0.x / 16.0).floor() as i32;
        let chunk_z = (position.0.z / 16.0).floor() as i32;
        let current_pos = loader.position();

        trace!(
            player = ?player_entity,
            player_chunk = ?(chunk_x, chunk_z),
            loader_chunk = ?current_pos,
            "Update chunk loaders - checking position"
        );

        // Check if player crossed into a new chunk
        if current_pos == (chunk_x, chunk_z) {
            continue;
        }

        // Update loader position - this evicts out-of-range chunks
        let evicted = loader.move_to(chunk_x, chunk_z);

        // Handle radius changes
        if loader.radius() != radius.0 {
            let radius_evicted = loader.set_radius(radius.0);
            for pos in radius_evicted {
                if !evicted.contains(&pos)
                    && let Some(chunk_entity) = chunk_manager.get_by_coords(pos.0, pos.1)
                    && let Ok(mut viewers) = chunks.get_mut(chunk_entity)
                {
                    viewers.remove(player_entity);
                }
            }
        }

        // Remove player as viewer from evicted chunks
        for (ex, ez) in &evicted {
            if let Some(chunk_entity) = chunk_manager.get_by_coords(*ex, *ez)
                && let Ok(mut viewers) = chunks.get_mut(chunk_entity)
            {
                viewers.remove(player_entity);

                trace!(
                    player = ?player_entity,
                    chunk = ?(ex, ez),
                    remaining_viewers = viewers.len(),
                    "Player left chunk view"
                );
            }
        }

        // NOTE: Don't update publisher_state here - send_chunk_publisher_updates
        // detects the change and sends the NetworkChunkPublisherUpdate

        trace!(
            player = ?player_entity,
            from = ?current_pos,
            to = ?(chunk_x, chunk_z),
            pending = loader.queue_len(),
            evicted = evicted.len(),
            "Player crossed chunk boundary - queuing new chunks"
        );
    }
}

// =============================================================================
// Streaming (see `streaming.rs` for the stages)
// =============================================================================

/// Spawns a loaded column as a chunk entity with its encoding. A column the
/// world already has (created synchronously meanwhile) is dropped.
fn spawn_column(
    commands: &mut Commands,
    manager: &mut ChunkManager,
    pos: (i32, i32),
    column: ChunkColumn,
    encoded: Arc<EncodedColumn>,
    stored: bool,
) {
    if manager.contains_coords(pos.0, pos.1) {
        return;
    }
    let position = ChunkPosition::new(pos.0, pos.1);
    let (mut components, actors) = chunk_entity_components(position, column, stored);
    components.6 = ChunkNetCache::new(encoded);
    let mut spawned = commands.spawn(components);
    // Stored actors are restored by `restore_chunk_actors`.
    if let Some(actors) = actors {
        spawned.insert(actors);
    }
    let entity = spawned.id();
    manager.insert(position, entity);
}

/// Tick time spent spawning finished columns at most; the rest wait for the
/// next tick.
const SPAWN_BUDGET: Duration = Duration::from_millis(4);

/// System: turns finished background loads into chunk entities, within
/// [`SPAWN_BUDGET`], installs finished re-encodings of edited chunks, and puts
/// back columns whose unload write failed. A column that failed to load leaves
/// every queue (it is requested again only when the view is rebuilt), never
/// replaced by generated terrain.
pub fn apply_loaded_columns(
    mut commands: Commands,
    mut manager: ResMut<ChunkManager>,
    mut loads: ResMut<ColumnLoads>,
    mut loaders: Query<&mut ChunkLoader>,
    mut caches: Query<&mut ChunkNetCache>,
) {
    let started = Instant::now();
    let drained = loads.drain(manager.dimension());
    for (pos, encoded) in drained.reencoded {
        if let Some(entity) = manager.get_by_coords(pos.0, pos.1)
            && let Ok(mut cache) = caches.get_mut(entity)
        {
            cache.install(encoded);
        }
    }
    while started.elapsed() < SPAWN_BUDGET {
        let Some(loaded) = loads.next_ready() else {
            break;
        };
        spawn_column(
            &mut commands,
            &mut manager,
            loaded.pos,
            loaded.column,
            loaded.encoded,
            loaded.stored,
        );
    }
    if !drained.failed.is_empty() {
        for mut loader in loaders.iter_mut() {
            loader.retain_queue(|pos| !drained.failed.contains(pos));
        }
    }
    if !drained.held.is_empty() {
        let held = drained.held;
        commands.queue(move |world: &mut World| restore_held_columns(world, held));
    }
}

/// Sends a column's `LevelChunk` to one player and records it in the
/// player's loader. Returns `false` when the session's queue is full.
fn send_column(
    session: &PlayerSession,
    loader: &mut ChunkLoader,
    encoded: &EncodedColumn,
    pos: (i32, i32),
    dimension: i32,
    now: Instant,
) -> bool {
    if !session.send(McpePacket::from(
        encoded.level_chunk(pos.0, pos.1, dimension),
    )) {
        return false;
    }
    loader
        .window_mut()
        .sent(pos, encoded.highest_subchunk() > 0, now);
    loader.mark_loaded(pos.0, pos.1);
    true
}

/// System: streams every player's view.
///
/// Walks each player's queue in priority order and sends the columns that are
/// in memory and encoded while the player's
/// [`SendWindow`](super::streaming::SendWindow) has room (unacknowledged
/// columns, and the transport's queue below the byte budget; the answers
/// carrying the terrain are bounded when they are queued, in
/// `apply_chunk_requests`); there is no per-tick cap, and the first column
/// that does not fit ends the player's sending for the tick so the order
/// holds. Columns not in memory are collected as loads (storage
/// reads, or generation for worlds without storage) and generation requests,
/// and the players' wishes are then started in turns (one per player per
/// round, nearest first), up to [`MAX_STORAGE_READS`] loads and
/// [`PENDING_GENERATIONS_PER_WORKER`] per pool worker in flight. What the
/// views leave of those budgets loads the columns each moving player reaches
/// next ([`ChunkLoader::prefetch`]), so they are in memory when they enter
/// the view. Nothing is read, generated or encoded on the tick: an edited
/// chunk is copied and encoded in the background, and sent once that
/// finished.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn stream_chunks(
    mut commands: Commands,
    config: Res<ChunkLoadConfig>,
    mut manager: ResMut<ChunkManager>,
    mut loads: ResMut<ColumnLoads>,
    stats: Res<StreamStats>,
    mut players: Query<
        (
            Entity,
            &PlayerSession,
            &mut ChunkLoader,
            &LastPublisherState,
        ),
        With<Player>,
    >,
    mut chunks: Query<(&ChunkData, Option<&mut ChunkNetCache>)>,
    mut pending_viewers: Local<Vec<((i32, i32), Entity)>>,
    mut ticks: Local<u32>,
) {
    let now = Instant::now();
    let ids = config.block_network_ids;
    let has_storage = manager.provider().is_some();
    let has_pool = manager.has_generation_pool();
    // Worlds with storage read every column first; worlds without storage
    // and a cheap generator generate it in the background the same way.
    let loads_first = has_storage || !has_pool;
    let budget = LoadBudget {
        reads: if loads_first {
            MAX_STORAGE_READS.saturating_sub(loads.reads_in_flight())
        } else {
            0
        },
        generations: (manager.generation_threads() * PENDING_GENERATIONS_PER_WORKER)
            .saturating_sub(loads.generations_in_flight()),
        loads_first,
    };
    // Per player, nearest first: columns to load and columns to generate, for
    // the view, then ahead of it.
    let mut wishes: Vec<Wish> = Vec::new();
    let mut prefetches: Vec<Wish> = Vec::new();

    for (player, session, mut loader, publisher) in players.iter_mut() {
        // `send_chunk_publisher_updates` ran first; terrain waits for the view.
        if !publisher.published() {
            continue;
        }
        let unacked = loader.window_mut().expire(now);
        if unacked > 0 {
            stats.unacked.fetch_add(unacked as u64, Ordering::Relaxed);
        }
        let transport_queued = session.transport_queued_bytes();
        stats.record_queue(transport_queued);
        let started = Instant::now();
        let mut wish = Wish::default();
        let mut sent = HashSet::new();
        let mut can_send = true;
        for pos in loader.queue().to_vec() {
            if !can_send && wish.is_full(&budget) {
                break;
            }
            if let Some(entity) = manager.get_by_coords(pos.0, pos.1) {
                if !can_send {
                    continue;
                }
                // Spawned by this tick's loads only once commands apply.
                let Ok((data, cache)) = chunks.get_mut(entity) else {
                    continue;
                };
                // Being encoded: it goes out once the encoding finished.
                let Some(encoded) = current_encoding(
                    &mut commands,
                    entity,
                    pos,
                    &data.inner,
                    cache,
                    &mut loads,
                    manager.runtime(),
                    ids,
                    &stats,
                ) else {
                    continue;
                };
                if !loader.window_mut().may_send(transport_queued, now) {
                    can_send = false;
                    continue;
                }
                if !send_column(session, &mut loader, &encoded, pos, config.dimension, now) {
                    debug!(chunk = ?pos, player = ?player, "Session queue full; the chunk waits for the next tick");
                    can_send = false;
                    continue;
                }
                sent.insert(pos);
                pending_viewers.push((pos, player));
                continue;
            }
            wish.want(pos, loads.state(pos), &budget);
        }
        if !sent.is_empty() {
            loader.retain_queue(|pos| !sent.contains(pos));
            stats.sent.record_many(sent.len() as u64, started.elapsed());
        }
        wishes.push(wish);
        // Columns about to enter the view are loaded (not sent) with what
        // the view leaves of the budgets.
        let mut prefetch = Wish::default();
        for pos in loader.prefetch() {
            if prefetch.is_full(&budget) {
                break;
            }
            if !manager.contains_coords(pos.0, pos.1) {
                prefetch.want(pos, loads.state(pos), &budget);
            }
        }
        prefetches.push(prefetch);
    }
    for (pos, player) in pending_viewers.drain(..) {
        manager.pending_viewers.entry(pos).or_default().push(player);
    }
    let left = start_loads(&manager, &mut loads, &stats, ids, &wishes, budget);
    start_loads(&manager, &mut loads, &stats, ids, &prefetches, left);

    // Now and then, forget storage misses and failed loads no player wants.
    *ticks += 1;
    if (*ticks).is_multiple_of(200) {
        let wanted: HashSet<(i32, i32)> = players
            .iter()
            .flat_map(|(_, _, loader, _)| loader.queue().iter().copied().chain(loader.prefetch()))
            .collect();
        loads.forget_unwanted(|pos| wanted.contains(&pos));
    }
}

/// The encoding of a chunk entity's current contents, or `None` while it is
/// being encoded in the background. A chunk spawned without a cache (tools and
/// tests) gets one.
#[allow(clippy::too_many_arguments)]
fn current_encoding(
    commands: &mut Commands,
    entity: Entity,
    pos: (i32, i32),
    chunk: &crate::world::Chunk,
    cache: Option<Mut<ChunkNetCache>>,
    loads: &mut ColumnLoads,
    runtime: Option<&tokio::runtime::Handle>,
    ids: BlockNetworkIds,
    stats: &StreamStats,
) -> Option<Arc<EncodedColumn>> {
    match cache {
        Some(mut cache) => cache.current(pos, chunk, loads, runtime, ids, stats),
        None => {
            let mut cache = ChunkNetCache::default();
            let encoded = cache.current(pos, chunk, loads, runtime, ids, stats);
            commands.entity(entity).insert(cache);
            encoded
        }
    }
}

/// What this tick may still start: storage reads (or background generation
/// for worlds that load first) and pool generations.
#[derive(Debug, Clone, Copy)]
struct LoadBudget {
    reads: usize,
    generations: usize,
    /// Worlds with storage, or without a generation pool, read (or generate
    /// in the background) every column first; the pool gets storage misses.
    loads_first: bool,
}

/// One player's wished loads, nearest first, within a [`LoadBudget`].
#[derive(Debug, Default)]
struct Wish {
    reads: Vec<(i32, i32)>,
    generations: Vec<(i32, i32)>,
}

impl Wish {
    /// Whether no more of either kind fits the budget.
    fn is_full(&self, budget: &LoadBudget) -> bool {
        self.reads.len() >= budget.reads && self.generations.len() >= budget.generations
    }

    /// Wishes for `pos`, which is not in memory, given its load state: a
    /// storage miss (or, in a pool world without storage, a column not yet
    /// asked for) is generated; any other column not yet asked for is read.
    fn want(&mut self, pos: (i32, i32), state: Option<LoadState>, budget: &LoadBudget) {
        let generate = match state {
            Some(LoadState::Missing) => true,
            None => !budget.loads_first,
            Some(_) => return,
        };
        if generate {
            if self.generations.len() < budget.generations {
                self.generations.push(pos);
            }
        } else if self.reads.len() < budget.reads {
            self.reads.push(pos);
        }
    }
}

/// Starts the players' wished loads in turns, one per player per round.
/// Returns what is left of the budget.
fn start_loads(
    manager: &ChunkManager,
    loads: &mut ColumnLoads,
    stats: &StreamStats,
    ids: BlockNetworkIds,
    wishes: &[Wish],
    mut budget: LoadBudget,
) -> LoadBudget {
    let mut round = 0;
    while budget.reads + budget.generations > 0 {
        let mut more = false;
        for wish in wishes {
            if let Some(&pos) = wish.reads.get(round) {
                more = true;
                if budget.reads > 0
                    && loads.state(pos).is_none()
                    && !manager.contains_coords(pos.0, pos.1)
                {
                    loads.start_read(pos, manager, ids, stats);
                    budget.reads -= 1;
                }
            }
            if let Some(&pos) = wish.generations.get(round) {
                more = true;
                if budget.generations > 0
                    && matches!(loads.state(pos), None | Some(LoadState::Missing))
                    && !manager.contains_coords(pos.0, pos.1)
                    && loads.start_generation(pos, manager, ids, stats)
                {
                    budget.generations -= 1;
                }
            }
        }
        if !more {
            break;
        }
        round += 1;
    }
    budget
}

/// System: Handle ChunkRadius changes.
/// When a player's chunk radius changes, update their loader; the publisher
/// update follows from the loader's new radius.
#[allow(clippy::type_complexity)]
pub fn handle_radius_changes(
    chunk_manager: Res<ChunkManager>,
    mut players: Query<
        (Entity, &ChunkRadius, &mut ChunkLoader),
        (With<Player>, Changed<ChunkRadius>),
    >,
    mut chunks: Query<&mut ChunkViewers>,
) {
    for (player, radius, mut loader) in players.iter_mut() {
        if loader.radius() != radius.0 {
            let evicted = loader.set_radius(radius.0);
            for (x, z) in &evicted {
                if let Some(chunk) = chunk_manager.get_by_coords(*x, *z)
                    && let Ok(mut viewers) = chunks.get_mut(chunk)
                {
                    viewers.remove(player);
                }
            }

            trace!(
                new_radius = radius.0,
                evicted_count = evicted.len(),
                "Player chunk radius changed"
            );
        }
    }
}

/// System: Schedule chunks for unload when they have no viewers.
/// Adds ChunkPendingUnload with grace period.
#[allow(clippy::type_complexity)]
pub fn schedule_chunk_unloads(
    mut commands: Commands,
    config: Res<ChunkLoadConfig>,
    chunks: Query<
        (Entity, &ChunkPosition, &ChunkViewers),
        (With<ChunkState>, Without<ChunkPendingUnload>),
    >,
) {
    for (entity, pos, viewers) in chunks.iter() {
        if viewers.is_empty() {
            commands
                .entity(entity)
                .insert(ChunkPendingUnload::new(config.unload_grace_ticks));

            trace!(
                chunk = ?(pos.x, pos.z),
                grace_ticks = config.unload_grace_ticks,
                "Chunk scheduled for unload"
            );
        }
    }
}

/// System: Cancel chunk unload if viewers return.
pub fn cancel_chunk_unloads(
    mut commands: Commands,
    chunks: Query<(Entity, &ChunkPosition, &ChunkViewers), With<ChunkPendingUnload>>,
) {
    for (entity, pos, viewers) in chunks.iter() {
        if !viewers.is_empty() {
            commands.entity(entity).remove::<ChunkPendingUnload>();

            trace!(
                chunk = ?(pos.x, pos.z),
                viewers = viewers.len(),
                "Chunk unload cancelled - viewers returned"
            );
        }
    }
}

/// Rate limit for the summary of chunks held in memory because their save
/// before unload failed: one line when the first chunk is held, then at most
/// one per [`Self::INTERVAL_TICKS`] while any stay held.
#[derive(Resource, Debug, Default)]
pub struct HeldChunkReport {
    ticks_until_next: u32,
}

impl HeldChunkReport {
    /// 30 seconds at 20 TPS.
    pub const INTERVAL_TICKS: u32 = 600;
    /// From this many held chunks the summary is an error, not a warning.
    pub const ERROR_THRESHOLD: usize = 64;

    /// Advance one tick with `held` chunks held; true when a summary is due.
    pub fn due(&mut self, held: usize) -> bool {
        if held == 0 {
            // The next incident is reported as soon as it starts.
            self.ticks_until_next = 0;
            return false;
        }
        if self.ticks_until_next == 0 {
            self.ticks_until_next = Self::INTERVAL_TICKS;
            true
        } else {
            self.ticks_until_next -= 1;
            false
        }
    }
}

/// Tick time spent unloading expired chunks at most; chunks left over stay
/// expired and unload on the next ticks.
const UNLOAD_BUDGET: Duration = Duration::from_millis(4);
/// Expired chunks unloaded together (one pass over the mobs per batch).
const UNLOAD_BATCH: usize = 16;
/// Unloads stop while this many writes are in flight. Each write holds a
/// blocking-pool thread for its storage write, which can stall for a while
/// when storage compacts; storage reads (and so generation of fresh terrain,
/// which starts with a read) share that pool, so writes must never take all
/// of it. Expired chunks wait in memory until the writes catch up.
const MAX_SAVES_IN_FLIGHT: usize = 64;

/// System: Process pending chunk unloads.
/// Ticks down grace periods. An expired chunk is written in the background
/// together with the mobs standing in it (when anything changed), and the mobs
/// and the chunk are removed from the world; the column is not read again
/// until the write finished. Unloading stops for the tick after
/// [`UNLOAD_BUDGET`], or while [`MAX_SAVES_IN_FLIGHT`] writes are pending. If a
/// write fails the column comes back with its edits and mobs (see
/// [`ColumnLoads::take_held`]) and its unload is retried with an exponential
/// backoff per chunk (see [`ChunkPendingUnload::after_failed_save`]); one
/// rate-limited summary reports how many chunks are held (see
/// [`HeldChunkReport`]).
pub fn process_chunk_unloads(world: &mut World) {
    let started = Instant::now();
    let mut pending = world.query::<(Entity, &mut ChunkPendingUnload)>();
    let expired: Vec<Entity> = pending
        .iter_mut(world)
        .filter_map(|(entity, mut grace)| grace.tick().then_some(entity))
        .collect();
    for batch in expired.chunks(UNLOAD_BATCH) {
        if started.elapsed() >= UNLOAD_BUDGET
            || world.resource::<ColumnLoads>().saves_in_flight() >= MAX_SAVES_IN_FLIGHT
        {
            break;
        }
        unload_expired_chunks(world, batch);
    }
    report_held_chunks(world);
}

/// Count held chunks and log the rate-limited summary when one is due.
fn report_held_chunks(world: &mut World) {
    let held = world
        .query::<&ChunkPendingUnload>()
        .iter(world)
        .filter(|pending| pending.is_held())
        .count();
    let due = world
        .get_resource_or_insert_with(HeldChunkReport::default)
        .due(held);
    if !due {
        return;
    }
    if held >= HeldChunkReport::ERROR_THRESHOLD {
        error!(
            held,
            "{held} chunks held in memory awaiting save; storage writes are failing and memory use grows with every chunk left"
        );
    } else {
        warn!(
            held,
            "{held} chunks held in memory awaiting save; storage writes are failing"
        );
    }
}

/// Start writing the chunks whose grace period ran out and remove them (and
/// their mobs) from the world.
fn unload_expired_chunks(world: &mut World, expired: &[Entity]) {
    let failed_saves: HashMap<Entity, u32> = expired
        .iter()
        .filter_map(|&chunk| Some((chunk, world.get::<ChunkPendingUnload>(chunk)?.failed_saves)))
        .collect();
    for snapshot in snapshot_chunks(world, expired, true) {
        let pos = snapshot.pos;
        if let Some(column) = snapshot.column {
            let failed = failed_saves.get(&snapshot.chunk).copied().unwrap_or(0);
            world.resource_scope(|world, mut loads: Mut<ColumnLoads>| {
                loads.start_save(
                    (pos.x, pos.z),
                    column,
                    failed,
                    world.resource::<ChunkManager>(),
                );
            });
        }
        for mob in snapshot.mobs {
            world.despawn(mob);
        }
        world
            .resource_mut::<ChunkManager>()
            .remove_by_coords(pos.x, pos.z);
        world.despawn(snapshot.chunk);
        debug!(chunk = ?(pos.x, pos.z), "Chunk unloaded");
    }
}

/// Tick time spent restoring the stored mobs of loaded chunks at most.
const RESTORE_BUDGET: Duration = Duration::from_millis(4);

/// System: restores the mobs stored with chunks that loaded, chunk by chunk,
/// until [`RESTORE_BUDGET`] is spent; the rest follow on the next ticks.
pub fn restore_loaded_chunk_actors(world: &mut World) {
    let started = Instant::now();
    let mut pending = world.query_filtered::<Entity, With<PendingActorRestore>>();
    let chunks: Vec<Entity> = pending.iter(world).collect();
    for chunk in chunks {
        if started.elapsed() >= RESTORE_BUDGET {
            break;
        }
        restore_actors_of_chunk(world, chunk);
    }
}

/// System: Flush pending viewers from ChunkManager to ECS components.
/// Ensures that even if a chunk was just spawned, its viewers are eventually synchronized.
pub fn flush_pending_viewers(
    mut chunk_manager: ResMut<ChunkManager>,
    mut chunks: Query<&mut ChunkViewers>,
) {
    if chunk_manager.is_empty() {
        return;
    }

    // We use a temporary vector to avoid borrowing issues while draining
    let mut still_pending = std::collections::HashMap::new();

    // Drain pending viewers and try to apply them to chunk entities
    let pending_items: Vec<_> = chunk_manager.pending_viewers.drain().collect();

    for (pos, viewers) in pending_items {
        if let Some(chunk_entity) = chunk_manager.get_by_coords(pos.0, pos.1) {
            if let Ok(mut viewers_comp) = chunks.get_mut(chunk_entity) {
                for viewer in viewers {
                    viewers_comp.insert(viewer);
                }
            } else {
                // Entity exists in manager but component not ready yet (likely spawned this tick)
                still_pending.insert(pos, viewers);
            }
        }
    }

    chunk_manager.pending_viewers = still_pending;
}

/// System: Update ChunkStateFlags::TICKING based on simulation distance.
///
/// OPTIMIZED (Phase 2): Uses bitflags mutation instead of component insert/remove,
/// avoiding all archetype thrashing. Reuses a persistent HashSet via ChunkTickingState
/// to eliminate per-tick allocations (~57KB/tick with many players).
pub fn update_chunk_ticking(
    config: Res<ChunkLoadConfig>,
    mut ticking_state: ResMut<ChunkTickingState>,
    players: Query<&Position, With<Player>>,
    mut chunks: Query<(&ChunkPosition, &mut ChunkStateFlags)>,
) {
    let sim_dist = config.simulation_distance;

    // Fast path: no players = clear all ticking flags
    if players.is_empty() {
        for (_, mut state) in chunks.iter_mut() {
            if state.is_ticking() {
                state.set_ticking(false);
            }
        }
        return;
    }

    // Clear and reuse the HashSet instead of allocating new
    // O(Players × SimDist²) - e.g., 50 players × 6² = 1,800 entries max
    ticking_state.should_tick.clear();

    for pos in players.iter() {
        let cx = (pos.0.x / 16.0).floor() as i32;
        let cz = (pos.0.z / 16.0).floor() as i32;

        for x in (cx - sim_dist)..=(cx + sim_dist) {
            for z in (cz - sim_dist)..=(cz + sim_dist) {
                ticking_state.should_tick.insert((x, z));
            }
        }
    }

    // Update chunk flags - no archetype changes!
    // Simply mutate the bitflag instead of inserting/removing components
    for (pos, mut state) in chunks.iter_mut() {
        let is_in_range = ticking_state.should_tick.contains(&pos.as_tuple());
        if state.is_ticking() != is_in_range {
            state.set_ticking(is_in_range);

            if !is_in_range {
                trace!(
                    chunk = ?(pos.x, pos.z),
                    "Chunk stopped ticking (outside sim distance)"
                );
            }
        }
    }
}

// NOTE: cleanup_disconnecting_player_views system has been removed.
// ChunkLoader's on_remove hook now handles cleanup automatically when
// the entity is despawned. This eliminates the need for the
// PlayerDisconnecting marker and the explicit cleanup system.

/// Helper to compute chunk coordinates from world coordinates.
#[inline]
pub fn world_to_chunk_coords(world_x: i32, world_z: i32) -> (i32, i32) {
    (world_x >> 4, world_z >> 4)
}

/// Helper to compute local block coordinates from world coordinates.
/// Unsupported heights map to an out-of-bounds sentinel rejected by Chunk's
/// get/set methods, never to a wrapped, valid block coordinate.
#[inline]
pub fn world_to_local_coords(world_x: i32, world_y: i32, world_z: i32) -> (u8, i16, u8) {
    let local_x = (world_x & 15) as u8;
    let local_z = (world_z & 15) as u8;
    let max_y = crate::world::chunk::MIN_Y + i32::from(crate::world::chunk::SUBCHUNK_COUNT) * 16;
    let local_y = if (crate::world::chunk::MIN_Y..max_y).contains(&world_y) {
        world_y as i16
    } else {
        i16::MAX
    };
    (local_x, local_y, local_z)
}

// ============================================================================
// Block Update Observer
// ============================================================================

use super::events::BlockChanged;

/// Observer for immediate reaction to block changes.
///
/// This observer fires synchronously within the same tick as the block change,
/// enabling immediate game logic reactions:
/// - Mark chunk dirty for persistence
/// - TODO: Check neighbor blocks for physics (sand falling, etc.)
/// - TODO: Update lighting
/// - TODO: Trigger redstone updates
///
/// Register with: `world.add_observer(on_block_changed)`
pub fn on_block_changed(trigger: On<BlockChanged>, mut chunks: Query<&mut ChunkStateFlags>) {
    let event = trigger.event();

    // Mark chunk dirty for persistence
    if let Ok(mut state) = chunks.get_mut(event.chunk_entity) {
        state.mark_dirty();
    }

    trace!(
        chunk = ?event.chunk_entity,
        pos = ?event.block_pos,
        old = event.old_block,
        new = event.new_block,
        "Block changed (observer)"
    );

    // TODO: Check neighbor blocks for physics (sand falling, etc.)
    // TODO: Update lighting
    // TODO: Trigger redstone updates
}

/// Plugin-like function to add all chunk systems to a schedule.
/// Call this during ECS setup.
///
/// NOTE: initialize_chunk_loaders was removed. ChunkLoader and LastPublisherState
/// are now included in PlayerBundle at spawn time to avoid archetype changes.
///
/// NOTE: cleanup_disconnecting_player_views was removed. ChunkLoader's on_remove
/// hook now handles cleanup automatically when player entities are despawned.
///
/// ## Streaming
///
/// - `apply_loaded_columns` spawns the columns background loads finished (storage reads,
///   generation, encoding); it runs first so the load slots it frees are refilled by the
///   same tick's requests.
/// - `stream_chunks` sends what each view needs within the player's send window and starts
///   the loads it is missing. Nothing on the tick waits for storage or generation.
pub fn register_chunk_systems(schedule: &mut bevy_ecs::schedule::Schedule) {
    use crate::ecs::{ChunkSet, stage_marker};

    schedule.add_systems(
        (
            // NOTE: initialize_chunk_loaders removed - components now in PlayerBundle
            update_chunk_loaders,
            send_chunk_publisher_updates, // the view precedes every chunk below
            flush_pending_viewers,
            stage_marker("chunks: apply loads"),
            // Finished loads first: the slots they free are refilled this tick.
            apply_loaded_columns,
            stage_marker("chunks: stream"),
            stream_chunks,
            stage_marker("chunks: restore actors"),
            restore_loaded_chunk_actors, // Mobs stored with chunks loaded above
            send_spawn_status,           // after the first view and chunks
            handle_radius_changes,
            stage_marker("chunks: unloads"),
            schedule_chunk_unloads,
            cancel_chunk_unloads,
            process_chunk_unloads,
            stage_marker("chunks: ticking"),
            update_chunk_ticking,
            log_stream_stats,
        )
            .chain()
            .in_set(ChunkSet),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wish_reads_new_columns_and_generates_misses_within_the_budget() {
        let budget = LoadBudget {
            reads: 1,
            generations: 1,
            loads_first: true,
        };
        let mut wish = Wish::default();
        wish.want((0, 0), None, &budget);
        wish.want((1, 0), None, &budget);
        wish.want((2, 0), Some(LoadState::Missing), &budget);
        wish.want((3, 0), Some(LoadState::Reading), &budget);
        assert_eq!(wish.reads, [(0, 0)]);
        assert_eq!(wish.generations, [(2, 0)]);
        assert!(wish.is_full(&budget));

        // A pool world without storage generates what it has not asked for.
        let pool_only = LoadBudget {
            reads: 0,
            generations: 2,
            loads_first: false,
        };
        let mut wish = Wish::default();
        wish.want((0, 0), None, &pool_only);
        assert_eq!(wish.generations, [(0, 0)]);
        assert!(wish.reads.is_empty());
    }

    #[test]
    fn held_chunk_summary_is_rate_limited() {
        let mut report = HeldChunkReport::default();
        assert!(!report.due(0), "nothing held, nothing to report");
        assert!(report.due(1), "the first held chunk is reported at once");
        let repeats = (0..HeldChunkReport::INTERVAL_TICKS * 3)
            .filter(|_| report.due(5))
            .count();
        assert_eq!(repeats, 2, "then once per interval while chunks stay held");
        assert!(!report.due(0));
        assert!(report.due(1), "a new incident is reported at once");
    }
}
