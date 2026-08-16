//! Player join packet handling.
//!
//! Contains the send_join_packets method for sending initial game state.

use super::GameServer;
use crate::entity::components::{GameMode, PlayerSession, RuntimeEntityId};
use jolyne::valentine::types::{
    ActorRuntimeId, AttributeData, CerealizerNetworkItemStackDescriptorSerializedData,
    DataItemEntry, DataItemEntryPayload, DataItemFloatPayload, DataItemInt64Payload,
    DataItemShortPayload, EnumsCommandPermissionLevel, EnumsContainerEnumName, EnumsDataItemType,
    EnumsGameType, EnumsPlayerPermissionLevel, FullContainerName, PlayerInputTick,
    PropertySyncData, SerializedAbilitiesData, SerializedAbilitiesDataSerializedLayer,
    SynchedActorDataCopyableDataList,
};
use jolyne::valentine::{
    ChunkRadiusUpdatedPacket, SetActorDataPacket, UpdateAbilitiesPacket, UpdateAttributesPacket,
};
use jolyne::valentine::{InventoryContentPacket, McpePacket, SetPlayerGameTypePacket};
use tracing::debug;

const BUILD: u32 = 1;
const MINE: u32 = 1 << 1;
const DOORS_AND_SWITCHES: u32 = 1 << 2;
const OPEN_CONTAINERS: u32 = 1 << 3;
const ATTACK_PLAYERS: u32 = 1 << 4;
const ATTACK_MOBS: u32 = 1 << 5;
const INVULNERABLE: u32 = 1 << 8;
const MAY_FLY: u32 = 1 << 10;
const INSTANT_BUILD: u32 = 1 << 11;
const FLY_SPEED: u32 = 1 << 13;
const WALK_SPEED: u32 = 1 << 14;
const VERTICAL_FLY_SPEED: u32 = 1 << 19;

const BREATHING: u64 = 1 << 35;
const CAN_CLIMB: u64 = 1 << 19;
const HAS_COLLISION: u64 = 1 << 48;
const AFFECTED_BY_GRAVITY: u64 = 1 << 49;

