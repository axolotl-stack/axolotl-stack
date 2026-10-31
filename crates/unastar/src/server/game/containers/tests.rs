//! Container block behaviour against a real `GameServer` world: windows,
//! moves through the I4 engine, pairing, breaking, persistence in the chunk
//! actor, furnace ticks and hoppers.

use super::*;
use crate::entity::components::{GameMode, MainInventory, PlayerUi};
use crate::item::test_support::stack;
use crate::registry::BlockRegistry;
use crate::server::game::block_interaction::{
    BlockInteraction, BlockInteractionHandler, BlockInteractionHooks, BlockInteractionKind,
};
use crate::server::game::packet_queues::{InventoryAction, InventoryPacketQueue};
use crate::world::chunk::Chunk;
use crate::world::ecs::ChunkPosition;
use bevy_ecs::schedule::Schedule;
use jolyne::valentine::{
    EnumsContainerEnumName as Name, EnumsItemStackNetResult as Status, FullContainerName,
    ItemStackRequestCerealSlotInfoData, ItemStackRequestCerealTakeActionData,
    ItemStackRequestPacket, ItemStackRequestPacketDataRequestData,
    ItemStackRequestPacketDataRequestDataActionsItem as Action, McpePacketData,
    TypedClientNetIdstructItemStackRequestIdTagint32T0,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::mpsc::Receiver;

/// A policy that can revoke one block after a window has already been opened.
struct ContainerPermission {
    denied: Arc<AtomicBool>,
    position: IVec3,
}

impl BlockInteractionHandler for ContainerPermission {
    fn on_interaction(&self, _world: &mut World, event: &mut BlockInteraction) {
        if self.denied.load(Ordering::SeqCst)
            && event.kind() == BlockInteractionKind::Use
            && event.positions().contains(&self.position)
        {
            event.cancel();
        }
    }
}

/// Despawn during authorization to exercise cleanup without borrowed player data.
struct DespawnContainerUser;

impl BlockInteractionHandler for DespawnContainerUser {
    fn on_interaction(&self, world: &mut World, event: &mut BlockInteraction) {
        world.despawn(event.player());
    }
}

struct Harness {
    server: crate::GameServer,
    player: Entity,
    rx: Receiver<McpePacket>,
    chunk: Entity,
    next_request: i32,
}

/// Run `test` on a server thread with one survival player at (0.5, 66, 0.5)
/// viewing a test chunk at (0, 0).
fn with_world(mode: GameMode, test: impl FnOnce(&mut Harness) + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let _guard = runtime.enter();
            let mut config = crate::world::WorldConfig::void_spawn_platform(1);
            config.generation_threads = 1;
            let mut server = crate::GameServer::with_config(crate::ServerConfig {
                default_gamemode: mode,
                world: config,
                ..Default::default()
            });
            let (tx, mut rx) = tokio::sync::mpsc::channel(8192);
            let player = server
                .spawn_player(crate::PlayerSpawnData {
                    session_id: 7,
                    display_name: "containers".into(),
                    xuid: None,
                    uuid: Some(uuid::Uuid::from_u128(7).to_string()),
                    runtime_id: 77,
                    position: glam::DVec3::new(0.5, 66.62, 0.5),
                    saved_data: None,
                    outbound_tx: tx,
                    chunk_radius: 0,
                })
                .unwrap();
            let world = server.ecs.world_mut();
            let mut viewers = ChunkViewers::default();
            viewers.insert(player);
            let mut chunk = Chunk::new(0, 0);
            for x in 0..16 {
                for z in 0..16 {
                    chunk.set_block(x, 63, z, *crate::world::chunk::blocks::STONE);
                }
            }
            let chunk = world
                .spawn((ChunkData::new(chunk), ChunkStateFlags::default(), viewers))
                .id();
            world
                .resource_mut::<ChunkManager>()
                .insert(ChunkPosition::new(0, 0), chunk);
            while rx.try_recv().is_ok() {}
            test(&mut Harness {
                server,
                player,
                rx,
                chunk,
                next_request: -1,
            });
        })
        .unwrap()
        .join()
        .unwrap();
}

impl Harness {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    fn state(&self, name: &str, props: &[(&str, StateValue)]) -> u32 {
        let blocks = BlockRegistry::vanilla();
        let entry = blocks.get_by_name(name).unwrap();
        props
            .iter()
            .fold(entry.default_state_id, |state, (prop, value)| {
                entry.with_state_value(state, prop, *value).unwrap()
            })
    }

    /// Place a block the way a committed placement does.
    fn place(&mut self, pos: IVec3, name: &str, props: &[(&str, StateValue)]) {
        let state = self.state(name, props);
        let world = self.world();
        super::super::blocks::set_block(world, pos, state, false);
        placed(world, pos);
    }

    fn packets(&mut self) -> Vec<McpePacketData> {
        let mut out = Vec::new();
        while let Ok(packet) = self.rx.try_recv() {
            out.push(packet.data);
        }
        out
    }

    fn give(&mut self, slot: usize, name: &str, count: u8) -> i32 {
        let player = self.player;
        let mut main = self.world().get_mut::<MainInventory>(player).unwrap();
        main.0
            .set_item(slot, stack(name, count).into_slot())
            .unwrap();
        main.0.item(slot).unwrap().net_id
    }

    fn main_slot(&mut self, slot: usize) -> ItemStack {
        let player = self.player;
        self.world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(slot)
            .unwrap()
            .clone()
    }

    fn open(&mut self, pos: IVec3) -> u8 {
        let player = self.player;
        assert!(open(self.world(), player, pos));
        self.world()
            .get::<ContainerWindows>(player)
            .unwrap()
            .open_id()
            .expect("window opened")
    }

    fn run_inventory(&mut self, action: InventoryAction) {
        let player = self.player;
        let world = self.world();
        world
            .resource_mut::<InventoryPacketQueue>()
            .0
            .push((player, action));
        let mut schedule = Schedule::default();
        schedule.add_systems(super::super::inventory::apply_inventory);
        schedule.run(world);
    }

