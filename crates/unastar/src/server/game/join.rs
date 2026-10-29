//! Player join packet handling.
//!
//! Contains the send_join_packets method for sending initial game state.

use super::GameServer;
use crate::entity::components::{
    ArmourInventory, Experience, GameMode, Health, HeldSlot, Hunger, MainInventory, OffhandSlot,
    PlayerSession, PlayerUi, RuntimeEntityId,
};
use crate::entity::player_life::attributes::player_attributes;
use crate::world::clock;
use crate::world::rules::{Difficulty, GameRules, WorldTime};
use bevy_ecs::prelude::{Entity, World};
use jolyne::valentine::types::{
    ActorRuntimeId, CerealizerNetworkItemStackDescriptorSerializedData, DataItemEntry,
    DataItemEntryPayload, DataItemFloatPayload, DataItemInt64Payload, DataItemShortPayload,
    EnumsCommandPermissionLevel, EnumsDataItemType, EnumsGameType, EnumsPlayerPermissionLevel,
    PlayerInputTick, PropertySyncData, SerializedAbilitiesData,
    SerializedAbilitiesDataSerializedLayer, SynchedActorDataCopyableDataList,
};
use jolyne::valentine::{
    GameRulesChangedPacket, InventoryContentPacket, McpePacket, SetDifficultyPacket,
    SetPlayerGameTypePacket,
};
use jolyne::valentine::{SetActorDataPacket, UpdateAbilitiesPacket, UpdateAttributesPacket};
use jolyne::valentine::{actor_data_i_ds as actor_data_ids, actor_flags};
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

        let current_tick = world
            .get_resource::<crate::ecs::resources::TickCounter>()
            .unwrap()
            .current;

        for packet in world
            .resource::<crate::entity::definition::properties::ActorPropertyRegistry>()
            .packets()
        {
            let _ = session.send(McpePacket::from(packet.clone()));
        }

        // The world clock registry, then the difficulty (BDS join order).
        let _ = session.send(McpePacket::from(clock::clock_registry_packet(
            *world.resource::<WorldTime>(),
        )));
        let _ = session.send(McpePacket::from(SetDifficultyPacket {
            difficulty: world.resource::<Difficulty>().wire(),
        }));

        // Send gamemode to client (hides hunger bar in creative, etc.)
        let _ = session.send(McpePacket::from(SetPlayerGameTypePacket {
            player_game_type: game_type(game_mode),
        }));
        debug!("Sent SetPlayerGameType: {:?}", game_mode);
        let _ = session.send(McpePacket::from(abilities_packet(runtime_id, game_mode)));
        let _ = session.send(McpePacket::from(GameRulesChangedPacket {
            rule_data: world.resource::<GameRules>().packet_data(),
        }));

        let health = world.get::<Health>(entity).cloned().unwrap_or_default();
        let hunger = world.get::<Hunger>(entity).cloned().unwrap_or_default();
        let experience = world.get::<Experience>(entity).cloned().unwrap_or_default();
        let _ = session.send(McpePacket::from(UpdateAttributesPacket {
            target_runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            attribute_list: player_attributes(&health, &hunger, &experience),
            tick: PlayerInputTick {
                inputtick: current_tick,
            },
        }));

        let actor_data = initial_player_actor_data();

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
        send_inventory_contents(world, entity);
        let crafting = world
            .resource::<super::types::CraftingCatalogResource>()
            .0
            .wire_packet(super::types::block_network_ids(world));
        let _ = session.send(McpePacket::from(crafting));

        // Creative content is sent via WorldTemplate during the StartGame sequence
        // (see jolyne::stream::server). Network IDs are now correct from required_item_list.json.
    }
}

/// Send a player's inventory, armour, UI (cursor and crafting grid) and
/// offhand contents and its selected hotbar slot to its client: the restored
/// state at join, the emptied containers after a death.
///
/// Initial item state matches BDS: main 36, armour 5, UI 54, offhand 1,
/// then the selected hotbar slot. The fifth armour slot is empty for players.
pub(crate) fn send_inventory_contents(world: &World, entity: Entity) {
    use super::inventory::{
        ARMOR_WINDOW, INVENTORY_WINDOW, OFFHAND_WINDOW, UI_WINDOW, UNNAMED_CONTAINER, wire_item,
    };
    let Some(session) = world.get::<PlayerSession>(entity) else {
        return;
    };
    let block_ids = super::types::block_network_ids(world);
    let encode = |stack: &crate::item::ItemStack| {
        wire_item(stack, block_ids).expect("registered player stack has encodable fields")
    };
    let main = world
        .get::<MainInventory>(entity)
        .expect("player inventory");
    let armour = world.get::<ArmourInventory>(entity).expect("player armour");
    let offhand = world.get::<OffhandSlot>(entity).expect("player offhand");
    let ui = world.get::<PlayerUi>(entity).expect("player UI");
    let empty_item = CerealizerNetworkItemStackDescriptorSerializedData::default();
    let mut armour_slots = Vec::with_capacity(5);
    armour_slots.extend(armour.slots().iter().map(&encode));
    armour_slots.push(empty_item.clone());
    for (container_id, slots) in [
        (
            INVENTORY_WINDOW,
            main.0.slots().iter().map(&encode).collect(),
        ),
        (ARMOR_WINDOW, armour_slots),
        (UI_WINDOW, ui.0.slots().iter().map(&encode).collect()),
        (OFFHAND_WINDOW, vec![encode(&offhand.0)]),
    ] {
        let _ = session.send(McpePacket::from(InventoryContentPacket {
            container_id: container_id.into(),
            slots,
            full_container_name: UNNAMED_CONTAINER,
            storage_item: empty_item.clone(),
        }));
    }
    let held_slot = world.get::<HeldSlot>(entity).expect("player held slot").0;
    let _ = session.send(McpePacket::from(jolyne::valentine::PlayerHotbarPacket {
        selected_slot: u32::from(held_slot),
        container_id: INVENTORY_WINDOW,
        shouldselectslot: true,
    }));
}