impl GameServer {
    /// Send all join packets to a newly spawned player.
    pub(super) fn send_join_packets(&self, entity: bevy_ecs::entity::Entity) {
        let world = self.ecs.world();
        let session = match world.get::<PlayerSession>(entity) {
            Some(s) => s,
            None => return,
        };
        let runtime_id = world
            .get::<RuntimeEntityId>(entity)
            .map(|r| r.0)
            .unwrap_or(1);
        let game_mode = world
            .get::<GameMode>(entity)
            .copied()
            .unwrap_or(GameMode::Survival);

        let config = world
            .get_resource::<super::types::ServerConfigResource>()
            .unwrap();
        let world_template = world
            .get_resource::<super::types::ServerWorldTemplate>()
            .unwrap();
        let current_tick = world
            .get_resource::<crate::ecs::resources::TickCounter>()
            .unwrap()
            .current;

        let _ = session.send(McpePacket::from(ChunkRadiusUpdatedPacket {
            chunk_radius: config.0.default_chunk_radius,
        }));
        let _ = session.send(McpePacket::from(
            world_template.0.available_entities.as_ref().clone(),
        ));

        // Send gamemode to client (hides hunger bar in creative, etc.)
        let protocol_gamemode = match game_mode {
            GameMode::Survival => EnumsGameType::Survival,
            GameMode::Creative => EnumsGameType::Creative,
            GameMode::Adventure => EnumsGameType::Adventure,
            GameMode::Spectator => EnumsGameType::Spectator,
        };
        let _ = session.send(McpePacket::from(SetPlayerGameTypePacket {
            player_game_type: protocol_gamemode,
        }));
        debug!("Sent SetPlayerGameType: {:?}", game_mode);

        // Build abilities based on gamemode (following Dragonfly's approach)
        let mut abilities = WALK_SPEED | FLY_SPEED | VERTICAL_FLY_SPEED;

        // All modes can interact (except spectator limitations handled elsewhere)
        if game_mode.can_break_blocks() {
            abilities |= BUILD | MINE;
        }
        abilities |= DOORS_AND_SWITCHES | OPEN_CONTAINERS;
        abilities |= ATTACK_PLAYERS | ATTACK_MOBS;

        // Creative/Spectator: allow flight and invulnerability
        if game_mode.allows_flight() {
            abilities |= MAY_FLY;
        }
        if !game_mode.allows_damage() {
            abilities |= INVULNERABLE;
        }
        // Creative: instant break
        if game_mode.instant_break() {
            abilities |= INSTANT_BUILD;
        }

        let layer = SerializedAbilitiesDataSerializedLayer {
            serialized_layer: 1,
            // Allowed = all abilities that CAN be toggled
            abilities_set: BUILD
                | MINE
                | DOORS_AND_SWITCHES
                | OPEN_CONTAINERS
                | ATTACK_PLAYERS
                | ATTACK_MOBS
                | WALK_SPEED
                | FLY_SPEED
                | VERTICAL_FLY_SPEED
                | MAY_FLY
                | INVULNERABLE
                | INSTANT_BUILD,
            // Enabled = abilities that are currently active
            ability_values: abilities,
            fly_speed: 0.05,         // Horizontal flight speed (Dragonfly default)
            vertical_fly_speed: 1.0, // Vertical flight speed (Dragonfly default)
            walk_speed: 0.1,
        };

        let _ = session.send(McpePacket::from(UpdateAbilitiesPacket {
            data: SerializedAbilitiesData {
                target_player_raw_id: runtime_id,
                player_permissions: EnumsPlayerPermissionLevel::Member,
                command_permissions: EnumsCommandPermissionLevel::Any,
                layers: vec![layer],
            },
        }));

        fn attr(
            name: &str,
            current: f32,
            max: f32,
            default: f32,
            default_max: f32,
        ) -> AttributeData {
            AttributeData {
                min_value: 0.0,
                max_value: max,
                current_value: current,
                default_min_value: 0.0,
                default_max_value: default_max,
                default_value: default,
                name: name.to_string(),
                modifiers: vec![],
            }
        }

        let attributes = vec![
            attr("minecraft:health", 20.0, 20.0, 20.0, 20.0),
            attr("minecraft:absorption", 0.0, f32::MAX, 0.0, f32::MAX),
            attr("minecraft:movement", 0.1, f32::MAX, 0.1, f32::MAX),
            attr("minecraft:player.hunger", 20.0, 20.0, 20.0, 20.0),
            attr("minecraft:player.saturation", 20.0, 20.0, 20.0, 20.0),
            attr("minecraft:player.exhaustion", 0.0, 5.0, 0.0, 5.0),
            attr(
                "minecraft:player.level",
                0.0,
                i32::MAX as f32,
                0.0,
                i32::MAX as f32,
            ),
            attr("minecraft:player.experience", 0.0, 1.0, 0.0, 1.0),
        ];

        let _ = session.send(McpePacket::from(UpdateAttributesPacket {
            target_runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            attribute_list: attributes,
            tick: PlayerInputTick {
                inputtick: current_tick,
            },
        }));

        // Send entity metadata with proper flags for player behavior:
        // - BREATHING: prevents drowning UI/air bubbles
        // - CAN_CLIMB: enables ladder climbing
        // - HAS_COLLISION: enables player collision
        // - AFFECTED_BY_GRAVITY: enables gravity and jumping
        let flags = BREATHING | CAN_CLIMB | HAS_COLLISION | AFFECTED_BY_GRAVITY;
        let actor_data = vec![
            DataItemEntry {
                id: 0,
                payload: DataItemEntryPayload::DataItemInt64Payload(DataItemInt64Payload {
                    type_: EnumsDataItemType::Int64,
                    value: flags as i64,
                }),
            },
            DataItemEntry {
                id: 7,
                payload: DataItemEntryPayload::DataItemShortPayload(DataItemShortPayload {
                    type_: EnumsDataItemType::Short,
                    value: 300,
                }),
            },
            DataItemEntry {
                id: 42,
                payload: DataItemEntryPayload::DataItemShortPayload(DataItemShortPayload {
                    type_: EnumsDataItemType::Short,
                    value: 300,
                }),
            },
            DataItemEntry {
                id: 53,
                payload: DataItemEntryPayload::DataItemFloatPayload(DataItemFloatPayload {
                    type_: EnumsDataItemType::Float,
                    value: 0.6,
                }),
            },
            DataItemEntry {
                id: 54,
                payload: DataItemEntryPayload::DataItemFloatPayload(DataItemFloatPayload {
                    type_: EnumsDataItemType::Float,
                    value: 1.8,
                }),
            },
            DataItemEntry {
                id: 38,
                payload: DataItemEntryPayload::DataItemFloatPayload(DataItemFloatPayload {
                    type_: EnumsDataItemType::Float,
                    value: 1.0,
                }),
            },
        ];

        let _ = session.send(McpePacket::from(SetActorDataPacket {
            target_runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            actor_data: SynchedActorDataCopyableDataList { data: actor_data },
            synched_properties: PropertySyncData::default(),
            tick: PlayerInputTick {
                inputtick: current_tick,
            },
        }));

        // Send inventory contents to enable inventory UI
        // Without these packets, the client won't allow opening the inventory
        self.send_inventory_contents(session);

        // Creative content is sent via WorldTemplate during the StartGame sequence
        // (see jolyne::stream::server). Network IDs are now correct from required_item_list.json.
    }

