//! Server-side command system.
//!
//! This module provides a small, extensible command dispatcher inspired by Dragonfly,
//! but designed around Rust's type system and ownership rules (no reflection).

mod player_state;
mod time;
mod world_settings;

use crate::network::SessionId;
use std::collections::HashMap;
use std::sync::Arc;

pub use player_state::PlayerTarget;
pub use time::{TimeQuery, TimeRequest};
pub use world_settings::GameRuleRequest;

#[derive(Debug, Default, Clone)]
pub struct CommandOutput {
    pub messages: Vec<String>,
    pub errors: Vec<String>,
    /// Server-side movement requested by a successfully authorized command.
    pub teleport: Option<glam::DVec3>,
    /// A mob to spawn; the server applies it with world access and replies.
    pub summon: Option<SummonRequest>,
    /// A world or player change the server applies with world access,
    /// replying with the outcome.
    pub world: Option<WorldRequest>,
}

/// A command effect on the world or its players.
#[derive(Debug, Clone, PartialEq)]
pub enum WorldRequest {
    Time(TimeRequest),
    Difficulty(crate::world::rules::Difficulty),
    GameRule(GameRuleRequest),
    GameMode {
        mode: crate::entity::components::GameMode,
        target: PlayerTarget,
    },
    Kill(PlayerTarget),
}

/// One coordinate of a command position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandCoordinate {
    Absolute(f64),
    /// `~` or `~offset`: relative to the executor's feet.
    Relative(f64),
}

impl CommandCoordinate {
    /// Parse one coordinate token. Bedrock centres a whole-number absolute
    /// x or z (written without a decimal point) on its block: `10` is 10.5
    /// (provisional: Minecraft Wiki "Coordinates", Bedrock Edition notes).
    pub fn parse(token: &str, horizontal: bool) -> Option<Self> {
        if let Some(offset) = token.strip_prefix('~') {
            return if offset.is_empty() {
                Some(Self::Relative(0.0))
            } else {
                offset
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .map(Self::Relative)
            };
        }
        let value = token.parse::<f64>().ok().filter(|v| v.is_finite())?;
        let centred = horizontal && !token.contains(['.', 'e', 'E']);
        Some(Self::Absolute(if centred { value + 0.5 } else { value }))
    }

    pub fn resolve(self, origin: f64) -> f64 {
        match self {
            Self::Absolute(value) => value,
            Self::Relative(offset) => origin + offset,
        }
    }
}

/// `/summon <entity> [x y z]`, applied by the server with world access.
#[derive(Debug, Clone, PartialEq)]
pub struct SummonRequest {
    /// Namespaced actor identifier (`minecraft:cow`).
    pub identifier: String,
    /// `None` summons at the executor's feet.
    pub position: Option<[CommandCoordinate; 3]>,
}

impl SummonRequest {
    /// The spawn position for an executor whose feet are at `feet`.
    pub fn resolve(&self, feet: glam::DVec3) -> glam::DVec3 {
        match self.position {
            None => feet,
            Some([x, y, z]) => {
                glam::DVec3::new(x.resolve(feet.x), y.resolve(feet.y), z.resolve(feet.z))
            }
        }
    }
}

impl CommandOutput {
    pub fn message(&mut self, message: impl Into<String>) {
        self.messages.push(message.into());
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.errors.push(message.into());
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.errors.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct CommandArgs {
    tokens: Vec<String>,
    index: usize,
}

impl CommandArgs {
    pub fn new(tokens: Vec<String>) -> Self {
        Self { tokens, index: 0 }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&str> {
        let token = self.tokens.get(self.index)?;
        self.index += 1;
        Some(token.as_str())
    }

    pub fn rest(&self) -> &[String] {
        &self.tokens[self.index..]
    }

    pub fn is_empty(&self) -> bool {
        self.index >= self.tokens.len()
    }
}

/// Server-owned operator grant. Client command origins never confer this marker.
#[derive(bevy_ecs::prelude::Component, Debug, Clone, Copy)]
pub struct Operator;

/// Context for command execution.
///
/// Note: Command execution in ECS is simplified - commands don't have
/// direct server access. Use systems for complex game logic.
pub struct CommandContext {
    pub sender: SessionId,
    pub is_operator: bool,
}

impl CommandContext {
    pub fn new(sender: SessionId, is_operator: bool) -> Self {
        Self {
            sender,
            is_operator,
        }
    }
}

pub trait Command: Send + Sync + 'static {
    fn name(&self) -> &'static str;

    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    fn description(&self) -> &'static str {
        ""
    }

    fn usage(&self) -> &'static str {
        ""
    }

