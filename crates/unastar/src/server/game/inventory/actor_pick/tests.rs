//! Actor picks against the BDS 1.26.51 `pick` and `pick-actors` probe
//! fixtures (`crates/bds-extractor/captures/1.26.51/protocol/`).

use super::*;
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::server::game::client_packets::tests::{decode, encode, files, payload};
use crate::server::game::packet_queues::{InventoryAction, InventoryPacketQueue};
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use jolyne::valentine::{GuiDataPickItemPacket, McpePacket, McpePacketData, McpePacketName};
use tokio::sync::mpsc;

/// Every name the table holds is a `GuiDataPickItem` BDS sent in the
/// `pick-actors` run and its two rechecks (slot 0: the first pick on an empty
/// inventory), and every name BDS sent there is in the table.
#[test]
fn the_measured_names_match_the_bds_pick_capture_byte_for_byte() {
    let table = unastar_data::actor_picks::vanilla();
    // The sweep and the recheck of the actors it missed.
    let native: std::collections::BTreeSet<Vec<u8>> =
        ["pick-actors", "pick-recheck", "pick-recheck2"]
            .into_iter()
            .flat_map(|run| files(&format!("{run}/pick-actors"), "GuiDataPickItemPacket"))
            .map(|file| payload(&file))
            .collect();
    let ours: std::collections::BTreeSet<Vec<u8>> = table
        .items
        .values()
        .map(|row| {
            encode(&GuiDataPickItemPacket {
                item_name: row.name.as_str().into(),
                item_effect_name: "".into(),
                slot: 0,
            })
        })
        .collect();
    // With data, chest boats and minecarts land in slot 1 (a second stack);
    // the slot is the packet's last field, an `i32`.
    let slot_zero: std::collections::BTreeSet<Vec<u8>> = native
        .into_iter()
        .filter(|bytes| {
            let packet: GuiDataPickItemPacket =
                valentine::bedrock::codec::BedrockCodec::decode(&mut bytes.as_slice(), ()).unwrap();
            packet.slot == 0
        })
        .collect();
    assert_eq!(ours, slot_zero);
}

struct Pick {
    server: GameServer,
    player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}

impl Pick {
    fn new(mode: GameMode) -> Self {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let (outbound_tx, rx) = mpsc::channel(4096);
        let player = server
            .spawn_player(PlayerSpawnData {
                session_id: 1,
                display_name: "Picker".into(),
                xuid: None,
                uuid: Some(uuid::Uuid::from_u128(9).to_string()),
                runtime_id: 1,
                position: glam::DVec3::new(0.5, 17.0, 0.5),
                outbound_tx,
                saved_data: None,
                chunk_radius: 2,
            })
            .unwrap();
        server.ecs.world_mut().entity_mut(player).insert(mode);
        Self { server, player, rx }
    }

    /// Picks `actor_id` and returns the pick packets the player got.
    fn pick(&mut self, actor_id: i64, with_data: bool) -> Vec<McpePacketData> {
        while self.rx.try_recv().is_ok() {}
        let world = self.server.ecs.world_mut();
        world.resource_mut::<InventoryPacketQueue>().0.push((
            self.player,
            InventoryAction::ActorPick(ActorPickRequestPacket {
                actor_id,
                max_slots: 9,
                with_data,
            }),
        ));
        self.server.tick();
        std::iter::from_fn(|| self.rx.try_recv().ok())
            .map(|packet| packet.data)
            .filter(|packet| {
                matches!(
                    packet.packet_id(),
                    McpePacketName::GuiDataPickItemPacket
                        | McpePacketName::InventoryContentPacket
                        | McpePacketName::PlayerHotbarPacket
                )
            })
            .collect()
    }

