//! Server runtime - orchestrates the main server loop.
//!
//! Provides the `UnastarServer` struct that ties together configuration,
//! networking, and the game server tick loop.

use jolyne::listener::RawListener;
use jolyne::proto::packs::ServerPacks;
use jolyne::status::{ServerStatus, StatusGameMode, StatusProvider};
use jolyne::{BedrockListener, BedrockListenerConfig, FlushMode};
use p384::SecretKey;
use rand::thread_rng;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Semaphore, broadcast, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tracing::{error, info, trace, warn};

use crate::config::{PlayerDataStore, UnastarConfig};
use crate::network::{NetworkEvent, run_network_loop_with_metrics};
use crate::plugin::PluginManager;
use crate::server::connect::{JoinWorldTemplate, accept_join_sequence_in_worlds, spawn_to_dvec3};
use crate::server::worlds::DimensionRouter;
use crate::server::{GameServer, PlayerSpawnData};
use crate::storage::LevelDBPlayerProvider;
mod configured_worlds;

/// Tick rate (20 TPS = 50ms per tick).
const TICK_DURATION: Duration = Duration::from_millis(50);
/// Inbound events buffered per configured player before network tasks apply backpressure.
const NETWORK_EVENTS_PER_PLAYER: usize = 128;
/// Minimum inbound queue capacity for small servers.
const MIN_NETWORK_EVENT_CAPACITY: usize = 1024;
/// Maximum packets from one session applied to domain queues in a single tick.
const MAX_PACKET_EVENTS_PER_SESSION_PER_TICK: u32 = 64;

/// The main server runtime.
///
/// Orchestrates:
/// - Network listener and accept loop
/// - Game server tick loop
/// - Event routing between network and game
pub struct UnastarServer {
    config: Arc<UnastarConfig>,
    player_data_store: Arc<PlayerDataStore>,
    server: DimensionRouter,
    server_key: SecretKey,
}

