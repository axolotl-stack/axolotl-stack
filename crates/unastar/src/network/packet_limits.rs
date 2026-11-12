//! Serverbound packet limits BDS 1.26.51 enforces before handling a packet.
//!
//! Measured with `tools/bds-sweep/protocol_probe.py size` (fixtures under
//! `crates/bds-extractor/captures/1.26.51/protocol/size/`): each session sent
//! longer and longer packets of one kind until BDS refused one. A refused
//! packet gets a terminating `PacketViolationWarning` (malformed packet) and
//! BDS closes the connection; it sends no `Disconnect`.
//!
//! - A `CommandRequest` whose packet (header and payload, the size its length
//!   prefix states) is over [`COMMAND_REQUEST_MAX_BYTES`] is refused before
//!   it is read: "Server-bound packet too large!". Commands, chat and book
//!   edits of any other size pass this check (`Text` and `BookEdit` packets
//!   of up to 131 KB were read).
//! - Strings over a per-field character limit (Unicode scalar values: two-
//!   and four-byte characters count once) are refused while the packet is
//!   read, with the reader's message and "readNoHeader failed!": the command
//!   request id ([`COMMAND_REQUEST_ID_MAX_CHARS`], with a message that names
//!   the command), the `Text` message ([`TEXT_MESSAGE_MAX_CHARS`]) and a
//!   `BookEdit` page, title and author ([`BOOK_TEXT_MAX_CHARS`]), whatever
//!   the book's state.

use jolyne::JolyneError;
use jolyne::raw::RawPacket;
use jolyne::valentine::types::{EnumsPacketViolationSeverity, EnumsPacketViolationType};
use jolyne::valentine::{
    BookEditPacketOperationView, BorrowedMcpePacket, BorrowedMcpePacketData, McpePacketName,
    PacketViolationWarningPacket, TextPacketBodyView,
};
use valentine::bedrock::borrowed::BorrowedStr;

/// Largest `CommandRequest` BDS reads, header included, in bytes.
pub const COMMAND_REQUEST_MAX_BYTES: usize = 550;
/// Longest command request id BDS accepts, in characters.
pub const COMMAND_REQUEST_ID_MAX_CHARS: usize = 39;
/// Longest `Text` message BDS accepts, in characters.
pub const TEXT_MESSAGE_MAX_CHARS: usize = 65_536;
/// Longest book page, title or author BDS accepts, in characters.
pub const BOOK_TEXT_MAX_CHARS: usize = 768;

/// Reads a received packet with `decode`, or gives the violation BDS refuses
/// it with: a packet too large to read, or a string field over its limit.
pub fn admit(
    raw: RawPacket,
    decode: impl FnOnce(RawPacket) -> Result<BorrowedMcpePacket, JolyneError>,
) -> Result<Result<BorrowedMcpePacket, PacketViolationWarningPacket>, JolyneError> {
    if let Some(violation) = oversized(raw.name(), raw.payload_len()) {
        return Ok(Err(violation));
    }
    let packet = decode(raw)?;
    Ok(field_violation(&packet).map_or(Ok(packet), Err))
}

fn violation(packet: McpePacketName, context: String) -> PacketViolationWarningPacket {
    PacketViolationWarningPacket {
        violation_type: EnumsPacketViolationType::Packetmalformed,
        violation_severity: EnumsPacketViolationSeverity::Terminatingconnection,
        violation_packet_id: packet as i32,
        violation_context: context.into(),
    }
}

/// The violation for a packet BDS refuses to read at all because of its
/// size, from its id and its size as [`RawPacket::payload_len`]
/// gives it.
pub fn oversized(
    packet: Option<McpePacketName>,
    size: usize,
) -> Option<PacketViolationWarningPacket> {
    let packet = packet?;
    (packet == McpePacketName::CommandRequestPacket && size > COMMAND_REQUEST_MAX_BYTES).then(
        || {
            violation(
                packet,
                format!(
                    "Server-bound packet too large! Type={} Size={size}",
                    packet as u32
                ),
            )
        },
    )
}

/// The violation for a read packet holding a string over its field's limit.
pub fn field_violation(packet: &BorrowedMcpePacket) -> Option<PacketViolationWarningPacket> {
    let message = match &packet.data {
        BorrowedMcpePacketData::CommandRequestPacket(request) => {
            over(&request.origin.request_id, COMMAND_REQUEST_ID_MAX_CHARS)
                .then(|| "Command exceeds maximum size of 512 characters.".to_owned())
        }
        BorrowedMcpePacketData::TextPacket(text) => {
            let message = match &text.body {
                TextPacketBodyView::MessageOnly(body) => &body.message,
                TextPacketBodyView::AuthorAndMessage(body) => &body.message,
                TextPacketBodyView::MessageAndParams(body) => &body.message,
            };
            too_many(message, TEXT_MESSAGE_MAX_CHARS)
        }
        BorrowedMcpePacketData::BookEditPacket(edit) => {
            let texts: &[&BorrowedStr] = match &edit.operation {
                BookEditPacketOperationView::ReplacePage(page) => &[&page.page_text],
                BookEditPacketOperationView::AddPage(page) => &[&page.page_text],
                BookEditPacketOperationView::Finalize(sign) => &[&sign.title, &sign.author],
                BookEditPacketOperationView::DeletePage(_)
                | BookEditPacketOperationView::SwapPages(_) => &[],
            };
            texts
                .iter()
                .find_map(|text| too_many(text, BOOK_TEXT_MAX_CHARS))
        }
        _ => None,
    }?;
    let id = packet.data.packet_id();
    Some(violation(
        id,
        format!("{message}\nreadNoHeader failed! packetId: {}", id as u32),
    ))
}

/// The characters (code points) of a string. Invalid UTF-8 counts each
/// invalid sequence as one replacement character, so stray continuation
/// bytes cannot slip past a limit (what BDS does with invalid UTF-8 is not
/// measured).
fn characters(text: &BorrowedStr) -> usize {
    match text.as_str() {
        Ok(text) => text.chars().count(),
        Err(_) => String::from_utf8_lossy(text.as_bytes()).chars().count(),
    }
}

fn over(text: &BorrowedStr, limit: usize) -> bool {
    characters(text) > limit
}

fn too_many(text: &BorrowedStr, limit: usize) -> Option<String> {
    over(text, limit).then(|| format!("too many characters, expected no more than {limit}"))
}

#[cfg(test)]
mod tests;
