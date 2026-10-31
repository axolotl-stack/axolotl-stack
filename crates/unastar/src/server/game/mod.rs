//! ECS-based game server.
//!
//! This module provides the pure ECS server implementation,
//! replacing the legacy Server struct.

pub mod attributes;
mod block_drops;
pub(crate) mod blocks;
mod chunks;
#[cfg(test)]
mod command_tests;
mod commands;
mod containers;
pub mod durability;
pub mod effects;
mod falling;
pub mod hunger;
mod inventory;
pub mod item_use;
mod items;
pub(crate) mod join;
mod join_data;
mod loot;
mod mining;
mod movement;
mod packet_domains;
mod packet_queues;
mod packets;
mod placement;
mod plugins;
pub mod types;

use bevy_ecs::prelude::*;
pub(crate) use block_drops::{drop_block_items, loot_stacks};
pub(crate) use blocks::{commit_block_event, reject_block_event};
pub use commands::ConsoleCommands;
pub use inventory::RequestIdAssignments;
pub(crate) use items::{PICKUP_REACH, item_descriptor, player_box};
pub(crate) use items::{send_slot, world_stack};
pub(crate) use join::send_inventory_contents;
use jolyne::WorldTemplate;
use jolyne::valentine::EnumsGameType;
use jolyne::valentine::types::{BlockPos, Vec3};
pub(crate) use loot::{LootContext, LootLevel};
use std::sync::Arc;
use tracing::{info, warn};

use crate::ecs::{
    CleanupSet, EntityLogicSet, NetworkSendSet, PacketApplySet, PhysicsSet, UnastarEcs,
};
use crate::entity::bundles::PlayerBundle;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{
    ArmourInventory, BreakingState, ChunkRadius, ContainerWindows, EnderChestInventory, GameMode,
    HeldSlot, Inventory, LastBroadcastPosition, MainInventory, OffhandSlot, Player, PlayerInput,
    PlayerName, PlayerSession, PlayerState, PlayerUi, PlayerUuid, RuntimeEntityId, SpatialChunk,
};
use crate::entity::systems::{
    apply_poison, apply_regeneration, apply_wither, despawn_dead, despawn_out_of_world,
    merge_item_entities, tick_age, tick_air_supply, tick_damage_immunity, tick_effects, tick_fire,
    tick_item_despawn, tick_item_pickup_delay, tick_physics, tick_projectile_lifetime,
};
use crate::network::SessionId;
use crate::registry::{BiomeRegistry, BlockRegistry, EntityRegistry, ItemRegistry};
use crate::server::broadcast::{
    EntityGrid, broadcast_block_updates, broadcast_despawn_system, broadcast_movement_system,
    broadcast_spawn_system, cleanup_despawned_entities, sync_actor_viewers, sync_spatial_chunks,
    tick_block_breaking,
};
use crate::storage::PlayerData;
use crate::world::ChunkManager;
use crate::world::ecs::{
    AwaitingSpawnStatus, BlockBroadcastEvent, ChunkLoadConfig, ChunkLoader, ChunkPosition,
    ChunkTickingState, LastPublisherState, PendingChunkGenerations, PlayerDespawnedEvent,
    PlayerSpawnedEvent, on_block_changed, register_chunk_systems,
};

// Re-export public types
pub use super::config::ServerConfig;
pub use types::{
    ItemEntityIdCounter, PlayerPersistenceData, PlayerSpawnData, ServerConfigResource,
    SessionEntityMap,
};

/// The ECS-based game server.
///
/// Thin wrapper around the ECS World and Schedule.
/// All game state lives inside the World as Resources.
pub struct GameServer {
    pub ecs: UnastarEcs,
}

fn game_mode_to_storage_id(game_mode: GameMode) -> u8 {
    match game_mode {
        GameMode::Survival => 0,
        GameMode::Creative => 1,
        GameMode::Adventure => 2,
        GameMode::Spectator => 6,
    }
}

/// The stored state of a player. A player saved while dead is stored as it
/// will respawn (respawn position, full health, fresh hunger), so it never
/// reloads with zero health.
fn player_record(world: &World, entity: Entity) -> Option<(uuid::Uuid, PlayerData)> {
    use crate::entity::components::{Experience, Health, Hunger};
    let uuid = world.get::<PlayerUuid>(entity)?.0;
    let dead = world
        .get::<crate::entity::player_life::Dead>(entity)
        .is_some();
    let position = if dead {
        crate::entity::player_life::respawn::respawn_position(world, entity)
    } else {
        world.get::<Position>(entity)?.0
    };
    let rotation = world.get::<Rotation>(entity)?;
    let mut health = world.get::<Health>(entity).cloned().unwrap_or_default();
    let hunger = if dead {
        Hunger::default()
    } else {
        world.get::<Hunger>(entity).cloned().unwrap_or_default()
    };
    if dead {
        health.current = health.max;
    }
    let experience = world.get::<Experience>(entity).cloned().unwrap_or_default();
    let inventory = world.get::<MainInventory>(entity);
    let armour = world.get::<ArmourInventory>(entity);
    Some((
        uuid,
        PlayerData {
            uuid: uuid.to_string(),
            position: position.to_array(),
            rotation: [rotation.yaw, rotation.pitch],
            dimension: world.resource::<ServerConfigResource>().0.world.dimension,
            game_mode: game_mode_to_storage_id(*world.get::<GameMode>(entity)?),
            health: health.current,
            max_health: health.max,
            food: hunger.food_level,
            saturation: hunger.saturation,
            exhaustion: hunger.exhaustion,
            experience: experience.level,
            experience_progress: experience.progress,
            cold_exposure: if dead {
                0.0
            } else {
                world
                    .get::<crate::entity::freezing::FreezingState>(entity)
                    .map_or(0.0, |state| state.strength())
            },
            age_ticks: world
                .get::<crate::entity::components::Age>(entity)
                .map_or(0, |age| age.0),
            inventory: inventory
                .map(|inventory| inventory.0.slots().iter().map(Into::into).collect())
                .unwrap_or_default(),
            armour: std::array::from_fn(|i| {
                armour
                    .map(|armour| (&armour.slots()[i]).into())
                    .unwrap_or_default()
            }),
            offhand: world
                .get::<OffhandSlot>(entity)
                .map(|slot| (&slot.0).into())
                .unwrap_or_default(),
            cursor: world
                .get::<PlayerUi>(entity)
                .map(|ui| ui.cursor().into())
                .unwrap_or_default(),
            held_slot: world
                .get::<HeldSlot>(entity)
                .map(|slot| slot.0)
                .unwrap_or(0),
            ender_chest: world
                .get::<EnderChestInventory>(entity)
                .map(|ender| ender.0.slots().iter().map(Into::into).collect())
                .unwrap_or_default(),
        },
    ))
}