impl UnastarServer {
    /// Create a new server instance from config.
    pub async fn new(config: Arc<UnastarConfig>) -> Result<Self, Box<dyn std::error::Error>> {
        config.validate().map_err(std::io::Error::other)?;
        configured_worlds::validate_directories(&config)?;
        let player_data_store = Arc::new(PlayerDataStore::new(config.players.data_dir.clone()));

        // Create ECS-based game server
        let mut server = GameServer::with_config(config.server_config());
        if config.players.save_previous_position {
            server.ecs.world_mut().insert_resource(
                crate::server::game::types::PlayerDataStoreResource(player_data_store.clone()),
            );
        }

        if config.players.leveldb_enabled {
            let player_db_path = config.players.data_dir.join("db");
            let provider = LevelDBPlayerProvider::open(&player_db_path)?;
            server.ecs.world_mut().insert_resource(
                crate::server::game::types::PlayerProviderResource(Arc::new(provider)),
            );
            crate::server::game::inventory_effects::prepare_shared_writer(server.ecs.world_mut())?;
        }
        let primary_id =
            configured_worlds::install(&mut server, &config.world_storage, config.world)?;
        let mut server = DimensionRouter::new(server);
        let mut identities = std::collections::BTreeSet::from([primary_id]);
        for entry in &config.secondary_worlds {
            let mut native = config.server_config();
            native.world = entry.world;
            let mut world = server.create_world(native)?;
            let id = configured_worlds::install(&mut world, &entry.storage(), entry.world)?;
            if !identities.insert(id) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "configured worlds have duplicate durable identities",
                )
                .into());
            }
            server.add(world)?;
        }

        // Server key for encryption
        let server_key = SecretKey::random(&mut thread_rng());

        // Initialize Plugin Manager
        let mut plugin_manager = PluginManager::new()
            .map_err(|e| format!("Failed to initialize plugin manager: {}", e))?;
        plugin_manager.require_plugins(&config.plugins.required);
        plugin_manager.configure_storage(&config.plugins.storage)?;

        // Load plugins from "plugins" directory relative to CWD
        let plugins_dir = std::env::current_dir()?.join("plugins");
        info!(path = %plugins_dir.display(), "Loading plugins from directory");
        if let Err(error) = plugin_manager.load_plugins(&plugins_dir) {
            plugin_manager
                .check_required()
                .map_err(std::io::Error::other)?;
            warn!(%error, "Optional component discovery failed");
        }

        let plugin_manager = Arc::new(Mutex::new(plugin_manager));
        for (_, world) in server.iter_mut() {
            PluginManager::attach_to_world(Arc::clone(&plugin_manager), world.ecs.world_mut());
        }
        server.set_plugin_manager(plugin_manager.clone());
        info!("Plugin manager initialized, creating server struct");
        Ok(Self {
            config,
            player_data_store,
            server,
            server_key,
        })
    }

    /// Access the initialized game server and its ECS state.
    pub fn game_server(&self) -> &GameServer {
        &self.server
    }

    /// Configure the initialized game server before starting the network loop.
    ///
    /// Native extensions can insert ECS resources and observers or add systems to
    /// the existing tick schedule. These changes are retained by [`Self::run`].
    pub fn game_server_mut(&mut self) -> &mut GameServer {
        &mut self.server
    }

    /// Read the real dimension worlds and current session routing.
    pub fn worlds(&self) -> &DimensionRouter {
        &self.server
    }

    /// Register and configure additional worlds before starting the listener.
    /// Each world needs its own terrain provider and native application resources.
    pub fn worlds_mut(&mut self) -> &mut DimensionRouter {
        &mut self.server
    }

    /// Run the server main loop.
    ///
    /// This binds the listener, spawns the accept loop, and runs the tick loop until shutdown.
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("run() called, starting server initialization");

        self.config
            .validate_network()
            .map_err(std::io::Error::other)?;
        self.server.initialize_plugins()?;
        let status = Arc::new(RuntimeStatus::new(&self.config));

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
            status: Some(status.clone()),
            ..Default::default()
        };

        // Bind listener
        let listener = BedrockListener::raknet()
            .addr(&self.config.server.bind_address)
            .config(listener_config.clone())
            .bind()
            .await?;

        let local_addr = listener.local_addr();
        info!("Listening on {:?}", local_addr);
        let mut network_tasks = NetworkTasks::default();
        let nethernet = if let Some(endpoint) = &self.config.server.nethernet {
            let (handler, nethernet) = bind_nethernet(
                endpoint,
                listener_config,
                tokio_nethernet::TransportConfig::default(),
            )
            .await?;
            info!(address = ?handler.local_addr(), "NetherNet HTTP signaling listening");
            network_tasks.endpoint = Some(handler);
            Some(nethernet)
        } else {
            None
        };

        // Where dead players respawn: the configured or generated world
        // spawn (no player's previous position).
        for (_, server) in self.server.iter_mut() {
            let template = server
                .ecs
                .world()
                .resource::<crate::server::game::types::ServerWorldTemplate>()
                .0
                .clone();
            let mut config = (*self.config).clone();
            config.spawn_rules = self
                .config
                .spawn_rules_for(
                    server
                        .ecs
                        .world()
                        .resource::<crate::world::ecs::ChunkManager>()
                        .dimension(),
                )
                .unwrap_or(&self.config.spawn_rules)
                .to_vec();
            config.world = *server
                .ecs
                .world()
                .resource::<crate::world::ecs::ChunkManager>()
                .world_config();
            let spawn = crate::server::resolve_spawn_location(
                &config,
                None,
                &template,
                &self.player_data_store,
                None,
            )
            .await;
            server.ecs.world_mut().insert_resource(
                crate::entity::player_life::respawn::WorldSpawn(glam::DVec3::new(
                    f64::from(spawn.x),
                    f64::from(spawn.y),
                    f64::from(spawn.z),
                )),
            );
        }

        // Consolidated bounded event channel. This is the main inbound
        // backpressure point between network tasks and the 20 TPS game loop.
        let event_channel_capacity = (self.config.server.max_players.max(1) as usize)
            .saturating_mul(NETWORK_EVENTS_PER_PLAYER)
            .max(MIN_NETWORK_EVENT_CAPACITY);
        let (event_tx, mut event_rx) = mpsc::channel::<NetworkEvent>(event_channel_capacity);

        // Tick signal channel - signals network tasks to flush their buffers
        let (tick_tx, _) = broadcast::channel::<()>(16);

        // Gate handshaking + joined players before expensive auth/join work.
        let connection_gate = Arc::new(Semaphore::new(
            self.config.server.max_players.max(1) as usize
        ));

        // Spawn accept loop
        let accept_state = AcceptLoopState {
            worlds: self
                .server
                .iter()
                .map(|(dimension, server)| {
                    (
                        dimension,
                        JoinWorldTemplate {
                            template: server
                                .ecs
                                .world()
                                .resource::<crate::server::game::types::ServerWorldTemplate>()
                                .0
                                .clone(),
                            state: server
                                .ecs
                                .world()
                                .resource::<crate::world::rules::StartGameWorldState>()
                                .clone(),
                            world_config: *server
                                .ecs
                                .world()
                                .resource::<crate::world::ecs::ChunkManager>()
                                .world_config(),
                        },
                    )
                })
                .collect(),
            primary: self.server.primary_dimension(),
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
            next_session_id: Arc::new(AtomicU64::new(1)),
        };
        network_tasks
            .tasks
            .push(spawn_accept_loop(listener, accept_state.clone()));
        if let Some(nethernet) = nethernet {
            network_tasks
                .tasks
                .push(spawn_accept_loop(nethernet, accept_state));
        }

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
        let mut join_dimensions = HashMap::new();

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
                        let player_count: usize = self.server.iter().map(|(_,server)|server.ecs.world().resource::<crate::server::game::SessionEntityMap>().len()).sum();
                        info!(
                            tps = format!("{:.1}", current_tps),
                            players = player_count,
                            tick = tick_count,
                            "Server TPS"
                        );
                        last_tps_log = std::time::Instant::now();
                    }

                    let tick_start = std::time::Instant::now();

                    // Drain network events
                    let mut events_processed = 0u32;
                    let mut per_session_packets: HashMap<_, u32> = HashMap::new();
                    while let Ok(event) = event_rx.try_recv() {
                        events_processed += 1;
                        match event {
                            NetworkEvent::WorldSelected { session_id, dimension } => {
                                join_dimensions.insert(session_id, dimension);
                            }
                            NetworkEvent::Joined {
                                session_id,
                                display_name,
                                xuid,
                                uuid,
                                runtime_id,
                                initial_position,
                                saved_data,
                                chunk_radius,
                                outbound_tx,
                            } => {
                                let reject_tx = outbound_tx.clone();
                                let spawn_data = PlayerSpawnData {
                                    session_id,
                                    display_name,
                                    xuid,
                                    uuid,
                                    runtime_id,
                                    position: initial_position,
                                    saved_data: saved_data.map(|data| *data),
                                    outbound_tx,
                                    chunk_radius,
                                };
                                let dimension = join_dimensions.remove(&session_id).unwrap_or(self.server.primary_dimension());
                                if let Err(error) = self.server.spawn_player_in(dimension, spawn_data) {
                                    use jolyne::valentine::{
                                        DisconnectPacket, DisconnectPacketMessages,
                                        DisconnectPacketMessagesUnion, EnumsConnectionDisconnectFailReason, McpePacket,
                                    };
                                    warn!(session_id, %error, "Rejecting player with invalid saved inventory");
                                    let message = "Unable to restore player inventory.".into();
                                    let _ = reject_tx.try_send(McpePacket::from(DisconnectPacket {
                                        reason: EnumsConnectionDisconnectFailReason::Disconnected,
                                        messages: DisconnectPacketMessagesUnion::DisconnectPacketMessages(
                                            DisconnectPacketMessages {
                                                message,
                                                filtered_message: "Unable to restore player inventory.".into(),
                                            }
                                        ),
                                    }));
                                }
                            }
                            NetworkEvent::Packet {
                                session_id,
                                packet_args,
                                packet,
                            } => {
                                let count = per_session_packets.entry(session_id).or_default();
                                *count += 1;
                                if *count > MAX_PACKET_EVENTS_PER_SESSION_PER_TICK {
                                    warn!(
                                        session_id,
                                        budget = MAX_PACKET_EVENTS_PER_SESSION_PER_TICK,
                                        "dropping inbound packet over per-tick session budget"
                                    );
                                    continue;
                                }
                                self.server.route_packet(session_id, packet_args, *packet);
                            }
                            NetworkEvent::MetricsReady { session_id, metrics } => {
                                self.server.attach_network_metrics(session_id, metrics);
                            }
                            NetworkEvent::Disconnected { session_id } => {
                                join_dimensions.remove(&session_id);
                                if self.config.players.save_on_disconnect {
                                    let saved = self.server.save_player(session_id).await;
                                    if saved {
                                        info!(session_id, "Player data saved on disconnect");
                                    }
                                }
                                self.server.despawn_player(session_id);
                            }
                        }
                    }

                    let events_elapsed = tick_start.elapsed();

                    status.players.store(
                        self.server.iter().map(|(_, server)| server.ecs.world().resource::<crate::server::game::SessionEntityMap>().len()).sum(),
                        Ordering::Relaxed,
                    );
                    // Run game tick (this queues packets to broadcast systems)
                    let tick_logic_start = std::time::Instant::now();
                    self.server.tick().await?;

                    let tick_logic_elapsed = tick_logic_start.elapsed();

                    // Signal all network tasks to flush their buffers
                    let _ = tick_tx.send(());

                    let total_elapsed = tick_start.elapsed();

                    // Log slow ticks (> 20ms = taking more than half our budget)
                    if total_elapsed.as_millis() > 20 || tick_count.is_multiple_of(200) {
                        trace!(
                            tick = tick_count,
                            events = events_processed,
                            events_ms = events_elapsed.as_millis(),
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
                    if self.server.stop_requested() {
                        info!("Stop requested by command, saving data...");
                        network_tasks.stop().await;
                        return self.shutdown().await;
                    }
                }
                _ = &mut shutdown => {
                    info!("Shutdown signal received, saving data...");
                    network_tasks.stop().await;
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
        self.server.flush_worlds().await?;
        info!(chunks = chunks_saved, "Chunk data saved");
        info!("Server shutdown complete");
        Ok(())
    }
}