    /// Send initial inventory contents to the client.
    ///
    /// This is required for the inventory UI to work. We send empty inventories
    /// for all player inventory windows:
    /// - Main inventory (36 slots: 9 hotbar + 27 main)
    /// - Offhand (1 slot)
    /// - Armor (4 slots)
    /// - UI (for crafting grid, cursor, etc.)
    fn send_inventory_contents(&self, session: &PlayerSession) {
        debug!("Sending inventory contents to client");

        // Helper to create an empty item
        let empty_item = CerealizerNetworkItemStackDescriptorSerializedData::default();

        // Helper to create FullContainerName for inventory slots
        let container_name = FullContainerName {
            container_name: EnumsContainerEnumName::InventoryContainer,
            dynamic_id: None,
        };

        // Main inventory: 36 empty slots (hotbar 0-8, main 9-35)
        let result = session.send(McpePacket::from(InventoryContentPacket {
            container_id: 0,
            slots: vec![empty_item.clone(); 36],
            full_container_name: container_name.clone(),
            storage_item: empty_item.clone(),
        }));
        debug!("Sent main inventory (36 slots, window=0): {:?}", result);

        // Offhand: 1 empty slot
        let result = session.send(McpePacket::from(InventoryContentPacket {
            container_id: 119,
            slots: vec![empty_item.clone(); 1],
            full_container_name: FullContainerName {
                container_name: EnumsContainerEnumName::OffhandContainer,
                dynamic_id: None,
            },
            storage_item: empty_item.clone(),
        }));
        debug!("Sent offhand inventory (1 slot, window=119): {:?}", result);

        // Armor: 4 empty slots (helmet, chestplate, leggings, boots)
        let result = session.send(McpePacket::from(InventoryContentPacket {
            container_id: 120,
            slots: vec![empty_item.clone(); 4],
            full_container_name: FullContainerName {
                container_name: EnumsContainerEnumName::ArmorContainer,
                dynamic_id: None,
            },
            storage_item: empty_item.clone(),
        }));
        debug!("Sent armor inventory (4 slots, window=120): {:?}", result);

        // UI inventory (crafting grid, cursor, etc.)
        // The UI inventory needs a larger size to support crafting operations
        let result = session.send(McpePacket::from(InventoryContentPacket {
            container_id: 124,
            slots: vec![empty_item.clone(); 51], // UI inventory size from Dragonfly
            full_container_name: FullContainerName {
                container_name: EnumsContainerEnumName::CursorContainer,
                dynamic_id: None,
            },
            storage_item: empty_item,
        }));
        debug!("Sent UI inventory (51 slots, window=124): {:?}", result);
    }
}