/// Resolve every saved slot before assigning identities or publishing player state.
fn restore_items(
    saved: &mut PlayerData,
    items: &ItemRegistry,
) -> crate::storage::StorageResult<(
    MainInventory,
    ArmourInventory,
    OffhandSlot,
    PlayerUi,
    EnderChestInventory,
)> {
    let mut main = Vec::with_capacity(36);
    for item in std::mem::take(&mut saved.inventory) {
        main.push(item.restore(items)?);
    }
    main.resize_with(36, crate::item::ItemStack::empty);
    let [head, chest, legs, feet] =
        std::mem::take(&mut saved.armour).map(|item| item.restore(items));
    let armour_items = [head?, chest?, legs?, feet?];
    let offhand = std::mem::take(&mut saved.offhand).restore(items)?;
    let cursor = std::mem::take(&mut saved.cursor).restore(items)?;
    let mut ender = Vec::with_capacity(crate::entity::components::ENDER_CHEST_SIZE);
    for item in std::mem::take(&mut saved.ender_chest) {
        ender.push(item.restore(items)?);
    }
    ender.resize_with(
        crate::entity::components::ENDER_CHEST_SIZE,
        crate::item::ItemStack::empty,
    );
    for (slot, item) in armour_items.iter().enumerate().skip(1) {
        if item.count > 1 || !crate::item::equipment::accepts_armour(item, slot) {
            return Err(crate::storage::StorageError::Deserialization(format!(
                "invalid saved armour item {} in slot {slot}",
                item.item_id()
            )));
        }
    }

    let main = MainInventory(Inventory::from_slots(main));
    let mut armour = ArmourInventory::default();
    // BDS admin commands can seed arbitrary head contents. Chest/legs/feet
    // were checked above; the trusted restore path does not discard head items.
    for (slot, item) in armour_items.into_iter().enumerate() {
        armour
            .inventory_mut()
            .set_item(slot, item)
            .expect("four saved armour slots");
    }
    let offhand = OffhandSlot(offhand.into_slot());
    let mut ui = PlayerUi::default();
    ui.set_cursor(cursor);
    Ok((
        main,
        armour,
        offhand,
        ui,
        EnderChestInventory(Inventory::from_slots(ender)),
    ))
}

impl GameServer {
    pub fn new() -> Self {
        Self::with_config(ServerConfig::default())
    }

