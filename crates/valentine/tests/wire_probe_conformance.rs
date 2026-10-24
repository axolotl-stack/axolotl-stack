#![cfg(feature = "bedrock_1_26_51")]
//! Packets exchanged with BDS 1.26.51 (offline, creative, flat world) by a
//! gophertunnel-fork client with identity `8b8d7d29-25ef-4d61-8c7d-897a10c72a91`,
//! recorded on 2026-10-24 (`fixtures/conformance/wire_probe.jsonl`, packet
//! header plus payload without the batch-entry length).
//!
//! - `player_list`: BDS's own encoding of the client's identity UUID.
//! - `craft_recipe_auto_*`: a CraftRecipeAuto request that BDS accepted (result
//!   0 with the consumed and crafted slots). The same request with one extra byte
//!   after the crafts count (an older `timesCrafted` layout) made BDS close the
//!   connection, so the layout below is the only one it parses.
//! - `container_*`: opening and closing the player's own inventory. BDS opens
//!   it with container type `INVENTORY` (-1) and answers the close with type
//!   `NONE` (-9).

use bytes::Bytes;
use valentine::bedrock::version::v1_26_51::*;

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("fixture hex"))
        .collect()
}

/// Prefixes the batch-entry length the captures omit.
fn frame(packet: &[u8]) -> Bytes {
    let mut framed = Vec::with_capacity(packet.len() + 5);
    let mut len = u32::try_from(packet.len()).unwrap();
    while len >= 0x80 {
        framed.push(len as u8 | 0x80);
        len >>= 7;
    }
    framed.push(len as u8);
    framed.extend_from_slice(packet);
    framed.into()
}

fn fixture(case: &str) -> Vec<u8> {
    include_str!("fixtures/conformance/wire_probe.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("fixture json"))
        .find(|value| value["case"] == case)
        .map(|value| hex(value["hex"].as_str().unwrap()))
        .unwrap_or_else(|| panic!("fixture {case}"))
}

/// Decodes a captured packet and checks that the owned and borrowed paths
/// consume it exactly and re-encode the captured bytes.
fn decode_exact(case: &str) -> McpePacketData {
    let packet = fixture(case);
    let (header, data) = McpePacketData::decode_inner_bytes(&mut frame(&packet), McpePacketArgs)
        .unwrap_or_else(|err| panic!("{case}: owned decode: {err:?}"));
    let mut encoded = Vec::new();
    data.encode_inner(&mut encoded, header.from_subclient, header.to_subclient)
        .unwrap();
    assert_eq!(encoded, frame(&packet), "{case}: owned re-encode");

    let (borrowed, remaining) =
        BorrowedMcpePacket::decode_inner_with_remaining(&mut frame(&packet))
            .unwrap_or_else(|err| panic!("{case}: borrowed decode: {err:?}"));
    assert_eq!(remaining, 0, "{case}: borrowed trailing bytes");
    assert_eq!(borrowed.data.into_owned(McpePacketArgs).unwrap(), data);
    data
}

#[test]
fn player_list_uuid_decodes_to_the_client_identity() {
    let McpePacketData::PlayerListPacket(list) = decode_exact("player_list") else {
        panic!("not a PlayerList");
    };
    let [PlayerListPacketEntriesItem::AddEntry(entry)] = list.entries.as_slice() else {
        panic!("one added entry expected: {:?}", list.entries.len());
    };
    assert_eq!(
        entry.uuid,
        uuid::Uuid::parse_str("8b8d7d29-25ef-4d61-8c7d-897a10c72a91").unwrap()
    );
    assert_eq!(&*entry.player_name, "WireProbe");
}

#[test]
fn accepted_craft_recipe_auto_request_has_the_bds_layout() {
    let McpePacketData::ItemStackRequestPacket(request) = decode_exact("craft_recipe_auto_request")
    else {
        panic!("not an ItemStackRequest");
    };
    let [request] = request.requests.as_slice() else {
        panic!("one request expected");
    };
    let [
        ItemStackRequestPacketDataRequestDataActionsItem::CraftRecipeAutoActionData(auto),
        ItemStackRequestPacketDataRequestDataActionsItem::ConsumeActionData(_),
        ItemStackRequestPacketDataRequestDataActionsItem::TakeActionData(_),
    ] = request.actions.as_slice()
    else {
        panic!("auto craft, consume, take expected: {:?}", request.actions);
    };
    assert_eq!(auto.recipe_net_id.raw_id, 715, "minecraft:oak_planks");
    assert_eq!(auto.numberofrequestedcrafts, 1);
    let [ingredient] = auto.ingredients.as_slice() else {
        panic!("one ingredient expected");
    };
    let ItemStackRequestCerealRecipeIngredientDataItemDescriptor::ItemNameDescriptorData(name) =
        &ingredient.item_descriptor
    else {
        panic!(
            "item name descriptor expected: {:?}",
            ingredient.item_descriptor
        );
    };
    assert_eq!(&*name.full_name, "minecraft:oak_log");
    assert_eq!(name.aux_value, 32767);
    assert_eq!(ingredient.stack_size, 1);

    let McpePacketData::ItemStackResponsePacket(response) =
        decode_exact("craft_recipe_auto_response")
    else {
        panic!("not an ItemStackResponse");
    };
    let [response] = response.responses.as_slice() else {
        panic!("one response expected");
    };
    assert_eq!(response.result, EnumsItemStackNetResult::Success);
    assert_eq!(
        response.client_request_id, request.client_request_id,
        "response answers the captured request"
    );
}

#[test]
fn inventory_container_types_are_bds_wire_values() {
    let McpePacketData::ContainerOpenPacket(open) = decode_exact("container_open_inventory") else {
        panic!("not a ContainerOpen");
    };
    assert_eq!(open.container_type as i8, -1, "INVENTORY");
    let McpePacketData::ContainerClosePacket(close) = decode_exact("container_close_reply") else {
        panic!("not a ContainerClose");
    };
    assert_eq!(close.container_type as i8, -9, "NONE");
    assert!(!close.server_initiated_close);
    // BDS answers with its own window id for the inventory, not the client's.
    assert_eq!(close.container_id, open.container_id);
    let McpePacketData::ContainerClosePacket(request) = decode_exact("container_close_request")
    else {
        panic!("not a ContainerClose");
    };
    assert_eq!(request.container_id, 0);
}
