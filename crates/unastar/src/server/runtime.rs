//! Server runtime - orchestrates the main server loop.
//!
//! Provides the `UnastarServer` struct that ties together configuration,
//! networking, and the game server tick loop.

use jolyne::proto::packs::ServerPacks;
use jolyne::status::StaticStatus;
use jolyne::{BedrockListener, BedrockListenerConfig, FlushMode};
use p384::SecretKey;
use rand::thread_rng;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Semaphore, broadcast, mpsc};
use tokio_raknet::RaknetListener;
use tracing::{error, info, trace, warn};

use crate::config::{PlayerDataStore, UnastarConfig};
use crate::network::inbound::session_inbound;
use crate::network::{InboundPacket, InboundQueues, NetworkEvent, run_network_loop};
use crate::plugin::PluginManager;
use crate::server::connect::{accept_join_sequence, spawn_to_dvec3};
use crate::server::{GameServer, PlayerSpawnData};
use crate::storage::LevelDBPlayerProvider;

/// Tick rate (20 TPS = 50ms per tick).
const TICK_DURATION: Duration = Duration::from_millis(50);
/// Join and disconnect events buffered before network tasks wait for the tick loop.
/// Client packets use per-session queues instead (`network::inbound`).
const SESSION_EVENT_CAPACITY: usize = 256;

/// The main server runtime.
///
/// Orchestrates:
/// - Network listener and accept loop
/// - Game server tick loop
/// - Event routing between network and game
pub struct UnastarServer {
    config: Arc<UnastarConfig>,
    player_data_store: Arc<PlayerDataStore>,
    server: GameServer,
    server_key: SecretKey,
    plugin_manager: PluginManager,
}

impl UnastarServer {
    /// Create a new server instance from config.
    pub async fn new(config: Arc<UnastarConfig>) -> Result<Self, Box<dyn std::error::Error>> {
        let player_data_store = Arc::new(PlayerDataStore::new(config.players.data_dir.clone()));

        // Create ECS-based game server
        let mut server = GameServer::with_config(config.server_config());
        if config.players.save_previous_position {
            server.ecs.world_mut().insert_resource(
                crate::server::game::types::PlayerDataStoreResource(player_data_store.clone()),
            );
        }

        // Initialize LevelDB player provider (if enabled)
        if config.players.leveldb_enabled {
            let player_db_path = config.players.data_dir.join("db");
            match LevelDBPlayerProvider::open(&player_db_path) {
                Ok(provider) => {
                    info!(path = %player_db_path.display(), "Opened player LevelDB");
                    server.ecs.world_mut().insert_resource(
                        crate::server::game::types::PlayerProviderResource(Arc::new(provider)),
                    );
                }
                Err(e) => {
                    warn!(error = %e, "Failed to open player LevelDB, persistence disabled");
                }
            }
        }

        // Initialize world provider based on config
        let world_db_path = std::path::PathBuf::from("worlds")
            .join("default")
            .join("db");
        let dimension = config.server_config().world.dimension;
        let storage_provider = config.server_config().world.storage_provider;

        let provider: Option<Arc<dyn crate::storage::WorldProvider>> = match storage_provider {
            crate::world::StorageProvider::LevelDb => {
                match crate::storage::LevelDBWorldProvider::open(&world_db_path, dimension) {
                    Ok(provider) => {
                        info!(path = %world_db_path.display(), "Opened world LevelDB");
                        Some(Arc::new(provider))
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to open world LevelDB, chunk persistence disabled");
                        None
                    }
                }
            }
            crate::world::StorageProvider::BlazeDb => {
                let cache_capacity = config.server_config().world.blazedb_cache_chunks;
                let blaze_config = crate::storage::blazedb::BlazeConfig {
                    cache_capacity,
                    ..Default::default()
                };
                match crate::storage::BlazeDBProvider::open(&world_db_path, Some(blaze_config)) {
                    Ok(provider) => {
                        info!(path = %world_db_path.display(), cache_capacity, "Opened world BlazeDB");
                        Some(provider as Arc<dyn crate::storage::WorldProvider>)
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to open world BlazeDB, chunk persistence disabled");
                        None
                    }
                }
            }
        };

        if let Some(provider) = provider {
            // Set provider on ChunkManager resource for load-before-generate
            let world = server.ecs.world_mut();
            if let Some(mut chunk_manager) =
                world.get_resource_mut::<crate::world::ecs::ChunkManager>()
            {
                chunk_manager.set_provider(provider);
            }
        }

        // Server key for encryption
        let server_key = SecretKey::random(&mut thread_rng());

        // Initialize Plugin Manager
        let mut plugin_manager = PluginManager::new()
            .map_err(|e| format!("Failed to initialize plugin manager: {}", e))?;

        // Load plugins from "plugins" directory relative to CWD
        let plugins_dir = std::env::current_dir()?.join("plugins");
        info!(path = %plugins_dir.display(), "Loading plugins from directory");
        if let Err(e) = plugin_manager.load_plugins(&plugins_dir) {
            warn!(error = %e, "Failed to load plugins");
        }

        info!("Plugin manager initialized, creating server struct");
        Ok(Self {
            config,
            player_data_store,
            server,
            server_key,
            plugin_manager,
        })
    }

