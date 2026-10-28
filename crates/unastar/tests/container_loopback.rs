//! End to end over loopback RakNet: a real client joins the real server
//! runtime, creates a chest and cobblestone from the creative inventory,
//! places the chest, opens it, moves the cobblestone in and closes it.
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

fn slot(name: EnumsContainerEnumName, slot: u8, net: i32) -> ItemStackRequestCerealSlotInfoData {
    ItemStackRequestCerealSlotInfoData {
        fullcontainername: FullContainerName {
            container_name: name,
            dynamic_id: None,
        },
        slot,
        net_id_variant: net,
    }
}

fn request(
    id: i32,
    actions: Vec<ItemStackRequestPacketDataRequestDataActionsItem>,
) -> ItemStackRequestPacket {
    ItemStackRequestPacket {
        requests: vec![ItemStackRequestPacketDataRequestData {
            client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id },
            actions,
            ..Default::default()
        }],
    }
}

/// CraftCreative into the created-output slot, then take it to a hotbar slot.
fn creative_to_hotbar(id: i32, creative: u32, hotbar: u8) -> ItemStackRequestPacket {
    use ItemStackRequestPacketDataRequestDataActionsItem as Action;
    request(
        id,
        vec![
            Action::CraftCreativeActionData(ItemStackRequestCerealCraftCreativeActionData {
                creative_item_net_id: creative,
                numberofrequestedcrafts: 1,
                ..Default::default()
            }),
            Action::TakeActionData(ItemStackRequestCerealTakeActionData {
                amount: 1,
                source: slot(EnumsContainerEnumName::Createdoutputcontainer, 50, id),
                destination: slot(EnumsContainerEnumName::Hotbarcontainer, hotbar, 0),
                ..Default::default()
            }),
        ],
    )
}

