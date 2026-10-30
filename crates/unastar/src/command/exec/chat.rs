//! `/say`, `/me`, `/tell` (aliases `msg`, `w`) and `/list`.
//!
//! Measured on BDS 1.26.51:
//! - `/say hello` sends no command output; every player gets a `Text`
//!   announcement `[source] hello` with the source as player name.
//! - `/me waves` sends no command output; every player gets a `Text` chat
//!   `* source waves` with an empty player name.
//! - `/tell @s secret` whispers `secret` to each target (player name: the
//!   source) and answers `commands.message.display.outgoing` (target,
//!   message); telling yourself works.
//! - `/list` answers `commands.players.list` (online, max) and
//!   `commands.players.list.names` (the names, `, `-joined).
//! - `/me` and `/tell @s` without a message answer
//!   `commands.generic.invalidMessage` and send nothing.
//!
//! Selectors in a message are replaced by the names they match
//! (`/say hi @s` → `hi BDSExtractor`).

use crate::command::parser::Cursor;
use crate::command::prelude::*;
use crate::command::selector::parse_selector;
use crate::entity::components::{Player, PlayerSession};
use crate::server::game::ServerConfigResource;
use bevy_ecs::prelude::With;
use jolyne::valentine::TextPacket;
use jolyne::valentine::types::{
    TextPacketBody, TextPacketPayloadAuthorAndMessage, TextPacketPayloadAuthorAndMessageMessageType,
};

overloads! {
    pub enum SayArgs for "say" {
        0 => Say { message: Message },
    }
}

overloads! {
    pub enum MeArgs for "me" {
        0 => Me { message: Message },
    }
}

overloads! {
    pub enum TellArgs for "tell" {
        0 => Tell { target: Target, message: Message },
    }
}

overloads! {
    pub enum ListArgs for "list" {
        0 => List {},
    }
}

pub fn run_say(ctx: &mut CommandContext<'_>, args: SayArgs) -> CommandResult {
    let SayArgs::Say { message } = args;
    let message = expand(ctx, nonempty(&message)?);
    let packet = text(
        TextPacketPayloadAuthorAndMessageMessageType::Announcement,
        &ctx.source.name,
        format!("[{}] {message}", ctx.source.name),
    );
    ctx.broadcast(packet);
    Ok(CommandOutput::default())
}

pub fn run_me(ctx: &mut CommandContext<'_>, args: MeArgs) -> CommandResult {
    let MeArgs::Me { message } = args;
    let message = expand(ctx, nonempty(&message)?);
    let packet = text(
        TextPacketPayloadAuthorAndMessageMessageType::Chat,
        "",
        format!("* {} {message}", ctx.source.name),
    );
    ctx.broadcast(packet);
    Ok(CommandOutput::default())
}

pub fn run_tell(ctx: &mut CommandContext<'_>, args: TellArgs) -> CommandResult {
    let TellArgs::Tell { target, message } = args;
    let message = nonempty(&message)?;
    let targets = ctx.players(Some(&target))?;
    let message = expand(ctx, message);
    let packet = text(
        TextPacketPayloadAuthorAndMessageMessageType::Whisper,
        &ctx.source.name,
        message.clone(),
    );
    let mut output = CommandOutput::default();
    for target in targets {
        ctx.send(target, packet.clone());
        output.add_success(
            "commands.message.display.outgoing",
            vec![ctx.name(target), message.clone()],
        );
    }
    Ok(output)
}

/// The message, unless it is empty or blank (`commands.generic.invalidMessage`).
fn nonempty(message: &Message) -> Result<&str, CommandOutput> {
    if message.trim().is_empty() {
        return failure!("commands.generic.invalidMessage");
    }
    Ok(message)
}

pub fn run_list(ctx: &mut CommandContext<'_>, args: ListArgs) -> CommandResult {
    let ListArgs::List {} = args;
    let mut query = ctx
        .world
        .query_filtered::<Entity, (With<Player>, With<PlayerSession>)>();
    let players: Vec<Entity> = query.iter(ctx.world).collect();
    let max = ctx
        .world
        .get_resource::<ServerConfigResource>()
        .map_or(0, |config| config.0.max_players);
    let mut output = CommandOutput::success(
        "commands.players.list",
        vec![players.len().to_string(), max.to_string()],
    );
    output.add_message("commands.players.list.names", vec![ctx.names(&players)]);
    Ok(output)
}

/// `message` with every selector replaced by the names it matches.
/// [INFERENCE] A selector that matches nothing (or fails) becomes empty.
fn expand(ctx: &mut CommandContext<'_>, message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut cursor = Cursor::new(message);
    while let Some(c) = cursor.peek() {
        if c == '@' {
            let mut selector_end = cursor;
            if let Ok(selector) = parse_selector(&mut selector_end) {
                let target = Target::Selector(Box::new(selector));
                if let Ok(found) = ctx.entities(Some(&target)) {
                    out.push_str(&ctx.names(&found));
                }
                cursor = selector_end;
                continue;
            }
        }
        out.push(c);
        cursor.pos += c.len_utf8();
    }
    out
}

