//! End to end over loopback RakNet: a real client joins the real server
//! runtime as a saved survival player, eats an apple, mines stone with an
//! iron pickaxe (the pickaxe wears) and shoots a cow with a bow.
//!
//! Exclusive loopback socket run: take the native lock first
//! (`native-lock.sh acquire items ...`).

use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use jolyne::BedrockStream;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::stream::transport::RakNetTransport;
use jolyne::stream::{Client, Handshake, Play};
use jolyne::valentine::*;
use unastar::config::{SpawnLocation, SpawnRule, UnastarConfig};
use unastar::storage::{LevelDBPlayerProvider, PlayerData, PlayerProvider, StoredItemStack};

type ClientPlay = BedrockStream<Play, Client, RakNetTransport>;

const TIMEOUT: Duration = Duration::from_secs(20);

fn free_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn recv_until<T>(
    client: &mut ClientPlay,
    what: &str,
    mut pick: impl FnMut(McpePacketData) -> Option<T>,
) -> T {
    tokio::time::timeout(TIMEOUT, async {
        loop {
            let packet = client.recv_packet().await.expect("client receives");
            if let Some(found) = pick(packet.data) {
                return found;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

async fn send(client: &mut ClientPlay, packet: impl Into<McpePacket>) {
    client.send_packet(packet.into()).await.unwrap();
    client.flush().await.unwrap();
}

fn stored(item_id: &str, count: u8) -> StoredItemStack {
    StoredItemStack {
        item_id: item_id.into(),
        count,
        ..Default::default()
    }
}

fn use_air(slot: i32) -> InventoryTransactionPacket {
    InventoryTransactionPacket {
        transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(Box::new(
            ItemUseInventoryTransaction {
                action_type: EnumsItemUseInventoryTransactionActionType::Use,
                slot,
                hand: EnumsHandSlot::Mainhand,
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

fn release(slot: i32) -> InventoryTransactionPacket {
    InventoryTransactionPacket {
        transaction: InventoryTransactionPacketTransaction::ItemReleaseInventoryTransaction(
            ItemReleaseInventoryTransaction {
                action_type: EnumsItemReleaseInventoryTransactionActionType::Release,
                slot,
                ..Default::default()
            },
        ),
        ..Default::default()
    }
}

/// A `PlayerAuthInput` at the spawn eye position, looking south tilted
/// down by `pitch`, carrying `actions`.
fn auth_input(tick: u64, pitch: f32, actions: Vec<PlayerBlockActionData>) -> PlayerAuthInputPacket {
    PlayerAuthInputPacket {
        player_rotation: Vec2 { x: pitch, y: 0.0 },
        position: Vec3 {
            x: 0.5,
            y: 17.62,
            z: 0.5,
        },
        client_tick: PlayerInputTick { inputtick: tick },
        player_block_actions: (!actions.is_empty()).then_some(actions),
        ..Default::default()
    }
}

fn select(runtime_id: u64, slot: u8) -> MobEquipmentPacket {
    MobEquipmentPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id,
        },
        slot,
        selected_slot: slot,
        container_id: 0,
        ..Default::default()
    }
}

#[test]
fn eat_mine_and_shoot_over_loopback() {
    let directory = tempfile::tempdir().unwrap();
    std::env::set_current_dir(directory.path()).unwrap();
    let uuid = uuid::Uuid::new_v4();
    let name = "Archer";
    // A saved survival player: hungry, with an apple, an iron pickaxe, a
    // bow and arrows in the hotbar.
    let players = directory.path().join("players");
    {
        let provider = LevelDBPlayerProvider::open(players.join("db")).unwrap();
        let mut inventory = vec![StoredItemStack::default(); 36];
        inventory[0] = stored("minecraft:apple", 2);
        inventory[1] = stored("minecraft:iron_pickaxe", 1);
        inventory[2] = stored("minecraft:bow", 1);
        inventory[3] = stored("minecraft:arrow", 16);
        let data = PlayerData {
            uuid: uuid.to_string(),
            game_mode: 0,
            food: 10,
            saturation: 0.0,
            inventory,
            ender_chest: vec![StoredItemStack::default(); 27],
            ..Default::default()
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            provider.save(uuid, &data).await.unwrap();
            provider.close().await.unwrap();
        });
    }
    let port = free_port();
    let mut config = UnastarConfig::default();
    config.server.bind_address = format!("127.0.0.1:{port}");
    config.server.online_mode = false;
    config.server.default_gamemode = "survival".into();
    config.server.default_chunk_radius = 2;
    config.server.operators = vec![name.into()];
    config.world = unastar::world::WorldConfig::void_spawn_platform(1);
    config.players.data_dir = players;
    config.spawn_rules = vec![SpawnRule {
        always_at_location: true,
        location: Some(SpawnLocation::default()),
        ..SpawnRule::default()
    }];
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_stack_size(64 * 1024 * 1024)
        .build()
        .unwrap();
    runtime.block_on(async move {
        let mut server = unastar::server::UnastarServer::new(Arc::new(config))
            .await
            .unwrap();
        let server_task =
            tokio::spawn(async move { server.run().await.map_err(|e| e.to_string()) });
        let address: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let mut attempts = 0;
        let client = loop {
            match BedrockStream::<Handshake, Client, RakNetTransport>::connect(address).await {
                Ok(client) => break client,
                Err(_) if attempts < 50 => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(error) => panic!("connect: {error}"),
            }
        };
        let mut handshake = ClientHandshakeConfig::random(address, name);
        handshake.uuid = uuid;
        let (mut client, game) = tokio::time::timeout(TIMEOUT, client.join(handshake))
            .await
            .expect("join in time")
            .expect("join");
        let runtime_id = game.start_game.runtime_id.actor_runtime_id;
        send(
            &mut client,
            RequestChunkRadiusPacket {
                chunk_radius: 2,
                ..Default::default()
            },
        )
        .await;
        send(&mut client, SetLocalPlayerAsInitializedPacket::default()).await;
        send(&mut client, auth_input(1, 0.0, vec![])).await;

        // Eat: one use, the server finishes it after 32 ticks.
        send(&mut client, use_air(0)).await;
        let apple = game
            .item_registry
            .item_data
            .iter()
            .find(|item| &*item.item_name == "minecraft:apple")
            .unwrap()
            .item_id;
        // The default level is peaceful, which also refills food every 10
        // ticks: track the food level until the meal completes.
        let hunger_of = |update: &UpdateAttributesPacket| {
            update
                .attribute_list
                .iter()
                .find(|a| &*a.name == "minecraft:player.hunger")
                .map(|a| a.current_value)
        };
        let mut food = 10.0;
        let completed = recv_until(&mut client, "CompletedUsingItem", |data| match data {
            McpePacketData::CompletedUsingItemPacket(done) => Some(done),
            McpePacketData::UpdateAttributesPacket(update) => {
                food = hunger_of(&update).unwrap_or(food);
                None
            }
            _ => None,
        })
        .await;
        assert_eq!((completed.item_id, completed.item_use_method), (apple, 1));
        let eaten = recv_until(&mut client, "hunger after the apple", |data| match data {
            McpePacketData::UpdateAttributesPacket(update) => hunger_of(&update),
            _ => None,
        })
        .await;
        assert!(eaten >= food + 4.0, "food {food} -> {eaten}");

        // Mine the platform stone under the spawn with the iron pickaxe.
        send(&mut client, select(runtime_id, 1)).await;
        let stone = BlockPos { x: 0, y: 15, z: 1 };
        let action = |kind| PlayerBlockActionData {
            player_action_type: kind,
            position: stone.clone(),
            facing: 1,
        };
        send(
            &mut client,
            auth_input(
                2,
                60.0,
                vec![action(EnumsPlayerActionType::Startdestroyblock)],
            ),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(1500)).await;
        send(
            &mut client,
            auth_input(
                32,
                60.0,
                vec![action(EnumsPlayerActionType::Stopdestroyblock)],
            ),
        )
        .await;
        let worn = recv_until(&mut client, "pickaxe slot update", |data| match data {
            McpePacketData::InventorySlotPacket(slot)
                if slot.container_id == 0 && slot.slot == 1 =>
            {
                Some(slot.item)
            }
            _ => None,
        })
        .await;
        // The worn pickaxe carries user data with its Damage tag.
        assert_ne!(
            worn.user_data_buffer,
            bytes::Bytes::from_static(&[0; 10]),
            "the pickaxe's damage is in its user data"
        );

        // Draw the bow, then summon a cow four blocks south (just before
        // letting go, so it has no time to wander off) and shoot it.
        send(&mut client, select(runtime_id, 2)).await;
        send(&mut client, auth_input(40, 10.0, vec![])).await;
        send(&mut client, use_air(2)).await;
        tokio::time::sleep(Duration::from_millis(1200)).await;
        send(
            &mut client,
            CommandRequestPacket {
                command: "/summon minecraft:cow 0.5 16 4.5".into(),
                ..Default::default()
            },
        )
        .await;
        let cow = recv_until(&mut client, "cow appears", |data| match data {
            McpePacketData::AddActorPacket(actor) if &*actor.actor_type == "minecraft:cow" => {
                Some(actor.target_runtime_id.actor_runtime_id)
            }
            _ => None,
        })
        .await;
        send(&mut client, release(2)).await;
        recv_until(&mut client, "arrow flies", |data| match data {
            McpePacketData::AddActorPacket(actor) if &*actor.actor_type == "minecraft:arrow" => {
                Some(())
            }
            _ => None,
        })
        .await;
        recv_until(&mut client, "cow is hurt", |data| match data {
            McpePacketData::ActorEventPacket(event)
                if event.target_runtime_id.actor_runtime_id == cow
                    && event.event_id == EnumsActorEvent::Hurt =>
            {
                Some(())
            }
            _ => None,
        })
        .await;
        server_task.abort();
    });
}