    /// Run the server main loop.
    ///
    /// This binds the listener, spawns the accept loop, and runs the tick loop until shutdown.
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("run() called, starting server initialization");

        // Build listener config from server settings
        let listener_config = BedrockListenerConfig {
            online_mode: self.config.server.online_mode,
            allow_legacy_auth: self.config.server.allow_legacy_auth,
            encryption_enabled: self.config.server.encryption_enabled,
            resource_packs: Arc::new(ServerPacks {
                required: self.config.server.require_resource_packs,
                ..ServerPacks::default()
            }),
            // Advertise the configured MOTD, live player count and limit
            // (protocol and game version come from jolyne's pinned surface).
            max_players: self.config.server.max_players as usize,
            status: Some(Arc::new(StaticStatus::new(
                self.config.server.motd.clone(),
                "Unastar",
            ))),
            ..Default::default()
        };

        // Bind listener
        let listener = BedrockListener::raknet()
            .addr(&self.config.server.bind_address)
            .config(listener_config)
            .bind()
            .await?;

        let local_addr = listener.local_addr();
        info!("Listening on {:?}", local_addr);

        // Where dead players respawn: the configured or generated world
        // spawn (no player's previous position).
        let world_spawn = {
            let template = self
                .server
                .ecs
                .world()
                .resource::<crate::server::game::types::ServerWorldTemplate>()
                .0
                .clone();
            crate::server::resolve_spawn_location(
                &self.config,
                None,
                &template,
                &self.player_data_store,
                None,
            )
            .await
        };
        if !self
            .config
            .world
            .bounds
            .contains_block(world_spawn.x, world_spawn.z)
        {
            return Err(format!(
                "the world spawn ({}, {}, {}) is outside `world.bounds`; set a spawn rule \
                 `location` inside the bounds",
                world_spawn.x, world_spawn.y, world_spawn.z
            )
            .into());
        }
        self.server.ecs.world_mut().insert_resource(
            crate::entity::player_life::respawn::WorldSpawn(glam::DVec3::new(
                f64::from(world_spawn.x),
                f64::from(world_spawn.y),
                f64::from(world_spawn.z),
            )),
        );

        // Session lifecycle events. Each session's packets arrive on its own
        // bounded queue, registered here when its `Joined` event is applied.
        let (event_tx, mut event_rx) = mpsc::channel::<NetworkEvent>(SESSION_EVENT_CAPACITY);
        let mut inbound = InboundQueues::default();

        // Tick signal channel - signals network tasks to flush their buffers
        let (tick_tx, _) = broadcast::channel::<()>(16);

        // Gate handshaking + joined players before expensive auth/join work.
        let connection_gate = Arc::new(Semaphore::new(
            self.config.server.max_players.max(1) as usize
        ));

        // Spawn accept loop
        spawn_accept_loop(
            listener,
            AcceptLoopState {
                template: self
                    .server
                    .ecs
                    .world()
                    .get_resource::<crate::server::game::types::ServerWorldTemplate>()
                    .unwrap()
                    .0
                    .clone(),
                world_state: self
                    .server
                    .ecs
                    .world()
                    .resource::<crate::world::rules::StartGameWorldState>()
                    .clone(),
                roster: self
                    .server
                    .ecs
                    .world()
                    .resource::<crate::server::game::roster::OnlineRoster>()
                    .clone(),
                key: self.server_key.clone(),
                config: self.config.clone(),
                player_data_store: self.player_data_store.clone(),
                player_provider: self
                    .server
                    .ecs
                    .world()
                    .get_resource::<crate::server::game::types::PlayerProviderResource>()
                    .map(|resource| resource.0.clone()),
                event_tx,
                tick_tx: tick_tx.clone(),
                connection_gate,
            },
        );

