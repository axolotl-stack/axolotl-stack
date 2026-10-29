//! Command name → executor, next to the frozen syntax.
//!
//! Adding a command is a typed declaration, one function and one line:
//!
//! ```ignore
//! // command/exec/difficulty.rs
//! overloads! { pub enum Difficulty for "difficulty" { 0 => Named { … }, 1 => Number { … } } }
//! pub fn run(ctx: &mut CommandContext<'_>, args: Difficulty) -> CommandResult { match args { … } }
//! // command/exec/mod.rs, in `register_vanilla`
//! registry.register(difficulty::run);
//! ```
//!
//! The syntax (overloads, enums, permission) already comes from the captured
//! packet; registering a name the syntax does not have panics at startup.

use super::args::{Args, Overloads, check_shape};
use super::context::CommandContext;
use super::output::CommandResult;
use super::schema::CommandSchema;
use bevy_ecs::prelude::Resource;
use std::collections::HashMap;
use std::sync::Arc;
use unastar_data::commands::CommandDefinition;

/// Runs one parsed command. Plain `fn(&mut CommandContext, &Args) -> CommandResult`
/// items implement it; plugins may register closures.
pub trait CommandHandler: Send + Sync + 'static {
    fn run(&self, ctx: &mut CommandContext<'_>, args: &Args) -> CommandResult;
}

impl<F> CommandHandler for F
where
    F: Fn(&mut CommandContext<'_>, &Args) -> CommandResult + Send + Sync + 'static,
{
    fn run(&self, ctx: &mut CommandContext<'_>, args: &Args) -> CommandResult {
        self(ctx, args)
    }
}

/// The command syntax plus the executors registered for it.
#[derive(Resource, Clone)]
pub struct CommandRegistry {
    schema: Arc<CommandSchema>,
    handlers: HashMap<String, Arc<dyn CommandHandler>>,
}

impl std::fmt::Debug for CommandRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names: Vec<&str> = self.handlers.keys().map(String::as_str).collect();
        names.sort_unstable();
        f.debug_struct("CommandRegistry")
            .field("schema", &self.schema)
            .field("handlers", &names)
            .finish()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::vanilla()
    }
}

impl CommandRegistry {
    /// An empty registry over a syntax.
    pub fn new(schema: CommandSchema) -> Self {
        Self {
            schema: Arc::new(schema),
            handlers: HashMap::new(),
        }
    }

    /// The BDS 1.26.51 syntax with every vanilla executor unastar has.
    pub fn vanilla() -> Self {
        let mut registry = Self::new(CommandSchema::vanilla());
        super::exec::register_vanilla(&mut registry);
        registry
    }

    /// Registers a command's executor over its typed overloads. Panics when
    /// the declaration does not match the captured syntax (see
    /// [`check_shape`](super::args::check_shape)).
    pub fn register<T: Overloads>(
        &mut self,
        run: fn(&mut CommandContext<'_>, T) -> CommandResult,
    ) -> &mut Self {
        let definition = self.definition(T::COMMAND);
        check_shape(T::SHAPE, definition).unwrap_or_else(|error| panic!("{error}"));
        self.insert(
            T::COMMAND,
            move |ctx: &mut CommandContext<'_>, args: &Args| run(ctx, T::from_args(args)),
        )
    }

    /// Registers an executor over the raw arguments (for commands added at
    /// runtime, e.g. by plugins, without a typed declaration).
    pub fn register_raw(&mut self, name: &str, handler: impl CommandHandler) -> &mut Self {
        self.definition(name);
        self.insert(name, handler)
    }

    fn definition(&self, name: &str) -> &'static CommandDefinition {
        let definition = self
            .schema
            .command(name)
            .unwrap_or_else(|| panic!("/{name} is not in the command syntax"));
        assert_eq!(
            definition.name, name,
            "register /{} by its canonical name",
            definition.name
        );
        definition
    }

    fn insert(&mut self, name: &str, handler: impl CommandHandler) -> &mut Self {
        let previous = self.handlers.insert(name.to_owned(), Arc::new(handler));
        assert!(previous.is_none(), "/{name} registered twice");
        self
    }

    pub fn schema(&self) -> &Arc<CommandSchema> {
        &self.schema
    }

    pub fn handler(&self, name: &str) -> Option<Arc<dyn CommandHandler>> {
        self.handlers.get(name).cloned()
    }

    /// Captured commands without an executor, in syntax order.
    pub fn unimplemented(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.schema
            .syntax()
            .commands
            .iter()
            .map(|c| c.name.as_str())
            .filter(|name| !self.handlers.contains_key(*name))
    }
}
