//! Session routing, aggregate native requests and independently ticking worlds.

use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};

use bevy_ecs::prelude::*;
use jolyne::valentine::{BorrowedMcpePacket, McpePacketArgs};

use super::directory::{RouterRequests, RouterTickHandler, WorldDirectory, WorldPlayer};
use super::hooks::{self, TransferQueue};
use super::{TransferComponents, TransferError};
use crate::entity::components::PlayerNetworkMetrics;
use crate::server::game::{GameServer, PlayerSpawnData, SessionEntityMap, types};
use crate::world::ecs::ChunkManager;

/// The router leaves simulation stopped if its shared plugin budget is unavailable.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WorldTickError {
    #[error("plugin manager lock is poisoned")]
    PluginManagerUnavailable,
}

/// Separate ECS worlds share transport routing without sharing terrain, physics,
/// container resources, viewers or world providers. One world owns each session.
pub struct DimensionRouter {
    pub(super) worlds: BTreeMap<i32, GameServer>,
    primary: i32,
    tick_handlers: Vec<Arc<dyn RouterTickHandler>>,
    pub(super) loading_sequence: u32,
    plugin_manager: Option<Arc<Mutex<crate::plugin::PluginManager>>>,
}
impl DimensionRouter {
    /// Wrap the primary initialized server without recreating its ECS/resources.
    pub fn new(mut primary: GameServer) -> Self {
        let dimension = primary.ecs.world().resource::<ChunkManager>().dimension();
        primary
            .ecs
            .world_mut()
            .init_resource::<TransferComponents>();
        let mut router = Self {
            worlds: BTreeMap::from([(dimension, primary)]),
            primary: dimension,
            tick_handlers: Vec::new(),
            loading_sequence: 0,
            plugin_manager: None,
        };
        router.refresh_directory();
        router
    }
    /// Record the runtime's manager after its primary world was attached.
    pub(crate) fn set_plugin_manager(&mut self, manager: Arc<Mutex<crate::plugin::PluginManager>>) {
        self.plugin_manager = Some(manager);
    }
    /// Read the primary server's protocol dimension.
    pub fn primary_dimension(&self) -> i32 {
        self.primary
    }
    /// Read one real dimension world.
    pub fn world(&self, dimension: i32) -> Option<&GameServer> {
        self.worlds.get(&dimension)
    }
    /// Configure or apply effects only within the selected world.
    pub fn world_mut(&mut self, dimension: i32) -> Option<&mut GameServer> {
        self.worlds.get_mut(&dimension)
    }
    /// Enumerate isolated worlds in deterministic dimension order.
    pub fn iter(&self) -> impl Iterator<Item = (i32, &GameServer)> {
        self.worlds
            .iter()
            .map(|(&dimension, world)| (dimension, world))
    }
    /// Apply native setup/effects across worlds without storing raw world pointers.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (i32, &mut GameServer)> {
        self.worlds
            .iter_mut()
            .map(|(&dimension, world)| (dimension, world))
    }
    /// Construct a compatible world with shared account storage before installing
    /// native application features. Its terrain provider remains caller-owned.
    pub fn create_world(&self, config: crate::ServerConfig) -> Result<GameServer, TransferError> {
        let mut world = GameServer::with_config(config);
        self.inherit_account_storage(&mut world)?;
        world.ecs.world_mut().init_resource::<TransferComponents>();
        Ok(world)
    }
    /// Register a startup world only when protocol registries and account storage
    /// match the active connection contract. Terrain and clocks stay independent.
    pub fn add(&mut self, mut world: GameServer) -> Result<(), TransferError> {
        let dimension = world.ecs.world().resource::<ChunkManager>().dimension();
        if !(0..=2).contains(&dimension) || self.worlds.contains_key(&dimension) {
            return Err(TransferError::InvalidDimension);
        }
        let primary = self.worlds.get(&self.primary).expect("registered primary");
        let a = &primary
            .ecs
            .world()
            .resource::<types::ServerWorldTemplate>()
            .0;
        let b = &world.ecs.world().resource::<types::ServerWorldTemplate>().0;
        if a.item_registry != b.item_registry
            || a.available_entities != b.available_entities
            || a.biome_definitions != b.biome_definitions
            || a.start_game_template.block_properties != b.start_game_template.block_properties
            || a.start_game_template.block_network_ids_are_hashes
                != b.start_game_template.block_network_ids_are_hashes
            || a.start_game_template.server_block_type_registry_checksum
                != b.start_game_template.server_block_type_registry_checksum
            || a.start_game_template.movement_settings != b.start_game_template.movement_settings
            || a.start_game_template.server_enabled_client_side_generation
                != b.start_game_template.server_enabled_client_side_generation
            || a.dimension_data != b.dimension_data
        {
            return Err(TransferError::IncompatibleRegistries);
        }
        if world
            .ecs
            .world()
            .resource::<SessionEntityMap>()
            .iter()
            .any(|(session, _)| self.locate_session(session).is_some())
        {
            return Err(TransferError::InvalidRequest);
        }
        self.inherit_account_storage(&mut world)?;
        world.ecs.world_mut().init_resource::<TransferComponents>();
        if let Some(manager) = &self.plugin_manager {
            crate::plugin::PluginManager::attach_to_world(manager.clone(), world.ecs.world_mut());
        }
        self.worlds.insert(dimension, world);
        self.refresh_directory();
        Ok(())
    }
    /// Load destination geometry without moving a player, changing account state
    /// or producing dimension packets. `None` means background generation is
    /// pending; a loaded unsupported/obstructed destination returns an error.
    /// Applications may retry from an aggregate callback. Load pins expire 200
    /// world ticks after the latest preparation; the radius is at most 256.
    pub fn prepare_destination(
        &mut self,
        dimension: i32,
        feet: glam::DVec3,
        radius: u32,
    ) -> Result<Option<glam::DVec3>, TransferError> {
        if radius > 256 {
            return Err(TransferError::InvalidRequest);
        }
        let server = self
            .worlds
            .get_mut(&dimension)
            .ok_or(TransferError::UnknownDimension)?;
        let world = server.ecs.world_mut();
        let until = world
            .resource::<crate::ecs::resources::TickCounter>()
            .current
            .saturating_add(200);
        world.insert_resource(super::transfer::PreloadUntil(until));
        if !super::transfer::prepare_chunks(world, feet)? {
            return Ok(None);
        }
        super::transfer::find_safe_destination(world, feet, radius)
            .map(Some)
            .ok_or(TransferError::UnsafeDestination)
    }

    /// Share one account provider so reconnects and cross-world receipts cannot
    /// race independent player databases. World terrain providers are unaffected.
    fn inherit_account_storage(&self, world: &mut GameServer) -> Result<(), TransferError> {
        let primary = self.worlds.get(&self.primary).expect("registered primary");
        match (
            primary
                .ecs
                .world()
                .get_resource::<types::PlayerProviderResource>(),
            world
                .ecs
                .world()
                .get_resource::<types::PlayerProviderResource>(),
        ) {
            (Some(a), Some(b)) if !Arc::ptr_eq(&a.0, &b.0) => {
                return Err(TransferError::IncompatibleRegistries);
            }
            (Some(provider), None) => {
                world
                    .ecs
                    .world_mut()
                    .insert_resource(types::PlayerProviderResource(provider.0.clone()));
            }
            (None, Some(_)) => return Err(TransferError::IncompatibleRegistries),
            _ => {}
        }
        if let Some(store) = primary
            .ecs
            .world()
            .get_resource::<types::PlayerDataStoreResource>()
        {
            world
                .ecs
                .world_mut()
                .insert_resource(types::PlayerDataStoreResource(store.0.clone()));
        }
        Ok(())
    }
    /// Find a session only if it has one exact world owner.
    pub fn locate_session(&self, session: u64) -> Option<(i32, Entity)> {
        let mut owners = self.worlds.iter().filter_map(|(&dimension, world)| {
            world
                .ecs
                .world()
                .resource::<SessionEntityMap>()
                .get(session)
                .map(|entity| (dimension, entity))
        });
        let owner = owners.next()?;
        owners.next().is_none().then_some(owner)
    }
    /// Revalidate a scoped directory handle before selecting its mutable world.
    pub fn resolve(&self, player: &WorldPlayer) -> Option<(i32, Entity)> {
        self.worlds
            .get(&player.dimension())
            .filter(|world| player.matches(world.ecs.world()))
            .map(|_| (player.dimension(), player.player()))
    }
    /// Register one application-wide tick, separate from per-world simulation.
    pub fn register_tick_handler(&mut self, handler: impl RouterTickHandler) {
        self.tick_handlers.push(Arc::new(handler));
    }
    /// Spawn in a selected dimension after the join sequence chose its template.
    pub fn spawn_player_in(
        &mut self,
        dimension: i32,
        data: PlayerSpawnData,
    ) -> crate::storage::StorageResult<Entity> {
        if self.locate_session(data.session_id).is_some() {
            return Err(crate::storage::StorageError::Deserialization(
                "session already has a world owner".into(),
            ));
        }
        let uuid = data
            .uuid
            .as_deref()
            .and_then(|value| uuid::Uuid::parse_str(value).ok())
            .unwrap_or_else(uuid::Uuid::nil);
        if self.worlds.values().any(|world| {
            crate::server::game::inventory_effects::recovery_account_fenced(
                world.ecs.world(),
                uuid,
                data.xuid.as_deref(),
            )
        }) {
            return Err(crate::storage::StorageError::PlayerRecoveryRequired);
        }
        self.worlds
            .get_mut(&dimension)
            .ok_or(crate::storage::StorageError::NotFound)?
            .spawn_player(data)
    }
    /// Preserve ordinary callers by spawning new sessions in the primary world.
    pub fn spawn_player(&mut self, data: PlayerSpawnData) -> crate::storage::StorageResult<Entity> {
        self.spawn_player_in(self.primary, data)
    }
    /// Route authenticated input to its actual dimension world.
    pub fn route_packet(&mut self, session: u64, args: McpePacketArgs, packet: BorrowedMcpePacket) {
        if let Some((dimension, _)) = self.locate_session(session) {
            self.worlds
                .get_mut(&dimension)
                .unwrap()
                .route_packet(session, args, packet);
        }
    }
    /// Preserve measured RTT across migration and late transport attachment.
    pub(crate) fn attach_network_metrics(
        &mut self,
        session: u64,
        metrics: PlayerNetworkMetrics,
    ) -> bool {
        self.locate_session(session).is_some_and(|(dimension, _)| {
            self.worlds
                .get_mut(&dimension)
                .unwrap()
                .attach_network_metrics(session, metrics)
        })
    }
    /// Disconnect only the world that actually owns this transport.
    pub fn despawn_player(&mut self, session: u64) {
        if let Some((dimension, _)) = self.locate_session(session) {
            self.worlds
                .get_mut(&dimension)
                .unwrap()
                .despawn_player(session);
        }
    }
    /// Persist the current world's player record, including its actual dimension.
    pub async fn save_player(&mut self, session: u64) -> bool {
        let Some((dimension, _)) = self.locate_session(session) else {
            return false;
        };
        self.worlds
            .get_mut(&dimension)
            .unwrap()
            .save_player(session)
            .await
    }
    /// Save each account in its one current world during autosave/shutdown.
    pub async fn save_all_players(&mut self) -> usize {
        let mut saved = 0;
        for world in self.worlds.values_mut() {
            saved += world.save_all_players().await;
        }
        saved
    }
    /// Save independent terrain providers using each world's dimension keys.
    pub async fn save_all_chunks(&mut self) -> usize {
        let mut saved = 0;
        for world in self.worlds.values_mut() {
            saved += world.save_all_chunks().await;
        }
        saved
    }
    /// Acknowledge pending terrain writes in every registered dimension. A failed
    /// provider flush remains an error instead of reporting a completed shutdown.
    pub async fn flush_worlds(&self) -> crate::storage::StorageResult<()> {
        for server in self.worlds.values() {
            let provider = server.ecs.world().resource::<ChunkManager>().provider();
            if let Some(provider) = provider {
                provider.flush().await?;
            }
        }
        Ok(())
    }
    /// A server-level stop request applies regardless of the player's current world.
    pub fn stop_requested(&self) -> bool {
        self.worlds.values().any(|server| {
            server
                .ecs
                .world()
                .contains_resource::<crate::command::exec::StopRequested>()
        })
    }

    /// Tick every world, then run owned global work and migration outside all
    /// local ECS borrows. Aggregate application ticks run exactly once.
    pub async fn tick(&mut self) -> Result<(), WorldTickError> {
        self.refresh_directory();
        for world in self.worlds.values_mut() {
            if let Some(manager) = &self.plugin_manager {
                manager
                    .lock()
                    .map_err(|_| WorldTickError::PluginManagerUnavailable)?
                    .begin_tick(world.ecs.world_mut());
            }
            world.tick();
            if let Some(manager) = &self.plugin_manager {
                manager
                    .lock()
                    .map_err(|_| WorldTickError::PluginManagerUnavailable)?
                    .tick(world.ecs.world_mut());
            }
            super::loading::expire(world);
        }
        self.apply_requests();
        self.apply_relocations().await;
        self.apply_transfers().await;
        self.apply_requests();
        let handlers = self.tick_handlers.clone();
        for handler in handlers {
            let directory = self.refresh_directory();
            handler.on_tick(self, &directory);
        }
        self.refresh_directory();
        Ok(())
    }
    /// Replace immutable directory resources without holding a world borrow.
    pub fn refresh_directory(&mut self) -> WorldDirectory {
        let directory = WorldDirectory::capture(self);
        for world in self.worlds.values_mut() {
            world.ecs.world_mut().insert_resource(directory.clone());
        }
        directory
    }
    /// Drain bounded one-shot requests once per phase; callbacks can enqueue
    /// later work without recursively borrowing a local ECS world.
    fn apply_requests(&mut self) {
        let mut requests = Vec::new();
        for world in self.worlds.values_mut() {
            if let Some(mut queued) = world.ecs.world_mut().get_resource_mut::<RouterRequests>() {
                requests.append(&mut queued.0);
            }
        }
        for request in requests {
            let directory = self.refresh_directory();
            request(self, &directory);
        }
    }
    /// Execute same-world durable moves without borrowing an ECS world twice.
    async fn apply_relocations(&mut self) {
        for server in self.worlds.values_mut() {
            let requests = server
                .ecs
                .world_mut()
                .get_resource_mut::<super::relocate::RelocationQueue>()
                .map(|mut queue| std::mem::take(&mut queue.0))
                .unwrap_or_default();
            for request in requests {
                let player = request.player;
                server
                    .ecs
                    .world_mut()
                    .get_resource_or_init::<super::relocate::ActiveRelocations>()
                    .0
                    .insert(player);
                super::relocate::attempt(server, request).await;
                server
                    .ecs
                    .world_mut()
                    .resource_mut::<super::relocate::ActiveRelocations>()
                    .0
                    .remove(&player);
            }
        }
    }

    /// Temporarily own both ECS instances so an async storage acknowledgement or
    /// native callback never aliases a borrowed router entry.
    async fn apply_transfers(&mut self) {
        let mut requests = Vec::new();
        for (&dimension, world) in &mut self.worlds {
            if let Some(mut queue) = world.ecs.world_mut().get_resource_mut::<TransferQueue>() {
                requests.extend(
                    std::mem::take(&mut queue.0)
                        .into_iter()
                        .map(|request| (dimension, request)),
                );
            }
        }
        for (source_dimension, request) in requests {
            let Some(mut source) = self.worlds.remove(&source_dimension) else {
                continue;
            };
            let target_dimension = request.destination_dimension;
            if source_dimension == target_dimension {
                hooks::failed(
                    source.ecs.world_mut(),
                    request,
                    TransferError::InvalidDimension,
                );
            } else if let Some(mut target) = self.worlds.remove(&target_dimension) {
                super::transfer::attempt(
                    &mut source,
                    &mut target,
                    request,
                    &mut self.loading_sequence,
                )
                .await;
                self.worlds.insert(target_dimension, target);
            } else {
                hooks::failed(
                    source.ecs.world_mut(),
                    request,
                    TransferError::UnknownDimension,
                );
            }
            self.worlds.insert(source_dimension, source);
        }
        // Pins exist only while a source still needs generation/loading retries.
        let mut destinations: std::collections::BTreeSet<_> = self
            .worlds
            .values()
            .flat_map(|server| {
                server
                    .ecs
                    .world()
                    .get_resource::<TransferQueue>()
                    .into_iter()
                    .flat_map(|queue| queue.0.iter().map(|request| request.destination_dimension))
            })
            .collect();
        destinations.extend(self.worlds.iter().filter_map(|(&dimension, server)| {
            server
                .ecs
                .world()
                .get_resource::<super::relocate::RelocationQueue>()
                .filter(|queue| !queue.0.is_empty())
                .map(|_| dimension)
        }));
        for (dimension, server) in &mut self.worlds {
            let preload = server
                .ecs
                .world()
                .get_resource::<super::transfer::PreloadUntil>()
                .is_some_and(|until| {
                    server
                        .ecs
                        .world()
                        .resource::<crate::ecs::resources::TickCounter>()
                        .current
                        < until.0
                });
            if !preload {
                server
                    .ecs
                    .world_mut()
                    .remove_resource::<super::transfer::PreloadUntil>();
            }
            if !preload
                && !destinations.contains(dimension)
                && let Some(anchor) = server
                    .ecs
                    .world_mut()
                    .remove_resource::<super::transfer::PreloadAnchor>()
            {
                super::loading::detach_chunk_viewer(server.ecs.world_mut(), anchor.0);
                server.ecs.world_mut().despawn(anchor.0);
            }
        }
    }
}
impl Deref for DimensionRouter {
    type Target = GameServer;
    /// Keep existing single-world setup APIs focused on the primary world.
    fn deref(&self) -> &Self::Target {
        self.worlds.get(&self.primary).expect("registered primary")
    }
}
impl DerefMut for DimensionRouter {
    /// Keep existing mutable setup APIs focused on the primary world.
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.worlds
            .get_mut(&self.primary)
            .expect("registered primary")
    }
}
