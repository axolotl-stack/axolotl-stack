//! `/tellraw <target> <raw json message>`.
//!
//! Measured on BDS 1.26.51: each target gets a `Text` of type
//! `Textobjectwhisper` with the raw text resolved for it (see
//! [`crate::command::rawtext`]); a delivered message sends no command
//! output, like `/say`.

use crate::command::prelude::*;
use crate::command::rawtext::RawText;
use jolyne::valentine::TextPacket;
use jolyne::valentine::types::{EnumsTextPacketType, TextPacketBody, TextPacketPayloadMessageOnly};

overloads! {
    pub enum TellRawArgs for "tellraw" {
        0 => TellRaw { target: Target, message: serde_json::Value = "raw json message" },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: TellRawArgs) -> CommandResult {
    let TellRawArgs::TellRaw { target, message } = args;
    let raw = RawText::parse(&message)?;
    let players = ctx.players(Some(&target))?;
    for &player in &players {
        let message = raw.render(ctx, player);
        ctx.send(
            player,
            raw_text_packet(EnumsTextPacketType::Textobjectwhisper, message),
        );
    }
    Ok(CommandOutput::default())
}

/// A `Text` carrying rendered raw text.
pub(super) fn raw_text_packet(kind: EnumsTextPacketType, message: String) -> TextPacket {
    TextPacket {
        localize: false,
        body: TextPacketBody::MessageOnly(TextPacketPayloadMessageOnly {
            message_type: kind,
            message: message.into(),
        }),
        senders_xuid: "".into(),
        platform_id: "".into(),
        filtered_message: None,
    }
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{Replay, assert_same_packets};

    #[test]
    fn tellraw_matches_the_bds_capture() {
        let differ = [
            // BDS points the syntax error inside malformed JSON.
            "/tellraw @s {bad json",
            // BDS refuses a JSON array as a syntax error inside it.
            "/tellraw @s [1]",
        ];
        let steps = Replay::new().assert_segment("tellraw", &differ);
        assert_same_packets(&steps, &["TextPacket"], &[]);
    }
}