    fn close(&mut self) {
        self.run_inventory(InventoryAction::ContainerClose(
            jolyne::valentine::ContainerClosePacket {
                container_id: 0,
                container_type: 0,
                server_initiated_close: false,
            },
        ));
    }

    /// Build a real transfer packet without consuming the resulting corrections.
    fn transfer(
        &mut self,
        amount: u8,
        from: (Name, u8, i32),
        to: (Name, u8, i32),
    ) -> InventoryAction {
        let id = self.next_request;
        self.next_request -= 2;
        let slot = |(name, slot, net): (Name, u8, i32)| ItemStackRequestCerealSlotInfoData {
            fullcontainername: FullContainerName {
                container_name: name,
                dynamic_id: None,
            },
            slot,
            net_id_variant: net,
        };
        InventoryAction::ItemStackRequest(ItemStackRequestPacket {
            requests: vec![ItemStackRequestPacketDataRequestData {
                client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id },
                actions: vec![Action::TakeActionData(
                    ItemStackRequestCerealTakeActionData {
                        amount,
                        source: slot(from),
                        destination: slot(to),
                        ..Default::default()
                    },
                )],
                ..Default::default()
            }],
        })
    }

    /// Take `amount` from `from` to `to`; returns the response status.
    fn take(&mut self, amount: u8, from: (Name, u8, i32), to: (Name, u8, i32)) -> Status {
        let action = self.transfer(amount, from, to);
        self.run_inventory(action);
        self.packets()
            .into_iter()
            .find_map(|packet| match packet {
                McpePacketData::ItemStackResponsePacket(response) => {
                    Some(response.responses[0].result)
                }
                _ => None,
            })
            .expect("one response")
    }

    /// The container decoded from the chunk's saved actor (not the cache).
    fn saved(&mut self, pos: IVec3) -> BlockContainer {
        let chunk = self.chunk;
        let world = self.world();
        let actor = world
            .get::<ChunkData>(chunk)
            .unwrap()
            .inner
            .block_entities()
            .iter()
            .find(|actor| actor.position() == pos.to_array())
            .expect("saved actor")
            .clone();
        let kind = kind_at(world, pos).unwrap().0;
        BlockContainer::decode(
            kind,
            pos,
            actor.bedrock_compound().unwrap().unwrap(),
            &world.resource::<ItemRegistryResource>().0,
            &world.resource::<BlockRegistryResource>().0,
        )
    }

    fn tick(&mut self, ticks: usize) {
        for _ in 0..ticks {
            tick_block_containers(self.world());
        }
    }

    fn block_name(&mut self, pos: IVec3) -> String {
        let runtime_id =
            super::super::blocks::block_at(self.world(), (pos.x, pos.y, pos.z)).unwrap();
        BlockRegistry::vanilla()
            .get_by_runtime_id(runtime_id)
            .unwrap()
            .string_id
            .clone()
    }
}

fn north() -> (&'static str, StateValue) {
    ("minecraft:cardinal_direction", StateValue::Str("north"))
}

/// Reserve a batch before exercising actual native writers.
fn reserve(h: &mut Harness, cells: impl IntoIterator<Item = IVec3>) {
    h.world()
        .init_resource::<super::super::ContainerMutationLocks>();
    assert!(
        h.world()
            .resource_mut::<super::super::ContainerMutationLocks>()
            .acquire("operation", cells)
    );
}

#[test]
fn reserved_pair_blocks_both_opening_and_native_structural_mutations_before_cache_load() {
    with_world(GameMode::Survival, |h| {
        let first = IVec3::new(2, 64, 2);
        let second = first + IVec3::X;
        h.place(first, "minecraft:chest", &[north()]);
        h.place(second, "minecraft:chest", &[north()]);
        h.place(IVec3::new(5, 64, 2), "minecraft:stone", &[]);
        let before = h.saved(first).encode();
        h.world().resource_mut::<BlockContainers>().live.clear();
        reserve(h, [second]);
        let player = h.player;
        assert!(open(h.world(), player, first));
        assert!(
            h.world()
                .get::<ContainerWindows>(player)
                .unwrap()
                .open_id()
                .is_none()
        );
        assert!(mutation_locked(h.world(), first));
        super::super::blocks::destroy_block(h.world(), first);
        assert_eq!(h.block_name(first), "minecraft:chest");
        assert_eq!(h.saved(first).encode(), before);
        let output = crate::command::execute(
            h.world(),
            crate::command::CommandSource::server(),
            "/fill 2 64 2 3 64 2 stone",
        );
        assert!(!output.is_success());
        assert_eq!(h.block_name(second), "minecraft:chest");
        for (line, key) in [
            ("/setblock 2 64 2 air", "commands.setblock.noChange"),
            (
                "/clone 5 64 2 5 64 2 2 64 2 replace move",
                "commands.clone.failed",
            ),
        ] {
            let output =
                crate::command::execute(h.world(), crate::command::CommandSource::server(), line);
            assert!(!output.is_success());
            assert_eq!(output.messages[0].key, key);
        }
        assert_eq!(h.block_name(IVec3::new(5, 64, 2)), "minecraft:stone");
    });
}