#[derive(Clone)]
struct AcceptLoopState {
    worlds: BTreeMap<i32, JoinWorldTemplate>,
    primary: i32,
    key: SecretKey,
    config: Arc<UnastarConfig>,
    player_data_store: Arc<PlayerDataStore>,
    player_provider: Option<Arc<dyn crate::storage::PlayerProvider>>,
    event_tx: mpsc::Sender<NetworkEvent>,
    tick_tx: broadcast::Sender<()>,
    connection_gate: Arc<Semaphore>,
    next_session_id: Arc<AtomicU64>,
}

/// Spawn the accept loop as a background task.
fn spawn_accept_loop<L: RawListener + Sync + 'static>(
    mut listener: BedrockListener<L>,
    state: AcceptLoopState,
) -> JoinHandle<()>
where
    L::Transport: 'static,
{
    tokio::spawn(async move {
        let mut connections = JoinSet::new();
        let AcceptLoopState {
            worlds,
            primary,
            key,
            config,
            player_data_store,
            player_provider,
            event_tx,
            tick_tx,
            connection_gate,
            next_session_id,
        } = state;

        loop {
            let accepted = tokio::select! {
                result = listener.accept() => result,
                Some(_) = connections.join_next(), if !connections.is_empty() => continue,
            };
            match accepted {
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

                    let worlds = worlds.clone();
                    let key = key.clone();
                    let config = config.clone();
                    let player_data_store = player_data_store.clone();
                    let player_provider = player_provider.clone();
                    let event_tx = event_tx.clone();
                    let tick_rx = tick_tx.subscribe();
                    let session_id = match next_session_id.fetch_update(
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                        |id| (id < i64::MAX as u64).then_some(id + 1),
                    ) {
                        Ok(id) => id,
                        Err(_) => {
                            error!("Session identity space exhausted");
                            break;
                        }
                    };

                    connections.spawn(async move {
                        let _connection_permit = permit;
                        let timeout = Duration::from_secs(config.server.handshake_timeout_seconds);
                        match tokio::time::timeout(
                            timeout,
                            accept_join_sequence_in_worlds(
                                &worlds,
                                primary,
                                &key,
                                &config,
                                &player_data_store,
                                player_provider.as_deref(),
                                session_id,
                                handshake_stream,
                            ),
                        )
                        .await
                        {
                            Ok(Ok((
                                mut play_stream,
                                identity,
                                initial_position,
                                saved_data,
                                dimension,
                            ))) => {
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

                                if event_tx
                                    .send(NetworkEvent::WorldSelected {
                                        session_id,
                                        dimension,
                                    })
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
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
                                        chunk_radius,
                                        outbound_tx: outbound_tx.clone(),
                                    })
                                    .await
                                    .is_err()
                                {
                                    return; // Server shutting down
                                }

                                let (metrics, publisher) =
                                    crate::entity::components::PlayerNetworkMetrics::bind(
                                        session_id,
                                        &outbound_tx,
                                    );
                                if event_tx
                                    .send(NetworkEvent::MetricsReady {
                                        session_id,
                                        metrics,
                                    })
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                                run_network_loop_with_metrics(
                                    play_stream,
                                    session_id,
                                    event_tx,
                                    outbound_rx,
                                    tick_rx,
                                    Some(publisher),
                                )
                                .await;
                            }
                            Ok(Err(e)) => {
                                error!(%addr, "Handshake failed: {:?}", e);
                            }
                            Err(_) => {
                                warn!(%addr, "Bedrock join timed out");
                            }
                        }
                    });
                }
                Err(e) => {
                    error!("Accept failed: {:?}", e);
                    break;
                }
            }
        }
    })
}

