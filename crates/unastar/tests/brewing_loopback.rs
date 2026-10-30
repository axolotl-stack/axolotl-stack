//! End to end over loopback RakNet: a real client joins the real server
//! runtime, takes a brewing stand, blaze powder, three water bottles and
//! nether wart from the creative inventory, places and opens the stand,
//! fills it, and watches three awkward potions brew.
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
/// One brew is 400 ticks (20 s at 20 TPS); leave room for a slow machine.
const BREW_TIMEOUT: Duration = Duration::from_secs(60);
/// Potion type aux of water and awkward potions.
const WATER: u32 = 0;
const AWKWARD: u32 = 4;

fn free_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn recv_within<T>(
    client: &mut ClientPlay,
    what: &str,
    timeout: Duration,
    mut pick: impl FnMut(McpePacketData) -> Option<T>,
) -> T {
    tokio::time::timeout(timeout, async {
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

async fn recv_until<T>(
    client: &mut ClientPlay,
    what: &str,
    pick: impl FnMut(McpePacketData) -> Option<T>,
) -> T {
    recv_within(client, what, TIMEOUT, pick).await
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

/// CraftCreative into the created-output slot, then take one to a hotbar slot.
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

/// Move one item from a hotbar slot into a slot of the open window.
fn take_into(
    id: i32,
    hotbar: u8,
    net: i32,
    to: EnumsContainerEnumName,
    at: u8,
) -> ItemStackRequestPacket {
    use ItemStackRequestPacketDataRequestDataActionsItem as Action;
    request(
        id,
        vec![Action::TakeActionData(
            ItemStackRequestCerealTakeActionData {
                amount: 1,
                source: slot(EnumsContainerEnumName::Hotbarcontainer, hotbar, net),
                destination: slot(to, at, 0),
                ..Default::default()
            },
        )],
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

async fn response(client: &mut ClientPlay, id: i32) -> ItemStackResponseInfo {
    let response = recv_until(client, "item stack response", |data| match data {
        McpePacketData::ItemStackResponsePacket(packet) => Some(packet.responses[0].clone()),
        _ => None,
    })
    .await;
    assert_eq!(
        (response.client_request_id.id, response.result),
        (id, EnumsItemStackNetResult::Success)
    );
    response
}

#[test]
fn brewing_stand_brews_awkward_potions_over_loopback() {
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
            client.join(ClientHandshakeConfig::random(address, "brewing")),
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
        let stand_id = item_id("minecraft:brewing_stand");
        let powder_id = item_id("minecraft:blaze_powder");
        let potion_id = item_id("minecraft:potion");
        let wart_id = item_id("minecraft:nether_wart");
        let creative = recv_until(&mut client, "CreativeContent", |data| match data {
            McpePacketData::CreativeContentPacket(content) => Some(content),
            _ => None,
        })
        .await;
        let entry = |id: i16, aux: u32| {
            creative
                .entries
                .iter()
                .find(|entry| {
                    entry.item_instance.id == i32::from(id) && entry.item_instance.auxvalue == aux
                })
                .unwrap_or_else(|| panic!("creative item {id}:{aux}"))
                .clone()
        };
        let stand = entry(stand_id, 0);
        send(
            &mut client,
            RequestChunkRadiusPacket {
                chunk_radius: 2,
                ..Default::default()
            },
        )
        .await;
        send(&mut client, SetLocalPlayerAsInitializedPacket::default()).await;

        // Hotbar: 0 brewing stand, 1 blaze powder, 2..=4 water bottles,
        // 5 nether wart.
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
        let picks = [
            (stand.creative_net_id.id, 0),
            (entry(powder_id, 0).creative_net_id.id, 1),
            (entry(potion_id, WATER).creative_net_id.id, 2),
            (entry(potion_id, WATER).creative_net_id.id, 3),
            (entry(potion_id, WATER).creative_net_id.id, 4),
            (entry(wart_id, 0).creative_net_id.id, 5),
        ];
        let mut nets = [0; 6];
        for (index, (creative, hotbar)) in picks.into_iter().enumerate() {
            let id = -1 - 2 * index as i32;
            send(&mut client, creative_to_hotbar(id, creative, hotbar)).await;
            let response = response(&mut client, id).await;
            nets[index] = response.containers.unwrap()[0].slots[0]
                .item_stack_net_id
                .as_ref()
                .unwrap()
                .id;
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

        // Place the stand on the spawn platform (top at y 15), then open it.
        let held = CerealizerNetworkItemStackDescriptorSerializedData {
            id: stand_id,
            stacksize: 1,
            auxvalue: stand.item_instance.auxvalue,
            net_id_variant: None,
            block_runtime_id: stand.item_instance.block_runtime_id as u32,
            user_data_buffer: stand.item_instance.user_data_buffer.clone(),
        };
        let platform = BlockPos { x: 2, y: 15, z: 0 };
        let stand_pos = BlockPos { x: 2, y: 16, z: 0 };
        let mut placed = false;
        for _ in 0..40 {
            send(&mut client, click_block(platform.clone(), 0, held.clone())).await;
            placed = tokio::time::timeout(Duration::from_millis(500), async {
                loop {
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
        assert!(placed, "the brewing stand was placed (spawn chunk loaded)");

        send(&mut client, click_block(stand_pos.clone(), 0, held)).await;
        let open = recv_until(&mut client, "ContainerOpen", |data| match data {
            McpePacketData::ContainerOpenPacket(open) => Some(open),
            _ => None,
        })
        .await;
        assert_eq!(
            (open.container_type, open.position.clone()),
            (4, stand_pos.clone())
        );
        let window = u32::from(open.container_id);
        let content = recv_until(&mut client, "stand content", |data| match data {
            McpePacketData::InventoryContentPacket(content) if content.container_id == window => {
                Some(content)
            }
            _ => None,
        })
        .await;
        assert_eq!(content.slots.len(), 5);

        use EnumsContainerEnumName as Name;
        let moves = [
            (1, Name::Brewingstandfuelcontainer, 4),
            (2, Name::Brewingstandresultcontainer, 1),
            (3, Name::Brewingstandresultcontainer, 2),
            (4, Name::Brewingstandresultcontainer, 3),
            (5, Name::Brewingstandinputcontainer, 0),
        ];
        for (index, (hotbar, to, at)) in moves.into_iter().enumerate() {
            let id = -21 - 2 * index as i32;
            send(
                &mut client,
                take_into(id, hotbar, nets[hotbar as usize], to, at),
            )
            .await;
            response(&mut client, id).await;
        }

        // The brew starts: 400 ticks to go, one of 20 brews used.
        let mut started = Vec::new();
        recv_until(&mut client, "brew starts", |data| match data {
            McpePacketData::ContainerSetDataPacket(data)
                if u32::from(data.container_id) == window =>
            {
                started.push((data.id, data.value));
                (started.len() == 3).then_some(())
            }
            _ => None,
        })
        .await;
        assert_eq!(started, [(0, 400), (1, 19), (2, 20)]);

        let brewed = recv_within(
            &mut client,
            "awkward potions",
            BREW_TIMEOUT,
            |data| match data {
                McpePacketData::InventoryContentPacket(content)
                    if content.container_id == window && content.slots[1].auxvalue == AWKWARD =>
                {
                    Some(content)
                }
                _ => None,
            },
        )
        .await;
        for bottle in &brewed.slots[1..4] {
            assert_eq!(
                (bottle.id, bottle.auxvalue, bottle.stacksize),
                (potion_id, AWKWARD, 1)
            );
        }
        assert_eq!(brewed.slots[0].id, 0, "the only wart was used");
        server_task.abort();
    });
}