#[test]
fn retained_window_cannot_move_items_and_completion_discards_stale_decoded_contents() {
    with_world(GameMode::Survival, |h| {
        let pos = IVec3::new(2, 64, 2);
        h.place(pos, "minecraft:chest", &[north()]);
        h.open(pos);
        let net = h.give(0, "minecraft:cobblestone", 20);
        h.packets();
        reserve(h, [pos]);
        assert_ne!(
            h.take(
                12,
                (Name::Hotbarcontainer, 0, net),
                (Name::Levelentitycontainer, 3, 0)
            ),
            Status::Success
        );
        assert_eq!(h.main_slot(0).count, 20);
        assert!(h.saved(pos).items.item(3).unwrap().is_empty());
        let mut replacement = BlockContainer::new(ContainerKind::Chest, pos);
        replacement
            .items
            .set_item(0, stack("minecraft:diamond", 5).into_slot())
            .unwrap();
        let chunk = h.chunk;
        h.world()
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block_entity(BlockEntity::from_bedrock_compound(replacement.encode()).unwrap());
        let player = h.player;
        let before_wrong_owner = h.world().get::<ContainerWindows>(player).unwrap().open_id();
        assert!(!super::super::complete_container_mutations(
            h.world(),
            "different",
            [pos]
        ));
        assert_eq!(
            h.world().get::<ContainerWindows>(player).unwrap().open_id(),
            before_wrong_owner
        );
        assert!(super::super::complete_container_mutations(
            h.world(),
            "operation",
            [pos]
        ));
        let player = h.player;
        assert!(
            h.world()
                .get::<ContainerWindows>(player)
                .unwrap()
                .open_id()
                .is_none()
        );
        assert_eq!(
            container(h.world(), pos)
                .unwrap()
                .items
                .item(0)
                .unwrap()
                .item_id(),
            "minecraft:diamond"
        );
        let completion_packets = h.packets();
        if before_wrong_owner.is_some() {
            assert!(completion_packets.iter().any(|packet| matches!(packet,McpePacketData::ContainerClosePacket(close) if close.server_initiated_close)));
        }
        h.open(pos);
        let net = container(h.world(), pos)
            .unwrap()
            .items
            .item(0)
            .unwrap()
            .net_id;
        assert_eq!(
            h.take(
                5,
                (Name::Levelentitycontainer, 0, net),
                (Name::Hotbarcontainer, 1, 0)
            ),
            Status::Success
        );
        assert_eq!(h.main_slot(1).count, 5);
        assert!(h.saved(pos).items.item(0).unwrap().is_empty());
    });
}

#[test]
fn reserved_furnace_keeps_fuel_progress_inventory_and_actor_unchanged_until_completion() {
    with_world(GameMode::Survival, |h| {
        let pos = IVec3::new(6, 64, 6);
        h.place(pos, "minecraft:furnace", &[north()]);
        {
            let mut furnace = container_mut(h.world(), pos).unwrap();
            furnace
                .items
                .set_item(0, stack("minecraft:raw_iron", 2).into_slot())
                .unwrap();
            furnace
                .items
                .set_item(1, stack("minecraft:coal", 1).into_slot())
                .unwrap();
        }
        save(h.world(), pos);
        h.tick(1);
        let before = h.saved(pos);
        reserve(h, [pos]);
        h.tick(50);
        assert_eq!(h.saved(pos).encode(), before.encode());
        assert!(super::super::complete_container_mutations(
            h.world(),
            "operation",
            [pos]
        ));
        h.tick(1);
        let after = h.saved(pos);
        assert_eq!(after.furnace.cook_time, before.furnace.cook_time + 1);
        assert_eq!(after.furnace.lit_time, before.furnace.lit_time - 1);
    });
}

#[test]
fn hopper_respects_reserved_source_and_destination_cells() {
    with_world(GameMode::Survival, |h| {
        let source = IVec3::new(8, 66, 8);
        let hopper = source - IVec3::Y;
        let target = hopper - IVec3::Y;
        h.place(source, "minecraft:chest", &[north()]);
        h.place(
            hopper,
            "minecraft:hopper",
            &[("facing_direction", StateValue::Int(0))],
        );
        h.place(target, "minecraft:furnace", &[north()]);
        container_mut(h.world(), source)
            .unwrap()
            .items
            .set_item(4, stack("minecraft:raw_iron", 3).into_slot())
            .unwrap();
        save(h.world(), source);
        reserve(h, [source]);
        h.tick(10);
        assert_eq!(h.saved(source).items.item(4).unwrap().count, 3);
        assert!(h.saved(hopper).items.item(0).unwrap().is_empty());
        assert!(super::super::complete_container_mutations(
            h.world(),
            "operation",
            [source]
        ));
        h.tick(1);
        assert_eq!(h.saved(hopper).items.item(0).unwrap().count, 1);
        reserve(h, [target]);
        h.tick(8);
        assert!(h.saved(target).items.item(0).unwrap().is_empty());
        assert!(super::super::complete_container_mutations(
            h.world(),
            "operation",
            [target]
        ));
        h.tick(8);
        assert_eq!(
            h.saved(target).items.item(0).unwrap().item_id(),
            "minecraft:raw_iron"
        );
    });
}

#[test]
fn reserved_generated_loot_is_not_consumed_until_the_container_is_released_and_opened() {
    with_world(GameMode::Survival, |h| {
        let pos = IVec3::new(12, 64, 12);
        let state = h.state("minecraft:chest", &[north()]);
        let chunk = h.chunk;
        super::super::blocks::set_block(h.world(), pos, state, false);
        h.world()
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block_entity(BlockEntity::generated(
                pos.to_array(),
                "Chest",
                &[],
                Some(crate::world::block_entity::LootTable::Bedrock(
                    "loot_tables/chests/simple_dungeon.json",
                    4242,
                )),
                None,
            ));
        reserve(h, [pos]);
        let player = h.player;
        assert!(open(h.world(), player, pos));
        assert!(h.saved(pos).encode().0.contains_key("LootTable"));
        assert!(super::super::complete_container_mutations(
            h.world(),
            "operation",
            [pos]
        ));
        assert!(
            h.saved(pos).encode().0.contains_key("LootTable"),
            "completion loads but does not open/unpack a container"
        );
        h.open(pos);
        assert!(!h.saved(pos).encode().0.contains_key("LootTable"));
    });
}