/// Shared advertised totals across every native transport listener.
struct RuntimeStatus {
    name: String,
    max: usize,
    mode: StatusGameMode,
    players: AtomicUsize,
}
impl RuntimeStatus {
    /// Build the shared status independently of each listener's transport slots.
    fn new(config: &UnastarConfig) -> Self {
        let mode = match config.server.default_gamemode.to_ascii_lowercase().as_str() {
            "creative" => StatusGameMode::Creative,
            "adventure" => StatusGameMode::Adventure,
            "spectator" => StatusGameMode::Spectator,
            _ => StatusGameMode::Survival,
        };
        Self {
            name: config.server.motd.clone(),
            max: config.server.max_players as usize,
            mode,
            players: AtomicUsize::new(0),
        }
    }
}
impl StatusProvider for RuntimeStatus {
    fn status(&self, _player_count: usize, _max_players: usize) -> ServerStatus {
        ServerStatus {
            server_name: self.name.clone(),
            sub_name: "Unastar".into(),
            player_count: self.players.load(Ordering::Relaxed),
            max_players: self.max,
            game_mode: self.mode,
        }
    }
}

/// Runtime-owned tasks never outlive their server or a failed startup.
#[derive(Default)]
struct NetworkTasks {
    tasks: Vec<JoinHandle<()>>,
    endpoint: Option<Arc<tokio_nethernet::endpoint::Handler>>,
}
impl NetworkTasks {
    /// Stop accepting and cancel active network pumps before ordinary saves.
    async fn stop(&mut self) {
        if let Some(endpoint) = self.endpoint.take() {
            endpoint.close();
        }
        for task in &self.tasks {
            task.abort();
        }
        for task in self.tasks.drain(..) {
            let _ = task.await;
        }
    }
}
impl Drop for NetworkTasks {
    fn drop(&mut self) {
        if let Some(endpoint) = &self.endpoint {
            endpoint.close();
        }
        for task in &self.tasks {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests;

/// Bind the real HTTP signaling and identity-verifying transport together.
/// A failed bind closes its HTTP listener instead of leaving a partial service.
async fn bind_nethernet(
    endpoint: &crate::config::NetherNetConfig,
    config: BedrockListenerConfig,
    mut transport: tokio_nethernet::TransportConfig,
) -> Result<
    (
        Arc<tokio_nethernet::endpoint::Handler>,
        BedrockListener<tokio_nethernet::NetherNetListener>,
    ),
    Box<dyn std::error::Error>,
> {
    let signing_key = endpoint
        .signing_key_path
        .as_ref()
        .map(tokio_nethernet::identity::ServerSigningKey::load_or_create)
        .transpose()?
        .map(Arc::new);
    let timeout = Duration::from_secs(endpoint.negotiation_timeout_seconds);
    let handler = tokio_nethernet::endpoint::HandlerConfig {
        negotiation_timeout: timeout,
        ..Default::default()
    }
    .serve(endpoint.bind_address)
    .await?;
    transport.relay_only = endpoint.relay_only;
    let result = BedrockListener::nethernet()
        .endpoint(
            handler.clone(),
            tokio_nethernet::NetherNetListenerConfig {
                signing_key,
                connection_timeout: timeout,
                max_pending_connections: endpoint.max_pending_connections,
                ice_servers: endpoint
                    .ice_servers
                    .iter()
                    .map(|server| tokio_nethernet::IceServer {
                        urls: server.urls.clone(),
                        username: server.username.clone(),
                        password: server.password.clone(),
                    })
                    .collect(),
                transport,
                // Login authenticates the account and binds its key to the signed
                // SDP assertion already verified against the DTLS fingerprint.
                allow_identityless_client: false,
                ..Default::default()
            },
        )
        .config(BedrockListenerConfig {
            encryption_enabled: false,
            ..config
        })
        .bind()
        .await;
    match result {
        Ok(listener) => Ok((handler, listener)),
        Err(error) => {
            handler.close();
            Err(error.into())
        }
    }
}
