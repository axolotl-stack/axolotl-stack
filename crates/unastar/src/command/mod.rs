//! Server-side command system.
//!
//! This module provides a small, extensible command dispatcher inspired by Dragonfly,
//! but designed around Rust's type system and ownership rules (no reflection).

use crate::network::SessionId;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Default, Clone)]
pub struct CommandOutput {
    pub messages: Vec<String>,
    pub errors: Vec<String>,
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

/// Context for command execution.
///
/// Note: Command execution in ECS is simplified - commands don't have
/// direct server access. Use systems for complex game logic.
pub struct CommandContext {
    pub sender: SessionId,
}

impl CommandContext {
    pub fn new(sender: SessionId) -> Self {
        Self { sender }
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