#[test]
fn completion_registers_new_authoritative_furnace_for_ordinary_ticking() {
    with_world(GameMode::Survival, |h| {
        let pos = IVec3::new(11, 64, 11);
        reserve(h, [pos]);
        let state = h.state("minecraft:furnace", &[north()]);
        let mut furnace = BlockContainer::new(ContainerKind::Furnace, pos);
        furnace
            .items
            .set_item(0, stack("minecraft:raw_gold", 1).into_slot())
            .unwrap();
        furnace
            .items
            .set_item(1, stack("minecraft:coal", 1).into_slot())
            .unwrap();
        let chunk = h.chunk;
        {
            let mut data = h.world().get_mut::<ChunkData>(chunk).unwrap();
            data.inner
                .set_block(pos.x as u8, pos.y as i16, pos.z as u8, state);
            data.inner
                .set_block_entity(BlockEntity::from_bedrock_compound(furnace.encode()).unwrap());
        }
        h.tick(5);
        assert_eq!(h.saved(pos).furnace.cook_time, 0);
        assert!(super::super::complete_container_mutations(
            h.world(),
            "operation",
            [pos]
        ));
        h.tick(1);
        assert_eq!(h.saved(pos).furnace.cook_time, 1);
        assert!(h.saved(pos).furnace.lit_time > 0);
    });
}

#[test]
fn reserved_window_still_rejects_fatal_request_ids_without_mutation() {
    with_world(GameMode::Survival, |h| {
        let pos = IVec3::new(2, 64, 2);
        h.place(pos, "minecraft:chest", &[north()]);
        h.open(pos);
        reserve(h, [pos]);
        h.packets();
        h.run_inventory(InventoryAction::ItemStackRequest(ItemStackRequestPacket {
            requests: vec![ItemStackRequestPacketDataRequestData {
                client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id: 2 },
                ..Default::default()
            }],
        }));
        assert!(
            h.packets()
                .iter()
                .any(|packet| matches!(packet, McpePacketData::PacketViolationWarningPacket(_)))
        );
        assert!(h.saved(pos).items.slots().iter().all(ItemStack::is_empty));
    });
}

#[test]
fn native_retained_container_permission_is_checked_before_stack_request_mutation() {
    with_world(GameMode::Survival, |h| {
        let chest = IVec3::new(2, 64, 2);
        h.place(chest, "minecraft:chest", &[north()]);
        let denied = Arc::new(AtomicBool::new(false));
        h.world()
            .resource_mut::<BlockInteractionHooks>()
            .register(ContainerPermission {
                denied: denied.clone(),
                position: chest,
            });
        let net = h.give(0, "minecraft:stone", 10);
        let window = h.open(chest);
        h.packets();
        assert_eq!(
            h.take(
                4,
                (Name::Hotbarcontainer, 0, net),
                (Name::Levelentitycontainer, 0, 0)
            ),
            Status::Success
        );
        let source = h.saved(chest).items.item(0).unwrap().clone();
        let before = h.main_slot(0);
        let player = h.player;
        h.world()
            .get_mut::<PlayerUi>(player)
            .unwrap()
            .set_cursor(stack("minecraft:stick", 2));
        denied.store(true, Ordering::SeqCst);
        let action = h.transfer(
            2,
            (Name::Levelentitycontainer, 0, source.net_id),
            (Name::Hotbarcontainer, 1, 0),
        );
        h.run_inventory(action);
        assert_eq!(h.saved(chest).items.item(0).unwrap(), &source);
        assert_eq!(h.main_slot(0), before);
        assert!(h.main_slot(1).is_empty());
        assert_eq!(
            h.world().get::<PlayerUi>(player).unwrap().cursor(),
            &stack("minecraft:stick", 2)
        );
        assert_eq!(
            h.world().get::<ContainerWindows>(player).unwrap().screen(),
            ScreenKind::None
        );
        let packets = h.packets();
        assert!(packets.iter().any(|packet| matches!(packet,McpePacketData::ItemStackResponsePacket(response) if response.responses.len()==1 && response.responses[0].result == Status::Error)));
        assert!(packets.iter().any(|packet| matches!(packet,McpePacketData::ContainerClosePacket(close) if close.container_id==window && close.server_initiated_close)));
        assert!(packets.iter().any(|packet| matches!(packet,McpePacketData::InventoryContentPacket(content) if content.container_id == 0)));
    });
}

#[test]
fn native_retained_container_permission_covers_the_other_double_chest_half() {
    with_world(GameMode::Survival, |h| {
        let lead = IVec3::new(2, 64, 2);
        let other = IVec3::new(3, 64, 2);
        h.place(lead, "minecraft:chest", &[north()]);
        h.place(other, "minecraft:chest", &[north()]);
        let net = h.give(0, "minecraft:stone", 5);
        h.open(other);
        h.packets();
        h.world()
            .resource_mut::<BlockInteractionHooks>()
            .register(ContainerPermission {
                denied: Arc::new(AtomicBool::new(true)),
                position: lead,
            });
        assert_eq!(
            h.take(
                5,
                (Name::Hotbarcontainer, 0, net),
                (Name::Levelentitycontainer, 30, 0)
            ),
            Status::Error
        );
        assert_eq!(h.main_slot(0).count, 5);
        assert!(h.saved(lead).items.item(3).unwrap().is_empty());
        assert!(h.saved(other).items.item(3).unwrap().is_empty());
    });
}

#[test]
fn native_retained_container_callback_despawn_preserves_stored_items() {
    with_world(GameMode::Survival, |h| {
        let chest = IVec3::new(2, 64, 2);
        h.place(chest, "minecraft:chest", &[north()]);
        let net = h.give(0, "minecraft:stone", 5);
        h.open(chest);
        h.packets();
        h.world()
            .resource_mut::<BlockInteractionHooks>()
            .register(DespawnContainerUser);
        let action = h.transfer(
            5,
            (Name::Hotbarcontainer, 0, net),
            (Name::Levelentitycontainer, 0, 0),
        );
        h.run_inventory(action);
        let player = h.player;
        assert!(h.world().get_entity(player).is_err());
        assert!(h.saved(chest).items.item(0).unwrap().is_empty());
    });
}

