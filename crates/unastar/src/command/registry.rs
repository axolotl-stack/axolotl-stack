//! Command name → executor, next to the frozen syntax.
//!
//! Adding a command is one function and one `register` line:
//!
//! ```ignore
//! // command/exec/difficulty.rs
//! pub fn run(ctx: &mut CommandContext<'_>, args: &Args) -> CommandResult { … }
//! // command/exec/mod.rs, in `register_vanilla`
//! registry.register("difficulty", difficulty::run);
//! ```
//!
//! The syntax (overloads, enums, permission) already comes from the captured
//! packet; registering a name the syntax does not have panics at startup.

use super::args::Args;
use super::context::CommandContext;
use super::output::CommandResult;
use super::schema::CommandSchema;
use bevy_ecs::prelude::Resource;
use std::collections::HashMap;
use std::sync::Arc;

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

    /// Registers the executor of a captured command (canonical name, not an alias).
    pub fn register(&mut self, name: &str, handler: impl CommandHandler) -> &mut Self {
        let definition = self
            .schema
            .command(name)
            .unwrap_or_else(|| panic!("/{name} is not in the command syntax"));
        assert_eq!(
            definition.name, name,
            "register /{} by its canonical name",
            definition.name
        );
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
