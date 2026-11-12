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
        let steps = Replay::new().assert_segment("tellraw", &[]);
        assert_same_packets(&steps, &["TextPacket"], &[]);
    }
}

/// Malformed and odd JSON messages of the BDS 1.26.51 command probe
/// (`testdata/bds-command-probe.json`): where the syntax error points, the
/// JSON reader's errors, and the message's own refusals.
#[cfg(test)]
mod probe {
    use crate::command::exec::probe::{Probe, captured_player, player_commands};

    #[test]
    fn json_messages_answer_like_bds() {
        let mut probe = Probe::new();
        let player = probe.player();
        let commands = player_commands("/tellraw @s ");
        assert!(commands.len() > 50, "the probe's /tellraw lines");
        for command in commands {
            let output = probe.h.run(player, &command);
            assert_eq!(output.messages, captured_player(&command), "{command}");
        }
    }

    /// Syntax errors outside JSON: `{ } [ ] , =` are tokens of their own.
    #[test]
    fn syntax_errors_point_at_the_token_bds_shows() {
        let mut probe = Probe::new();
        let player = probe.player();
        for command in [
            "/testfor @e[type=cow,bogus=1]",
            "/kill @e[typ=cow]",
            "/give @s stone{x}",
            "/give @s stone[",
            "/give @s stone,",
            "/give @s {stone}",
            "/setblock 0 -60 0 stone [\"a\"=1]x",
        ] {
            let output = probe.h.run(player, command);
            assert_eq!(output.messages, captured_player(command), "{command}");
        }
    }
}