#[test]
fn chest_window_opens_moves_through_the_engine_saves_and_animates_the_lid() {
    with_world(GameMode::Survival, |h| {
        let chest = IVec3::new(2, 64, 2);
        h.place(chest, "minecraft:chest", &[north()]);
        assert_eq!(h.saved(chest).items.size(), 27, "placing creates the actor");
        let net = h.give(0, "minecraft:cobblestone", 20);
        h.packets();

        let window = h.open(chest);
        let packets = h.packets();
        let McpePacketData::ContainerOpenPacket(open) = &packets[0] else {
            panic!("ContainerOpen first, got {:?}", packets[0]);
        };
        assert_eq!(
            (
                open.container_id,
                open.container_type,
                open.target_actor_id.actor_unique_id
            ),
            (window, 0, -1)
        );
        assert_eq!(
            (open.position.x, open.position.y, open.position.z),
            (2, 64, 2)
        );
        let McpePacketData::InventoryContentPacket(content) = &packets[1] else {
            panic!("InventoryContent second");
        };
        assert_eq!(
            (content.container_id, content.slots.len()),
            (u32::from(window), 27)
        );
        assert!(
            packets.iter().any(|packet| matches!(packet,
            McpePacketData::BlockEventPacket(event)
                if event.event_type == 1 && event.event_value == 1 && event.block_position.x == 2))
        );
        assert!(packets.iter().any(|packet| matches!(packet,
            McpePacketData::LevelSoundEventPacket(sound) if &*sound.sound_event == "chest.open")));
        // The open sound names the chest by its wire id, as BDS 1.26.51 does.
        assert!(
            crate::server::wire_audit::assert_block_ids_on_wire(
                crate::registry::block::BlockNetworkIds::Hashed,
                &packets
            ) > 0
        );

        // Chest screens reject player containers outside BDS's allow-list.
        assert_eq!(
            h.take(
                1,
                (Name::Hotbarcontainer, 0, net),
                (Name::Armorcontainer, 0, 0)
            ),
            Status::Failedtovalidatedstslot
        );
        assert_eq!(
            h.take(
                12,
                (Name::Hotbarcontainer, 0, net),
                (Name::Levelentitycontainer, 3, 0)
            ),
            Status::Success
        );
        let saved = h.saved(chest);
        assert_eq!(
            (
                saved.items.item(3).unwrap().item_id(),
                saved.items.item(3).unwrap().count
            ),
            ("minecraft:cobblestone", 12)
        );
        assert_eq!(h.main_slot(0).count, 8);

        h.close();
        let packets = h.packets();
        assert!(packets.iter().any(|packet| matches!(packet,
            McpePacketData::BlockEventPacket(event) if event.event_value == 0)));
        assert!(
            packets.iter().any(|packet| matches!(packet,
            McpePacketData::LevelSoundEventPacket(sound) if &*sound.sound_event == "chest.closed"))
        );
        let player = h.player;
        assert_eq!(
            h.world().get::<ContainerWindows>(player).unwrap().screen(),
            ScreenKind::None
        );
    });
}

#[test]
fn double_chest_pairs_with_the_existing_chest_as_lead_and_unpairs_on_break() {
    with_world(GameMode::Creative, |h| {
        let lead = IVec3::new(2, 64, 2);
        let newer = IVec3::new(3, 64, 2);
        h.place(lead, "minecraft:chest", &[north()]);
        // Different chest type and different facing never pair.
        h.place(IVec3::new(1, 64, 2), "minecraft:trapped_chest", &[north()]);
        h.place(
            IVec3::new(2, 64, 3),
            "minecraft:chest",
            &[("minecraft:cardinal_direction", StateValue::Str("north"))],
        );
        assert!(h.saved(lead).pair.is_none());
        h.place(newer, "minecraft:chest", &[north()]);
        assert_eq!(
            h.saved(lead).pair,
            Some(Pair {
                x: 3,
                z: 2,
                lead: true
            })
        );
        assert_eq!(
            h.saved(newer).pair,
            Some(Pair {
                x: 2,
                z: 2,
                lead: false
            })
        );
        assert!(h.packets().iter().any(|packet| matches!(packet,
            McpePacketData::BlockActorDataPacket(data) if data.block_position.x == 2)));

        let net = h.give(0, "minecraft:stone", 5);
        // Opening the non-lead half still shows 54 slots, lead first.
        let window = h.open(newer);
        let content = h.packets().into_iter().find_map(|packet| match packet {
            McpePacketData::InventoryContentPacket(content) => Some(content),
            _ => None,
        });
        assert_eq!(content.unwrap().slots.len(), 54);
        assert_eq!(
            h.take(
                5,
                (Name::Hotbarcontainer, 0, net),
                (Name::Levelentitycontainer, 30, 0)
            ),
            Status::Success
        );
        assert_eq!(h.saved(newer).items.item(3).unwrap().count, 5);
        assert!(h.saved(lead).items.item(3).unwrap().is_empty());
        let _ = window;

        // Breaking the half with the items: viewers close, partner unpairs,
        // contents drop.
        let player = h.player;
        let world = h.world();
        let broken = breaking(world, newer).unwrap();
        assert_eq!(broken.drops.len(), 1);
        assert!(broken.randomize);
        assert_eq!(
            world.get::<ContainerWindows>(player).unwrap().screen(),
            ScreenKind::None
        );
        let packets = h.packets();
        assert!(packets.iter().any(|packet| matches!(packet,
            McpePacketData::ContainerClosePacket(close) if close.server_initiated_close && close.container_type == 0)));
        assert!(h.saved(lead).pair.is_none());
        let world = h.world();
        drop_contents(world, newer, broken);
        let dropped: u32 = world
            .query::<&ItemEntityStack>()
            .iter(world)
            .map(|stack| u32::from(stack.0.count))
            .sum();
        assert_eq!(dropped, 5);
    });
}

