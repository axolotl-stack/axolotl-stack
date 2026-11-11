//! Player join packet handling.
//!
//! Contains the send_join_packets method for sending initial game state.

use super::GameServer;
use super::roster;
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
    PropertySyncData, SerializedAbilitiesData, SerializedAbilitiesDataSerializedLayer,
    SynchedActorDataCopyableDataList,
};
use jolyne::valentine::types::{BlockPos, DimensionType, EnumsSpawnPositionType};
use jolyne::valentine::{
    CameraSplinePacket, GameRulesChangedPacket, InventoryContentPacket, McpePacket,
    PlayerFogPacket, SetCommandsEnabledPacket, SetDifficultyPacket, SetSpawnPositionPacket,
};
use jolyne::valentine::{SetActorDataPacket, UpdateAbilitiesPacket, UpdateAttributesPacket};
use jolyne::valentine::{actor_data_i_ds as actor_data_ids, actor_flags};
use tracing::debug;

/// Every ability bit BDS 1.26.51 marks as set in a player's base layer.
const BASE_ABILITIES_SET: u32 = 0xF_FFFF;
/// Base-layer values every player has: build, mine, doors and switches,
/// open containers, attack players, attack mobs.
const MEMBER_ABILITIES: u32 = 0x3F;
/// Operators add the operator-commands and teleport bits.
const OPERATOR_ABILITIES: u32 = 0xC0;
/// Creative adds invulnerable, may-fly and instant-build.
const CREATIVE_ABILITIES: u32 = 0xD00;
/// The spectator layer BDS puts in front of the base layer: its set and
/// values, with every speed 0.
const SPECTATOR_LAYER: u16 = 2;
const SPECTATOR_ABILITIES_SET: u32 = 0x2_0F3F;
const SPECTATOR_ABILITIES: u32 = 0x2_0300;

/// A player's `UpdateAbilities` as BDS 1.26.51 sends it (2026-11-11
/// `protocol_probe.py abilities`, every mode with and without operator):
/// one base layer with every bit set, member values plus operator and
/// creative bits; adventure is survival's; spectator puts its own layer in
/// front. Operators have permission `Operator` and command level
/// `gamedirectors`. `target` is the player's unique id.
pub(crate) fn abilities_packet(
    target: i64,
    game_mode: GameMode,
    operator: bool,
) -> UpdateAbilitiesPacket {
    let mut values = MEMBER_ABILITIES;
    if operator {
        values |= OPERATOR_ABILITIES;
    }
    if game_mode == GameMode::Creative {
        values |= CREATIVE_ABILITIES;
    }
    let mut layers = Vec::with_capacity(2);
    if game_mode == GameMode::Spectator {
        layers.push(SerializedAbilitiesDataSerializedLayer {
            serialized_layer: SPECTATOR_LAYER,
            abilities_set: SPECTATOR_ABILITIES_SET,
            ability_values: SPECTATOR_ABILITIES,
            fly_speed: 0.0,
            vertical_fly_speed: 0.0,
            walk_speed: 0.0,
        });
    }
    layers.push(SerializedAbilitiesDataSerializedLayer {
        serialized_layer: 1,
        abilities_set: BASE_ABILITIES_SET,
        ability_values: values,
        fly_speed: 0.05,
        vertical_fly_speed: 1.0,
        walk_speed: 0.1,
    });
    UpdateAbilitiesPacket {
        data: SerializedAbilitiesData {
            target_player_raw_id: target,
            player_permissions: if operator {
                EnumsPlayerPermissionLevel::Operator
            } else {
                EnumsPlayerPermissionLevel::Member
            },
            command_permissions: if operator {
                EnumsCommandPermissionLevel::Gamedirectors
            } else {
                EnumsCommandPermissionLevel::Any
            },
            layers,
        },
    }
}

/// A player's current abilities, from its game mode and operator status;
/// players use one id as unique and runtime id.
pub(crate) fn player_abilities(world: &World, player: Entity) -> Option<UpdateAbilitiesPacket> {
    let id = world.get::<RuntimeEntityId>(player)?.0;
    let mode = world.get::<GameMode>(player).copied().unwrap_or_default();
    let operator = world.get::<crate::command::Operator>(player).is_some();
    Some(abilities_packet(id, mode, operator))
}

