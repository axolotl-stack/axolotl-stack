//! `/title` and `/titleraw`: `SetTitle` to each target player.
//!
//! Measured on BDS 1.26.51: every form answers
//! `commands.title.success` / `commands.titleraw.success` with the targets'
//! names (an empty `/title` text answers `commands.generic.invalidMessage`); text forms send their text with all fade times -1, `times` sends
//! the three times with an empty text, `clear` and `reset` an empty text and
//! -1 times. `/title` text has its selectors replaced by names (`hi @s` →
//! `hi BDSExtractor`); `/titleraw` text is raw JSON text resolved for each
//! target (`Titletextobject`, …).

use super::chat::expand;
use crate::command::prelude::*;
use crate::command::rawtext::RawText;
use jolyne::valentine::SetTitlePacket;
use jolyne::valentine::types::EnumsSetTitlePacketPayloadTitleType as TitleType;

arg_enum! {
    /// Captured `TitleSet`.
    pub enum TitleSlot in "TitleSet" {
        Title = "title",
        Subtitle = "subtitle",
        Actionbar = "actionbar",
    }
}

arg_enum! {
    /// Captured `TitleRawSet`.
    pub enum TitleRawSlot in "TitleRawSet" {
        Title = "title",
        Subtitle = "subtitle",
        Actionbar = "actionbar",
    }
}

overloads! {
    pub enum TitleArgs for "title" {
        0 => Clear { player: Target, clear: EnumValue },
        1 => Reset { player: Target, reset: EnumValue },
        2 => Set { player: Target, location: TitleSlot = "titleLocation", text: Message = "titleText" },
        3 => Times {
            player: Target,
            times: EnumValue,
            fade_in: i32 = "fadeIn",
            stay: i32,
            fade_out: i32 = "fadeOut",
        },
    }
}

overloads! {
    pub enum TitleRawArgs for "titleraw" {
        0 => Clear { player: Target, clear: EnumValue },
        1 => Reset { player: Target, reset: EnumValue },
        2 => Set {
            player: Target,
            location: TitleRawSlot = "titleLocation",
            text: serde_json::Value = "raw json titleText",
        },
        3 => Times {
            player: Target,
            times: EnumValue,
            fade_in: i32 = "fadeIn",
            stay: i32,
            fade_out: i32 = "fadeOut",
        },
    }
}

/// What a title command does to each target.
enum Action {
    Clear,
    Reset,
    Text(TitleType, String),
    Raw(TitleType, RawText),
    Times(i32, i32, i32),
}

pub fn run(ctx: &mut CommandContext<'_>, args: TitleArgs) -> CommandResult {
    let (player, action) = match args {
        TitleArgs::Clear { player, .. } => (player, Action::Clear),
        TitleArgs::Reset { player, .. } => (player, Action::Reset),
        TitleArgs::Set {
            player,
            location,
            text,
        } => {
            // Measured: a missing text reaches the command and answers
            // `commands.generic.invalidMessage`, as `/tell` does.
            if text.trim().is_empty() {
                return failure!("commands.generic.invalidMessage");
            }
            let kind = match location {
                TitleSlot::Title => TitleType::Title,
                TitleSlot::Subtitle => TitleType::Subtitle,
                TitleSlot::Actionbar => TitleType::Actionbar,
            };
            let text = expand(ctx, &text);
            (player, Action::Text(kind, text))
        }
        TitleArgs::Times {
            player,
            fade_in,
            stay,
            fade_out,
            ..
        } => (player, Action::Times(fade_in, stay, fade_out)),
    };
    send_titles(ctx, &player, action, "commands.title.success")
}

pub fn run_raw(ctx: &mut CommandContext<'_>, args: TitleRawArgs) -> CommandResult {
    let (player, action) = match args {
        TitleRawArgs::Clear { player, .. } => (player, Action::Clear),
        TitleRawArgs::Reset { player, .. } => (player, Action::Reset),
        TitleRawArgs::Set {
            player,
            location,
            text,
        } => {
            let kind = match location {
                TitleRawSlot::Title => TitleType::Titletextobject,
                TitleRawSlot::Subtitle => TitleType::Subtitletextobject,
                TitleRawSlot::Actionbar => TitleType::Actionbartextobject,
            };
            (player, Action::Raw(kind, RawText::parse(&text)?))
        }
        TitleRawArgs::Times {
            player,
            fade_in,
            stay,
            fade_out,
            ..
        } => (player, Action::Times(fade_in, stay, fade_out)),
    };
    send_titles(ctx, &player, action, "commands.titleraw.success")
}

fn send_titles(
    ctx: &mut CommandContext<'_>,
    player: &Target,
    action: Action,
    success: &str,
) -> CommandResult {
    let players = ctx.players(Some(player))?;
    for &target in &players {
        let packet = match &action {
            Action::Clear => title(TitleType::Clear, String::new(), (-1, -1, -1)),
            Action::Reset => title(TitleType::Reset, String::new(), (-1, -1, -1)),
            Action::Text(kind, text) => title(*kind, text.clone(), (-1, -1, -1)),
            Action::Raw(kind, raw) => title(*kind, raw.render(ctx, target), (-1, -1, -1)),
            Action::Times(fade_in, stay, fade_out) => title(
                TitleType::Times,
                String::new(),
                (*fade_in, *stay, *fade_out),
            ),
        };
        ctx.send(target, packet);
    }
    Ok(CommandOutput::success(success, vec![ctx.names(&players)]).with_count(players.len()))
}

fn title(
    kind: TitleType,
    text: String,
    (fade_in, stay, fade_out): (i32, i32, i32),
) -> SetTitlePacket {
    SetTitlePacket {
        title_type: kind,
        title_text: text.into(),
        fade_in_time: fade_in,
        stay_time: stay,
        fade_out_time: fade_out,
        xuid: "".into(),
        platform_online_id: "".into(),
        filtered_title_message: "".into(),
    }
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{Replay, assert_same_packets};

    #[test]
    fn title_matches_the_bds_capture() {
        let steps = Replay::new().assert_segment("title", &[]);
        assert_same_packets(&steps, &["SetTitlePacket"], &[]);
    }

    #[test]
    fn titleraw_matches_the_bds_capture() {
        let steps = Replay::new().assert_segment("titleraw", &[]);
        assert_same_packets(&steps, &["SetTitlePacket"], &[]);
    }
}
