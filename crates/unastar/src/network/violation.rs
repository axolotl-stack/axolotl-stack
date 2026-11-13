//! `PacketViolationWarning` packets unastar sends when it refuses a serverbound
//! packet.
//!
//! BDS 1.26.51 ends every such message with a line naming its internal packet
//! reader, `<reader> failed! packetId: N`. unastar omits the reader's failure
//! suffix and keeps only `packetId: N` (owner decision 2026-11-12; see the
//! "Network / packet violations" row in `docs/quirks/bds.md`). Every warning is
//! built by [`packet_violation`], so no caller can put the suffix back.

use jolyne::valentine::{
    EnumsPacketViolationSeverity, EnumsPacketViolationType, PacketViolationWarningPacket,
};

/// What follows the reader name on BDS's last message line.
const READER_FAILURE: &str = " failed! ";

/// Builds the warning for `packet_id`. `detail` is the human-readable line BDS
/// sends before its reader-failure line (for example
/// `expected a valid ItemStackRequestId`); without one the message is just
/// `packetId: N`.
pub fn packet_violation(
    violation_type: EnumsPacketViolationType,
    violation_severity: EnumsPacketViolationSeverity,
    packet_id: i32,
    detail: Option<&str>,
) -> PacketViolationWarningPacket {
    let packet_line = format!("packetId: {packet_id}");
    PacketViolationWarningPacket {
        violation_type,
        violation_severity,
        violation_packet_id: packet_id,
        violation_context: match detail {
            Some(detail) => format!("{detail}\n{packet_line}"),
            None => packet_line,
        }
        .into(),
    }
}

/// Removes BDS's reader-failure suffix from a captured violation message, so
/// it can be compared with what unastar sends: a line
/// `<reader> failed! packetId: N` becomes `packetId: N`. Other text is kept.
pub fn redact_violation_message(message: &str) -> String {
    message
        .split('\n')
        .map(|line| match line.split_once(READER_FAILURE) {
            Some((reader, rest))
                if rest.starts_with("packetId: ") && !reader.contains(char::is_whitespace) =>
            {
                rest
            }
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `message` still carries a reader-failure suffix.
pub fn has_reader_failure(message: &str) -> bool {
    redact_violation_message(message) != message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_suffix_is_redacted_to_the_packet_id() {
        let captured = "expected a valid ItemStackRequestId\nreadPacket failed! packetId: 147";
        assert_eq!(
            redact_violation_message(captured),
            "expected a valid ItemStackRequestId\npacketId: 147"
        );
        assert!(has_reader_failure(captured));
        assert_eq!(
            redact_violation_message("anyReader failed! packetId: 9"),
            "packetId: 9"
        );
    }

    #[test]
    fn other_text_is_left_alone() {
        for message in ["expected a valid id", "two words failed! packetId: 1", ""] {
            assert_eq!(redact_violation_message(message), message);
        }
    }

    #[test]
    fn built_messages_never_carry_the_suffix() {
        for detail in [None, Some("expected a valid ItemStackRequestId")] {
            let warning = packet_violation(
                EnumsPacketViolationType::Packetmalformed,
                EnumsPacketViolationSeverity::Terminatingconnection,
                147,
                detail,
            );
            assert!(!has_reader_failure(&warning.violation_context));
            assert!(warning.violation_context.ends_with("packetId: 147"));
        }
    }
}