impl GameServer {
    /// Send the join packets that follow `StartGame` to a newly spawned
    /// player, in BDS 1.26.51 order (member join capture
    /// `crates/bds-extractor/captures/1.26.51/member/join/`). The packets
    /// jolyne sends with `StartGame` (item registry, biome definitions, actor
    /// identifiers, creative content) are left out of that order.
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

        let input_tick = crate::entity::components::input_tick_of(world, entity);

        // BDS sends these two before StartGame; the earliest unastar can is
        // right after it. The count includes the joining player.
        let online = roster::online(world);
        let _ = session.send(McpePacket::from(roster::sleeping_players(
            online.len() as i32,
            0,
        )));
        if let Some(own) = world.get::<roster::RosterEntry>(entity) {
            let _ = session.send(McpePacket::from(roster::add_packet([own])));
        }

        for packet in world
            .resource::<crate::entity::definition::properties::ActorPropertyRegistry>()
            .packets()
        {
            let _ = session.send(McpePacket::from(packet.clone()));
        }

        // No personal spawn point: every coordinate `i32::MIN`, dimension 3.
        let _ = session.send(McpePacket::from(join_spawn_position()));
        // The world clock registry, then the difficulty (BDS join order).
        let _ = session.send(McpePacket::from(clock::clock_registry_packet(
            *world.resource::<WorldTime>(),
        )));
        let _ = session.send(McpePacket::from(SetDifficultyPacket {
            difficulty: world.resource::<Difficulty>().wire(),
        }));

        let operator = world.get::<crate::command::Operator>(entity).is_some();
        let _ = session.send(McpePacket::from(SetCommandsEnabledPacket {
            commands_enabled: true,
        }));
        // Ongoing weather comes between SetCommandsEnabled and the adventure
        // settings (2026-11-11 `capture.py --pair` guest joins).
        for event in crate::world::weather::join_packets(world) {
            let _ = session.send(McpePacket::from(event));
        }
        let _ = session.send(McpePacket::from(roster::adventure_settings()));
        let _ = session.send(McpePacket::from(abilities_packet(
            runtime_id, game_mode, operator,
        )));
        let _ = session.send(McpePacket::from(GameRulesChangedPacket {
            rule_data: world.resource::<GameRules>().packet_data(),
        }));
        // Every online player, the joining one included, then each one's
        // abilities in the same order.
        let _ = session.send(McpePacket::from(roster::add_packet(
            online.iter().map(|(_, entry)| *entry),
        )));
        for (player, _) in &online {
            if let Some(abilities) = player_abilities(world, *player) {
                let _ = session.send(McpePacket::from(abilities));
            }
        }

        let _ = session.send(McpePacket::from(PlayerFogPacket::default()));
        let _ = session.send(McpePacket::from(super::join_data::camera_presets()));
        let _ = session.send(McpePacket::from(
            super::join_data::camera_aim_assist_presets(),
        ));
        let _ = session.send(McpePacket::from(CameraSplinePacket::default()));

        let health = world.get::<Health>(entity).cloned().unwrap_or_default();
        let hunger = world.get::<Hunger>(entity).cloned().unwrap_or_default();
        let experience = world.get::<Experience>(entity).cloned().unwrap_or_default();
        let absorption = world
            .get::<super::attributes::Absorption>(entity)
            .map_or(0.0, |a| a.0);
        let _ = session.send(McpePacket::from(UpdateAttributesPacket {
            target_runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            attribute_list: player_attributes(&health, &hunger, absorption, &experience),
            tick: input_tick.clone(),
        }));
        let _ = session.send(McpePacket::from(super::join_data::trim_data()));

        let actor_data = initial_player_actor_data();

        let _ = session.send(McpePacket::from(SetActorDataPacket {
            target_runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            actor_data: SynchedActorDataCopyableDataList { data: actor_data },
            synched_properties: PropertySyncData::default(),
            tick: input_tick,
        }));

