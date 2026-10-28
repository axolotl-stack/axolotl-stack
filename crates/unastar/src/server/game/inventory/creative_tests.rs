//! Taking items out of the creative menu through a real `GameServer`, with the
//! request shapes a 1.26.51 client sends: `CraftCreative` followed by a `Take`
//! to the cursor or `Place` actions into hotbar/inventory slots, sourced from
//! the created-output slot (50) named by the request's own ID.
//!
//! The creative content is the server's real advertisement, so block items
//! carry hashed wire block IDs exactly as the client received them.

use super::InventoryAction;
use crate::entity::components::{GameMode, MainInventory, PlayerUi};
use crate::registry::block::BlockNetworkIds;
use jolyne::valentine::*;

type Name = EnumsContainerEnumName;
type Action = ItemStackRequestPacketDataRequestDataActionsItem;

fn info(container_name: Name, slot: u8, net_id_variant: i32) -> ItemStackRequestCerealSlotInfoData {
    ItemStackRequestCerealSlotInfoData {
        fullcontainername: FullContainerName {
            container_name,
            dynamic_id: None,
        },
        slot,
        net_id_variant,
    }
}

fn craft(creative_item_net_id: u32) -> Action {
    Action::CraftCreativeActionData(ItemStackRequestCerealCraftCreativeActionData {
        creative_item_net_id,
        numberofrequestedcrafts: 1,
        ..Default::default()
    })
}

/// The created output as the client names it: slot 50, net ID = request ID.
fn created(request: i32) -> ItemStackRequestCerealSlotInfoData {
    info(Name::Createdoutputcontainer, 50, request)
}

fn take(amount: u8, source: ItemStackRequestCerealSlotInfoData, destination: ItemStackRequestCerealSlotInfoData) -> Action {
    Action::TakeActionData(ItemStackRequestCerealTakeActionData {
        amount,
        source,
        destination,
        ..Default::default()
    })
}

fn place(amount: u8, source: ItemStackRequestCerealSlotInfoData, destination: ItemStackRequestCerealSlotInfoData) -> Action {
    Action::PlaceActionData(ItemStackRequestCerealPlaceActionData {
        amount,
        source,
        destination,
        ..Default::default()
    })
}

struct Session {
    server: crate::GameServer,
    player: bevy_ecs::entity::Entity,
    rx: tokio::sync::mpsc::Receiver<McpePacket>,
}

impl Session {
    fn creative_net_id(&self, item: &str) -> u32 {
        let world = self.server.ecs.world();
        let network_id = world
            .resource::<super::super::types::ItemRegistryResource>()
            .0
            .get_by_name(item)
            .unwrap()
            .network_id();
        world
            .resource::<super::super::types::ServerWorldTemplate>()
            .0
            .creative_content
            .entries
            .iter()
            .find(|entry| entry.item_instance.id == network_id && entry.item_instance.auxvalue == 0)
            .unwrap_or_else(|| panic!("{item} is in the creative menu"))
            .creative_net_id
            .id
    }

    fn send(&mut self, action: InventoryAction) {
        let world = self.server.ecs.world_mut();
        world
            .resource_mut::<super::InventoryPacketQueue>()
            .0
            .push((self.player, action));
        self.server.tick();
    }

    /// One request; returns its response status.
    fn request(&mut self, id: i32, actions: Vec<Action>) -> EnumsItemStackNetResult {
        while self.rx.try_recv().is_ok() {}
        self.send(InventoryAction::ItemStackRequest(ItemStackRequestPacket {
            requests: vec![ItemStackRequestPacketDataRequestData {
                client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id },
                actions,
                ..Default::default()
            }],
        }));
        let responses: Vec<_> = std::iter::from_fn(|| self.rx.try_recv().ok())
            .filter_map(|packet| match packet.data {
                McpePacketData::ItemStackResponsePacket(packet) => Some(packet),
                _ => None,
            })
            .collect();
        assert_eq!(responses.len(), 1, "one response per request");
        assert_eq!(responses[0].responses[0].client_request_id.id, id);
        responses[0].responses[0].result
    }

    fn slot(&self, slot: usize) -> (String, u8, i32, i32) {
        let stack = self
            .server
            .ecs
            .world()
            .get::<MainInventory>(self.player)
            .unwrap()
            .0
            .item(slot)
            .unwrap()
            .clone();
        (stack.item_id().to_owned(), stack.count, stack.runtime_id, stack.net_id)
    }

    fn cursor(&self) -> (String, u8, i32, i32) {
        let world = self.server.ecs.world();
        let stack = world.get::<PlayerUi>(self.player).unwrap().cursor();
        (stack.item_id().to_owned(), stack.count, stack.runtime_id, stack.net_id)
    }
}

