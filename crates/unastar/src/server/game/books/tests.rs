//! Book editing against the BDS 1.26.51 `book` probe fixtures
//! (`crates/bds-extractor/captures/1.26.51/protocol/book/`): the sessions
//! `book` (survival), `creative`, `limits`, `gate` and `unopened`.

use super::*;
use crate::entity::components::HeldSlot;
use crate::server::game::client_packets::tests::{PROTOCOL, decode, encode, files, payload, seq};
use crate::server::game::inventory::validated_item;
use crate::server::game::item_use::{ItemUseQueue, UseRequest, apply_item_use};
use crate::server::game::types::{ItemRegistryResource, block_network_ids};
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use bevy_ecs::system::RunSystemOnce;
use jolyne::valentine::{InventoryContentPacket, McpePacketData};
use tokio::sync::mpsc;

const RUN: &str = "book";

fn server() -> GameServer {
    GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    })
}

/// Whether BDS answered the client packet `sent` with an
/// `InventoryTransaction` before the client's next scripted packet.
fn answered(session: &str, sent: u64) -> bool {
    let path = format!("{PROTOCOL}/{RUN}/{session}/timeline.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let timeline: Vec<(u64, f64, String, String)> = serde_json::from_str(&text).unwrap();
    timeline
        .into_iter()
        .skip_while(|(seq, ..)| *seq <= sent)
        .take_while(|(_, _, dir, name)| dir == "s2c" || name == "NetworkStackLatencyPacket")
        .any(|(_, _, dir, name)| dir == "s2c" && name == "InventoryTransactionPacket")
}

fn actions(packet: &InventoryTransactionPacket) -> &[InventoryAction] {
    let InventoryTransactionPacketTransaction::NormalTransactionData(normal) = &packet.transaction
    else {
        panic!("normal transaction expected");
    };
    &normal.actions.actions
}

/// Every edit BDS accepted, rebuilt from the book it started from: the edit,
/// the new book's NBT and the answering transaction match byte for byte.
/// Edits BDS refused with a book open in that slot are refused too.
#[test]
fn edits_match_the_bds_book_capture_byte_for_byte() {
    let server = server();
    let world = server.ecs.world();
    let books = world.resource::<BookItems>();
    let items = &world.resource::<ItemRegistryResource>().0;
    let block_ids = block_network_ids(world);
    let mut accepted = 0;
    let mut refused = 0;
    for session in ["book", "creative", "limits", "gate"] {
        let edits = files(&format!("{RUN}/{session}/sent"), "BookEditPacket");
        let answers = files(&format!("{RUN}/{session}"), "InventoryTransactionPacket");
        let mut book: Option<ItemStack> = None;
        for edit_file in &edits {
            let packet: BookEditPacket = decode(edit_file);
            let answer = answers.iter().find(|answer| {
                seq(answer) > seq(edit_file)
                    && edits
                        .iter()
                        .all(|other| seq(other) <= seq(edit_file) || seq(other) > seq(answer))
            });
            let Some(answer) = answer else {
                // Refusals of an open book (the gate is checked below).
                if let Some(before) = &book
                    && packet.book_slot == 0
                    && matches!(session, "book" | "limits")
                {
                    assert_eq!(edit(books, before, &packet.operation), None, "{edit_file}");
                    refused += 1;
                }
                continue;
            };
            assert!(answered(session, seq(edit_file)), "{edit_file}");
            let native: InventoryTransactionPacket = decode(answer);
            let slot_action = &actions(&native)[0];
            let before = validated_item(&slot_action.from_item, items, block_ids).unwrap();
            let after = edit(books, &before, &packet.operation)
                .unwrap_or_else(|| panic!("{edit_file} refused"))
                .with_net_id(slot_action.to_item.net_id_variant.unwrap());
            let ours = edit_transaction(slot_action.slot, &before, &after, block_ids).unwrap();
            assert_eq!(encode(&ours), payload(answer), "{edit_file} -> {answer}");
            book = Some(after);
            accepted += 1;
        }
    }
    assert_eq!((accepted, refused), (23, 6));
}

fn spawn(server: &mut GameServer) -> (Entity, mpsc::Receiver<McpePacket>) {
    let (outbound_tx, rx) = mpsc::channel(1024);
    let entity = server
        .spawn_player(PlayerSpawnData {
            session_id: 1,
            display_name: "BookWriter".into(),
            xuid: None,
            uuid: Some(uuid::Uuid::from_u128(5).to_string()),
            runtime_id: 1,
            position: glam::DVec3::new(0.5, 17.0, 0.5),
            outbound_tx,
            saved_data: None,
            chunk_radius: 2,
        })
        .unwrap();
    (entity, rx)
}

fn put_book(server: &mut GameServer, player: Entity, slot: usize) {
    let world = server.ecs.world_mut();
    let book = ItemStack::new(world.resource::<BookItems>().writable.clone(), 1).into_slot();
    let mut inventory = world.get_mut::<MainInventory>(player).unwrap();
    inventory.0.set_item(slot, book).unwrap();
}

fn use_book(server: &mut GameServer, player: Entity, slot: u8) {
    let world = server.ecs.world_mut();
    world.entity_mut(player).insert(HeldSlot(slot));
    world
        .get_resource_or_init::<ItemUseQueue>()
        .0
        .push((player, UseRequest::UseAir));
    apply_item_use(world);
}

fn book_edit(slot: i32, operation: Operation) -> BookEditPacket {
    BookEditPacket {
        book_slot: slot,
        operation,
    }
}

fn replace(text: &str) -> Operation {
    Operation::ReplacePage(jolyne::valentine::types::BookEditActionReplacePage {
        page_index: 0,
        page_text: text.into(),
        photo_name: "".into(),
    })
}

fn drain(rx: &mut mpsc::Receiver<McpePacket>) -> Vec<McpePacketData> {
    std::iter::from_fn(|| rx.try_recv().ok().map(|packet| packet.data)).collect()
}

/// Opening a book gives it the empty page list BDS gives it and resends the
/// inventory: the four containers, then the hotbar.
#[test]
fn opening_a_book_stores_an_empty_page_list_like_bds() {
    let mut server = server();
    let (player, mut rx) = spawn(&mut server);
    put_book(&mut server, player, 0);
    drain(&mut rx);
    use_book(&mut server, player, 0);
    let sent = drain(&mut rx);
    let names: Vec<_> = sent.iter().map(McpePacketData::packet_id).collect();
    use jolyne::valentine::McpePacketName as N;
    assert_eq!(
        names,
        [
            N::InventoryContentPacket,
            N::InventoryContentPacket,
            N::InventoryContentPacket,
            N::InventoryContentPacket,
            N::PlayerHotbarPacket
        ]
    );
    let native: InventoryContentPacket =
        decode(&format!("{RUN}/book/01631-InventoryContentPacket.bin"));
    let McpePacketData::InventoryContentPacket(ours) = &sent[0] else {
        unreachable!()
    };
    assert_eq!(
        ours.slots[0].user_data_buffer,
        native.slots[0].user_data_buffer
    );
    assert_eq!(
        server.ecs.world().get::<OpenBook>(player),
        Some(&OpenBook { slot: 0 })
    );
}

/// The `gate` session: which edits BDS answered as the player selects other
/// slots, signs, gets a new book and opens a second one.
#[test]
fn only_the_last_opened_book_is_edited_like_bds() {
    let native: Vec<bool> = files(&format!("{RUN}/gate/sent"), "BookEditPacket")
        .iter()
        .map(|edit| answered("gate", seq(edit)))
        .collect();
    let unopened: Vec<bool> = files(&format!("{RUN}/unopened/sent"), "BookEditPacket")
        .iter()
        .map(|edit| answered("unopened", seq(edit)))
        .collect();
    assert!(unopened.iter().all(|answered| !answered));

    let mut server = server();
    let (player, mut rx) = spawn(&mut server);
    put_book(&mut server, player, 0);
    put_book(&mut server, player, 1);
    // Before any book is opened, nothing is edited.
    let world = server.ecs.world_mut();
    apply_book_edit(world, player, &book_edit(0, replace("unopened")));
    drain(&mut rx);
    assert!(
        pages(
            world
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
        )
        .is_none()
    );

    let mut ours = Vec::new();
    let mut edit = |server: &mut GameServer, rx: &mut mpsc::Receiver<McpePacket>, slot, op| {
        apply_book_edit(server.ecs.world_mut(), player, &book_edit(slot, op));
        ours.push(
            drain(rx)
                .iter()
                .any(|packet| matches!(packet, McpePacketData::InventoryTransactionPacket(_))),
        );
    };
    use_book(&mut server, player, 0);
    drain(&mut rx);
    edit(&mut server, &mut rx, 0, replace("opened"));
    server
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert(HeldSlot(1));
    edit(&mut server, &mut rx, 0, replace("other slot selected"));
    edit(&mut server, &mut rx, 1, replace("never opened"));
    server
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert(HeldSlot(0));
    edit(&mut server, &mut rx, 0, replace("selected again"));
    edit(
        &mut server,
        &mut rx,
        0,
        Operation::Finalize(jolyne::valentine::types::BookEditActionFinalize {
            title: "GateTitle".into(),
            author: "GateAuthor".into(),
            xuid: "".into(),
        }),
    );
    put_book(&mut server, player, 0);
    edit(&mut server, &mut rx, 0, replace("new book"));
    use_book(&mut server, player, 1);
    drain(&mut rx);
    edit(&mut server, &mut rx, 1, replace("opened second"));
    edit(
        &mut server,
        &mut rx,
        0,
        replace("first after second opened"),
    );
    assert_eq!(ours, native);
}

#[test]
fn pages_pad_and_cap_at_fifty() {
    let add = |index, text: &str| {
        Operation::AddPage(jolyne::valentine::types::BookEditActionAddPage {
            page_index: index,
            page_text: text.into(),
            photo_name: "".into(),
        })
    };
    let padded = edit_pages(Vec::new(), &add(3, "far")).unwrap();
    assert_eq!(padded.len(), 4);
    assert_eq!(padded[3].text, "far");
    let full = vec![Page::default(); MAX_PAGES];
    assert_eq!(edit_pages(full.clone(), &add(0, "one too many")), None);
    assert_eq!(edit_pages(Vec::new(), &add(50, "past the end")), None);
    let swap = Operation::SwapPages(jolyne::valentine::types::BookEditActionSwapPages {
        page_index: 0,
        swap_with_index: 1,
    });
    assert_eq!(edit_pages(vec![Page::default()], &swap), None);
}

/// The player's own interact text updates of the `book-held` session, in
/// order: (fixture, text).
fn native_interact_texts() -> Vec<(String, String)> {
    use jolyne::valentine::types::DataItemEntryPayload;
    files("book-held/held", "SetActorDataPacket")
        .into_iter()
        .filter_map(|file| {
            let packet: SetActorDataPacket = decode(&file);
            match &packet.actor_data.data[..] {
                [entry] if entry.id == actor_data_ids::INTERACT_TEXT => {
                    let DataItemEntryPayload::DataItemStringPayload(text) = &entry.payload else {
                        panic!("{file}: interact text is not a string");
                    };
                    Some((file, text.value.to_string()))
                }
                _ => None,
            }
        })
        .collect()
}

/// The `book-held` session: holding a book and quill, another item, the
/// signed book, and the held slot replaced by command. unastar sends the
/// same texts in the same order, byte for byte, and nothing when the text
/// does not change.
#[test]
fn the_interact_text_follows_the_held_book_like_bds() {
    let native = native_interact_texts();
    let mut server = server();
    let (player, mut rx) = spawn(&mut server);
    let stone = {
        let world = server.ecs.world();
        let items = &world.resource::<ItemRegistryResource>().0;
        ItemStack::new(items.item_type("minecraft:stone").unwrap(), 1).into_slot()
    };
    let mut ours = Vec::new();
    let mut step = |server: &mut GameServer, change: &dyn Fn(&mut World)| {
        change(server.ecs.world_mut());
        drain(&mut rx);
        server
            .ecs
            .world_mut()
            .run_system_once(sync_interact_text)
            .unwrap();
        for packet in drain(&mut rx) {
            let McpePacketData::SetActorDataPacket(packet) = packet else {
                continue;
            };
            ours.push(packet);
        }
    };
    let select = |slot: u8| {
        move |world: &mut World| {
            world.entity_mut(player).insert(HeldSlot(slot));
        }
    };
    let set = |slot: usize, stack: ItemStack| {
        move |world: &mut World| {
            let mut inventory = world.get_mut::<MainInventory>(player).unwrap();
            inventory.0.set_item(slot, stack.clone()).unwrap();
        }
    };
    step(&mut server, &set(1, stone.clone()));
    step(&mut server, &select(1));
    put_book(&mut server, player, 0);
    step(&mut server, &select(1));
    step(&mut server, &select(0));
    step(&mut server, &select(1));
    step(&mut server, &select(0));
    use_book(&mut server, player, 0);
    apply_book_edit(
        server.ecs.world_mut(),
        player,
        &book_edit(0, replace("held")),
    );
    apply_book_edit(
        server.ecs.world_mut(),
        player,
        &book_edit(
            0,
            Operation::Finalize(jolyne::valentine::types::BookEditActionFinalize {
                title: "HeldTitle".into(),
                author: "HeldAuthor".into(),
                xuid: "".into(),
            }),
        ),
    );
    step(&mut server, &|_| {});
    step(&mut server, &select(1));
    step(&mut server, &select(0));
    step(&mut server, &set(0, stone.clone()));
    let book = {
        let world = server.ecs.world();
        ItemStack::new(world.resource::<BookItems>().writable.clone(), 1).into_slot()
    };
    step(&mut server, &set(0, book));

    assert_eq!(ours.len(), native.len());
    for (packet, (file, _)) in ours.iter_mut().zip(&native) {
        let expected: SetActorDataPacket = decode(file);
        packet.target_runtime_id = expected.target_runtime_id.clone();
        packet.tick = expected.tick.clone();
        assert_eq!(encode(packet), payload(file), "{file}");
    }
}

/// Pages past the client's 256 characters, up to BDS's 768-character read
/// limit (`size` probe: `book`, `book-limit`, `book-two-byte`,
/// `book-four-byte`): every page BDS read was written and answered like
/// BDS, byte for byte. Longer pages never reach the book (refused unread,
/// `crate::network::packet_limits`).
#[test]
fn long_pages_are_written_like_bds() {
    let server = server();
    let world = server.ecs.world();
    let books = world.resource::<BookItems>();
    let items = &world.resource::<ItemRegistryResource>().0;
    let block_ids = block_network_ids(world);
    let sessions: [(&str, &str, &[usize]); 4] = [
        ("book", "p", &[258, 300, 520, 540, 560, 600]),
        ("book-limit", "p", &[765, 766, 767, 768]),
        ("book-two-byte", "é", &[383, 384, 385, 767, 768]),
        ("book-four-byte", "\u{1F600}", &[384, 385, 768]),
    ];
    for (session, fill, lengths) in sessions {
        let answers = files(&format!("size/{session}"), "InventoryTransactionPacket");
        assert_eq!(answers.len(), lengths.len(), "{session}");
        for (page, (answer, length)) in answers.iter().zip(lengths).enumerate() {
            let native: InventoryTransactionPacket = decode(answer);
            let slot_action = &actions(&native)[0];
            let before = validated_item(&slot_action.from_item, items, block_ids).unwrap();
            let operation =
                Operation::ReplacePage(jolyne::valentine::types::BookEditActionReplacePage {
                    page_index: page as i32,
                    page_text: fill.repeat(*length).into(),
                    photo_name: "".into(),
                });
            let after = edit(books, &before, &operation)
                .unwrap_or_else(|| panic!("{answer} refused"))
                .with_net_id(slot_action.to_item.net_id_variant.unwrap());
            let ours = edit_transaction(slot_action.slot, &before, &after, block_ids).unwrap();
            assert_eq!(encode(&ours), payload(answer), "{answer}");
        }
    }
}
