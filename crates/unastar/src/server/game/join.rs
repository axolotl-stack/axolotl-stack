//! Player join packet handling.
//!
//! Contains the send_join_packets method for sending initial game state.

use super::GameServer;
use crate::entity::components::{GameMode, PlayerSession, RuntimeEntityId};
use jolyne::valentine::items::ITEMS;
use jolyne::valentine::types::{
    AbilityLayers, AbilityLayersType, AbilitySet, CommandPermissionLevel, ContainerSlotType,
    EntityProperties, FullContainerName, GameMode as ProtocolGameMode, Item, ItemLegacy,
    ItemLegacyContent, ItemLegacyContentExtra, MetadataDictionary, MetadataDictionaryItem,
    MetadataDictionaryItemKey, MetadataDictionaryItemType, MetadataDictionaryItemValue,
    MetadataDictionaryItemValueDefault, MetadataFlags1, PermissionLevel, PlayerAttributesItem,
    WindowIdVarint,
};
use jolyne::valentine::{
    ChunkRadiusUpdatePacket, SetEntityDataPacket, UpdateAbilitiesPacket, UpdateAttributesPacket,
};
use jolyne::valentine::{
    CreativeContentPacket, CreativeContentPacketGroupsItem,
    CreativeContentPacketGroupsItemCategory, CreativeContentPacketItemsItem,
    InventoryContentPacket, McpePacket, SetPlayerGameTypePacket,
};
use tracing::debug;

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

        let _ = session.send(McpePacket::from(ChunkRadiusUpdatePacket {
            chunk_radius: self.config.default_chunk_radius,
        }));
        let _ = session.send(McpePacket::from(
            self.world_template.available_entities.as_ref().clone(),
        ));

        // Send gamemode to client (hides hunger bar in creative, etc.)
        let protocol_gamemode = match game_mode {
            GameMode::Survival => ProtocolGameMode::Survival,
            GameMode::Creative => ProtocolGameMode::Creative,
            GameMode::Adventure => ProtocolGameMode::Adventure,
            GameMode::Spectator => ProtocolGameMode::Spectator,
        };
        let _ = session.send(McpePacket::from(SetPlayerGameTypePacket {
            gamemode: protocol_gamemode,
        }));
        debug!("Sent SetPlayerGameType: {:?}", game_mode);

        // Build abilities based on gamemode (following Dragonfly's approach)
        let mut abilities =
            AbilitySet::WALK_SPEED | AbilitySet::FLY_SPEED | AbilitySet::VERTICAL_FLY_SPEED;

        // All modes can interact (except spectator limitations handled elsewhere)
        if game_mode.can_break_blocks() {
            abilities |= AbilitySet::BUILD | AbilitySet::MINE;
        }
        abilities |= AbilitySet::DOORS_AND_SWITCHES | AbilitySet::OPEN_CONTAINERS;
        abilities |= AbilitySet::ATTACK_PLAYERS | AbilitySet::ATTACK_MOBS;

        // Creative/Spectator: allow flight and invulnerability
        if game_mode.allows_flight() {
            abilities |= AbilitySet::MAY_FLY;
        }
        if !game_mode.allows_damage() {
            abilities |= AbilitySet::INVULNERABLE;
        }
        // Creative: instant break
        if game_mode.instant_break() {
            abilities |= AbilitySet::INSTANT_BUILD;
        }

        let layer = AbilityLayers {
            type_: AbilityLayersType::Base,
            // Allowed = all abilities that CAN be toggled
            allowed: AbilitySet::BUILD
                | AbilitySet::MINE
                | AbilitySet::DOORS_AND_SWITCHES
                | AbilitySet::OPEN_CONTAINERS
                | AbilitySet::ATTACK_PLAYERS
                | AbilitySet::ATTACK_MOBS
                | AbilitySet::WALK_SPEED
                | AbilitySet::FLY_SPEED
                | AbilitySet::VERTICAL_FLY_SPEED
                | AbilitySet::MAY_FLY
                | AbilitySet::INVULNERABLE
                | AbilitySet::INSTANT_BUILD,
            // Enabled = abilities that are currently active
            enabled: abilities,
            fly_speed: 0.05,         // Horizontal flight speed (Dragonfly default)
            vertical_fly_speed: 1.0, // Vertical flight speed (Dragonfly default)
            walk_speed: 0.1,
        };

        let _ = session.send(McpePacket::from(UpdateAbilitiesPacket {
            entity_unique_id: runtime_id,
            permission_level: PermissionLevel::Member,
            command_permission: CommandPermissionLevel::Normal,
            abilities: vec![layer],
        }));

        fn attr(
            name: &str,
            current: f32,
            max: f32,
            default: f32,
            default_max: f32,
        ) -> PlayerAttributesItem {
            PlayerAttributesItem {
                min: 0.0,
                max,
                current,
                default_min: 0.0,
                default_max,
                default,
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
            runtime_entity_id: runtime_id,
            attributes,
            tick: self.current_tick as i64,
        }));

        // Send entity metadata with proper flags for player behavior:
        // - BREATHING: prevents drowning UI/air bubbles
        // - CAN_CLIMB: enables ladder climbing
        // - HAS_COLLISION: enables player collision
        // - AFFECTED_BY_GRAVITY: enables gravity and jumping
        let flags = MetadataFlags1::BREATHING
            | MetadataFlags1::CAN_CLIMB
            | MetadataFlags1::HAS_COLLISION
            | MetadataFlags1::AFFECTED_BY_GRAVITY;

        let metadata: MetadataDictionary = vec![
            // Entity flags
            MetadataDictionaryItem {
                key: MetadataDictionaryItemKey::Flags,
                type_: MetadataDictionaryItemType::Long,
                value: MetadataDictionaryItemValue::Flags(flags),
            },
            // Current air supply (300 ticks = 15 seconds)
            MetadataDictionaryItem {
                key: MetadataDictionaryItemKey::Air,
                type_: MetadataDictionaryItemType::Short,
                value: MetadataDictionaryItemValue::Default(Box::new(Some(
                    MetadataDictionaryItemValueDefault::Short(300),
                ))),
            },
            // Max air supply
            MetadataDictionaryItem {
                key: MetadataDictionaryItemKey::MaxAirdataMaxAir,
                type_: MetadataDictionaryItemType::Short,
                value: MetadataDictionaryItemValue::Default(Box::new(Some(
                    MetadataDictionaryItemValueDefault::Short(300),
                ))),
            },
            // Player bounding box width
            MetadataDictionaryItem {
                key: MetadataDictionaryItemKey::BoundingboxWidth,
                type_: MetadataDictionaryItemType::Float,
                value: MetadataDictionaryItemValue::Default(Box::new(Some(
                    MetadataDictionaryItemValueDefault::Float(0.6),
                ))),
            },
            // Player bounding box height
            MetadataDictionaryItem {
                key: MetadataDictionaryItemKey::BoundingboxHeight,
                type_: MetadataDictionaryItemType::Float,
                value: MetadataDictionaryItemValue::Default(Box::new(Some(
                    MetadataDictionaryItemValueDefault::Float(1.8),
                ))),
            },
            // Entity scale
            MetadataDictionaryItem {
                key: MetadataDictionaryItemKey::Scale,
                type_: MetadataDictionaryItemType::Float,
                value: MetadataDictionaryItemValue::Default(Box::new(Some(
                    MetadataDictionaryItemValueDefault::Float(1.0),
                ))),
            },
        ];

        let _ = session.send(McpePacket::from(SetEntityDataPacket {
            runtime_entity_id: runtime_id,
            metadata,
            properties: EntityProperties::default(),
            tick: self.current_tick as i64,
        }));

        // Send inventory contents to enable inventory UI
        // Without these packets, the client won't allow opening the inventory
        self.send_inventory_contents(session);

        // Creative content packet causes client disconnect - needs investigation
        // TODO: Fix item format in creative content packet
        // self.send_creative_content(session);  // DISABLED FOR TEST - use jolyne's empty one
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
        let empty_item = Item::default(); // content: None = empty/air

        // Helper to create FullContainerName for inventory slots
        let container_name = FullContainerName {
            container_id: ContainerSlotType::Inventory,
            dynamic_container_id: None,
        };

        // Main inventory: 36 empty slots (hotbar 0-8, main 9-35)
        let result = session.send(McpePacket::from(InventoryContentPacket {
            window_id: WindowIdVarint::Inventory, // 0
            input: vec![empty_item.clone(); 36],
            container: container_name.clone(),
            storage_item: empty_item.clone(),
        }));
        debug!("Sent main inventory (36 slots, window=0): {:?}", result);

        // Offhand: 1 empty slot
        let result = session.send(McpePacket::from(InventoryContentPacket {
            window_id: WindowIdVarint::Offhand, // 119
            input: vec![empty_item.clone(); 1],
            container: FullContainerName {
                container_id: ContainerSlotType::Offhand,
                dynamic_container_id: None,
            },
            storage_item: empty_item.clone(),
        }));
        debug!("Sent offhand inventory (1 slot, window=119): {:?}", result);

        // Armor: 4 empty slots (helmet, chestplate, leggings, boots)
        let result = session.send(McpePacket::from(InventoryContentPacket {
            window_id: WindowIdVarint::Armor, // 120
            input: vec![empty_item.clone(); 4],
            container: FullContainerName {
                container_id: ContainerSlotType::Armor,
                dynamic_container_id: None,
            },
            storage_item: empty_item.clone(),
        }));
        debug!("Sent armor inventory (4 slots, window=120): {:?}", result);

        // UI inventory (crafting grid, cursor, etc.)
        // The UI inventory needs a larger size to support crafting operations
        let result = session.send(McpePacket::from(InventoryContentPacket {
            window_id: WindowIdVarint::Ui,       // 124
            input: vec![empty_item.clone(); 51], // UI inventory size from Dragonfly
            container: FullContainerName {
                container_id: ContainerSlotType::Cursor,
                dynamic_container_id: None,
            },
            storage_item: empty_item,
        }));
        debug!("Sent UI inventory (51 slots, window=124): {:?}", result);
    }

    /// Send creative content to the client.
    ///
    /// This populates the creative inventory with all vanilla items
    /// from the ITEMS registry.
    fn send_creative_content(&self, session: &PlayerSession) {
        debug!("Sending creative content to client");

        // Define at least one anonymous group for items to reference.
        // Per gophertunnel: "Every item must be part of a group, any items that are not
        // part of a group will need to reference an 'anonymous group' which has an empty
        // name OR no icon."
        let groups = vec![CreativeContentPacketGroupsItem {
            category: CreativeContentPacketGroupsItemCategory::All,
            name: String::new(), // Empty name = anonymous group
            icon_item: ItemLegacy {
                network_id: 0, // Air = no icon
                content: None,
            },
        }];

        // Build items list from ITEMS registry
        // Each item needs: entry_id, ItemLegacy, group_index
        let items: Vec<CreativeContentPacketItemsItem> = ITEMS
            .iter()
            .enumerate()
            .filter(|(_, item)| item.id() != 0) // Skip air (id 0)
            .map(|(idx, item)| {
                CreativeContentPacketItemsItem {
                    entry_id: (idx + 1) as i32, // 1-indexed entry IDs
                    item: ItemLegacy {
                        network_id: item.id() as i32, // Item's runtime network ID
                        content: Some(Box::new(ItemLegacyContent {
                            count: 1,
                            metadata: item.metadata() as i32,
                            block_runtime_id: 0, // Not a block placement
                            extra: ItemLegacyContentExtra::default(),
                        })),
                    },
                    group_index: 0, // Reference the anonymous group at index 0
                }
            })
            .collect();

        let item_count = items.len();

        // Debug: Print the exact bytes of just the groups for comparison
        let test_packet = CreativeContentPacket {
            groups: groups.clone(), // Use groups for testing
            items: vec![],
        };

        // Manually encode to see bytes
        use jolyne::valentine::bedrock::codec::BedrockCodec;
        let mut debug_buf = bytes::BytesMut::new();
        if let Err(e) = test_packet.encode(&mut debug_buf) {
            debug!("Failed to encode for debug: {:?}", e);
        } else {
            debug!(
                "CreativeContent packet bytes (groups only): {:02x?}",
                debug_buf.as_ref()
            );
            debug!("CreativeContent packet len: {} bytes", debug_buf.len());
        }

        let result = session.send(McpePacket::from(CreativeContentPacket {
            groups,        // Send with groups
            items: vec![], // But no items for now
        }));

        debug!("Sent creative content ({} items): {:?}", item_count, result);
    }
}