fn with_creative_player(test: impl FnOnce(Session) + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let _guard = runtime.enter();
            let mut world = crate::world::WorldConfig::void_spawn_platform(1);
            world.generation_threads = 1;
            let mut server = crate::GameServer::with_config(crate::ServerConfig {
                default_gamemode: GameMode::Creative,
                world,
                ..Default::default()
            });
            let (tx, rx) = tokio::sync::mpsc::channel(256);
            let player = server
                .spawn_player(crate::PlayerSpawnData {
                    session_id: 21,
                    display_name: "creative".into(),
                    xuid: None,
                    uuid: Some(uuid::Uuid::from_u128(21).to_string()),
                    runtime_id: 51,
                    position: glam::DVec3::new(0.5, 17.0, 0.5),
                    saved_data: None,
                    outbound_tx: tx,
                    chunk_radius: 0,
                })
                .unwrap();
            let mut session = Session { server, player, rx };
            // Pressing E: the client opens its (creative) inventory screen.
            session.send(InventoryAction::Interact(InteractPacket {
                action: EnumsInteractPacketPayloadAction::Openinventory,
                ..Default::default()
            }));
            test(session);
        })
        .unwrap()
        .join()
        .unwrap();
}

fn stone_state() -> i32 {
    crate::registry::BlockRegistry::vanilla()
        .get_by_name("minecraft:stone")
        .unwrap()
        .default_state_id as i32
}

#[test]
fn creative_menu_take_to_cursor_then_place_full_stack() {
    with_creative_player(|mut s| {
        let stone = s.creative_net_id("minecraft:stone");
        // Click a menu item: the full stack goes to the cursor.
        let result = s.request(
            -1,
            vec![craft(stone), take(64, created(-1), info(Name::Cursorcontainer, 0, 0))],
        );
        assert_eq!(result, EnumsItemStackNetResult::Success);
        let (item, count, block, cursor_id) = s.cursor();
        assert_eq!((item.as_str(), count), ("minecraft:stone", 64));
        // The hashed creative block ID decodes to the canonical stone state.
        assert_eq!(block, stone_state());
        // Click an empty hotbar slot: the cursor stack lands there.
        let result = s.request(
            -3,
            vec![place(
                64,
                info(Name::Cursorcontainer, 0, cursor_id),
                info(Name::Hotbarcontainer, 2, 0),
            )],
        );
        assert_eq!(result, EnumsItemStackNetResult::Success);
        assert_eq!(s.cursor().1, 0);
        assert_eq!(s.slot(2), ("minecraft:stone".to_owned(), 64, stone_state(), cursor_id));
    });
}

#[test]
fn creative_menu_places_into_empty_and_occupied_slots_and_splits_shift_clicks() {
    with_creative_player(|mut s| {
        let diamond = s.creative_net_id("minecraft:diamond");
        // Right-click take of one item straight into an empty inventory slot.
        let result = s.request(
            -1,
            vec![craft(diamond), place(1, created(-1), info(Name::Inventorycontainer, 9, 0))],
        );
        assert_eq!(result, EnumsItemStackNetResult::Success);
        let (item, count, _, first_id) = s.slot(9);
        assert_eq!((item.as_str(), count), ("minecraft:diamond", 1));
        // Another onto the occupied stack, named by its stack ID.
        let result = s.request(
            -3,
            vec![craft(diamond), place(1, created(-3), info(Name::Inventorycontainer, 9, first_id))],
        );
        assert_eq!(result, EnumsItemStackNetResult::Success);
        assert_eq!(s.slot(9).1, 2);
        // Shift-click a full stack: top up the partial stack, rest to a free slot.
        let result = s.request(
            -5,
            vec![
                craft(diamond),
                place(62, created(-5), info(Name::Inventorycontainer, 9, first_id)),
                place(2, created(-5), info(Name::Hotbarcontainer, 0, 0)),
            ],
        );
        assert_eq!(result, EnumsItemStackNetResult::Success);
        assert_eq!(s.slot(9).1, 64);
        let (item, count, _, _) = s.slot(0);
        assert_eq!((item.as_str(), count), ("minecraft:diamond", 2));
        // The combined container names the same slots.
        let result = s.request(
            -7,
            vec![craft(diamond), take(64, created(-7), info(Name::Combinedhotbarandinventorycontainer, 35, 0))],
        );
        assert_eq!(result, EnumsItemStackNetResult::Success);
        assert_eq!(s.slot(35).1, 64);
    });
}

#[test]
fn creative_stacks_resync_with_the_advertised_hashed_block_id() {
    with_creative_player(|mut s| {
        let stone = s.creative_net_id("minecraft:stone");
        assert_eq!(
            s.request(-1, vec![craft(stone), place(64, created(-1), info(Name::Hotbarcontainer, 0, 0))]),
            EnumsItemStackNetResult::Success
        );
        let advertised = s
            .server
            .ecs
            .world()
            .resource::<super::super::types::ServerWorldTemplate>()
            .0
            .creative_content
            .entries
            .iter()
            .find(|entry| entry.creative_net_id.id == stone)
            .unwrap()
            .item_instance
            .block_runtime_id as u32;
        assert_eq!(advertised, BlockNetworkIds::Hashed.to_wire(stone_state() as u32));
        let stack = s
            .server
            .ecs
            .world()
            .get::<MainInventory>(s.player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .clone();
        let wire = super::wire_item(&stack, BlockNetworkIds::Hashed).unwrap();
        assert_eq!(wire.block_runtime_id, advertised);
    });
}