    pub fn with_config(config: ServerConfig) -> Self {
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        let items = Arc::new(items);
        let recipes = unastar_data::recipes::RecipeRegistry::vanilla()
            .expect("embedded vanilla recipes compile against the item registry");
        let definitions = crate::registry::entity::ActorDefinitionRegistry::vanilla();
        let actor_properties =
            crate::entity::definition::properties::ActorPropertyRegistry::compile(&definitions)
                .expect("vanilla actor property schemas and measured wire kinds");
        let mobs =
            crate::entity::mob::MobRegistry::compile(&definitions, &actor_properties, &items)
                .expect("vanilla mob component kinds and families");
        let mut entities = EntityRegistry::new();
        entities.load_vanilla(&definitions);
        let mut biomes = BiomeRegistry::new();
        biomes.load_vanilla();
        let mut blocks = BlockRegistry::new();
        blocks.load_vanilla();
        let crafting_catalog = crate::crafting::CraftingCatalog::new(&recipes, &items, &blocks)
            .expect("vanilla crafting advertisement compiles");
        let crafting_executor =
            crate::crafting::execution::CraftingExecutor::new(&crafting_catalog, &items)
                .expect("advertised vanilla grid recipes compile for execution");

        let mut world_template = WorldTemplate::default();
        world_template.start_game_template.position = Vec3 {
            x: 0.5,
            y: 17.0,
            z: 0.5,
        };
        world_template
            .start_game_template
            .settings
            .default_spawn_block_position = BlockPos { x: 0, y: 17, z: 0 };
        world_template
            .start_game_template
            .settings
            .spawn_settings
            .dimension = config.world.dimension;
        world_template.start_game_template.game_type = EnumsGameType::Survival;
        world_template.start_game_template.settings.game_type = EnumsGameType::Survival;
        world_template
            .start_game_template
            .enable_item_stack_net_manager = true;
        world_template
            .start_game_template
            .movement_settings
            .server_authoritative_block_breaking = true;
        world_template.item_registry = Arc::new(items.to_packet());
        world_template.biome_definitions = Arc::new(biomes.to_packet());
        world_template.available_entities =
            Arc::new(entities.to_available_entity_identifiers_packet());
        let block_ids = config.block_network_ids;
        world_template.creative_content =
            Arc::new(Self::build_creative_content(&items, &blocks, block_ids));
        world_template.start_game_template.block_properties = blocks.to_block_properties();
        world_template
            .start_game_template
            .block_network_ids_are_hashes = block_ids.is_hashed();
        world_template.jigsaw_structures = Arc::new(join_data::jigsaw_structures());
        world_template.voxel_shapes = Arc::new(join_data::voxel_shapes());

        // Difficulty from the config, Bedrock's default game rules and a
        // fresh clock (there is no level-data store to restore them from).
        let difficulty = config.difficulty;
        let game_rules = crate::world::rules::GameRules::default();
        let world_time = crate::world::rules::WorldTime::default();
        let start_game_state = crate::world::rules::StartGameWorldState::default();
        start_game_state.publish(world_time, difficulty, &game_rules);
        start_game_state.apply(&mut world_template.start_game_template);

        let world_template = Arc::new(world_template);
        let world_config = config.world;
        let mut ecs = UnastarEcs::new();

        // Insert all game state as ECS Resources
        ecs.world_mut().insert_resource(definitions);
        ecs.world_mut().insert_resource(actor_properties);
        ecs.world_mut().insert_resource(mobs);
        ecs.world_mut()
            .insert_resource(crate::registry::entity::EntityRegistryResource(Arc::new(
                entities,
            )));
        ecs.world_mut()
            .insert_resource(types::ServerWorldTemplate(world_template.clone()));
        ecs.world_mut()
            .insert_resource(ServerConfigResource(config.clone()));
        ecs.world_mut()
            .insert_resource(crate::command::exec::OperatorList::new(
                config.operators.clone(),
            ));
        ecs.world_mut().insert_resource(difficulty);
        ecs.world_mut().insert_resource(game_rules);
        ecs.world_mut().insert_resource(world_time);
        ecs.world_mut().insert_resource(start_game_state);
        ecs.world_mut()
            .init_resource::<crate::world::clock::ClockSync>();
        ecs.world_mut()
            .init_resource::<crate::command::CommandRegistry>();
        ecs.world_mut().init_resource::<commands::PendingCommands>();
        ecs.world_mut().insert_resource(ItemEntityIdCounter(100000));
        ecs.world_mut()
            .insert_resource(types::RecipeRegistryResource(Arc::new(recipes)));
        ecs.world_mut()
            .insert_resource(types::CraftingCatalogResource(crafting_catalog));
        ecs.world_mut()
            .insert_resource(types::CraftingExecutorResource(Arc::new(crafting_executor)));
        ecs.world_mut()
            .init_resource::<crate::entity::EntityRandom>();
        ecs.world_mut().init_resource::<falling::GravityChecks>();
        ecs.world_mut().insert_resource(SessionEntityMap::default());
        ecs.world_mut().insert_resource(EntityGrid::default());
        ecs.world_mut()
            .insert_resource(ChunkManager::new(world_config));
        ecs.world_mut()
            .insert_resource(ChunkLoadConfig::from_server_config(&config));
        ecs.world_mut().init_resource::<PendingChunkGenerations>();
        ecs.world_mut().init_resource::<ChunkTickingState>();
        ecs.world_mut()
            .init_resource::<bevy_ecs::message::Messages<BlockBroadcastEvent>>();
        ecs.world_mut()
            .init_resource::<bevy_ecs::message::Messages<PlayerSpawnedEvent>>();
        ecs.world_mut()
            .init_resource::<bevy_ecs::message::Messages<PlayerDespawnedEvent>>();

        // Per-domain packet queues and event buffers
        ecs.world_mut()
            .init_resource::<packet_queues::MovementPacketQueue>();
        ecs.world_mut()
            .init_resource::<packet_queues::BlockPacketQueue>();
        ecs.world_mut()
            .init_resource::<packet_queues::InventoryPacketQueue>();
        ecs.world_mut()
            .init_resource::<packet_queues::ChatPacketQueue>();
        ecs.world_mut()
            .init_resource::<packet_queues::ChunkPacketQueue>();
        ecs.world_mut()
            .init_resource::<packet_queues::MovementEvents>();
        ecs.world_mut()
            .init_resource::<packet_queues::InventoryEvents>();
        ecs.world_mut().init_resource::<packet_queues::ChatEvents>();
        ecs.world_mut()
            .init_resource::<crate::entity::mob::interaction::actions::MobActionQueue>();
        ecs.world_mut()
            .init_resource::<crate::entity::projectile::ThrowQueue>();
        ecs.world_mut().init_resource::<item_use::ItemUseQueue>();
        ecs.world_mut().init_resource::<hunger::ExhaustionValues>();
        ecs.world_mut()
            .init_resource::<bevy_ecs::message::Messages<crate::entity::mob::state::MateRequest>>();
        // Player damage, death and respawn; players respawn where the
        // template spawns them until the runtime resolves the world spawn.
        crate::entity::player_life::install(
            ecs.world_mut(),
            crate::server::connect::spawn_to_dvec3(&world_template.start_game_template.position),
        )
        .expect("vanilla player definition");

        ecs.world_mut().add_observer(on_block_changed);
        ecs.world_mut()
            .init_resource::<containers::BlockContainers>();
        ecs.world_mut()
            .add_observer(containers::register_chunk_tickers);
        ecs.world_mut().add_observer(containers::forget_replaced);
        register_chunk_systems(ecs.schedule_mut());

        // Domain systems in PacketApplySet (movement, inventory, chunks run parallel)
        ecs.schedule_mut().add_systems(
            (
                movement::apply_movement,
                inventory::apply_inventory,
                chunks::apply_chunk_requests,
                commands::apply_chat_and_commands,
            )
                .in_set(PacketApplySet),
        );
        // Exclusive block system runs after movement (which forwards block actions)
        ecs.schedule_mut().add_systems(
            blocks::apply_block_actions
                .in_set(PacketApplySet)
                .after(movement::apply_movement),
        );
        // Player attacks/interactions with mobs (exclusive), after inventory
        // so the held item reflects this tick's slot changes.
        ecs.schedule_mut().add_systems(
            crate::entity::mob::interaction::actions::apply_mob_actions
                .in_set(PacketApplySet)
                .after(inventory::apply_inventory)
                .after(blocks::apply_block_actions),
        );
        // Air use of the held item (eating, throwing), after the inventory
        // so the held slot reflects this tick's changes; thrown items it
        // queued fly the same tick.
        ecs.schedule_mut().add_systems(
            (
                item_use::apply_item_use,
                crate::entity::projectile::apply_throws,
            )
                .chain()
                .in_set(PacketApplySet)
                .after(inventory::apply_inventory)
                .after(blocks::apply_block_actions),
        );
        // Commands run with the whole world, after this tick's movement,
        // inventory and block/mob actions were applied.
        ecs.schedule_mut().add_systems(
            commands::run_commands
                .in_set(PacketApplySet)
                .after(commands::apply_chat_and_commands)
                .after(crate::entity::mob::interaction::actions::apply_mob_actions)
                .after(crate::entity::projectile::apply_throws),
        );
        // Consolidate per-domain events into main EventBuffer after all domain systems
        ecs.schedule_mut().add_systems(
            packet_queues::consolidate_domain_events
                .in_set(PacketApplySet)
                .after(blocks::apply_block_actions)
                .after(inventory::apply_inventory)
                .after(chunks::apply_chunk_requests)
                .after(commands::apply_chat_and_commands),
        );
        // Respawn requests from dead players, after the movement system has
        // applied this tick's input.
        ecs.schedule_mut().add_systems(
            crate::entity::player_life::respawn::apply_respawn_requests
                .in_set(PacketApplySet)
                .after(movement::apply_movement),
        );

        ecs.schedule_mut().add_systems(
            (
                crate::entity::definition::apply_pending_definitions,
                crate::entity::mob::derive_mob_state,
                tick_physics,
            )
                .chain()
                .in_set(PhysicsSet),
        );
        // Projectiles fly after the living bodies moved.
        ecs.schedule_mut().add_systems(
            crate::entity::projectile::tick_projectiles
                .in_set(PhysicsSet)
                .after(tick_physics),
        );
        // Mob AI turns goals into rotation and velocity before the physics
        // step integrates them; pushing separates overlapping mobs.
        ecs.schedule_mut().add_systems(
            (
                crate::entity::mob::ai::tick_mob_ai,
                crate::entity::mob::ai::push_mobs,
            )
                .chain()
                .in_set(PhysicsSet)
                .after(crate::entity::mob::derive_mob_state)
                .before(tick_physics),
        );

        ecs.schedule_mut().add_systems(
            (tick_block_breaking, plugins::process_plugin_actions).in_set(EntityLogicSet),
        );
        ecs.schedule_mut().add_systems(
            (
                tick_age,
                apply_regeneration,
                apply_poison,
                apply_wither,
                tick_air_supply,
                tick_fire,
                tick_damage_immunity,
                tick_effects,
                tick_item_pickup_delay,
                tick_item_despawn,
                merge_item_entities,
                items::pickup_items,
                despawn_out_of_world,
                tick_projectile_lifetime,
                despawn_dead,
            )
                .chain()
                .in_set(EntityLogicSet)
                .before(tick_block_breaking)
                .before(plugins::process_plugin_actions),
        );
        // Falling blocks: land the ones physics stopped this tick, then start
        // new ones whose gravity check is due (exclusive: they edit chunks).
        ecs.schedule_mut().add_systems(
            (
                falling::land_falling_blocks,
                falling::process_gravity_checks,
            )
                .chain()
                .in_set(EntityLogicSet)
                .after(despawn_dead)
                .before(tick_block_breaking),
        );
        // Furnaces, hoppers and container viewers (exclusive: edits chunks).
        ecs.schedule_mut().add_systems(
            containers::tick_block_containers
                .in_set(EntityLogicSet)
                .after(falling::process_gravity_checks)
                .before(tick_block_breaking),
        );
        // Mob fall damage, condition damage, daylight burning and fire, death,
        // then experience orbs. After the entity chain so deaths from
        // fire/effects are caught the same tick.
        ecs.schedule_mut().add_systems(
            (
                crate::entity::mob::combat::fall::apply_fall_damage,
                crate::entity::mob::combat::hurt_on_condition,
                crate::entity::mob::combat::fire::burn_in_daylight,
                crate::entity::mob::combat::fire::burn_mobs,
                crate::entity::freezing::tick,
                crate::entity::mob::combat::death::die_at_zero_health,
                crate::entity::mob::combat::death::tick_dying,
                crate::entity::xp_orb::tick_xp_orbs,
            )
                .chain()
                .in_set(EntityLogicSet)
                .after(despawn_dead)
                .before(tick_block_breaking),
        );
        // Player landings, then lava, fire, burning and the void.
        ecs.schedule_mut().add_systems(
            (
                crate::entity::player_life::environment::land_players,
                crate::entity::player_life::environment::hurt_players_by_environment,
            )
                .chain()
                .in_set(EntityLogicSet)
                .after(despawn_dead)
                .before(tick_block_breaking),
        );
        // Player food and effects after the damage above, then the eating
        // animation of players still using an item.
        ecs.schedule_mut().add_systems(
            (
                effects::apply_player_effects,
                hunger::tick_hunger,
                item_use::tick_using,
                crate::entity::projectile::pick_up_stuck_projectiles,
            )
                .chain()
                .in_set(EntityLogicSet)
                .after(crate::entity::xp_orb::tick_xp_orbs)
                .before(tick_block_breaking),
        );
        // What changed for each player's client this tick.
        ecs.schedule_mut().add_systems(
            (
                effects::sync_player_effects,
                attributes::sync_player_attributes,
            )
                .chain()
                .in_set(NetworkSendSet),
        );
        // Love and breed-cooldown timers and mating, then baby growth.
        ecs.schedule_mut().add_systems(
            (
                crate::entity::mob::life::breeding::tick_breeding,
                crate::entity::mob::life::ageing::tick_baby_growth,
            )
                .chain()
                .in_set(EntityLogicSet)
                .after(despawn_dead)
                .before(tick_block_breaking),
        );
        // `minecraft:spawn_entity` (egg laying) and ambient sound names.
        ecs.schedule_mut().add_systems(
            (
                crate::entity::mob::life::spawn_entity::tick_spawn_entity,
                crate::entity::mob::net::ambient::update_ambient_sound_names,
            )
                .chain()
                .in_set(EntityLogicSet)
                .after(despawn_dead)
                .before(tick_block_breaking),
        );
        // Environment sensors, then timers. Their group changes apply at the
        // next tick's definition barrier (`PhysicsSet`).
        ecs.schedule_mut().add_systems(
            (
                crate::entity::mob::life::sensors::tick_environment_sensors,
                crate::entity::mob::life::sensors::tick_timers,
            )
                .chain()
                .in_set(EntityLogicSet)
                .after(despawn_dead)
                .before(tick_block_breaking),
        );
        // Natural despawning of mobs far from every player.
        ecs.schedule_mut().add_systems(
            crate::entity::mob::life::despawn::despawn_mobs
                .in_set(EntityLogicSet)
                .after(despawn_dead),
        );
        // Natural spawning of mobs from the spawn rules.
        ecs.schedule_mut().add_systems(
            crate::entity::spawning::natural_spawning
                .in_set(EntityLogicSet)
                .after(crate::entity::mob::life::despawn::despawn_mobs),
        );
        // The world clock advances before anything reads the time of day.
        ecs.schedule_mut().add_systems(
            crate::world::clock::tick_world_clock
                .in_set(EntityLogicSet)
                .before(crate::entity::spawning::natural_spawning)
                .before(tick_age),
        );
        ecs.schedule_mut().add_systems(
            (
                sync_spatial_chunks,
                broadcast_spawn_system,
                broadcast_movement_system,
                broadcast_despawn_system,
                broadcast_block_updates,
                sync_actor_viewers,
                crate::entity::mob::net::network::sync_mob_viewers,
            )
                .chain()
                .in_set(NetworkSendSet),
        );
        ecs.schedule_mut()
            .add_systems(cleanup_despawned_entities.in_set(CleanupSet));

        ecs.world_mut()
            .insert_resource(types::ItemRegistryResource(items));
        ecs.world_mut()
            .insert_resource(crate::entity::mob::ai::PathBlockTable::new(&blocks));
        ecs.world_mut()
            .insert_resource(types::BlockRegistryResource(Arc::new(blocks)));
        crate::entity::spawning::install(ecs.world_mut());

        info!("Registries loaded");
        Self { ecs }
    }