#[test]
fn barrel_flips_open_bit_and_uses_its_own_container_name() {
    with_world(GameMode::Survival, |h| {
        let barrel = IVec3::new(4, 64, 4);
        h.place(barrel, "minecraft:barrel", &[]);
        let net = h.give(0, "minecraft:dirt", 3);
        h.open(barrel);
        let open_bit = state_value(h.world(), barrel, "open_bit");
        assert_eq!(open_bit, Some(StateValue::Byte(1)));
        assert_eq!(
            h.take(
                3,
                (Name::Hotbarcontainer, 0, net),
                (Name::Levelentitycontainer, 0, 0)
            ),
            Status::Failedtovalidatedstslot,
            "a barrel window has no level-entity container"
        );
        assert_eq!(
            h.take(
                3,
                (Name::Hotbarcontainer, 0, net),
                (Name::Barrelcontainer, 26, 0)
            ),
            Status::Success
        );
        h.close();
        assert_eq!(
            state_value(h.world(), barrel, "open_bit"),
            Some(StateValue::Byte(0))
        );
        assert_eq!(
            h.saved(barrel).items.item(26).unwrap().count,
            3,
            "actor survives the state flip"
        );
    });
}

#[test]
fn ender_chest_shows_and_keeps_the_players_own_inventory() {
    with_world(GameMode::Survival, |h| {
        let ender = IVec3::new(5, 64, 5);
        h.place(ender, "minecraft:ender_chest", &[]);
        let net = h.give(0, "minecraft:diamond", 2);
        h.open(ender);
        assert_eq!(
            h.take(
                2,
                (Name::Hotbarcontainer, 0, net),
                (Name::Levelentitycontainer, 26, 0)
            ),
            Status::Success
        );
        h.close();
        let player = h.player;
        let world = h.world();
        let ender_items = &world.get::<EnderChestInventory>(player).unwrap().0;
        assert_eq!(ender_items.item(26).unwrap().count, 2);
        assert!(
            !h.saved(ender).encode().0.contains_key("Items"),
            "the block keeps no items"
        );
        let player = h.player;
        let record = super::super::player_record(h.world(), player).unwrap().1;
        assert_eq!(record.ender_chest[26].item_id, "minecraft:diamond");
    });
}

#[test]
fn furnace_smelts_with_bds_timing_swaps_lit_state_and_pays_xp_on_take() {
    with_world(GameMode::Survival, |h| {
        let furnace = IVec3::new(6, 64, 6);
        h.place(furnace, "minecraft:furnace", &[north()]);
        let ore = h.give(0, "minecraft:raw_iron", 2);
        let coal = h.give(1, "minecraft:coal", 1);
        let window = h.open(furnace);
        h.packets();
        assert_eq!(
            h.take(
                1,
                (Name::Hotbarcontainer, 1, coal),
                (Name::Furnaceingredientcontainer, 1, 0)
            ),
            Status::Failedtovalidatedstslot,
            "slot 0 is the ingredient container's only slot"
        );
        assert_eq!(
            h.take(
                2,
                (Name::Hotbarcontainer, 0, ore),
                (Name::Furnacefuelcontainer, 1, 0)
            ),
            Status::Cannotplaceitem,
            "raw iron is not fuel"
        );
        assert_eq!(
            h.take(
                2,
                (Name::Hotbarcontainer, 0, ore),
                (Name::Furnaceingredientcontainer, 0, 0)
            ),
            Status::Success
        );
        assert_eq!(
            h.take(
                1,
                (Name::Hotbarcontainer, 1, coal),
                (Name::Furnacefuelcontainer, 1, 0)
            ),
            Status::Success
        );

        h.tick(1);
        assert_eq!(
            h.block_name(furnace),
            "minecraft:lit_furnace",
            "lit the tick fuel is used"
        );
        let saved = h.saved(furnace);
        assert_eq!(
            (
                saved.furnace.lit_time,
                saved.furnace.lit_duration,
                saved.furnace.cook_time
            ),
            (1600, 1600, 1)
        );
        assert!(
            saved.items.item(1).unwrap().is_empty(),
            "the coal was consumed"
        );
        let data: Vec<(i32, i32)> = h
            .packets()
            .into_iter()
            .filter_map(|packet| match packet {
                McpePacketData::ContainerSetDataPacket(data) if data.container_id == window => {
                    Some((data.id, data.value))
                }
                _ => None,
            })
            .collect();
        assert_eq!(data, [(0, 1), (1, 1600), (2, 1600)]);

        h.tick(199);
        let saved = h.saved(furnace);
        let output = saved.items.item(2).unwrap();
        assert_eq!(
            (output.item_id(), output.count),
            ("minecraft:iron_ingot", 1)
        );
        assert_eq!(saved.items.item(0).unwrap().count, 1);
        assert_eq!(saved.furnace.cook_time, 0);

        // Stack ids are runtime state: the window shows the live actor's ids.
        let net = container(h.world(), furnace)
            .unwrap()
            .items
            .item(2)
            .unwrap()
            .net_id;
        assert_eq!(
            h.take(
                1,
                (Name::Furnaceresultcontainer, 2, net),
                (Name::Hotbarcontainer, 5, 0)
            ),
            Status::Success
        );
        assert_eq!(h.main_slot(5).item_id(), "minecraft:iron_ingot");
        assert!(h.saved(furnace).items.item(2).unwrap().is_empty());
    });
}

#[test]
fn taking_furnace_output_pays_count_times_multiplier_plus_stored_xp() {
    with_world(GameMode::Survival, |h| {
        let furnace = IVec3::new(6, 64, 6);
        h.place(furnace, "minecraft:furnace", &[north()]);
        let player = h.player;
        let world = h.world();
        container_mut(world, furnace).unwrap().furnace.stored_xp = 200;
        // Ten glass at 0.1 is one point; the stored 200 is two more.
        pay_smelting_xp(world, player, furnace, &stack("minecraft:glass", 10));
        let experience = world.get::<Experience>(player).unwrap().clone();
        assert_eq!(experience.level, 0);
        assert!(
            (experience.progress - 3.0 / 7.0).abs() < 1e-6,
            "{experience:?}"
        );
        assert_eq!(container(world, furnace).unwrap().furnace.stored_xp, 0);
        assert!(h.packets().iter().any(|packet| matches!(packet,
            McpePacketData::UpdateAttributesPacket(update)
                if update.attribute_list.iter().any(|a| &*a.name == "minecraft:player.experience"))));
        let world = h.world();
        grant_experience(world, player, 4);
        let experience = world.get::<Experience>(player).unwrap().clone();
        assert_eq!((experience.level, experience.progress), (1, 0.0));
    });
}

