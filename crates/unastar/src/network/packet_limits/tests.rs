//! The limits against the BDS 1.26.51 `size` probe
//! (`crates/bds-extractor/captures/1.26.51/protocol/size/`). Each session
//! sent longer and longer packets of one kind; the tests rebuild them as the
//! extractor built them, check their sizes against the session's
//! `sent.json`, and check that unastar admits every packet BDS read and
//! refuses the last one with BDS's violation, byte for byte.

use super::*;
use crate::server::game::client_packets::tests::{PROTOCOL, encode, files, payload};
use jolyne::valentine::types::{
    BookEditActionFinalize, BookEditActionReplacePage, BookEditPacketOperation,
    CommandOriginDatajson, TextPacketBody, TextPacketPayloadAuthorAndMessage,
    TextPacketPayloadAuthorAndMessageMessageType,
};
use jolyne::valentine::{BookEditPacket, CommandRequestPacket, McpePacket, TextPacket};

/// A `/say` command of exactly `length` UTF-8 bytes, filled with `fill`
/// (the probe's `say`).
fn say(length: usize, fill: char) -> String {
    let (count, rest) = (
        (length - 5) / fill.len_utf8(),
        (length - 5) % fill.len_utf8(),
    );
    format!(
        "/say {}{}",
        fill.to_string().repeat(count),
        "b".repeat(rest)
    )
}

/// A `CommandRequest` as the extractor sends one.
fn command(request_id: &str, command: &str) -> McpePacket {
    CommandRequestPacket {
        command: command.into(),
        origin: CommandOriginDatajson {
            type_: "player".into(),
            request_id: request_id.into(),
            ..Default::default()
        },
        is_internal: false,
        version: "latest".into(),
    }
    .into()
}

fn chat(message: &str) -> McpePacket {
    TextPacket {
        localize: false,
        body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
            message_type: TextPacketPayloadAuthorAndMessageMessageType::Chat,
            player_name: "BDSExtractor".into(),
            message: message.into(),
        }),
        senders_xuid: "".into(),
        platform_id: "".into(),
        filtered_message: None,
    }
    .into()
}

fn page(index: i32, text: &str) -> McpePacket {
    BookEditPacket {
        book_slot: 0,
        operation: BookEditPacketOperation::ReplacePage(BookEditActionReplacePage {
            page_index: index,
            page_text: text.into(),
            photo_name: "".into(),
        }),
    }
    .into()
}

fn sign(title: &str, author: &str) -> McpePacket {
    BookEditPacket {
        book_slot: 0,
        operation: BookEditPacketOperation::Finalize(BookEditActionFinalize {
            title: title.into(),
            author: author.into(),
            xuid: "".into(),
        }),
    }
    .into()
}

/// unastar's answer to one client packet: `None` when it is read.
fn answer(packet: &McpePacket) -> Option<PacketViolationWarningPacket> {
    let raw = RawPacket::from_packet(packet).unwrap();
    admit(raw, RawPacket::decode_borrowed).unwrap().err()
}

/// Checks one session: `packets` are what it sent, in order; BDS read all
/// but the last, which it refused, or (`refused` false) read them all.
fn check(session: &str, packets: &[McpePacket], refused: bool) {
    let text = std::fs::read_to_string(format!("{PROTOCOL}/size/{session}/sent.json")).unwrap();
    let sent: Vec<(u64, String, usize)> = serde_json::from_str(&text).unwrap();
    let name = format!("{:?}", packets[0].data.packet_id());
    let sizes: Vec<usize> = sent
        .into_iter()
        .filter(|(_, packet, _)| *packet == name)
        .map(|(_, _, size)| size)
        .collect();
    let ours: Vec<usize> = packets
        .iter()
        .map(|packet| RawPacket::from_packet(packet).unwrap().payload_len())
        .collect();
    assert_eq!(ours, sizes, "{session}: packet sizes");

    let (read, last) = packets.split_at(packets.len() - usize::from(refused));
    for (index, packet) in read.iter().enumerate() {
        assert!(
            answer(packet).is_none(),
            "{session}: packet {index} refused"
        );
    }
    let violations = files(&format!("size/{session}"), "PacketViolationWarningPacket");
    match last {
        [packet] => {
            let violation = answer(packet).unwrap_or_else(|| panic!("{session}: last packet read"));
            assert_eq!(violations.len(), 1, "{session}");
            assert_eq!(encode(&violation), payload(&violations[0]), "{session}");
        }
        _ => assert!(violations.is_empty(), "{session}: BDS refused a packet"),
    }
}