    pub async fn save_all_players(&mut self) -> usize {
        let records = {
            let world = self.ecs.world_mut();
            let mut query = world.query_filtered::<Entity, With<PlayerUuid>>();
            query
                .iter(world)
                .filter_map(|entity| player_record(world, entity))
                .collect::<Vec<_>>()
        };
        let mut saved = 0;
        for (uuid, data) in records {
            if self.save_player_record(uuid, &data).await {
                saved += 1;
            }
        }
        saved
    }

    pub async fn save_all_chunks(&mut self) -> usize {
        let Some((provider, dimension)) =
            self.ecs
                .world()
                .get_resource::<ChunkManager>()
                .and_then(|manager| {
                    manager
                        .provider()
                        .map(|provider| (provider, manager.dimension()))
                })
        else {
            return 0;
        };

        // Every chunk with something to write: dirty terrain, mobs standing in
        // it, or actors stored with it earlier.
        let snapshots = {
            let world = self.ecs.world_mut();
            let mut query = world.query_filtered::<Entity, With<ChunkPosition>>();
            let chunks: Vec<Entity> = query.iter(world).collect();
            crate::entity::mob::persistence::snapshot_chunks(world, &chunks, false)
        };

        let mut saved = 0;
        for snapshot in snapshots {
            let Some(column) = &snapshot.column else {
                continue;
            };
            match provider.save_column(snapshot.pos, dimension, column).await {
                Ok(()) => {
                    saved += 1;
                    crate::entity::mob::persistence::mark_saved(self.ecs.world_mut(), &snapshot);
                }
                Err(e) => warn!(chunk = ?snapshot.pos, error = %e, "Failed to save chunk"),
            }
        }

        saved
    }