    fn requires_operator(&self) -> bool {
        false
    }

    fn execute(&self, ctx: &CommandContext, args: &mut CommandArgs) -> CommandOutput;
}

#[derive(Default)]
pub struct CommandRegistry {
    commands: HashMap<String, Arc<dyn Command>>,
}

impl std::fmt::Debug for CommandRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names: Vec<&str> = self.commands.keys().map(|name| name.as_str()).collect();
        names.sort_unstable();

        f.debug_struct("CommandRegistry")
            .field("names", &names)
            .finish()
    }
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(PosCommand);
        registry.register(TeleportCommand);
        registry.register(SummonCommand);
        registry.register(time::TimeCommand);
        registry.register(world_settings::DifficultyCommand);
        registry.register(world_settings::GameRuleCommand);
        registry.register(player_state::GameModeCommand);
        registry.register(player_state::KillCommand);
        registry
    }

    pub fn register<C: Command>(&mut self, command: C) {
        self.register_arc(Arc::new(command));
    }

    pub fn register_arc(&mut self, command: Arc<dyn Command>) {
        let name = command.name().to_ascii_lowercase();
        self.commands.insert(name, command.clone());

        for &alias in command.aliases() {
            self.commands
                .insert(alias.to_ascii_lowercase(), command.clone());
        }
    }

    pub fn find(&self, name: &str) -> Option<Arc<dyn Command>> {
        self.commands.get(&name.to_ascii_lowercase()).cloned()
    }

    /// Resolve aliases and enforce server permissions before invoking any handler.
    pub fn execute(
        &self,
        ctx: &CommandContext,
        invocation: &mut CommandInvocation,
    ) -> CommandOutput {
        let mut output = CommandOutput::default();
        let Some(command) = self.find(&invocation.name) else {
            output.error(format!("Unknown command: {}", invocation.name));
            return output;
        };
        if command.requires_operator() && !ctx.is_operator {
            output.error("You do not have permission to use this command");
            return output;
        }
        command.execute(ctx, &mut invocation.args)
    }
}

#[derive(Debug, Clone)]
pub struct CommandInvocation {
    pub name: String,
    pub args: CommandArgs,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum CommandParseError {
    #[error("empty command")]
    Empty,
    #[error("unterminated quote")]
    UnterminatedQuote,
}

pub fn parse_command_line(line: &str) -> Result<CommandInvocation, CommandParseError> {
    let trimmed = line.trim();
    let without_slash = trimmed.strip_prefix('/').unwrap_or(trimmed);
    let tokens = split_tokens(without_slash)?;
    if tokens.is_empty() {
        return Err(CommandParseError::Empty);
    }

    let mut tokens = tokens.into_iter();
    let name = tokens.next().ok_or(CommandParseError::Empty)?;
    Ok(CommandInvocation {
        name,
        args: CommandArgs::new(tokens.collect()),
    })
}

fn split_tokens(input: &str) -> Result<Vec<String>, CommandParseError> {
    let mut tokens = Vec::<String>::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escape = false;

    for ch in input.chars() {
        if escape {
            current.push(ch);
            escape = false;
            continue;
        }

        match ch {
            '\\' => {
                escape = true;
            }
            '"' | '\'' => {
                if quote == Some(ch) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(ch);
                } else {
                    current.push(ch);
                }
            }
            c if c.is_whitespace() && quote.is_none() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if escape {
        current.push('\\');
    }
    if quote.is_some() {
        return Err(CommandParseError::UnterminatedQuote);
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    Ok(tokens)
}

struct TeleportCommand;

impl Command for TeleportCommand {
    fn name(&self) -> &'static str {
        "tp"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["teleport"]
    }

    fn requires_operator(&self) -> bool {
        true
    }

    fn usage(&self) -> &'static str {
        "/tp <x> <y> <z>"
    }

    fn execute(&self, _ctx: &CommandContext, args: &mut CommandArgs) -> CommandOutput {
        let mut output = CommandOutput::default();
        let args = args.rest();
        if args.len() < 3 {
            output.error(format!("Usage: {}", self.usage()));
            return output;
        }
        let mut coordinates = [0.0; 3];
        for (index, axis) in ["x", "y", "z"].iter().enumerate() {
            match args[index].parse::<f64>() {
                Ok(value) => coordinates[index] = value,
                Err(_) => {
                    output.error(format!("Invalid {axis} coordinate: {}", args[index]));
                    return output;
                }
            }
        }
        output.teleport = Some(glam::DVec3::from_array(coordinates));
        output
    }
}

struct SummonCommand;

impl Command for SummonCommand {
    fn name(&self) -> &'static str {
        "summon"
    }

