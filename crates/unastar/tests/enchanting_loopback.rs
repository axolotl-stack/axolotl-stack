//! End to end over loopback RakNet: a saved survival player places an
//! enchanting table, opens it, puts in an iron sword and lapis, receives the
//! enchanting offers, enchants with the first one (one lapis, one level)
//! and gets the enchanted sword back when the window closes.
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
type Action = ItemStackRequestPacketDataRequestDataActionsItem;

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

fn request(id: i32, actions: Vec<Action>) -> ItemStackRequestPacket {
    ItemStackRequestPacket {
        requests: vec![ItemStackRequestPacketDataRequestData {
            client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id },
            actions,
            ..Default::default()
        }],
    }
}

fn place(
    amount: u8,
    from: ItemStackRequestCerealSlotInfoData,
    to: ItemStackRequestCerealSlotInfoData,
) -> Action {
    Action::PlaceActionData(ItemStackRequestCerealPlaceActionData {
        amount,
        source: from,
        destination: to,
        ..Default::default()
    })
}

fn consume(name: EnumsContainerEnumName, ui_slot: u8, net: i32, amount: u8) -> Action {
    Action::ConsumeActionData(ItemStackRequestCerealConsumeActionData {
        amount,
        source: slot(name, ui_slot, net),
        ..Default::default()
    })
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

async fn response(client: &mut ClientPlay, id: i32) -> ItemStackResponseInfo {
    let response = recv_until(client, "ItemStackResponse", |data| match data {
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
fn enchant_a_sword_over_loopback() {
    let directory = tempfile::tempdir().unwrap();
    std::env::set_current_dir(directory.path()).unwrap();
    let uuid = uuid::Uuid::new_v4();
    let name = "Enchanter";
    // A saved survival player with 30 levels, an enchanting table, an iron
    // sword and three lapis lazuli in the hotbar.
    let players = directory.path().join("players");
    {
        let provider = LevelDBPlayerProvider::open(players.join("db")).unwrap();
        let mut inventory = vec![StoredItemStack::default(); 36];
        inventory[0] = stored("minecraft:enchanting_table", 1);
        inventory[1] = stored("minecraft:iron_sword", 1);
        inventory[2] = stored("minecraft:lapis_lazuli", 4);
        let data = PlayerData {
            uuid: uuid.to_string(),
            game_mode: 0,
            experience: 30,
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
        let item_id = |name: &str| {
            game.item_registry
                .item_data
                .iter()
                .find(|item| &*item.item_name == name)
                .unwrap_or_else(|| panic!("{name} registered"))
                .item_id
        };
        let hotbar = recv_until(&mut client, "inventory content", |data| match data {
            McpePacketData::InventoryContentPacket(content) if content.container_id == 0 => {
                Some(content.slots)
            }
            _ => None,
        })
        .await;
        let net = |index: usize| hotbar[index].net_id_variant.expect("stack network ID");
        let (sword_net, lapis_net) = (net(1), net(2));
        send(
            &mut client,
            RequestChunkRadiusPacket {
                chunk_radius: 2,
                ..Default::default()
            },
        )
        .await;
        send(&mut client, SetLocalPlayerAsInitializedPacket::default()).await;

        // Place the table on the spawn platform (top at y 15).
        let platform = BlockPos { x: 2, y: 15, z: 0 };
        let table = BlockPos { x: 2, y: 16, z: 0 };
        let mut placed = false;
        for _ in 0..40 {
            send(
                &mut client,
                click_block(platform.clone(), 0, hotbar[0].clone()),
            )
            .await;
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
        assert!(placed, "the table was placed (spawn chunk loaded)");

        // Open it holding the sword.
        send(&mut client, select(runtime_id, 1)).await;
        send(
            &mut client,
            click_block(table.clone(), 1, hotbar[1].clone()),
        )
        .await;
        let open = recv_until(&mut client, "ContainerOpen", |data| match data {
            McpePacketData::ContainerOpenPacket(open) => Some(open),
            _ => None,
        })
        .await;
        assert_eq!((open.container_type, open.position.clone()), (3, table));
        let content = recv_until(&mut client, "table content", |data| match data {
            McpePacketData::InventoryContentPacket(content)
                if content.container_id == u32::from(open.container_id) =>
            {
                Some(content)
            }
            _ => None,
        })
        .await;
        assert_eq!(content.slots.len(), 2);

        // Sword into the table: the offers follow.
        use EnumsContainerEnumName as Name;
        send(
            &mut client,
            request(
                -1,
                vec![place(
                    1,
                    slot(Name::Hotbarcontainer, 1, sword_net),
                    slot(Name::Enchantinginputcontainer, 14, 0),
                )],
            ),
        )
        .await;
        // Measured: the options precede the response.
        let options = recv_until(&mut client, "PlayerEnchantOptions", |data| match data {
            McpePacketData::PlayerEnchantOptionsPacket(options) => Some(options.options),
            McpePacketData::ItemStackResponsePacket(_) => {
                panic!("the response came before the offers")
            }
            _ => None,
        })
        .await;
        response(&mut client, -1).await;
        assert_eq!(options.len(), 3, "three offers");
        assert!(options.iter().all(|option| option.enchants.slot == 16));
        assert!(
            options[0].cost >= 1 && options[0].cost <= 2,
            "{}",
            options[0].cost
        );
        // The cheapest offer that enchants anything.
        let (index, chosen) = options
            .iter()
            .enumerate()
            .find(|(_, option)| {
                option
                    .enchants
                    .item_enchants
                    .iter()
                    .any(|list| !list.is_empty())
            })
            .map(|(index, option)| (index as u8, option.clone()))
            .expect("an offer with enchantments");

        send(
            &mut client,
            request(
                -3,
                vec![place(
                    4,
                    slot(Name::Hotbarcontainer, 2, lapis_net),
                    slot(Name::Enchantingmaterialcontainer, 15, 0),
                )],
            ),
        )
        .await;
        response(&mut client, -3).await;

        // Enchant: one lapis and one level per offer slot.
        send(
            &mut client,
            request(
                -5,
                vec![
                    Action::CraftRecipeActionData(ItemStackRequestCerealCraftRecipeActionData {
                        recipe_net_id: chosen.enchant_net_id.clone(),
                        numberofrequestedcrafts: 1,
                        ..Default::default()
                    }),
                    consume(Name::Enchantinginputcontainer, 14, sword_net, 1),
                    consume(Name::Enchantingmaterialcontainer, 15, lapis_net, index + 1),
                    Action::TakeActionData(ItemStackRequestCerealTakeActionData {
                        amount: 1,
                        source: slot(Name::Createdoutputcontainer, 50, -5),
                        destination: slot(Name::Hotbarcontainer, 1, 0),
                        ..Default::default()
                    }),
                ],
            ),
        )
        .await;
        let (mut done, mut level) = (None, None);
        recv_until(&mut client, "enchant response and level", |data| {
            match data {
                McpePacketData::ItemStackResponsePacket(packet) => {
                    done = Some(packet.responses[0].clone());
                }
                McpePacketData::UpdateAttributesPacket(update) => {
                    if let Some(value) = update
                        .attribute_list
                        .iter()
                        .find(|a| &*a.name == "minecraft:player.level")
                        .map(|a| a.current_value)
                    {
                        level = Some(value);
                    }
                }
                McpePacketData::PlayerEnchantOptionsPacket(_) => {
                    panic!("no new offers after an enchant")
                }
                _ => {}
            }
            (done.is_some() && level.is_some()).then_some(())
        })
        .await;
        let done = done.unwrap();
        assert_eq!(done.result, EnumsItemStackNetResult::Success);
        assert_eq!(level, Some(30.0 - f32::from(index + 1)));

        // Closing returns the lapis left in the table; the inventory resync
        // shows the enchanted sword in hotbar slot 1.
        send(
            &mut client,
            ContainerClosePacket {
                container_id: open.container_id,
                container_type: 3,
                server_initiated_close: false,
            },
        )
        .await;
        let inventory = recv_until(&mut client, "inventory after close", |data| match data {
            McpePacketData::InventoryContentPacket(content) if content.container_id == 0 => {
                Some(content.slots)
            }
            _ => None,
        })
        .await;
        let sword = &inventory[1];
        assert_eq!(sword.id, item_id("minecraft:iron_sword"));
        assert!(
            sword
                .user_data_buffer
                .windows(4)
                .any(|window| window == b"ench"),
            "the sword carries an ench list"
        );
        let lapis = inventory
            .iter()
            .find(|stack| stack.id == item_id("minecraft:lapis_lazuli"))
            .expect("lapis back in the inventory");
        assert_eq!(lapis.stacksize, 3 - u16::from(index));
        server_task.abort();
    });
}