#[test]
fn furnace_burns_out_unlit_and_cook_progress_decays_by_two() {
    with_world(GameMode::Survival, |h| {
        let furnace = IVec3::new(6, 64, 6);
        h.place(furnace, "minecraft:furnace", &[north()]);
        let world = h.world();
        load(world, furnace);
        {
            let mut live = container_mut(world, furnace).unwrap();
            live.items
                .set_item(0, stack("minecraft:raw_iron", 1).into_slot())
                .unwrap();
            live.furnace.lit_time = 3;
            live.furnace.lit_duration = 1600;
            live.furnace.cook_time = 50;
        }
        h.tick(2);
        assert_eq!(h.saved(furnace).furnace.cook_time, 52);
        h.tick(1);
        assert_eq!(h.block_name(furnace), "minecraft:furnace");
        // The tick that burns out does not cook; then progress decays by 2.
        let saved = h.saved(furnace);
        assert_eq!((saved.furnace.lit_time, saved.furnace.cook_time), (0, 50));
        h.tick(1);
        assert_eq!(h.saved(furnace).furnace.cook_time, 48);
    });
}

#[test]
fn blast_furnace_cooks_in_100_ticks_and_only_its_own_recipes() {
    with_world(GameMode::Survival, |h| {
        let blast = IVec3::new(7, 64, 7);
        h.place(blast, "minecraft:blast_furnace", &[north()]);
        let world = h.world();
        load(world, blast);
        {
            let mut live = container_mut(world, blast).unwrap();
            live.items
                .set_item(0, stack("minecraft:beef", 1).into_slot())
                .unwrap();
            live.items
                .set_item(1, stack("minecraft:coal", 1).into_slot())
                .unwrap();
        }
        h.tick(5);
        assert_eq!(
            h.saved(blast).furnace.lit_time,
            0,
            "beef is not a blast furnace recipe"
        );
        let world = h.world();
        container_mut(world, blast)
            .unwrap()
            .items
            .set_item(0, stack("minecraft:raw_iron", 1).into_slot())
            .unwrap();
        h.tick(100);
        let saved = h.saved(blast);
        assert_eq!(
            saved.items.item(2).unwrap().item_id(),
            "minecraft:iron_ingot"
        );
        assert_eq!(saved.furnace.lit_duration, 800, "coal burns half as long");
    });
}

#[test]
fn hoppers_pull_from_above_push_below_and_respect_furnace_faces() {
    with_world(GameMode::Survival, |h| {
        let chest = IVec3::new(8, 66, 8);
        let hopper = IVec3::new(8, 65, 8);
        let furnace = IVec3::new(8, 64, 8);
        h.place(chest, "minecraft:chest", &[north()]);
        h.place(
            hopper,
            "minecraft:hopper",
            &[("facing_direction", StateValue::Int(0))],
        );
        h.place(furnace, "minecraft:furnace", &[north()]);
        let world = h.world();
        container_mut(world, chest)
            .unwrap()
            .items
            .set_item(4, stack("minecraft:raw_iron", 3).into_slot())
            .unwrap();
        save(world, chest);

        h.tick(1);
        assert_eq!(
            h.saved(chest).items.item(4).unwrap().count,
            2,
            "one item per transfer"
        );
        assert_eq!(h.saved(hopper).items.item(0).unwrap().count, 1);
        assert_eq!(h.saved(hopper).transfer_cooldown, 8);
        h.tick(7);
        assert_eq!(
            h.saved(chest).items.item(4).unwrap().count,
            2,
            "eight-tick cooldown"
        );
        h.tick(1);
        // The pushed item entered the furnace from above: its input slot.
        let saved = h.saved(furnace);
        assert_eq!(saved.items.item(0).unwrap().item_id(), "minecraft:raw_iron");
        assert!(saved.items.item(1).unwrap().is_empty());
    });
}

#[test]
fn side_hopper_feeds_only_fuel_into_a_furnace() {
    with_world(GameMode::Survival, |h| {
        let hopper = IVec3::new(9, 64, 9);
        let furnace = IVec3::new(10, 64, 9);
        h.place(
            hopper,
            "minecraft:hopper",
            &[("facing_direction", StateValue::Int(5))],
        );
        h.place(furnace, "minecraft:furnace", &[north()]);
        let world = h.world();
        {
            let mut live = container_mut(world, hopper).unwrap();
            live.items
                .set_item(0, stack("minecraft:raw_iron", 1).into_slot())
                .unwrap();
            live.items
                .set_item(1, stack("minecraft:coal", 1).into_slot())
                .unwrap();
        }
        h.tick(1);
        let saved = h.saved(furnace);
        assert!(saved.items.item(0).unwrap().is_empty());
        assert_eq!(saved.items.item(1).unwrap().item_id(), "minecraft:coal");
        assert_eq!(
            h.saved(hopper).items.item(0).unwrap().item_id(),
            "minecraft:raw_iron"
        );
    });
}

#[test]
fn hopper_collects_dropped_items_resting_on_it() {
    with_world(GameMode::Survival, |h| {
        let hopper = IVec3::new(11, 64, 11);
        h.place(
            hopper,
            "minecraft:hopper",
            &[("facing_direction", StateValue::Int(0))],
        );
        let world = h.world();
        crate::entity::item_drop::spawn_item_drop(
            world,
            DVec3::new(11.5, 65.0, 11.5),
            stack("minecraft:stick", 7).into_slot(),
            None,
        )
        .unwrap();
        h.tick(1);
        assert_eq!(h.saved(hopper).items.item(0).unwrap().count, 7);
        let world = h.world();
        assert_eq!(world.query::<&ItemEntityStack>().iter(world).count(), 0);
    });
}

