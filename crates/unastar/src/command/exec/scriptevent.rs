//! `/scriptevent <messageId> <message>`.
//!
//! Measured on BDS 1.26.51: an identifier with exactly one `:`, a namespace
//! other than `minecraft` and a name answers the literal `Script event %%s
//! has been sent` with the identifier as its parameter (an empty message
//! too); any other identifier answers `commands.scriptevent.invalidNamespace`.
//!
//! unastar runs no behaviour-pack scripts, so nothing receives the event.

use crate::command::prelude::*;

overloads! {
    pub enum ScriptEventArgs for "scriptevent" {
        0 => Send { message_id: Text = "messageId", message: Message },
    }
}

pub fn run(_ctx: &mut CommandContext<'_>, args: ScriptEventArgs) -> CommandResult {
    let ScriptEventArgs::Send { message_id, .. } = args;
    let valid = match message_id.split_once(':') {
        Some((namespace, name)) => {
            !namespace.is_empty()
                && !name.is_empty()
                && !name.contains(':')
                && namespace != "minecraft"
        }
        None => false,
    };
    if !valid {
        return failure!("commands.scriptevent.invalidNamespace");
    }
    success!("Script event %%s has been sent", &*message_id)
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::Replay;

    #[test]
    fn scriptevent_matches_the_bds_capture() {
        Replay::new().assert_segment("scriptevent", &[]);
    }
}