        // Send inventory contents to enable inventory UI
        // Without these packets, the client won't allow opening the inventory
        send_inventory_contents(world, entity);
        let crafting = world
            .resource::<super::types::CraftingCatalogResource>()
            .0
            .wire_packet(super::types::block_network_ids(world));
        let _ = session.send(McpePacket::from(crafting));
        // BDS ends the join with its command list (identical for every
        // permission level; the client filters by UpdateAbilities).
        let commands = world
            .resource::<crate::command::CommandRegistry>()
            .schema()
            .packet()
            .clone();
        let _ = session.send(McpePacket::from(commands));
        debug!(?game_mode, "Sent join packets");
    }

    /// Tells every other online player that `entity` joined: the new
    /// sleeping-player count, its player-list entry, the adventure settings
    /// and its abilities, in BDS order and before its `AddPlayer` (which the
    /// spawn broadcast sends on a later tick).
    pub(super) fn announce_join(&self, entity: Entity) {
        let world = self.ecs.world();
        let Some(entry) = world.get::<roster::RosterEntry>(entity) else {
            return;
        };
        let online = roster::online(world);
        let count = roster::sleeping_players(online.len() as i32, 0);
        let abilities = player_abilities(world, entity);
        for (other, _) in &online {
            if *other == entity {
                continue;
            }
            if let Some(session) = world.get::<PlayerSession>(*other) {
                let _ = session.send(McpePacket::from(count.clone()));
                let _ = session.send(McpePacket::from(roster::add_packet([entry])));
                let _ = session.send(McpePacket::from(roster::adventure_settings()));
                if let Some(abilities) = &abilities {
                    let _ = session.send(McpePacket::from(abilities.clone()));
                }
            }
        }
    }

    /// Tells every other online player that `entity` is leaving: the new
    /// sleeping-player count and the removal of its player-list entry.
    pub(super) fn announce_leave(&self, entity: Entity) {
        let world = self.ecs.world();
        let Some(entry) = world.get::<roster::RosterEntry>(entity) else {
            return;
        };
        let online = roster::online(world);
        let remaining = online.iter().filter(|(other, _)| *other != entity).count();
        let count = roster::sleeping_players(remaining as i32, 0);
        let remove = roster::remove_packet(entry.uuid());
        for (other, _) in &online {
            if *other == entity {
                continue;
            }
            if let Some(session) = world.get::<PlayerSession>(*other) {
                let _ = session.send(McpePacket::from(count.clone()));
                let _ = session.send(McpePacket::from(remove.clone()));
            }
        }
    }
}

/// The join's `SetSpawnPosition` for a player without a spawn point of its
/// own (unastar does not save spawn points): BDS sends `Playerrespawn` with
/// every coordinate `i32::MIN` and dimension 3, which names no dimension.
pub(crate) fn join_spawn_position() -> SetSpawnPositionPacket {
    let unset = BlockPos {
        x: i32::MIN,
        y: i32::MIN,
        z: i32::MIN,
    };
    SetSpawnPositionPacket {
        spawn_position_type: EnumsSpawnPositionType::Playerrespawn,
        block_position: unset.clone(),
        dimensiontype: DimensionType { value: 3 },
        spawn_block_pos: unset,
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

#[cfg(test)]
mod bds_join_payload_tests {
    use super::*;
    use valentine::bedrock::codec::BedrockCodec;

    /// A member join packet's payload, without its varuint packet ID header.
    fn captured(file: &str) -> Vec<u8> {
        let path = format!(
            "{}/../bds-extractor/captures/1.26.51/member/join/{file}",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
        let header = bytes.iter().position(|byte| byte & 0x80 == 0).unwrap() + 1;
        bytes[header..].to_vec()
    }

    fn encode<T: BedrockCodec>(packet: &T) -> Vec<u8> {
        let mut out = Vec::new();
        packet.encode(&mut out).unwrap();
        out
    }

    /// The fixed packets of the join are byte-identical to BDS's.
    #[test]
    fn fixed_join_packets_match_the_bds_member_join_byte_for_byte() {
        assert_eq!(
            encode(&join_spawn_position()),
            captured("00031-SetSpawnPositionPacket.bin")
        );
        assert_eq!(
            encode(&SetCommandsEnabledPacket {
                commands_enabled: true
            }),
            captured("00034-SetCommandsEnabledPacket.bin")
        );
        assert_eq!(
            encode(&PlayerFogPacket::default()),
            captured("00042-PlayerFogPacket.bin")
        );
        assert_eq!(
            encode(&CameraSplinePacket::default()),
            captured("00045-CameraSplinePacket.bin")
        );
        assert_eq!(
            encode(&jolyne::valentine::SetHealthPacket { health: 20 }),
            captured("00202-SetHealthPacket.bin")
        );
    }
}