    pub async fn save_player(&mut self, session_id: SessionId) -> bool {
        let record = {
            let world = self.ecs.world();
            world
                .resource::<SessionEntityMap>()
                .get(session_id)
                .and_then(|entity| player_record(world, entity))
        };
        let Some((uuid, data)) = record else {
            return false;
        };
        self.save_player_record(uuid, &data).await
    }

    async fn save_player_record(&self, uuid: uuid::Uuid, data: &PlayerData) -> bool {
        let provider = self
            .ecs
            .world()
            .get_resource::<types::PlayerProviderResource>()
            .map(|r| r.0.clone());
        let positions = self
            .ecs
            .world()
            .get_resource::<types::PlayerDataStoreResource>()
            .map(|r| r.0.clone());
        let mut saved = provider.is_some() || positions.is_some();
        if let Some(provider) = provider
            && let Err(error) = provider.save(uuid, data).await
        {
            warn!(%uuid, %error, "Failed to save player data");
            saved = false;
        }
        if let Some(positions) = positions {
            let last = crate::config::PlayerLastPosition {
                dimension: data.dimension,
                location: crate::config::SpawnLocation {
                    x: data.position[0] as f32,
                    y: data.position[1] as f32,
                    z: data.position[2] as f32,
                    yaw: data.rotation[0],
                    pitch: data.rotation[1],
                },
            };
            if let Err(error) = positions.save_last_position(&uuid.to_string(), &last).await {
                warn!(%uuid, %error, "Failed to save last player position");
                saved = false;
            }
        }
        saved
    }

