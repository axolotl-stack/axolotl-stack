//! End to end over loopback RakNet: a real client joins the real server
//! runtime as a saved survival player, places an oak door, opens and
//! closes it, then breaks its upper half. The client's view of the world,
//! rebuilt only from the UpdateBlock packets it receives, must end with no
//! door block left in either cell (the "invisible lower half" bug).
//!
//! Exclusive loopback socket run: take the native lock first
//! (`native-lock.sh acquire blocks ...`).

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use jolyne::BedrockStream;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::stream::transport::RakNetTransport;
use jolyne::stream::{Client, Handshake, Play};
use jolyne::valentine::*;
use unastar::config::{SpawnLocation, SpawnRule, UnastarConfig};
use unastar::registry::BlockRegistry;
use unastar::registry::block::StateValue;
use unastar::storage::{LevelDBPlayerProvider, PlayerData, PlayerProvider, StoredItemStack};

type ClientPlay = BedrockStream<Play, Client, RakNetTransport>;

const TIMEOUT: Duration = Duration::from_secs(20);

/// The platform block clicked to place the door, and the door's cells.
const FLOOR: BlockPos = BlockPos { x: 0, y: 15, z: 2 };
const LOWER: (i32, i32, i32) = (0, 16, 2);
const UPPER: (i32, i32, i32) = (0, 17, 2);

fn free_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn send(client: &mut ClientPlay, packet: impl Into<McpePacket>) {
    client.send_packet(packet.into()).await.unwrap();
    client.flush().await.unwrap();
}

/// The client's world as UpdateBlock packets describe it, in runtime ids.
#[derive(Default)]
struct ClientView {
    blocks: HashMap<(i32, i32, i32), u32>,
    by_hash: HashMap<u32, u32>,
    hotbar: Vec<CerealizerNetworkItemStackDescriptorSerializedData>,
}

impl ClientView {
    fn new() -> Self {
        let registry = BlockRegistry::vanilla();
        let by_hash = registry
            .iter()
            .flat_map(|entry| entry.min_state_id..=entry.max_state_id)
            .filter_map(|state| registry.network_hash(state).map(|hash| (hash, state)))
            .collect();
        Self {
            by_hash,
            ..Self::default()
        }
    }

    fn see(&mut self, packet: &McpePacketData) {
        match packet {
            McpePacketData::UpdateBlockPacket(update) if update.layer == 0 => {
                let pos = (
                    update.block_position.x,
                    update.block_position.y,
                    update.block_position.z,
                );
                let state = *self
                    .by_hash
                    .get(&update.block_runtime_id)
                    .unwrap_or_else(|| panic!("unknown wire id {}", update.block_runtime_id));
                self.blocks.insert(pos, state);
            }
            McpePacketData::InventoryContentPacket(content) if content.container_id == 0 => {
                self.hotbar = content.slots.iter().take(9).cloned().collect();
            }
            _ => {}
        }
    }

    fn name(&self, pos: (i32, i32, i32)) -> Option<&str> {
        let state = *self.blocks.get(&pos)?;
        BlockRegistry::vanilla()
            .get_by_runtime_id(state)
            .map(|entry| entry.string_id.as_str())
    }

    fn value(&self, pos: (i32, i32, i32), property: &str) -> Option<StateValue> {
        let state = *self.blocks.get(&pos)?;
        BlockRegistry::vanilla()
            .get_by_runtime_id(state)?
            .state_value(state, property)
    }
}