        // Main tick loop
        let mut interval = tokio::time::interval(TICK_DURATION);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        // Server console: stdin lines run as commands with server permission.
        self.server
            .ecs
            .world_mut()
            .insert_resource(crate::server::game::ConsoleCommands::stdin());

        info!("Server ready, entering tick loop");

        let mut tick_count: u64 = 0;

        // TPS tracking
        let mut tps_tick_count: u32 = 0;
        let mut tps_start = std::time::Instant::now();
        let mut last_tps_log = std::time::Instant::now();
        let mut current_tps: f32 = 20.0;

        // Shutdown signal
        let mut shutdown = Box::pin(tokio::signal::ctrl_c());

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    tick_count += 1;
                    tps_tick_count += 1;

                    // Calculate TPS every second
                    let tps_elapsed = tps_start.elapsed();
                    if tps_elapsed.as_secs_f32() >= 1.0 {
                        current_tps = tps_tick_count as f32 / tps_elapsed.as_secs_f32();
                        tps_tick_count = 0;
                        tps_start = std::time::Instant::now();
                    }

                    // Log TPS every 5 seconds
                    if last_tps_log.elapsed().as_secs() >= 5 {
                        let player_count = self
                            .server
                            .ecs
                            .world()
                            .get_resource::<crate::server::game::SessionEntityMap>()
                            .map(|m| m.len())
                            .unwrap_or(0);
                        info!(
                            tps = format!("{:.1}", current_tps),
                            players = player_count,
                            tick = tick_count,
                            "Server TPS"
                        );
                        last_tps_log = std::time::Instant::now();
                    }

                    let tick_start = std::time::Instant::now();

                    let tick_logic_start = std::time::Instant::now();
                    let plugin_manager = &mut self.plugin_manager;
                    let events_processed = network_tick(
                        &mut self.server,
                        &mut event_rx,
                        &mut inbound,
                        self.config.players.save_on_disconnect,
                        |server| {
                            // Game tick (queues packets to broadcast systems), then plugins.
                            server.tick();
                            plugin_manager.tick(server.ecs.world_mut());
                        },
                    )
                    .await;
                    let tick_logic_elapsed = tick_logic_start.elapsed();

                    // Signal all network tasks to flush their buffers
                    let _ = tick_tx.send(());

                    let total_elapsed = tick_start.elapsed();

                    // Log slow ticks (> 20ms = taking more than half our budget)
                    if total_elapsed.as_millis() > 20 || tick_count.is_multiple_of(200) {
                        trace!(
                            tick = tick_count,
                            events = events_processed,
                            tick_ms = tick_logic_elapsed.as_millis(),
                            total_ms = total_elapsed.as_millis(),
                            "Tick timing"
                        );
                    }
                    if total_elapsed.as_millis() > 40 {
                        warn!(
                            tick = tick_count,
                            total_ms = total_elapsed.as_millis(),
                            "SLOW TICK detected"
                        );
                    }
                    if self.server.ecs.world().contains_resource::<crate::command::exec::StopRequested>() {
                        info!("Stop requested by command, saving data...");
                        return self.shutdown().await;
                    }
                }
                _ = &mut shutdown => {
                    info!("Shutdown signal received, saving data...");
                    return self.shutdown().await;
                }
            }
        }
    }

    /// Saves players and chunks before the process exits.
    async fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let players_saved = self.server.save_all_players().await;
        info!(players = players_saved, "Player data saved");
        let chunks_saved = self.server.save_all_chunks().await;
        info!(chunks = chunks_saved, "Chunk data saved");
        info!("Server shutdown complete");
        Ok(())
    }
}

