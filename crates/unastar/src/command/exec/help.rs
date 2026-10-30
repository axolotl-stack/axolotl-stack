//! `/help [page]` and `/help <command>` (alias `/?`), from BDS's measured
//! listing (`unastar_data::help`).
//!
//! Measured (BDS 1.26.51, operator):
//! - a page: `§2%commands.help.header [page, pages]`, the page's 7 usage lines
//!   as one literal message, `§2%commands.help.footer`; a page past the end
//!   shows the last page;
//! - a command: `commands.help.command.aliases ["§e<name>", "<alias>, <alias>"]`
//!   (or the literal `§e<name>:` without aliases), `§e%<description key>`,
//!   `commands.generic.usage.noparam`, then its usage lines prefixed `- ` and
//!   written with the name as typed.
//!
//! Lines are listed for commands the source may run. The listing was captured
//! for an operator, so the console sees the operator listing too (the
//! owner-only commands' usage lines were not captured).

use crate::command::prelude::*;
use unastar_data::help::{PAGE_SIZE, line_command};

overloads! {
    pub enum HelpArgs for "help" {
        0 => Page { page: i32 },
        1 => Command { command: Option<EnumValue> },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: HelpArgs) -> CommandResult {
    match args {
        HelpArgs::Page { page } => Ok(page_output(ctx, page)),
        HelpArgs::Command { command: None } => Ok(page_output(ctx, 1)),
        HelpArgs::Command {
            command: Some(name),
        } => command_output(ctx, &name),
    }
}

/// Usage lines of the commands the source may run.
fn visible_lines(ctx: &CommandContext<'_>) -> Vec<&'static str> {
    unastar_data::help::vanilla()
        .iter()
        .filter(|line| {
            ctx.schema
                .command(line_command(line))
                .is_some_and(|c| c.permission <= ctx.source.permission)
        })
        .map(String::as_str)
        .collect()
}

fn page_output(ctx: &CommandContext<'_>, page: i32) -> CommandOutput {
    let lines = visible_lines(ctx);
    let pages = lines.len().div_ceil(PAGE_SIZE).max(1);
    let page = (page.max(1) as usize).min(pages);
    let body = lines
        .chunks(PAGE_SIZE)
        .nth(page - 1)
        .unwrap_or_default()
        .join("\n");
    let mut output = CommandOutput::default();
    output.add_success(
        "§2%commands.help.header",
        vec![page.to_string(), pages.to_string()],
    );
    output.add_message(body, vec![]);
    output.add_message("§2%commands.help.footer", vec![]);
    output
}

fn command_output(ctx: &CommandContext<'_>, typed: &str) -> CommandResult {
    // [INFERENCE] A command above the source's permission answers like an
    // unknown one (captured only for an operator's permitted commands).
    let command = ctx
        .schema
        .command(typed)
        .filter(|c| c.permission <= ctx.source.permission)
        .ok_or_else(|| CommandOutput::failure("commands.generic.unknown", vec![typed.into()]))?;
    let names = ctx.schema.names(command);
    let mut output = CommandOutput::default();
    let others: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| *name != command.name)
        .collect();
    if others.is_empty() {
        output.add_success(format!("§e{}:", command.name), vec![]);
    } else {
        output.add_success(
            "commands.help.command.aliases",
            vec![format!("§e{}", command.name), others.join(", ")],
        );
    }
    output.add_message(format!("§e%{}", command.description), vec![]);
    output.add_message("commands.generic.usage.noparam", vec![]);
    let usage = unastar_data::help::vanilla()
        .iter()
        .filter(|line| line_command(line) == command.name)
        .map(|line| format!("- /{typed}{}", &line[1 + command.name.len()..]))
        .collect::<Vec<_>>()
        .join("\n");
    output.add_message(usage, vec![]);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;

    fn keys(output: &CommandOutput) -> Vec<(&str, Vec<&str>)> {
        output
            .messages
            .iter()
            .map(|m| {
                (
                    m.key.as_str(),
                    m.params.iter().map(String::as_str).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn pages_match_the_bds_capture() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        let first = h.run(op, "/help");
        assert_eq!(first.success_count, 1);
        assert_eq!(
            keys(&first),
            [
                ("§2%commands.help.header", vec!["1", "39"]),
                (
                    "/? <page: int>\n/? [command: CommandName]\n/aimassist <players: target> clear\n\
                     /aimassist <players: target> set [x angle: float] [y angle: float] [max distance: float] [target mode: AimAssistTargetMode] [preset id: string]\n\
                     /alwaysday [lock: Boolean]\n/camera <players: target> attach_to_entity <entity: target>\n\
                     /camera <players: target> clear",
                    vec![]
                ),
                ("§2%commands.help.footer", vec![]),
            ]
        );
        let last = h.run(op, "/? 40");
        assert_eq!(last.messages[0].params, ["39", "39"]);
        assert_eq!(
            last.messages[1].key,
            "/weather query\n/xp <amount: int> [player: target]\n/xp <amount: int>L [player: target]"
        );
    }

    #[test]
    fn command_help_matches_the_bds_capture() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        let output = h.run(op, "/help tell");
        assert_eq!(
            keys(&output),
            [
                ("commands.help.command.aliases", vec!["§etell", "w, msg"]),
                ("§e%commands.tell.description", vec![]),
                ("commands.generic.usage.noparam", vec![]),
                ("- /tell <target: target> <message: message>", vec![]),
            ]
        );
        let output = h.run(op, "/help give");
        assert_eq!(output.messages[0].key, "§egive:");
        assert_eq!(
            output.messages[3].key,
            "- /give <player: target> <itemName: Item> [amount: int] [data: int] [components: json]"
        );
        let output = h.run(op, "/help tp");
        assert!(
            output.messages[3]
                .key
                .starts_with("- /tp <destination: target>\n")
        );
        assert_eq!(output.messages[0].params, ["§eteleport", "tp"]);
    }

    #[test]
    fn members_see_only_their_commands() {
        let mut h = Harness::new();
        let member = h.player("Member", false);
        let output = h.run(member, "/help");
        let pages: usize = output.messages[0].params[1].parse().unwrap();
        assert!(pages < 39);
        assert!(!output.messages[1].key.contains("/aimassist"));
        assert_eq!(
            h.run(member, "/help give"),
            CommandOutput::failure("commands.generic.unknown", vec!["give".into()])
        );
    }
}