/// Receive packets until `done` holds for the client's view.
async fn until(
    client: &mut ClientPlay,
    view: &mut ClientView,
    what: &str,
    mut done: impl FnMut(&ClientView, &McpePacketData) -> bool,
) {
    tokio::time::timeout(TIMEOUT, async {
        loop {
            let packet = client.recv_packet().await.expect("client receives");
            view.see(&packet.data);
            if done(view, &packet.data) {
                return;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
}

/// Receive everything that arrives within `window`.
async fn drain(client: &mut ClientPlay, view: &mut ClientView, window: Duration) {
    let _ = tokio::time::timeout(window, async {
        loop {
            let packet = client.recv_packet().await.expect("client receives");
            view.see(&packet.data);
        }
    })
    .await;
}

fn auth_input(tick: u64, actions: Vec<PlayerBlockActionData>) -> PlayerAuthInputPacket {
    PlayerAuthInputPacket {
        // Looking south, tilted down at the floor in front.
        player_rotation: Vec2 { x: 45.0, y: 0.0 },
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

fn click(
    position: BlockPos,
    face: u8,
    item: CerealizerNetworkItemStackDescriptorSerializedData,
) -> InventoryTransactionPacket {
    InventoryTransactionPacket {
        transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(Box::new(
            ItemUseInventoryTransaction {
                action_type: EnumsItemUseInventoryTransactionActionType::Place,
                position,
                face,
                slot: 0,
                hand: EnumsHandSlot::Mainhand,
                item,
                click_position: Vec3 {
                    x: 0.5,
                    y: 1.0,
                    z: 0.5,
                },
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

#[test]
fn placed_toggled_and_broken_door_leaves_nothing_on_the_client() {
    let directory = tempfile::tempdir().unwrap();
    std::env::set_current_dir(directory.path()).unwrap();
    let uuid = uuid::Uuid::new_v4();
    let name = "Doorman";
    let players = directory.path().join("players");
    {
        let provider = LevelDBPlayerProvider::open(players.join("db")).unwrap();
        let mut inventory = vec![StoredItemStack::default(); 36];
        inventory[0] = StoredItemStack {
            item_id: "minecraft:wooden_door".into(),
            count: 2,
            ..Default::default()
        };
        let data = PlayerData {
            uuid: uuid.to_string(),
            game_mode: 0,
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
    config.server.max_chunk_radius = 2;
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
        let (mut client, _game) = tokio::time::timeout(TIMEOUT, client.join(handshake))
            .await
            .expect("join in time")
            .expect("join");
        let mut view = ClientView::new();
        send(
            &mut client,
            RequestChunkRadiusPacket {
                chunk_radius: 2,
                ..Default::default()
            },
        )
        .await;
        send(&mut client, SetLocalPlayerAsInitializedPacket::default()).await;
        send(&mut client, auth_input(1, vec![])).await;
        until(&mut client, &mut view, "the hotbar", |view, _| {
            view.hotbar.first().is_some_and(|item| item.id != 0)
        })
        .await;

        // Place: both halves arrive.
        let door = view.hotbar[0].clone();
        send(&mut client, click(FLOOR, 1, door)).await;
        until(&mut client, &mut view, "both door halves", |view, _| {
            view.name(LOWER) == Some("minecraft:wooden_door")
                && view.name(UPPER) == Some("minecraft:wooden_door")
        })
        .await;
        assert_eq!(
            view.value(UPPER, "upper_block_bit"),
            Some(StateValue::Byte(1))
        );

        // Open and close it: every click flips the lower half.
        for open in [1u8, 0] {
            let hand = view.hotbar[0].clone();
            let target = BlockPos {
                x: LOWER.0,
                y: LOWER.1,
                z: LOWER.2,
            };
            send(&mut client, click(target, 2, hand)).await;
            until(
                &mut client,
                &mut view,
                "the door toggles",
                |view, packet| {
                    matches!(packet, McpePacketData::LevelSoundEventPacket(sound)
                    if sound.sound_event.starts_with("door."))
                        && view.value(LOWER, "open_bit") == Some(StateValue::Byte(open))
                },
            )
            .await;
        }

        // Break the upper half by hand (wooden door: 90 ticks by hand).
        let upper = BlockPos {
            x: UPPER.0,
            y: UPPER.1,
            z: UPPER.2,
        };
        let action = |kind| PlayerBlockActionData {
            player_action_type: kind,
            position: upper.clone(),
            facing: 2,
        };
        send(
            &mut client,
            auth_input(10, vec![action(EnumsPlayerActionType::Startdestroyblock)]),
        )
        .await;
        drain(&mut client, &mut view, Duration::from_millis(5000)).await;
        send(
            &mut client,
            auth_input(120, vec![action(EnumsPlayerActionType::Stopdestroyblock)]),
        )
        .await;
        until(
            &mut client,
            &mut view,
            "the door item drops",
            |_, packet| matches!(packet, McpePacketData::AddItemActorPacket(_)),
        )
        .await;
        drain(&mut client, &mut view, Duration::from_millis(500)).await;
        assert_eq!(view.name(UPPER), Some("minecraft:air"), "upper half gone");
        assert_eq!(
            view.name(LOWER),
            Some("minecraft:air"),
            "no invisible lower half left on the client"
        );
        server_task.abort();
    });
}