/// One tick of the server loop as far as the network is concerned, in this order:
/// apply join events, route each session's packets within its per-tick budget, drop
/// what leaving sessions still had queued, run `game_tick` (which applies the routed
/// packets), then save and despawn the leaving sessions. Saving after the game tick is
/// what makes a leaving player's last routed packets part of its saved state. Returns
/// how many events and packets were taken.
async fn network_tick(
    server: &mut GameServer,
    event_rx: &mut mpsc::Receiver<NetworkEvent>,
    inbound: &mut InboundQueues,
    save_on_disconnect: bool,
    game_tick: impl FnOnce(&mut GameServer),
) -> usize {
    let mut events_processed = 0usize;
    let mut disconnected = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        events_processed += 1;
        match event {
            NetworkEvent::Joined {
                session_id,
                display_name,
                xuid,
                uuid,
                runtime_id,
                initial_position,
                saved_data,
                client_data,
                chunk_radius,
                outbound_tx,
                inbound_rx,
            } => {
                // Registered even when the spawn is refused: the router drops packets
                // of sessions without a player.
                inbound.register(session_id, inbound_rx);
                spawn_or_reject(
                    server,
                    client_data,
                    PlayerSpawnData {
                        session_id,
                        display_name,
                        xuid,
                        uuid,
                        runtime_id,
                        position: initial_position,
                        saved_data: saved_data.map(|data| *data),
                        outbound_tx,
                        chunk_radius,
                    },
                );
            }
            NetworkEvent::Disconnected { session_id } => disconnected.push(session_id),
        }
    }
    let mut route = |session_id, (packet_args, packet): InboundPacket| {
        server.route_packet(session_id, packet_args, *packet);
    };
    events_processed += inbound.drain_tick(&mut route);
    for &session_id in &disconnected {
        inbound.close(session_id);
    }
    game_tick(server);
    for session_id in disconnected {
        if save_on_disconnect && server.save_player(session_id).await {
            info!(session_id, "Player data saved on disconnect");
        }
        server.despawn_player(session_id);
    }
    events_processed
}

/// Message of the `Disconnect` sent to a player whose saved data cannot be restored.
const UNRESTORABLE_PLAYER_MESSAGE: &str = "Unable to restore player inventory.";

/// Spawn a joined player, or refuse the join with a `Disconnect` when its saved data
/// cannot be restored (for example an item the registry does not know). A refused
/// player never enters the world, so its stored record is left untouched.
fn spawn_or_reject(
    server: &mut GameServer,
    client_data: Option<Box<jolyne::auth::ClientData>>,
    data: PlayerSpawnData,
) -> bool {
    use jolyne::valentine::{
        DisconnectPacket, DisconnectPacketMessages, DisconnectPacketMessagesUnion,
        EnumsConnectionDisconnectFailReason, McpePacket,
    };
    let session_id = data.session_id;
    let reject_tx = data.outbound_tx.clone();
    let Err(error) = server.spawn_player_with_login(data, client_data.as_deref()) else {
        return true;
    };
    warn!(session_id, %error, "Rejecting player with invalid saved data");
    let _ = reject_tx.try_send(McpePacket::from(DisconnectPacket {
        reason: EnumsConnectionDisconnectFailReason::Disconnected,
        messages: DisconnectPacketMessagesUnion::DisconnectPacketMessages(
            DisconnectPacketMessages {
                message: UNRESTORABLE_PLAYER_MESSAGE.into(),
                filtered_message: UNRESTORABLE_PLAYER_MESSAGE.into(),
            },
        ),
    }));
    false
}

struct AcceptLoopState {
    template: Arc<jolyne::WorldTemplate>,
    world_state: crate::world::rules::StartGameWorldState,
    roster: crate::server::game::roster::OnlineRoster,
    key: SecretKey,
    config: Arc<UnastarConfig>,
    player_data_store: Arc<PlayerDataStore>,
    player_provider: Option<Arc<dyn crate::storage::PlayerProvider>>,
    event_tx: mpsc::Sender<NetworkEvent>,
    tick_tx: broadcast::Sender<()>,
    connection_gate: Arc<Semaphore>,
}