/// A game mode as the wire names it.
pub(crate) fn game_type(game_mode: GameMode) -> EnumsGameType {
    match game_mode {
        GameMode::Survival => EnumsGameType::Survival,
        GameMode::Creative => EnumsGameType::Creative,
        GameMode::Adventure => EnumsGameType::Adventure,
        GameMode::Spectator => EnumsGameType::Spectator,
    }
}

/// The abilities a game mode grants a player (following Dragonfly's
/// approach).
pub(crate) fn abilities_packet(runtime_id: i64, game_mode: GameMode) -> UpdateAbilitiesPacket {
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

    UpdateAbilitiesPacket {
        data: SerializedAbilitiesData {
            target_player_raw_id: runtime_id,
            player_permissions: EnumsPlayerPermissionLevel::Member,
            command_permissions: EnumsCommandPermissionLevel::Any,
            layers: vec![layer],
        },
    }
}

fn initial_player_actor_data() -> Vec<DataItemEntry> {
    let flags = (1u64 << actor_flags::CANCLIMB)
        | (1u64 << actor_flags::HAS_COLLISION)
        | (1u64 << actor_flags::HAS_GRAVITY);
    vec![
        DataItemEntry {
            id: actor_data_ids::RESERVED_0,
            payload: DataItemEntryPayload::DataItemInt64Payload(DataItemInt64Payload {
                type_: EnumsDataItemType::Int64,
                value: flags as i64,
            }),
        },
        DataItemEntry {
            id: actor_data_ids::AIR_SUPPLY,
            payload: DataItemEntryPayload::DataItemShortPayload(DataItemShortPayload {
                type_: EnumsDataItemType::Short,
                value: 300,
            }),
        },
        DataItemEntry {
            id: actor_data_ids::AIR_SUPPLY_MAX,
            payload: DataItemEntryPayload::DataItemShortPayload(DataItemShortPayload {
                type_: EnumsDataItemType::Short,
                value: 300,
            }),
        },
        DataItemEntry {
            id: actor_data_ids::RESERVED_053,
            payload: DataItemEntryPayload::DataItemFloatPayload(DataItemFloatPayload {
                type_: EnumsDataItemType::Float,
                value: 0.6,
            }),
        },
        DataItemEntry {
            id: actor_data_ids::RESERVED_054,
            payload: DataItemEntryPayload::DataItemFloatPayload(DataItemFloatPayload {
                type_: EnumsDataItemType::Float,
                value: 1.8,
            }),
        },
        DataItemEntry {
            id: actor_data_ids::RESERVED_038,
            payload: DataItemEntryPayload::DataItemFloatPayload(DataItemFloatPayload {
                type_: EnumsDataItemType::Float,
                value: 1.0,
            }),
        },
    ]
}

#[cfg(test)]
mod metadata_control_tests {
    use super::*;
    use valentine::bedrock::codec::BedrockCodec;

    #[test]
    fn initial_player_flags_match_native_join_control() {
        let bytes = include_bytes!(
            "../../../../bds-extractor/captures/1.26.51/member/join/00062-SetActorDataPacket.bin"
        );
        let mut input = &bytes[1..];
        let native = SetActorDataPacket::decode(&mut input, ()).unwrap();
        assert!(input.is_empty());
        let actual = initial_player_actor_data();
        for id in [0, 7, 42] {
            let expected = &native
                .actor_data
                .data
                .iter()
                .find(|row| row.id == id)
                .unwrap()
                .payload;
            let observed = &actual.iter().find(|row| row.id == id).unwrap().payload;
            match (observed, expected) {
                (
                    DataItemEntryPayload::DataItemInt64Payload(actual),
                    DataItemEntryPayload::DataItemInt64Payload(expected),
                ) => {
                    assert_eq!(actual.value, expected.value, "initial player metadata {id}");
                }
                (
                    DataItemEntryPayload::DataItemShortPayload(actual),
                    DataItemEntryPayload::DataItemShortPayload(expected),
                ) => {
                    assert_eq!(actual.value, expected.value, "initial player metadata {id}");
                }
                _ => panic!("native player metadata {id} payload type changed"),
            }
        }
    }
}