    fn mob(&mut self, identifier: &str) -> i64 {
        let world = self.server.ecs.world_mut();
        let cow = spawn_mob(
            world,
            identifier,
            glam::DVec3::new(0.5, 15.38, 3.5),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        world.get::<RuntimeId>(cow).unwrap().0
    }
}

/// A creative pick of a cow: `GuiDataPickItem` with the egg's name, the
/// inventory, then the hotbar selection (`protocol/relay-first` guest
/// `02909`-`02911`); with data the same.
#[test]
fn a_creative_cow_pick_names_and_selects_its_spawn_egg_like_bds() {
    let mut pick = Pick::new(GameMode::Creative);
    let cow = pick.mob("minecraft:cow");
    for with_data in [false, true] {
        let sent = pick.pick(cow, with_data);
        let names: Vec<_> = sent.iter().map(McpePacketData::packet_id).collect();
        assert_eq!(
            names,
            [
                McpePacketName::GuiDataPickItemPacket,
                McpePacketName::InventoryContentPacket,
                McpePacketName::PlayerHotbarPacket
            ]
        );
        let McpePacketData::GuiDataPickItemPacket(gui) = &sent[0] else {
            unreachable!()
        };
        let native: GuiDataPickItemPacket =
            decode("relay-first/guest/02909-GuiDataPickItemPacket.bin");
        assert_eq!(encode(gui), encode(&native));
        let McpePacketData::InventoryContentPacket(inventory) = &sent[1] else {
            unreachable!()
        };
        let egg = pick
            .server
            .ecs
            .world()
            .resource::<crate::server::game::types::ItemRegistryResource>()
            .0
            .get_by_name("minecraft:cow_spawn_egg")
            .unwrap()
            .network_id();
        assert_eq!(i32::from(inventory.slots[0].id), egg);
    }
}

/// A cow 14 blocks away is picked, one 16 blocks away is not (`pick-reach`).
#[test]
fn actors_beyond_the_creative_reach_are_not_picked_like_bds() {
    let mut pick = Pick::new(GameMode::Creative);
    let feet = pick
        .server
        .ecs
        .world()
        .get::<crate::entity::components::Position>(pick.player)
        .unwrap()
        .0
        - glam::DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
    for (distance, answered) in [(14.0, true), (16.0, false)] {
        let world = pick.server.ecs.world_mut();
        let cow = spawn_mob(
            world,
            "minecraft:cow",
            feet + glam::DVec3::Z * distance,
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        let id = world.get::<RuntimeId>(cow).unwrap().0;
        assert_eq!(
            !pick.pick(id, false).is_empty(),
            answered,
            "{distance} blocks"
        );
    }
}

/// A survival pick without the egg and picks of players get no answer
/// (`protocol/relay-first` and the `pick` run).
#[test]
fn survival_picks_without_the_egg_and_player_picks_are_ignored_like_bds() {
    let mut survival = Pick::new(GameMode::Survival);
    let cow = survival.mob("minecraft:cow");
    assert!(survival.pick(cow, false).is_empty());
    let mut creative = Pick::new(GameMode::Creative);
    assert!(creative.pick(1, false).is_empty());
}

/// A survival pick of a cow whose egg the player has (`pick-data` run): an
/// egg in hotbar slot 4 is selected, with or without data; an egg in main
/// inventory slot 12 moves to the first empty hotbar slot and is selected.
/// `GuiDataPickItem` and `PlayerHotbar` match BDS byte for byte.
#[test]
fn a_survival_pick_selects_the_egg_the_player_has_like_bds() {
    const RUN: &str = "pick-data/pick-data";
    let mut pick = Pick::new(GameMode::Survival);
    let cow = pick.mob("minecraft:cow");
    let egg = {
        let items = &pick
            .server
            .ecs
            .world()
            .resource::<crate::server::game::types::ItemRegistryResource>()
            .0;
        ItemStack::new(items.item_type("minecraft:cow_spawn_egg").unwrap(), 1)
    };
    let put = |pick: &mut Pick, slot: usize| {
        let world = pick.server.ecs.world_mut();
        let mut inventory = world
            .get_mut::<crate::entity::components::MainInventory>(pick.player)
            .unwrap();
        for index in 0..inventory.0.size() {
            inventory.0.set_item(index, ItemStack::empty()).unwrap();
        }
        inventory.0.set_item(slot, egg.clone().into_slot()).unwrap();
    };
    let check = |sent: &[McpePacketData], gui: &str, hotbar: &str, slot: usize| {
        let [
            McpePacketData::GuiDataPickItemPacket(name),
            McpePacketData::InventoryContentPacket(inventory),
            McpePacketData::PlayerHotbarPacket(selected),
        ] = sent
        else {
            panic!("{gui}: {sent:?}");
        };
        assert_eq!(encode(name), payload(&format!("{RUN}/{gui}")), "{gui}");
        assert_eq!(
            encode(selected),
            payload(&format!("{RUN}/{hotbar}")),
            "{hotbar}"
        );
        let held: Vec<usize> = inventory
            .slots
            .iter()
            .enumerate()
            .filter(|(_, item)| item.id != 0)
            .map(|(index, _)| index)
            .collect();
        assert_eq!(held, [slot], "{gui}");
    };
    put(&mut pick, 4);
    let sent = pick.pick(cow, false);
    check(
        &sent,
        "03127-GuiDataPickItemPacket.bin",
        "03129-PlayerHotbarPacket.bin",
        4,
    );
    let sent = pick.pick(cow, true);
    check(
        &sent,
        "03169-GuiDataPickItemPacket.bin",
        "03171-PlayerHotbarPacket.bin",
        4,
    );
    put(&mut pick, 12);
    let sent = pick.pick(cow, false);
    check(
        &sent,
        "03352-GuiDataPickItemPacket.bin",
        "03354-PlayerHotbarPacket.bin",
        0,
    );
}
