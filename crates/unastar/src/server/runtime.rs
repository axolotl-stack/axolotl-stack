//! Server runtime - orchestrates the main server loop.
//!
//! Provides the `UnastarServer` struct that ties together configuration,
//! networking, and the game server tick loop.

use jolyne::{BedrockListener, BedrockListenerConfig};
use p384::SecretKey;
use rand::thread_rng;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio_raknet::RaknetListener;
use tracing::{error, info, trace, warn};

use crate::config::{PlayerDataStore, UnastarConfig};
use crate::network::{NetworkEvent, spawn_network_task};
use crate::plugin::PluginManager;
use crate::server::connect::{accept_join_sequence, spawn_to_dvec3};
use crate::server::{GameServer, PlayerSpawnData};
use crate::storage::LevelDBPlayerProvider;

/// Tick rate (20 TPS = 50ms per tick).
const TICK_DURATION: Duration = Duration::from_millis(50);

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
        let save_previous_position = config.players.save_previous_position;

        // Create ECS-based game server
        let mut server = GameServer::with_config(config.server_config());
        server.set_player_data_store(player_data_store.clone(), save_previous_position);

        // Initialize LevelDB player provider (if enabled)
        if config.players.leveldb_enabled {
            let player_db_path = config.players.data_dir.join("db");
            match LevelDBPlayerProvider::open(&player_db_path) {
                Ok(provider) => {
                    info!(path = %player_db_path.display(), "Opened player LevelDB");
                    server
                        .set_player_provider(Arc::new(provider), config.players.save_on_disconnect);
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
            server.set_world_provider(provider.clone());

            // Also set provider on ChunkManager resource for load-before-generate
            {
                let world = server.ecs.world_mut();
                if let Some(mut chunk_manager) =
                    world.get_resource_mut::<crate::world::ecs::ChunkManager>()
                {
                    chunk_manager.set_provider(provider);
                }
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
        if let Err(e) = plugin_manager.load_plugins(&plugins_dir).await {
            warn!(error = %e, "Failed to load plugins");
        }

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
        let default_chunk_radius = self.server.config.default_chunk_radius;

        // Build listener config from server settings
        let listener_config = BedrockListenerConfig {
            online_mode: self.config.server.online_mode,
            allow_legacy_auth: self.config.server.allow_legacy_auth,
            encryption_enabled: self.config.server.encryption_enabled,
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

        // Consolidated event channel
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<NetworkEvent>();

        // Tick signal channel - signals network tasks to flush their buffers
        let (tick_tx, _) = broadcast::channel::<()>(16);

        // Spawn accept loop
        spawn_accept_loop(
            listener,
            self.server.world_template.clone(),
            self.server_key.clone(),
            self.config.clone(),
            self.player_data_store.clone(),
            event_tx,
            tick_tx.clone(),
        );

        // Main tick loop
        let mut interval = tokio::time::interval(TICK_DURATION);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

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

                    // Drain network events
                    let mut events_processed = 0u32;
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
                                outbound_tx,
                            } => {
                                let spawn_data = PlayerSpawnData {
                                    session_id,
                                    display_name,
                                    xuid,
                                    uuid,
                                    runtime_id,
                                    position: initial_position,
                                    outbound_tx,
                                    chunk_radius: default_chunk_radius,
                                };
                                self.server.spawn_player(spawn_data);
                            }
                            NetworkEvent::Packet { session_id, packet } => {
                                self.server.handle_packet(session_id, packet);
                            }
                            NetworkEvent::Disconnected { session_id } => {
                                self.server.despawn_player(session_id);
                            }
                        }
                    }

                    let events_elapsed = tick_start.elapsed();

                    // Run game tick (this queues packets to broadcast systems)
                    let tick_logic_start = std::time::Instant::now();
                    self.server.tick();
                    
                    // Run plugin tick
                    self.plugin_manager.tick(self.server.ecs.world_mut()).await;

                    let tick_logic_elapsed = tick_logic_start.elapsed();

                    // Signal all network tasks to flush their buffers
                    let _ = tick_tx.send(());

                    let total_elapsed = tick_start.elapsed();

                    // Log slow ticks (> 20ms = taking more than half our budget)
                    if total_elapsed.as_millis() > 20 || tick_count % 200 == 0 {
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
                }
                _ = &mut shutdown => {
                    info!("Shutdown signal received, saving data...");

                    // Save all player data
                    let players_saved = self.server.save_all_players().await;
                    info!(players = players_saved, "Player data saved");

                    // Save all modified chunks
                    let chunks_saved = self.server.save_all_chunks().await;
                    info!(chunks = chunks_saved, "Chunk data saved");

                    info!("Server shutdown complete");
                    return Ok(());
                }
            }
        }
    }
}

/// Spawn the accept loop as a background task.
fn spawn_accept_loop(
    mut listener: BedrockListener<RaknetListener>,
    template: Arc<jolyne::WorldTemplate>,
    key: SecretKey,
    config: Arc<UnastarConfig>,
    player_data_store: Arc<PlayerDataStore>,
    event_tx: mpsc::UnboundedSender<NetworkEvent>,
    tick_tx: broadcast::Sender<()>,
) {
    tokio::spawn(async move {
        let mut next_session_id: u64 = 1;

        loop {
            match listener.accept().await {
                Ok(handshake_stream) => {
                    let addr = handshake_stream.peer_addr();
                    info!(%addr, "Connection accepted");

                    let template = template.clone();
                    let key = key.clone();
                    let config = config.clone();
                    let player_data_store = player_data_store.clone();
                    let event_tx = event_tx.clone();
                    let tick_rx = tick_tx.subscribe();
                    let session_id = next_session_id;
                    next_session_id += 1;

                    tokio::spawn(async move {
                        match accept_join_sequence(
                            &template,
                            &key,
                            &config,
                            &player_data_store,
                            session_id,
                            handshake_stream,
                        )
                        .await
                        {
                            Ok((mut play_stream, identity, initial_position)) => {
                                let display_name = identity
                                    .display_name
                                    .as_deref()
                                    .unwrap_or("Unknown")
                                    .to_string();

                                info!(%addr, name = %display_name, "Player joined");

                                // set to manual flushing.
                                play_stream.set_auto_flush(false);

                                // Create bounded outbound channel to prevent memory explosion.
                                // Bound to ~50MB worth of packets (assuming average 50KB per packet = 1024 packets).
                                // This prevents memory explosion while allowing reasonable buffering.
                                const OUTBOUND_CHANNEL_CAPACITY: usize = 1024;
                                let (outbound_tx, outbound_rx) =
                                    mpsc::channel(OUTBOUND_CHANNEL_CAPACITY);

                                // Send joined event
                                if event_tx
                                    .send(NetworkEvent::Joined {
                                        session_id,
                                        display_name: display_name.clone(),
                                        xuid: identity.xuid.clone(),
                                        uuid: identity.uuid.clone(),
                                        runtime_id: session_id as i64,
                                        initial_position: spawn_to_dvec3(&initial_position),
                                        outbound_tx: outbound_tx.clone(),
                                    })
                                    .is_err()
                                {
                                    return; // Server shutting down
                                }

                                // Spawn network task with tick receiver
                                spawn_network_task(
                                    play_stream,
                                    session_id,
                                    display_name,
                                    event_tx,
                                    outbound_rx,
                                    tick_rx,
                                );
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