    pub fn spawn_player(&mut self, data: PlayerSpawnData) -> crate::storage::StorageResult<Entity> {
        let player_uuid = data
            .uuid
            .as_ref()
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .unwrap_or_else(uuid::Uuid::new_v4);
        let player_name = data.display_name.clone();
        let position = Position(data.position);
        let runtime_id = data.runtime_id;
        let chunk_x = (position.0.x / 16.0).floor() as i32;
        let chunk_z = (position.0.z / 16.0).floor() as i32;

        let mut chunk_loader = ChunkLoader::new(data.chunk_radius);
        chunk_loader.move_to(chunk_x, chunk_z);
        if !chunk_loader.has_pending() {
            chunk_loader.force_reload();
        }

        let config = &self
            .ecs
            .world()
            .get_resource::<ServerConfigResource>()
            .unwrap()
            .0;
        let default_gamemode = config.default_gamemode;
        let operator = self
            .ecs
            .world()
            .resource::<crate::command::exec::OperatorList>()
            .grants(&data.display_name, data.xuid.as_deref());

        let mut rotation = Rotation::default();
        let mut game_mode = default_gamemode;
        let mut saved_data = data.saved_data;
        let (main_inventory, armour, offhand, ui, ender_chest) = match saved_data.as_mut() {
            Some(saved) => {
                saved.validate(player_uuid)?;
                let items = &self.ecs.world().resource::<types::ItemRegistryResource>().0;
                restore_items(saved, items)?
            }
            None => (
                MainInventory::default(),
                ArmourInventory::default(),
                OffhandSlot::default(),
                PlayerUi::default(),
                EnderChestInventory::default(),
            ),
        };
        let mut held_slot = HeldSlot::default();
        let mut health = crate::entity::components::Health::default();
        let mut hunger = crate::entity::components::Hunger::default();
        let mut experience = crate::entity::components::Experience::default();
        let mut cold = crate::entity::freezing::FreezingState::default();
        let mut age = crate::entity::components::Age::default();
        if let Some(saved) = saved_data {
            rotation = Rotation::new(saved.rotation[0], saved.rotation[1]);
            game_mode = match saved.game_mode {
                0 => GameMode::Survival,
                1 => GameMode::Creative,
                2 => GameMode::Adventure,
                6 => GameMode::Spectator,
                _ => unreachable!("saved player data was validated before spawning"),
            };
            held_slot.0 = saved.held_slot;
            health.current = saved.health;
            health.max = saved.max_health;
            hunger.food_level = saved.food;
            hunger.saturation = saved.saturation;
            hunger.exhaustion = saved.exhaustion;
            experience.level = saved.experience;
            experience.progress = saved.experience_progress;
            cold = crate::entity::freezing::FreezingState::from_strength(saved.cold_exposure)
                .expect("saved cold exposure was validated before spawning");
            age.0 = saved.age_ticks;
        }

        let entity = self
            .ecs
            .world_mut()
            .spawn(PlayerBundle {
                player: Player,
                name: PlayerName(player_name),
                uuid: PlayerUuid(player_uuid),
                session: PlayerSession::new(
                    data.session_id,
                    data.display_name,
                    data.xuid,
                    data.uuid,
                    data.outbound_tx,
                ),
                runtime_id: RuntimeEntityId(runtime_id),
                position,
                rotation,
                game_mode,
                state: PlayerState::default(),
                input: PlayerInput::default(),
                chunk_radius: ChunkRadius(data.chunk_radius),
                breaking_state: BreakingState::default(),
                spatial_chunk: SpatialChunk::from_position(&position),
                last_broadcast: LastBroadcastPosition {
                    x: position.0.x,
                    y: position.0.y,
                    z: position.0.z,
                    yaw: rotation.yaw,
                    pitch: rotation.pitch,
                },
                chunk_loader,
                last_publisher_state: LastPublisherState::default(),
                main_inventory,
                armour,
                offhand,
                held_slot,
                ui,
                ender_chest,
                windows: ContainerWindows::default(),
                request_ids: RequestIdAssignments::default(),
            })
            .id();
        // The streamed spawn still owes `PlayStatus(PlayerSpawn)`; the chunk
        // systems send it after the player's first view and chunks.
        let (health_sent, hunger_sent) = (health.clone(), hunger.clone());
        self.ecs.world_mut().entity_mut(entity).insert((
            age,
            cold,
            health,
            hunger,
            experience,
            crate::entity::player_life::environment::PlayerFall::default(),
            AwaitingSpawnStatus,
            hunger::FoodTimer::default(),
            // Join sends these values in its first UpdateAttributes.
            attributes::SentAttributes::sent(&health_sent, &hunger_sent, 0.0),
            crate::entity::components::Effects::default(),
            effects::SentEffects::default(),
        ));
        if operator {
            self.ecs
                .world_mut()
                .entity_mut(entity)
                .insert(crate::command::Operator);
        }

        if let Some(mut session_map) = self.ecs.world_mut().get_resource_mut::<SessionEntityMap>() {
            session_map.insert(data.session_id, entity);
        }

        self.ecs.world_mut().write_message(PlayerSpawnedEvent {
            entity,
            position: data.position,
            runtime_id,
        });
        self.send_join_packets(entity);
        info!(session_id = data.session_id, "Player spawned as ECS entity");
        Ok(entity)
    }

    pub fn despawn_player(&mut self, session_id: SessionId) {
        let entity = {
            if let Some(mut map) = self.ecs.world_mut().get_resource_mut::<SessionEntityMap>() {
                map.remove(session_id)
            } else {
                None
            }
        };
        if let Some(entity) = entity {
            self.ecs.world_mut().despawn(entity);
            info!(session_id, "Player despawned");
        }
    }

    pub fn tick(&mut self) {
        self.ecs.tick();
    }

    fn build_creative_content(
        items: &ItemRegistry,
        blocks: &BlockRegistry,
        block_ids: crate::registry::block::BlockNetworkIds,
    ) -> jolyne::valentine::CreativeContentPacket {
        use crate::registry::CreativeInventoryData;
        use jolyne::valentine::{
            CerealizerNetworkItemInstanceDescriptorSerializedData, CreativeContentPacket,
            CreativeGroupInfoPayload, CreativeItemEntryPayload,
            EnumsSharedTypesCreativeItemCategory, TypedServerNetIdstructCreativeItemNetIdTag,
        };

        let creative_data = CreativeInventoryData::load();

        let mut protocol_groups = Vec::new();
        let mut items_list = Vec::new();
        let mut entry_id_counter = 1u32;
        let mut global_group_index = 0u32;

        for (tab_name, tab_groups) in creative_data.all_groups_ordered() {
            let category_map = |tab_name: &str| match tab_name {
                "Construction" => EnumsSharedTypesCreativeItemCategory::Construction,
                "Nature" => EnumsSharedTypesCreativeItemCategory::Nature,
                "Equipment" => EnumsSharedTypesCreativeItemCategory::Equipment,
                _ => EnumsSharedTypesCreativeItemCategory::Items,
            };
            let category = category_map(tab_name);

            for group in tab_groups {
                protocol_groups.push(CreativeGroupInfoPayload {
                    creative_category: category,
                    name: group.group_name.into(),
                    group_icon_item: CerealizerNetworkItemInstanceDescriptorSerializedData::default(
                    ),
                });

                for creative_item in group.items {
                    let Some(item_entry) = items.get_by_name(creative_item.item_id()) else {
                        // Skip items not in the registry (internal state blocks, etc.)
                        continue;
                    };
                    let item_network_id = item_entry.network_id();

                    let block_runtime_id =
                        blocks.get_by_name(creative_item.item_id()).map_or(0, |b| {
                            block_ids.item_block_to_wire(item_network_id, b.default_state_id) as i32
                        });

                    items_list.push(CreativeItemEntryPayload {
                        creative_net_id: TypedServerNetIdstructCreativeItemNetIdTag {
                            id: entry_id_counter,
                        },
                        item_instance: CerealizerNetworkItemInstanceDescriptorSerializedData {
                            id: item_network_id,
                            stacksize: 1,
                            auxvalue: creative_item.damage() as u32,
                            block_runtime_id,
                            user_data_buffer: crate::item::user_data::network_user_data(
                                creative_item.item_id(),
                                None,
                            ),
                        },
                        group_index: global_group_index,
                    });
                    entry_id_counter += 1;
                }
                global_group_index += 1;
            }
        }
        CreativeContentPacket {
            groups: protocol_groups,
            entries: items_list,
        }
    }
}

