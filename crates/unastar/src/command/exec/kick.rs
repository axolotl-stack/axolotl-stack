//! `/kick <name> <reason>`.
//!
//! Measured on BDS 1.26.51: the kicked player gets `Disconnect` with reason
//! `Kicked` and the given reason as its message; a name nobody has answers
//! `commands.generic.noTargetMatch` followed by `commands.kick.not.found
//! [name]`.

use crate::command::prelude::*;
use jolyne::valentine::DisconnectPacket;
use jolyne::valentine::types::{
    DisconnectPacketMessages, DisconnectPacketMessagesUnion, EnumsConnectionDisconnectFailReason,
};

overloads! {
    pub enum KickArgs for "kick" {
        0 => Kick { name: Target, reason: Message },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: KickArgs) -> CommandResult {
    let KickArgs::Kick { name, reason } = args;
    let players = match ctx.players(Some(&name)) {
        Ok(players) => players,
        Err(mut output) => {
            if let (Target::Name(name), [first]) = (&name, output.messages.as_slice())
                && first.key == "commands.generic.noTargetMatch"
            {
                output.add_failure("commands.kick.not.found", vec![name.clone()]);
            }
            return Err(output);
        }
    };
    let names = ctx.names(&players);
    let kicks_source = ctx.source.entity().is_some_and(|s| players.contains(&s));
    for &player in &players {
        ctx.send(
            player,
            DisconnectPacket {
                reason: EnumsConnectionDisconnectFailReason::Kicked,
                messages: DisconnectPacketMessagesUnion::DisconnectPacketMessages(
                    DisconnectPacketMessages {
                        message: reason.0.as_str().into(),
                        filtered_message: "".into(),
                    },
                ),
            },
        );
    }
    // Measured: a player who kicks itself gets no answer (it is gone).
    if kicks_source {
        return Ok(CommandOutput::default());
    }
    // [INFERENCE] BDS's answer for kicking someone else: the lang's success
    // lines (the capture has one player).
    let output = if reason.trim().is_empty() {
        CommandOutput::success("commands.kick.success", vec![names])
    } else {
        CommandOutput::success(
            "commands.kick.success.reason",
            vec![names, reason.to_string()],
        )
    };
    Ok(output.with_count(players.len()))
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{Replay, assert_same_packets};

    #[test]
    fn kick_matches_the_bds_capture() {
        let steps = Replay::new().assert_segment("kick", &[]);
        assert_same_packets(&steps, &["DisconnectPacket"], &[]);
    }
}
