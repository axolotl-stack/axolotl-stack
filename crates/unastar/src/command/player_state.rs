//! `/gamemode` and `/kill`: commands that change a player's state.

use super::{Command, CommandArgs, CommandContext, CommandOutput, WorldRequest};
use crate::entity::components::GameMode;

/// The players a command targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerTarget {
    /// No target, `@s` or `@p`: the executing player.
    Executor,
    /// `@a`: every player.
    All,
    /// A player by display name (ASCII case-insensitive).
    Named(String),
}

impl PlayerTarget {
    fn parse(token: Option<&str>) -> Result<Self, String> {
        Ok(match token {
            None | Some("@s" | "@p") => Self::Executor,
            Some("@a") => Self::All,
            Some(selector) if selector.starts_with('@') => {
                return Err(format!("Unsupported target selector: {selector}"));
            }
            Some(name) => Self::Named(name.to_owned()),
        })
    }
}

pub(super) struct GameModeCommand;

impl Command for GameModeCommand {
    fn name(&self) -> &'static str {
        "gamemode"
    }

    fn description(&self) -> &'static str {
        "Sets a player's game mode"
    }

    fn requires_operator(&self) -> bool {
        true
    }

    fn usage(&self) -> &'static str {
        "/gamemode <survival|creative|adventure|spectator> [player]"
    }

    fn execute(&self, _ctx: &CommandContext, args: &mut CommandArgs) -> CommandOutput {
        let mut output = CommandOutput::default();
        let Some(mode) = args.next().and_then(game_mode) else {
            output.error(format!("Usage: {}", self.usage()));
            return output;
        };
        let target = PlayerTarget::parse(args.next());
        if !args.is_empty() {
            output.error(format!("Usage: {}", self.usage()));
            return output;
        }
        match target {
            Ok(target) => output.world = Some(WorldRequest::GameMode { mode, target }),
            Err(error) => output.error(error),
        }
        output
    }
}

/// A game mode by name, letter or Bedrock number.
fn game_mode(name: &str) -> Option<GameMode> {
    Some(match name.to_ascii_lowercase().as_str() {
        "survival" | "s" | "0" => GameMode::Survival,
        "creative" | "c" | "1" => GameMode::Creative,
        "adventure" | "a" | "2" => GameMode::Adventure,
        "spectator" | "6" => GameMode::Spectator,
        _ => return None,
    })
}

pub(super) struct KillCommand;

impl Command for KillCommand {
    fn name(&self) -> &'static str {
        "kill"
    }

    fn description(&self) -> &'static str {
        "Kills players"
    }

    fn requires_operator(&self) -> bool {
        true
    }

    fn usage(&self) -> &'static str {
        "/kill [player]"
    }

    fn execute(&self, _ctx: &CommandContext, args: &mut CommandArgs) -> CommandOutput {
        let mut output = CommandOutput::default();
        let target = PlayerTarget::parse(args.next());
        if !args.is_empty() {
            output.error(format!("Usage: {}", self.usage()));
            return output;
        }
        match target {
            Ok(target) => output.world = Some(WorldRequest::Kill(target)),
            Err(error) => output.error(error),
        }
        output
    }
}