#[test]
fn command_requests_over_550_bytes_are_refused_unread() {
    let packets: Vec<_> = (480..=508).map(|n| command("", &say(n, 'a'))).collect();
    check("command", &packets, true);
    // Two-byte characters: the size is in bytes.
    let packets: Vec<_> = (480..=508).map(|n| command("", &say(n, 'é'))).collect();
    check("two-byte", &packets, true);
    // A long request id counts too, and its own limit does not bite at 39.
    let id = "r".repeat(39);
    let packets: Vec<_> = (8..=472)
        .step_by(8)
        .map(|n| command(&id, &say(n, 'a')))
        .collect();
    check("request-id-command", &packets, true);
}

#[test]
fn request_ids_over_39_characters_are_refused() {
    let packets: Vec<_> = (1..=40)
        .map(|n| command(&"r".repeat(n), "/say hi"))
        .collect();
    check("request-id", &packets, true);
    let packets: Vec<_> = (15..=40)
        .map(|n| command(&"é".repeat(n), "/say hi"))
        .collect();
    check("request-id-two-byte", &packets, true);
}

/// Continuation bytes alone are not free: each counts as a character.
#[test]
fn invalid_utf8_still_counts_toward_the_limit() {
    let bytes = bytes::Bytes::from(vec![0x80_u8; BOOK_TEXT_MAX_CHARS + 1]);
    assert!(over(&BorrowedStr::from(bytes), BOOK_TEXT_MAX_CHARS));
    let bytes = bytes::Bytes::from(vec![0x80_u8; BOOK_TEXT_MAX_CHARS]);
    assert!(!over(&BorrowedStr::from(bytes), BOOK_TEXT_MAX_CHARS));
}

#[test]
fn chat_over_65536_characters_is_refused() {
    let lengths = [
        100, 256, 257, 300, 500, 512, 540, 550, 560, 600, 1000, 4000, 20000, 100000,
    ];
    let packets: Vec<_> = lengths.map(|n| chat(&"c".repeat(n))).to_vec();
    check("chat", &packets, true);
    let packets: Vec<_> = [65535, 65536, 65537].map(|n| chat(&"c".repeat(n))).to_vec();
    check("chat-limit", &packets, true);
    // Characters, not bytes: 65536 two-byte characters are read.
    let packets: Vec<_> = [32768, 32769, 65536].map(|n| chat(&"é".repeat(n))).to_vec();
    check("chat-two-byte", &packets, false);
}

#[test]
fn book_text_over_768_characters_is_refused() {
    let pages = |fill: &str, lengths: &[usize]| -> Vec<McpePacket> {
        lengths
            .iter()
            .enumerate()
            .map(|(index, n)| page(index as i32, &fill.repeat(*n)))
            .collect()
    };
    check(
        "book",
        &pages("p", &[258, 300, 520, 540, 560, 600, 1000]),
        true,
    );
    check("book-limit", &pages("p", &[765, 766, 767, 768, 769]), true);
    check(
        "book-two-byte",
        &pages("é", &[383, 384, 385, 767, 768, 769]),
        true,
    );
    check(
        "book-four-byte",
        &pages("\u{1F600}", &[384, 385, 768, 769]),
        true,
    );
    let lengths = [100, 256, 512, 768, 769];
    let titles: Vec<_> = lengths.map(|n| sign(&"t".repeat(n), "Author")).to_vec();
    check("book-title", &titles, true);
    let authors: Vec<_> = lengths.map(|n| sign("Title", &"a".repeat(n))).to_vec();
    check("book-author", &authors, true);
}
