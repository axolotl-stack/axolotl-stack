//! `/difficulty` and `/gamerule`: the world settings operators change.

use super::{Command, CommandArgs, CommandContext, CommandOutput, WorldRequest};
use crate::world::rules::{Difficulty, GameRule};

/// What a `/gamerule` command does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameRuleRequest {
    /// `/gamerule`: every rule and its value.
    List,
    /// `/gamerule <rule>`.
    Query(GameRule),
    /// `/gamerule <rule> <value>`.
    Set(GameRule, bool),
}

pub(super) struct DifficultyCommand;

impl Command for DifficultyCommand {
    fn name(&self) -> &'static str {
        "difficulty"
    }

    fn description(&self) -> &'static str {
        "Sets the difficulty level"
    }

    fn requires_operator(&self) -> bool {
        true
    }

    fn usage(&self) -> &'static str {
        "/difficulty <peaceful|easy|normal|hard>"
    }

    fn execute(&self, _ctx: &CommandContext, args: &mut CommandArgs) -> CommandOutput {
        let mut output = CommandOutput::default();
        let difficulty = match args.rest() {
            [name] => Difficulty::from_name(name),
            _ => None,
        };
        match difficulty {
            Some(difficulty) => output.world = Some(WorldRequest::Difficulty(difficulty)),
            None => output.error(format!("Usage: {}", self.usage())),
        }
        output
    }
}

pub(super) struct GameRuleCommand;

impl Command for GameRuleCommand {
    fn name(&self) -> &'static str {
        "gamerule"
    }

    fn description(&self) -> &'static str {
        "Sets or queries a game rule value"
    }

    fn requires_operator(&self) -> bool {
        true
    }

    fn usage(&self) -> &'static str {
        "/gamerule [rule] [true|false]"
    }

    fn execute(&self, _ctx: &CommandContext, args: &mut CommandArgs) -> CommandOutput {
        let mut output = CommandOutput::default();
        let (name, value) = match args.rest() {
            [] => {
                output.world = Some(WorldRequest::GameRule(GameRuleRequest::List));
                return output;
            }
            [name] => (name, None),
            [name, value] => (name, Some(value.to_ascii_lowercase())),
            _ => {
                output.error(format!("Usage: {}", self.usage()));
                return output;
            }
        };
        let Some(rule) = GameRule::from_name(name) else {
            output.error(format!("Unknown game rule: {name}"));
            return output;
        };
        let request = match value.as_deref() {
            None => Some(GameRuleRequest::Query(rule)),
            Some("true") => Some(GameRuleRequest::Set(rule, true)),
            Some("false") => Some(GameRuleRequest::Set(rule, false)),
            Some(_) => None,
        };
        match request {
            Some(request) => output.world = Some(WorldRequest::GameRule(request)),
            None => output.error(format!("Usage: {}", self.usage())),
        }
        output
    }
}