    fn description(&self) -> &'static str {
        "Summons an entity"
    }

    fn requires_operator(&self) -> bool {
        true
    }

    fn usage(&self) -> &'static str {
        "/summon <entity> [x y z]"
    }

    fn execute(&self, _ctx: &CommandContext, args: &mut CommandArgs) -> CommandOutput {
        let mut output = CommandOutput::default();
        let args = args.rest();
        if !matches!(args.len(), 1 | 4) {
            output.error(format!("Usage: {}", self.usage()));
            return output;
        }
        let entity = args[0].to_ascii_lowercase();
        let identifier = if entity.contains(':') {
            entity
        } else {
            format!("minecraft:{entity}")
        };
        let position = if args.len() == 4 {
            let mut coordinates = [CommandCoordinate::Relative(0.0); 3];
            for (index, axis) in ["x", "y", "z"].iter().enumerate() {
                let token = &args[index + 1];
                match CommandCoordinate::parse(token, index != 1) {
                    Some(coordinate) => coordinates[index] = coordinate,
                    None => {
                        output.error(format!("Invalid {axis} coordinate: {token}"));
                        return output;
                    }
                }
            }
            Some(coordinates)
        } else {
            None
        };
        output.summon = Some(SummonRequest {
            identifier,
            position,
        });
        output
    }
}

struct PosCommand;

impl Command for PosCommand {
    fn name(&self) -> &'static str {
        "pos"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["position", "coords"]
    }

    fn description(&self) -> &'static str {
        "Show your current position"
    }

    fn usage(&self) -> &'static str {
        "/pos"
    }

    fn execute(&self, _ctx: &CommandContext, args: &mut CommandArgs) -> CommandOutput {
        let mut out = CommandOutput::default();
        if !args.is_empty() {
            out.error(format!("Usage: {}", self.usage()));
            return out;
        }

        // Position will be fetched from ECS by the server when executing
        out.message("Use a system to query position from ECS");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_command_line_splits_tokens() {
        let inv = parse_command_line("/say hello world").unwrap();
        assert_eq!(inv.name, "say");
        assert_eq!(
            inv.args.tokens,
            vec!["hello".to_string(), "world".to_string()]
        );
    }

    #[test]
    fn summon_parses_identifier_and_bedrock_coordinates() {
        let parse = |line: &str| {
            let mut invocation = parse_command_line(line).unwrap();
            SummonCommand.execute(&CommandContext::new(1, true), &mut invocation.args)
        };
        let output = parse("/summon Cow 10 64 -3.25");
        let request = output.summon.unwrap();
        assert_eq!(request.identifier, "minecraft:cow");
        assert_eq!(
            request.resolve(glam::DVec3::ZERO),
            glam::DVec3::new(10.5, 64.0, -3.25),
            "whole-number x/z are block centres; y is not"
        );
        let request = parse("/summon minecraft:cow ~ ~1 ~-2").summon.unwrap();
        assert_eq!(
            request.resolve(glam::DVec3::new(1.5, 60.0, 1.5)),
            glam::DVec3::new(1.5, 61.0, -0.5)
        );
        assert_eq!(parse("/summon cow").summon.unwrap().position, None);
        for bad in ["/summon", "/summon cow 1 2", "/summon cow 1 x 2"] {
            let output = parse(bad);
            assert!(
                output.summon.is_none() && !output.errors.is_empty(),
                "{bad}"
            );
        }
    }

    #[test]
    fn split_tokens_handles_quotes() {
        let tokens = split_tokens(r#"pos "hello world" '#'"#).unwrap();
        assert_eq!(
            tokens,
            vec![
                "pos".to_string(),
                "hello world".to_string(),
                "#".to_string()
            ]
        );
    }
}