impl Default for GameServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::components::{
        Age, ChunkRadius, DroppedItem, Living, OnGround, PickupDelay, PlayerSession, Velocity,
    };
    use glam::DVec3;
    use jolyne::valentine::{McpePacketData, McpePacketName};

    /// A survival block break spawns a real item entity: viewers of the chunk
    /// get `AddItemActor`, the item falls onto the floor under block physics,
    /// and once its 10-tick pickup delay ran out the nearby player collects it
    /// (`InventorySlot` + `TakeItemActor`) and every viewer gets `RemoveActor`.
    #[test]
    fn broken_block_drops_a_real_item_that_players_pick_up() {
        let mut server = void_server();
        let world = server.ecs.world_mut();
        world.insert_resource(crate::entity::EntityRandom::seeded(9));
        let mut column = crate::world::chunk::Chunk::new(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                column.set_block(x, 63, z, *crate::world::chunk::blocks::STONE);
            }
        }
        let chunk = world.spawn(crate::world::ecs::ChunkData::new(column)).id();
        world
            .resource_mut::<ChunkManager>()
            .insert(ChunkPosition::new(0, 0), chunk);
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        let player = world
            .spawn((
                Player,
                Position(DVec3::new(
                    8.5,
                    64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                    8.5,
                )),
                Rotation::default(),
                GameMode::Survival,
                MainInventory::default(),
                RuntimeEntityId(1),
                PlayerSession::new(1, "viewer".into(), None, None, tx),
            ))
            .id();
        let mut viewers = crate::world::ecs::ChunkViewers::default();
        viewers.insert(player);
        world.entity_mut(chunk).insert(viewers);
        // Dirt drops itself without a tool (stone would need a pickaxe).
        let dirt = *crate::world::chunk::blocks::DIRT;
        block_drops::drop_block_items(world, glam::IVec3::new(8, 64, 8), dirt, None);

        let mut packets = Vec::new();
        for _ in 0..30 {
            server.tick();
            while let Ok(packet) = rx.try_recv() {
                packets.push(packet.data);
            }
        }
        let added = packets
            .iter()
            .position(|p| matches!(p, McpePacketData::AddItemActorPacket(_)))
            .expect("viewer saw the item spawn");
        let taken = packets
            .iter()
            .position(|p| matches!(p, McpePacketData::TakeItemActorPacket(_)))
            .expect("player picked the item up");
        let removed = packets
            .iter()
            .position(|p| matches!(p, McpePacketData::RemoveActorPacket(_)))
            .expect("item removed from viewers");
        assert!(added < taken && taken < removed);
        let world = server.ecs.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<DroppedItem>>()
                .iter(world)
                .count(),
            0
        );
        let inventory = &world.get::<MainInventory>(player).unwrap().0;
        assert_eq!(
            inventory
                .slots()
                .iter()
                .find(|s| !s.is_empty())
                .map(|s| (s.item_id(), s.count)),
            Some(("minecraft:dirt", 1))
        );
    }

    /// A server over a void world: no generation pool, so no tokio runtime needed.
    fn void_server() -> GameServer {
        GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        })
    }

    #[test]
    fn game_tick_runs_physics_set_for_bodies_in_loaded_chunks() {
        let mut server = void_server();
        let world = server.ecs.world_mut();
        let chunk = world
            .spawn(crate::world::ecs::ChunkData::new(
                crate::world::chunk::Chunk::new(0, 0),
            ))
            .id();
        world
            .resource_mut::<ChunkManager>()
            .insert(crate::world::ecs::ChunkPosition::new(0, 0), chunk);
        let entity = world
            .spawn((
                Living,
                // Centred in the chunk: neighbouring chunks are unloaded and
                // therefore solid.
                Position(DVec3::new(8.0, 10.0, 8.0)),
                Velocity(DVec3::ZERO),
                OnGround(false),
                crate::entity::components::PhysicsBody::living(0.6, 1.8),
            ))
            .id();

        // Living bodies apply gravity after the move: the first tick only
        // builds velocity, the second one moves.
        server.tick();
        server.tick();

        let world = server.ecs.world();
        let position = world.get::<Position>(entity).expect("position");
        let velocity = world.get::<Velocity>(entity).expect("velocity");
        assert!(
            position.0.y < 10.0,
            "scheduled physics should move airborne living entities downward"
        );
        assert!(
            velocity.0.y < 0.0,
            "scheduled physics should leave falling velocity after drag"
        );
    }

    /// The real 26.52 client disconnected with `BadPacket`
    /// (`InitialConnection-90`) right after spawning while every creative
    /// item carried an empty user-data buffer; BDS 1.26.51 sends ten zero
    /// bytes (eighteen for the shield) for every non-air item.
    #[test]
    fn creative_items_carry_bds_shaped_user_data() {
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        let mut blocks = BlockRegistry::new();
        blocks.load_vanilla();
        let shield = items
            .get_by_name("minecraft:shield")
            .expect("vanilla shield")
            .network_id();
        let content = GameServer::build_creative_content(
            &items,
            &blocks,
            crate::registry::block::BlockNetworkIds::Hashed,
        );
        assert!(content.entries.len() > 1000);
        for entry in &content.entries {
            let item = &entry.item_instance;
            let expected: &[u8] = if item.id == shield {
                &[0; 18]
            } else {
                &[0; 10]
            };
            assert_eq!(item.user_data_buffer[..], *expected, "item {}", item.id);
        }
    }

    #[test]
    fn game_tick_runs_entity_lifecycle_systems() {
        let mut server = void_server();
        let entity = server
            .ecs
            .world_mut()
            .spawn((Age(0), DroppedItem, PickupDelay::new(2)))
            .id();

        server.tick();

        let world = server.ecs.world();
        let age = world.get::<Age>(entity).expect("age");
        let pickup_delay = world.get::<PickupDelay>(entity).expect("pickup delay");
        assert_eq!(age.0, 1);
        assert_eq!(pickup_delay.0, 1);
    }

    #[test]
    fn configured_operators_are_granted_on_join_by_name_or_xuid() {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            operators: vec!["MobBot".into(), "2535400000000042".into()],
            ..ServerConfig::default()
        });
        let mut join = |session_id: u64, name: &str, xuid: Option<&str>| {
            let (outbound_tx, _rx) = tokio::sync::mpsc::channel(256);
            let entity = server
                .spawn_player(PlayerSpawnData {
                    session_id,
                    display_name: name.to_string(),
                    xuid: xuid.map(str::to_owned),
                    uuid: None,
                    runtime_id: session_id as i64,
                    position: DVec3::new(0.5, 17.0, 0.5),
                    outbound_tx,
                    saved_data: None,
                    chunk_radius: 2,
                })
                .expect("fresh player spawns without saved data");
            server
                .ecs
                .world()
                .get::<crate::command::Operator>(entity)
                .is_some()
        };
        assert!(join(1, "mobbot", None), "names match case-insensitively");
        assert!(join(2, "Someone", Some("2535400000000042")));
        assert!(!join(3, "Someone", Some("2535400000000043")));
        assert!(!join(4, "MobBot2", None));
    }

    #[test]
    fn player_spawn_sets_join_chunk_gate_without_network_io() {
        let mut server = GameServer::with_config(ServerConfig {
            default_chunk_radius: 2,
            max_chunk_radius: 4,
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel(32);

        let entity = server
            .spawn_player(PlayerSpawnData {
                session_id: 42,
                display_name: "smoke".to_string(),
                xuid: None,
                uuid: Some("00000000-0000-0000-0000-000000000042".to_string()),
                runtime_id: 7,
                position: DVec3::new(0.5, 17.0, 0.5),
                outbound_tx,
                saved_data: None,
                chunk_radius: 2,
            })
            .expect("valid player spawn");

        {
            let world = server.ecs.world();
            let chunk_radius = world.get::<ChunkRadius>(entity).expect("chunk radius");
            assert_eq!(chunk_radius.0, 2);

            let chunk_loader = world.get::<ChunkLoader>(entity).expect("chunk loader");
            assert_eq!(chunk_loader.position(), (0, 0));
            assert_eq!(chunk_loader.queue_len(), 25);
            assert_eq!(chunk_loader.peek_next_to_load(), Some((0, 0)));
            assert!(
                chunk_loader.has_pending(),
                "spawn should leave chunk publish work queued for the join gate"
            );

            let session = world.get::<PlayerSession>(entity).expect("player session");
            assert_eq!(session.packets_dropped(), 0);
        }

        // jolyne's join already answered RequestChunkRadius and sent the actor
        // identifiers with StartGame; BDS sends neither again after spawn.
        let mut names = Vec::new();
        while let Ok(packet) = outbound_rx.try_recv() {
            names.push(packet.data.packet_id());
        }
        for duplicate in [
            McpePacketName::ChunkRadiusUpdatedPacket,
            McpePacketName::AvailableActorIdentifiersPacket,
        ] {
            assert!(
                !names.contains(&duplicate),
                "{duplicate:?} resent: {names:?}"
            );
        }
    }

    /// The streamed spawn hands the player over at its radius request, before
    /// `PlayStatus(PlayerSpawn)`. The first tick sends BDS's order: the view
    /// (`NetworkChunkPublisherUpdate`), the chunks it has, then `PlayerSpawn`
    /// once; the rest of the view keeps streaming on later ticks.
    #[test]
    fn first_tick_after_join_sends_view_chunks_then_player_spawn() {
        let mut server = GameServer::with_config(ServerConfig {
            default_chunk_radius: 4,
            max_chunk_radius: 4,
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel(1024);
        server
            .spawn_player(PlayerSpawnData {
                session_id: 42,
                display_name: "smoke".to_string(),
                xuid: None,
                uuid: Some("00000000-0000-0000-0000-000000000042".to_string()),
                runtime_id: 7,
                position: DVec3::new(0.5, 17.0, 0.5),
                outbound_tx,
                saved_data: None,
                chunk_radius: 4,
            })
            .expect("fresh player spawns");
        let mut drain = || {
            let mut packets = Vec::new();
            while let Ok(packet) = outbound_rx.try_recv() {
                packets.push(packet);
            }
            packets
        };
        let names = |packets: &[jolyne::valentine::McpePacket]| -> Vec<_> {
            packets
                .iter()
                .map(|packet| packet.data.packet_id())
                .collect()
        };
        let joined = drain();
        assert!(
            !names(&joined).contains(&McpePacketName::PlayStatusPacket),
            "PlayerSpawn waits for the first view"
        );

        server.tick();
        let first = names(&drain());
        let position = |name| first.iter().position(|sent| *sent == name);
        let publisher = position(McpePacketName::NetworkChunkPublisherUpdatePacket)
            .unwrap_or_else(|| panic!("no publisher update on the first tick: {first:?}"));
        let first_chunk = position(McpePacketName::LevelChunkPacket)
            .unwrap_or_else(|| panic!("no chunk on the first tick: {first:?}"));
        let spawn = position(McpePacketName::PlayStatusPacket)
            .unwrap_or_else(|| panic!("no PlayerSpawn on the first tick: {first:?}"));
        assert!(publisher < first_chunk && first_chunk < spawn, "{first:?}");
        let chunks = |names: &[McpePacketName]| {
            names
                .iter()
                .filter(|name| **name == McpePacketName::LevelChunkPacket)
                .count()
        };

        server.tick();
        let second = names(&drain());
        assert!(chunks(&second) > 0, "the view keeps streaming: {second:?}");
        assert!(
            !second.contains(&McpePacketName::PlayStatusPacket),
            "PlayerSpawn is sent once"
        );
    }
}

#[cfg(test)]
mod saved_inventory_validation_tests {
    use super::*;
    use crate::storage::{StorageError, StoredItemStack};

    #[test]
    fn unknown_late_saved_slot_cannot_publish_a_reset_or_partial_player() {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let uuid = uuid::Uuid::from_u128(42).to_string();
        let saved = PlayerData {
            uuid: uuid.clone(),
            inventory: vec![StoredItemStack {
                item_id: "minecraft:stick".into(),
                count: 3,
                ..Default::default()
            }],
            cursor: StoredItemStack {
                item_id: "removed_addon:irreplaceable_item".into(),
                count: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let entities_before = server.ecs.world().entities().len();
        let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel(32);
        let result = server.spawn_player(PlayerSpawnData {
            session_id: 42,
            display_name: "saved-inventory".into(),
            xuid: None,
            uuid: Some(uuid),
            runtime_id: 7,
            position: glam::DVec3::new(0.5, 17.0, 0.5),
            outbound_tx,
            saved_data: Some(saved),
            chunk_radius: 0,
        });
        assert!(matches!(result, Err(StorageError::Deserialization(_))));
        assert_eq!(server.ecs.world().entities().len(), entities_before);
        assert!(
            server
                .ecs
                .world()
                .resource::<SessionEntityMap>()
                .get(42)
                .is_none()
        );
        assert!(
            outbound_rx.try_recv().is_err(),
            "invalid saved state must not emit a successful join"
        );
    }
}