#[test]
fn breaking_a_furnace_drops_whole_stacks() {
    with_world(GameMode::Survival, |h| {
        let furnace = IVec3::new(6, 64, 6);
        h.place(furnace, "minecraft:furnace", &[north()]);
        let world = h.world();
        container_mut(world, furnace)
            .unwrap()
            .items
            .set_item(2, stack("minecraft:iron_ingot", 40).into_slot())
            .unwrap();
        let broken = breaking(world, furnace).unwrap();
        assert!(!broken.randomize);
        drop_contents(world, furnace, broken);
        let counts: Vec<u8> = world
            .query::<&ItemEntityStack>()
            .iter(world)
            .map(|stack| stack.0.count)
            .collect();
        assert_eq!(counts, [40]);
    });
}

#[test]
fn closing_the_window_ignores_ui_items_only_when_none_and_keeps_cursor_rule() {
    with_world(GameMode::Survival, |h| {
        let chest = IVec3::new(2, 64, 2);
        h.place(chest, "minecraft:chest", &[north()]);
        h.open(chest);
        let player = h.player;
        h.world()
            .get_mut::<PlayerUi>(player)
            .unwrap()
            .0
            .set_item(0, stack("minecraft:stick", 2).into_slot())
            .unwrap();
        h.close();
        assert_eq!(
            h.main_slot(0).item_id(),
            "minecraft:stick",
            "cursor returns to the inventory"
        );
    });
}

#[test]
fn furnaces_in_a_chunk_that_loads_later_keep_smelting() {
    with_world(GameMode::Survival, |h| {
        let world = h.world();
        let blocks = BlockRegistry::vanilla();
        let lit = blocks
            .default_state_id_by_name("minecraft:lit_furnace")
            .unwrap();
        let pos = IVec3::new(20, 64, 3);
        let mut furnace = BlockContainer::new(ContainerKind::Furnace, pos);
        furnace
            .items
            .set_item(0, stack("minecraft:raw_gold", 1).into_slot())
            .unwrap();
        furnace.furnace.lit_time = 50;
        furnace.furnace.lit_duration = 1600;
        furnace.furnace.cook_time = 5;
        let mut chunk = Chunk::new(1, 0);
        chunk.set_block(4, 64, 3, lit);
        assert!(
            chunk.set_block_entity(BlockEntity::from_bedrock_compound(furnace.encode()).unwrap())
        );
        let entity = world
            .spawn((ChunkData::new(chunk), ChunkStateFlags::default()))
            .id();
        world
            .resource_mut::<ChunkManager>()
            .insert(ChunkPosition::new(1, 0), entity);
        h.tick(1);
        let world = h.world();
        let live = container(world, pos).expect("registered when the chunk loaded");
        assert_eq!((live.furnace.lit_time, live.furnace.cook_time), (49, 6));
    });
}

#[test]
fn generated_loot_unpacks_once_on_first_open() {
    use crate::world::block_entity::LootTable;
    with_world(GameMode::Survival, |h| {
        let pos = IVec3::new(12, 64, 12);
        let state = h.state("minecraft:chest", &[north()]);
        let chunk = h.chunk;
        let world = h.world();
        super::super::blocks::set_block(world, pos, state, false);
        world
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block_entity(BlockEntity::generated(
                pos.to_array(),
                "Chest",
                &[],
                Some(LootTable::Bedrock(
                    "loot_tables/chests/simple_dungeon.json",
                    4242,
                )),
                None,
            ));
        h.open(pos);
        let first: Vec<String> = h
            .saved(pos)
            .items
            .items()
            .filter(|(_, stack)| !stack.is_empty())
            .map(|(slot, stack)| format!("{slot}:{}x{}", stack.item_id(), stack.count))
            .collect();
        assert!(!first.is_empty(), "the dungeon table rolled items");
        let encoded = h.saved(pos).encode();
        assert!(!encoded.0.contains_key("LootTable") && !encoded.0.contains_key("LootTableSeed"));
        h.close();
        h.open(pos);
        let second: Vec<String> = h
            .saved(pos)
            .items
            .items()
            .filter(|(_, stack)| !stack.is_empty())
            .map(|(slot, stack)| format!("{slot}:{}x{}", stack.item_id(), stack.count))
            .collect();
        assert_eq!(first, second, "reopening does not roll again");
    });
}

/// BDS 1.26.51 announces a chest set with `/setblock ~2 ~ ~ chest` with a
/// `BlockActorData` of its client-side state (client-diff `containers-dry`
/// capture, seq 281; the bot stood at feet block (0, -60, 0)).
#[test]
fn placing_a_container_announces_the_same_client_state_as_bds() {
    const BDS: &str = include_str!("testdata/bds-setblock-chest-actor.jsonl");
    let row: serde_json::Value = serde_json::from_str(BDS.trim()).unwrap();
    let McpePacketData::BlockActorDataPacket(bds) =
        super::super::inventory::capture::decode(row["hex"].as_str().unwrap())
    else {
        panic!("BlockActorData fixture");
    };
    let compound = |nbt| {
        BlockEntity::from_network_nbt(nbt)
            .unwrap()
            .bedrock_compound()
            .unwrap()
            .unwrap()
    };
    with_world(GameMode::Survival, move |h| {
        let pos = IVec3::new(
            bds.block_position.x,
            bds.block_position.y,
            bds.block_position.z,
        );
        h.place(pos, "minecraft:chest", &[]);
        let ours = h
            .packets()
            .into_iter()
            .find_map(|packet| match packet {
                McpePacketData::BlockActorDataPacket(data) => Some(data),
                _ => None,
            })
            .expect("BlockActorData");
        assert_eq!(ours.block_position, bds.block_position);
        assert_eq!(
            compound(ours.actor_data_tags),
            compound(bds.actor_data_tags)
        );
    });
}