/// Spawn the accept loop as a background task.
fn spawn_accept_loop(mut listener: BedrockListener<RaknetListener>, state: AcceptLoopState) {
    tokio::spawn(async move {
        let mut next_session_id: u64 = 1;
        let AcceptLoopState {
            template,
            world_state,
            roster,
            key,
            config,
            player_data_store,
            player_provider,
            event_tx,
            tick_tx,
            connection_gate,
        } = state;

        loop {
            match listener.accept().await {
                Ok(handshake_stream) => {
                    let addr = handshake_stream.peer_addr();
                    info!(%addr, "Connection accepted");

                    let permit = match connection_gate.clone().try_acquire_owned() {
                        Ok(permit) => permit,
                        Err(_) => {
                            warn!(
                                %addr,
                                max_players = config.server.max_players,
                                "Rejecting connection because server is at capacity"
                            );
                            continue;
                        }
                    };

                    let template = template.clone();
                    let world_state = world_state.clone();
                    let roster = roster.clone();
                    let key = key.clone();
                    let config = config.clone();
                    let player_data_store = player_data_store.clone();
                    let player_provider = player_provider.clone();
                    let event_tx = event_tx.clone();
                    let tick_rx = tick_tx.subscribe();
                    let session_id = next_session_id;
                    next_session_id += 1;

                    tokio::spawn(async move {
                        let _connection_permit = permit;
                        match accept_join_sequence(
                            crate::server::connect::JoinWorld {
                                template: &template,
                                state: &world_state,
                                roster: &roster,
                            },
                            &key,
                            &config,
                            &player_data_store,
                            player_provider.as_deref(),
                            session_id,
                            handshake_stream,
                        )
                        .await
                        {
                            Ok(crate::server::connect::AcceptedJoin {
                                play: mut play_stream,
                                identity,
                                position: initial_position,
                                saved_data,
                                client_data,
                                reservation,
                            }) => {
                                let display_name = identity.display_name().to_string();

                                info!(%addr, name = %display_name, "Player joined");

                                // The streamed spawn ends at the radius request.
                                let Some(chunk_radius) = play_stream.chunk_radius() else {
                                    error!(%addr, "Streamed spawn ended without a chunk radius");
                                    return;
                                };

                                // Packets are batched per tick; the network loop flushes.
                                if let Err(e) = play_stream.set_flush_mode(FlushMode::Manual).await
                                {
                                    error!(%addr, "Switching to manual flush failed: {e}");
                                    return;
                                }

                                // Create bounded outbound channel to prevent memory explosion.
                                // Bound to ~50MB worth of packets (assuming average 50KB per packet = 1024 packets).
                                // This prevents memory explosion while allowing reasonable buffering.
                                const OUTBOUND_CHANNEL_CAPACITY: usize = 1024;
                                let (outbound_tx, outbound_rx) =
                                    mpsc::channel(OUTBOUND_CHANNEL_CAPACITY);
                                let (inbound_tx, inbound_rx) = session_inbound();

                                // Send joined event
                                if event_tx
                                    .send(NetworkEvent::Joined {
                                        session_id,
                                        display_name: display_name.clone(),
                                        xuid: identity.xuid().map(str::to_owned),
                                        uuid: Some(identity.uuid().to_string()),
                                        runtime_id: session_id as i64,
                                        initial_position: spawn_to_dvec3(&initial_position),
                                        saved_data: saved_data.map(Box::new),
                                        client_data: Some(Box::new(client_data)),
                                        chunk_radius,
                                        outbound_tx: outbound_tx.clone(),
                                        inbound_rx,
                                    })
                                    .await
                                    .is_err()
                                {
                                    return; // Server shutting down
                                }
                                // The game loop frees the place when the
                                // session's Disconnected arrives.
                                reservation.keep();

                                run_network_loop(
                                    play_stream,
                                    session_id,
                                    event_tx,
                                    inbound_tx,
                                    outbound_rx,
                                    tick_rx,
                                )
                                .await;
                            }
                            Err(e) => {
                                error!(%addr, "Handshake failed: {:?}", e);
                            }
                        }
                    });
                }
                Err(e) => {
                    error!("Accept failed: {:?}", e);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{PlayerData, StoredItemStack};
    use jolyne::valentine::{DisconnectPacketMessagesUnion, McpePacketData};

    /// A saved item the registry cannot resolve refuses the join with a `Disconnect`
    /// instead of spawning a player with a silently emptied inventory.
    #[test]
    fn join_with_unresolvable_saved_items_is_rejected() {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let _guard = runtime.enter();
                let mut server = GameServer::with_config(crate::ServerConfig {
                    world: crate::world::WorldConfig::void_spawn_platform(1),
                    ..Default::default()
                });
                let uuid = uuid::Uuid::from_u128(77).to_string();
                let join = |session_id, item_id: &str| {
                    let (tx, rx) = mpsc::channel(256);
                    let mut saved = PlayerData {
                        uuid: uuid.clone(),
                        ..PlayerData::default()
                    };
                    saved.inventory = vec![StoredItemStack {
                        item_id: item_id.into(),
                        count: 1,
                        ..StoredItemStack::default()
                    }];
                    let data = PlayerSpawnData {
                        session_id,
                        display_name: "restored".into(),
                        xuid: None,
                        uuid: Some(uuid.clone()),
                        runtime_id: session_id as i64,
                        position: glam::DVec3::new(0.5, 17.0, 0.5),
                        saved_data: Some(saved),
                        outbound_tx: tx,
                        chunk_radius: 0,
                    };
                    (data, rx)
                };

                let (data, mut rx) = join(1, "minecraft:not_a_real_item");
                assert!(!spawn_or_reject(&mut server, None, data));
                assert!(
                    server
                        .ecs
                        .world()
                        .resource::<crate::SessionEntityMap>()
                        .get(1)
                        .is_none()
                );
                let packets: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
                let [packet] = packets.as_slice() else {
                    panic!("expected only a Disconnect, got {packets:?}");
                };
                let McpePacketData::DisconnectPacket(disconnect) = &packet.data else {
                    panic!("expected a Disconnect, got {packet:?}");
                };
                let DisconnectPacketMessagesUnion::DisconnectPacketMessages(messages) =
                    &disconnect.messages
                else {
                    panic!("Disconnect without a message: {disconnect:?}");
                };
                assert_eq!(messages.message, UNRESTORABLE_PLAYER_MESSAGE);

                let (data, _rx) = join(2, "minecraft:stone");
                assert!(spawn_or_reject(&mut server, None, data));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

/// The tick loop's handling of joins, packets and leaves, driven through
/// [`network_tick`] exactly as `run()` drives it.
#[cfg(test)]
mod network_tick_tests {
    use super::*;
    use crate::network::inbound::session_inbound;
    use crate::storage::{PlayerData, PlayerProvider, StorageResult};
    use bytes::BytesMut;
    use jolyne::valentine::types::Vec3;
    use jolyne::valentine::{
        BorrowedMcpePacket, McpePacket, McpePacketArgs, PlayerAuthInputPacket,
    };
    use std::collections::HashMap;

    const SESSION: u64 = 5;
    const START_X: f64 = 0.5;

    /// Player records kept in memory.
    #[derive(Default)]
    struct MemoryPlayers(parking_lot::Mutex<HashMap<uuid::Uuid, PlayerData>>);

    #[async_trait::async_trait]
    impl PlayerProvider for MemoryPlayers {
        async fn load(&self, uuid: uuid::Uuid) -> StorageResult<Option<PlayerData>> {
            Ok(self.0.lock().get(&uuid).cloned())
        }
        async fn save(&self, uuid: uuid::Uuid, data: &PlayerData) -> StorageResult<()> {
            self.0.lock().insert(uuid, data.clone());
            Ok(())
        }
        async fn delete(&self, uuid: uuid::Uuid) -> StorageResult<()> {
            self.0.lock().remove(&uuid);
            Ok(())
        }
        async fn close(&self) -> StorageResult<()> {
            Ok(())
        }
    }

    /// A packet as the network task hands it over: encoded, then decoded borrowed.
    fn inbound(packet: impl Into<McpePacket>) -> InboundPacket {
        let packet: McpePacket = packet.into();
        let mut buf = BytesMut::new();
        packet.data.encode_inner(&mut buf, 0, 0).unwrap();
        let borrowed = BorrowedMcpePacket::decode_inner(&mut buf.freeze()).unwrap();
        (McpePacketArgs, Box::new(borrowed))
    }

    /// One `PlayerAuthInput` moving the player to `x` (eye height kept).
    fn move_to(x: f64) -> InboundPacket {
        inbound(PlayerAuthInputPacket {
            position: Vec3 {
                x: x as f32,
                y: 17.0 + 1.62,
                z: 0.5,
            },
            ..Default::default()
        })
    }

    struct Harness {
        server: GameServer,
        players: Arc<MemoryPlayers>,
        events: mpsc::Sender<NetworkEvent>,
        event_rx: mpsc::Receiver<NetworkEvent>,
        inbound: InboundQueues,
        packets: mpsc::Sender<InboundPacket>,
        /// Keeps the session's outbound queue open.
        _outbound: mpsc::Receiver<McpePacket>,
    }

    fn uuid() -> uuid::Uuid {
        uuid::Uuid::from_u128(u128::from(SESSION))
    }

    /// A server with in-memory player storage and one session's `Joined` queued.
    fn harness() -> Harness {
        let mut server = GameServer::with_config(crate::ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        let players = Arc::new(MemoryPlayers::default());
        server
            .ecs
            .world_mut()
            .insert_resource(crate::server::game::types::PlayerProviderResource(
                players.clone(),
            ));
        let (events, event_rx) = mpsc::channel(16);
        let (outbound_tx, outbound) = mpsc::channel(4096);
        let (packets, inbound_rx) = session_inbound();
        events
            .try_send(NetworkEvent::Joined {
                session_id: SESSION,
                display_name: "leaver".into(),
                xuid: None,
                uuid: Some(uuid().to_string()),
                runtime_id: SESSION as i64,
                initial_position: glam::DVec3::new(START_X, 17.0, 0.5),
                saved_data: None,
                client_data: None,
                chunk_radius: 0,
                outbound_tx,
                inbound_rx,
            })
            .unwrap();
        Harness {
            server,
            players,
            events,
            event_rx,
            inbound: InboundQueues::default(),
            packets,
            _outbound: outbound,
        }
    }

    impl Harness {
        async fn tick(&mut self) -> usize {
            network_tick(
                &mut self.server,
                &mut self.event_rx,
                &mut self.inbound,
                true,
                GameServer::tick,
            )
            .await
        }

        /// The network task ends: its queue closes, then `Disconnected` follows.
        fn leave(&mut self) {
            let (closed, _) = mpsc::channel(1);
            drop(std::mem::replace(&mut self.packets, closed));
            self.events
                .try_send(NetworkEvent::Disconnected {
                    session_id: SESSION,
                })
                .unwrap();
        }

        fn saved_x(&self) -> Option<f64> {
            self.players
                .0
                .lock()
                .get(&uuid())
                .map(|data| data.position[0])
        }

        fn joined(&self) -> bool {
            self.server
                .ecs
                .world()
                .resource::<crate::SessionEntityMap>()
                .get(SESSION)
                .is_some()
        }
    }

    /// Runs `test` on a current-thread runtime on a thread with a large stack (the
    /// server's join packets need it in debug builds).
    fn on_big_stack<F, Fut>(test: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()>,
    {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(test());
            })
            .unwrap()
            .join()
            .unwrap();
    }

    /// register -> drain -> close -> save -> despawn: the packets routed in the
    /// leaving tick are applied before the save, and the rest of the queue is dropped
    /// rather than routed past the per-tick budget.
    #[test]
    fn a_leaving_session_is_saved_after_its_budgeted_packets_and_the_rest_are_dropped() {
        on_big_stack(|| async {
            let mut h = harness();
            // 100 one-block steps east, all queued before the tick that sees the leave.
            for step in 1..=100 {
                h.packets
                    .try_send(move_to(START_X + f64::from(step)))
                    .unwrap();
            }
            h.leave();
            let taken = h.tick().await;

            // Joined + Disconnected + one tick's budget of packets.
            assert_eq!(
                taken,
                2 + crate::network::inbound::MAX_PACKETS_PER_SESSION_PER_TICK
            );
            assert!(!h.joined(), "the leaver is despawned");
            assert_eq!(
                h.saved_x(),
                Some(START_X + crate::network::inbound::MAX_PACKETS_PER_SESSION_PER_TICK as f64),
                "the save sees the 64 packets applied this tick and none of the rest"
            );
            assert!(h.inbound.is_empty());
        });
    }

    /// A client that says goodbye with its own `Disconnect` is saved like any other
    /// leaver; the packet used to despawn it at once, so the save found no player.
    #[test]
    fn a_client_disconnect_packet_still_saves_the_player() {
        on_big_stack(|| async {
            let mut h = harness();
            h.packets.try_send(move_to(START_X + 3.0)).unwrap();
            h.packets
                .try_send(inbound(jolyne::valentine::DisconnectPacket::default()))
                .unwrap();
            h.leave();
            h.tick().await;

            assert!(!h.joined(), "the leaver is despawned");
            assert_eq!(h.saved_x(), Some(START_X + 3.0), "the leaver is saved");
        });
    }
}