fn text(
    kind: TextPacketPayloadAuthorAndMessageMessageType,
    player_name: &str,
    message: String,
) -> TextPacket {
    TextPacket {
        localize: false,
        body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
            message_type: kind,
            player_name: player_name.into(),
            message: message.into(),
        }),
        senders_xuid: "".into(),
        platform_id: "".into(),
        filtered_message: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;
    use crate::command::{CommandSource, execute};
    use jolyne::valentine::McpePacketData;

    /// `(type, player_name, message)` of the author-and-message texts sent.
    fn texts(
        h: &mut Harness,
        player: Entity,
    ) -> Vec<(TextPacketPayloadAuthorAndMessageMessageType, String, String)> {
        h.sent(player)
            .into_iter()
            .filter_map(|p| match p {
                McpePacketData::TextPacket(TextPacket {
                    body: TextPacketBody::AuthorAndMessage(t),
                    ..
                }) => Some((
                    t.message_type,
                    t.player_name.to_string(),
                    t.message.to_string(),
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn say_announces_to_everyone_with_selectors_expanded() {
        let mut h = Harness::new();
        let alice = h.player("Alice", true);
        let bob = h.player("Bob", false);
        assert_eq!(h.run(alice, "/say hello world"), CommandOutput::default());
        let expected = (
            TextPacketPayloadAuthorAndMessageMessageType::Announcement,
            "Alice".to_string(),
            "[Alice] hello world".to_string(),
        );
        assert_eq!(texts(&mut h, alice), std::slice::from_ref(&expected));
        assert_eq!(texts(&mut h, bob), [expected]);

        assert_eq!(h.run(alice, "/say hi @s"), CommandOutput::default());
        assert_eq!(texts(&mut h, bob)[0].2, "[Alice] hi Alice");
        h.run(alice, "/say to: @a[name=Bob] and mail@example");
        assert_eq!(texts(&mut h, bob)[0].2, "[Alice] to: Bob and mail@example");

        execute(h.world(), CommandSource::server(), "say restart soon");
        assert_eq!(
            texts(&mut h, bob),
            [(
                TextPacketPayloadAuthorAndMessageMessageType::Announcement,
                "Server".to_string(),
                "[Server] restart soon".to_string(),
            )]
        );
        // Members may not /say (gamedirectors).
        assert!(!h.run(bob, "/say hi").is_success());
    }

    #[test]
    fn me_sends_an_emote_chat_line() {
        let mut h = Harness::new();
        let alice = h.player("Alice", false);
        assert_eq!(h.run(alice, "/me waves"), CommandOutput::default());
        assert_eq!(
            texts(&mut h, alice),
            [(
                TextPacketPayloadAuthorAndMessageMessageType::Chat,
                String::new(),
                "* Alice waves".to_string(),
            )]
        );
    }

    #[test]
    fn empty_messages_are_invalid_like_bds() {
        let mut h = Harness::new();
        let alice = h.player("Alice", true);
        let invalid = CommandOutput::failure("commands.generic.invalidMessage", vec![]);
        assert_eq!(h.run(alice, "/me"), invalid);
        assert_eq!(h.run(alice, "/tell @s"), invalid);
        assert_eq!(h.run(alice, "/say"), invalid);
        assert!(texts(&mut h, alice).is_empty());
        assert_eq!(nonempty(&Message("   ".into())), Err(invalid));
    }

    #[test]
    fn tell_and_msg_whisper_to_the_targets() {
        let mut h = Harness::new();
        let alice = h.player("Alice", false);
        let bob = h.player("Bob", false);

        let output = h.run(alice, "/tell @s secret message");
        assert_eq!(
            output,
            CommandOutput::success(
                "commands.message.display.outgoing",
                vec!["Alice".into(), "secret message".into()]
            )
        );
        assert_eq!(
            texts(&mut h, alice),
            [(
                TextPacketPayloadAuthorAndMessageMessageType::Whisper,
                "Alice".to_string(),
                "secret message".to_string(),
            )]
        );
        assert!(texts(&mut h, bob).is_empty());

        let output = h.run(alice, "/msg Bob again");
        assert_eq!(output.messages[0].params, ["Bob", "again"]);
        assert_eq!(texts(&mut h, bob)[0].2, "again");
        assert!(texts(&mut h, alice).is_empty());
        assert!(h.run(alice, "/w Bob psst").is_success());
        assert_eq!(h.run(alice, "/tell Nobody hi"), CommandOutput::no_targets());
    }

    #[test]
    fn list_counts_and_names_the_online_players() {
        let mut h = Harness::new();
        let alice = h.player("Alice", false);
        let output = h.run(alice, "/list");
        let mut expected =
            CommandOutput::success("commands.players.list", vec!["1".into(), "20".into()]);
        expected.add_message("commands.players.list.names", vec!["Alice".into()]);
        assert_eq!(output, expected);

        h.player("Bob", false);
        let output = h.run(alice, "/list");
        assert_eq!(output.messages[0].params, ["2", "20"]);
        assert_eq!(output.messages[1].params, ["Alice, Bob"]);
    }
}