fn click_block(
    position: BlockPos,
    hotbar: i32,
    item: CerealizerNetworkItemStackDescriptorSerializedData,
) -> InventoryTransactionPacket {
    InventoryTransactionPacket {
        transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(Box::new(
            ItemUseInventoryTransaction {
                action_type: EnumsItemUseInventoryTransactionActionType::Place,
                position,
                face: 1,
                slot: hotbar,
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
fn chest_open_move_close_over_loopback() {
    let directory = tempfile::tempdir().unwrap();
    // The runtime keeps worlds, players and plugins below the working directory.
    std::env::set_current_dir(directory.path()).unwrap();
    let port = free_port();
    let mut config = UnastarConfig::default();
    config.server.bind_address = format!("127.0.0.1:{port}");
    config.server.online_mode = false;
    config.server.default_gamemode = "creative".into();
    config.server.default_chunk_radius = 2;
    config.world = unastar::world::WorldConfig::void_spawn_platform(1);
    config.players.data_dir = directory.path().join("players");
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
                Err(error) if attempts < 50 => {
                    attempts += 1;
                    let _ = error;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(error) => panic!("connect: {error}"),
            }
        };
        let (mut client, game) = tokio::time::timeout(
            TIMEOUT,
            client.join(ClientHandshakeConfig::random(address, "containers")),
        )
        .await
        .expect("join in time")
        .expect("join");
        let item_id = |name: &str| {
            game.item_registry
                .item_data
                .iter()
                .find(|item| &*item.item_name == name)
                .unwrap_or_else(|| panic!("{name} registered"))
                .item_id
        };
        let (chest_id, cobble_id) = (item_id("minecraft:chest"), item_id("minecraft:cobblestone"));
        let creative = recv_until(&mut client, "CreativeContent", |data| match data {
            McpePacketData::CreativeContentPacket(content) => Some(content),
            _ => None,
        })
        .await;
        let entry = |id: i16| {
            creative
                .entries
                .iter()
                .find(|entry| entry.item_instance.id == i32::from(id))
                .unwrap()
                .clone()
        };
        let (chest, cobble) = (entry(chest_id), entry(cobble_id));
        send(
            &mut client,
            RequestChunkRadiusPacket {
                chunk_radius: 2,
                ..Default::default()
            },
        )
        .await;
        send(&mut client, SetLocalPlayerAsInitializedPacket::default()).await;

        // In the opened inventory: creative chest into hotbar 0 and
        // cobblestone into hotbar 1.
        send(
            &mut client,
            InteractPacket {
                action: EnumsInteractPacketPayloadAction::Openinventory,
                ..Default::default()
            },
        )
        .await;
        let inventory = recv_until(&mut client, "inventory opens", |data| match data {
            McpePacketData::ContainerOpenPacket(open) => Some(open.container_id),
            _ => None,
        })
        .await;
        send(
            &mut client,
            creative_to_hotbar(-1, chest.creative_net_id.id, 0),
        )
        .await;
        send(
            &mut client,
            creative_to_hotbar(-3, cobble.creative_net_id.id, 1),
        )
        .await;
        let mut cobble_net = 0;
        for want in [-1, -3] {
            let response = recv_until(&mut client, "creative response", |data| match data {
                McpePacketData::ItemStackResponsePacket(packet) => {
                    Some(packet.responses[0].clone())
                }
                _ => None,
            })
            .await;
            assert_eq!(
                (response.client_request_id.id, response.result),
                (want, EnumsItemStackNetResult::Success)
            );
            if want == -3 {
                cobble_net = response.containers.unwrap()[0].slots[0]
                    .item_stack_net_id
                    .as_ref()
                    .unwrap()
                    .id;
            }
        }

        send(
            &mut client,
            ContainerClosePacket {
                container_id: inventory,
                container_type: 0xff,
                server_initiated_close: false,
            },
        )
        .await;
        recv_until(&mut client, "inventory closes", |data| match data {
            McpePacketData::ContainerClosePacket(_) => Some(()),
            _ => None,
        })
        .await;

        // Place the chest on the spawn platform (top at y 15), then open it.
        let held = CerealizerNetworkItemStackDescriptorSerializedData {
            id: chest_id,
            stacksize: 1,
            auxvalue: chest.item_instance.auxvalue,
            net_id_variant: None,
            block_runtime_id: chest.item_instance.block_runtime_id as u32,
            user_data_buffer: chest.item_instance.user_data_buffer.clone(),
        };
        let platform = BlockPos { x: 2, y: 15, z: 0 };
        let chest_pos = BlockPos { x: 2, y: 16, z: 0 };
        let mut placed = false;
        for _ in 0..40 {
            send(&mut client, click_block(platform.clone(), 0, held.clone())).await;
            placed = tokio::time::timeout(Duration::from_millis(500), async {
                loop {
                    // The chest faces the player, so its state is not the
                    // creative item's default; the place sound marks success.
                    match client.recv_packet().await.unwrap().data {
                        McpePacketData::LevelSoundEventPacket(sound)
                            if &*sound.sound_event == "place"
                                && sound.position.x == 2.5
                                && sound.position.y == 16.5 =>
                        {
                            return true;
                        }
                        _ => {}
                    }
                }
            })
            .await
            .unwrap_or(false);
            if placed {
                break;
            }
        }
        assert!(placed, "the chest was placed (spawn chunk loaded)");

        send(&mut client, click_block(chest_pos.clone(), 0, held.clone())).await;
        let open = recv_until(&mut client, "ContainerOpen", |data| match data {
            McpePacketData::ContainerOpenPacket(open) => Some(open),
            _ => None,
        })
        .await;
        assert_eq!(open.container_type, 0);
        assert_eq!(open.position, chest_pos);
        assert_eq!(open.target_actor_id.actor_unique_id, -1);
        let content = recv_until(&mut client, "chest content", |data| match data {
            McpePacketData::InventoryContentPacket(content)
                if content.container_id == u32::from(open.container_id) =>
            {
                Some(content)
            }
            _ => None,
        })
        .await;
        assert_eq!(content.slots.len(), 27);
        recv_until(&mut client, "lid opens", |data| match data {
            McpePacketData::BlockEventPacket(event)
                if event.block_position == chest_pos && event.event_value == 1 =>
            {
                Some(())
            }
            _ => None,
        })
        .await;

        use ItemStackRequestPacketDataRequestDataActionsItem as Action;
        send(
            &mut client,
            request(
                -5,
                vec![Action::TakeActionData(
                    ItemStackRequestCerealTakeActionData {
                        amount: 1,
                        source: slot(EnumsContainerEnumName::Hotbarcontainer, 1, cobble_net),
                        destination: slot(EnumsContainerEnumName::Levelentitycontainer, 13, 0),
                        ..Default::default()
                    },
                )],
            ),
        )
        .await;
        let response = recv_until(&mut client, "move response", |data| match data {
            McpePacketData::ItemStackResponsePacket(packet) => Some(packet.responses[0].clone()),
            _ => None,
        })
        .await;
        assert_eq!(response.result, EnumsItemStackNetResult::Success);
        let containers = response.containers.unwrap();
        assert!(containers.iter().any(|c| {
            c.full_container_name.container_name == EnumsContainerEnumName::Levelentitycontainer
                && c.slots[0].slot == 13
                && c.slots[0].amount == 1
        }));

        send(
            &mut client,
            ContainerClosePacket {
                container_id: open.container_id,
                container_type: 0,
                server_initiated_close: false,
            },
        )
        .await;
        let close = recv_until(&mut client, "close reply", |data| match data {
            McpePacketData::ContainerClosePacket(close) => Some(close),
            _ => None,
        })
        .await;
        assert_eq!(
            (
                close.container_id,
                close.container_type as i8,
                close.server_initiated_close
            ),
            (open.container_id, -9, false)
        );
        recv_until(&mut client, "lid closes", |data| match data {
            McpePacketData::BlockEventPacket(event)
                if event.block_position == chest_pos && event.event_value == 0 =>
            {
                Some(())
            }
            _ => None,
        })
        .await;

        // Reopening shows the moved stack in slot 13.
        send(&mut client, click_block(chest_pos, 0, held)).await;
        let open = recv_until(&mut client, "second ContainerOpen", |data| match data {
            McpePacketData::ContainerOpenPacket(open) => Some(open),
            _ => None,
        })
        .await;
        let content = recv_until(&mut client, "second content", |data| match data {
            McpePacketData::InventoryContentPacket(content)
                if content.container_id == u32::from(open.container_id) =>
            {
                Some(content)
            }
            _ => None,
        })
        .await;
        assert_eq!(
            (content.slots[13].id, content.slots[13].stacksize),
            (cobble_id, 1)
        );
        server_task.abort();
    });
}
