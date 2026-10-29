//! Vanilla commands with BDS 1.26.51 syntax and feedback.
//!
//! - **Syntax is data**: [`schema::CommandSchema`] is built from the captured
//!   `AvailableCommandsPacket` (`unastar_data::commands`), sent unchanged to every
//!   player at join, and drives the one generic [`parser`].
//! - **Behaviour is code**: one executor function per command in [`exec`],
//!   registered by name in the [`CommandRegistry`]. Executors read typed
//!   arguments ([`Args::get`], [`Args::choice`]), resolve targets and positions
//!   through [`CommandContext`] and return a [`CommandResult`]; every helper
//!   that can fail returns BDS's failure message, so `?` is the error path.
//! - **Feedback** is [`CommandOutput`]: BDS's translation keys and parameters,
//!   sent as a `CommandOutputPacket` (players) or rendered with `en_US.lang`
//!   (server console).
//!
//! Commands BDS offers whose game system unastar does not have yet stay
//! advertised and answer with a clear "not implemented" failure.
//!
//! A new executor usually looks like:
//!
//! ```ignore
//! use crate::command::prelude::*;
//!
//! pub fn run(ctx: &mut CommandContext<'_>, args: &Args) -> CommandResult {
//!     let players = ctx.players(args.get("player"))?;
//!     let amount = args.get::<i32>("amount").unwrap_or(1);
//!     …
//!     Ok(CommandOutput::success("commands.xp.success", vec![…]).with_count(players.len()))
//! }
//! ```

pub mod args;
pub mod context;
pub mod exec;
pub mod names;
pub mod output;
pub mod parser;
pub mod registry;
pub mod schema;
pub mod selector;

pub(crate) use args::arg_enum;
pub use args::{ArgEnum, Args, FromArg, RelativeFloat};
pub use context::{CommandContext, CommandSource, Operator, SourceKind};
pub use output::{CommandMessage, CommandOutput, CommandResult};
pub use registry::{CommandHandler, CommandRegistry};
pub use schema::CommandSchema;

/// Everything an executor module needs.
pub mod prelude {
    pub(crate) use super::arg_enum;
    pub(crate) use super::output::{failure, success};
    pub use super::parser::{CommandPosition, Target};
    pub use super::{ArgEnum, Args, CommandContext, CommandOutput, CommandResult, RelativeFloat};
    pub use bevy_ecs::prelude::{Entity, World};
    pub use glam::{DVec3, IVec3};
}

use bevy_ecs::prelude::*;
use parser::ParseError;

/// Command flag bit BDS sets on commands that work without cheats.
pub const FLAG_NOT_CHEAT: u16 = 0x80;

/// Runs one command line (with or without its leading `/`) for `source`.
///
/// Unknown names answer `commands.generic.unknown`; commands above the
/// source's permission level answer `commands.generic.error.permissions`, as
/// BDS does. An output without messages (`/say`, `/me`) is not sent.
pub fn execute(world: &mut World, source: CommandSource, line: &str) -> CommandOutput {
    if !world.contains_resource::<CommandRegistry>() {
        world.insert_resource(CommandRegistry::vanilla());
    }
    world.resource_scope(|world, registry: Mut<CommandRegistry>| {
        dispatch(world, &registry, source, line)
    })
}

fn dispatch(
    world: &mut World,
    registry: &CommandRegistry,
    source: CommandSource,
    line: &str,
) -> CommandOutput {
    let schema = registry.schema();
    let line = line.trim_start();
    let line = line.strip_prefix('/').unwrap_or(line).trim_end();
    let name = parser::command_name(line);
    let unknown = || CommandOutput::failure("commands.generic.unknown", vec![name.to_owned()]);
    let Some(definition) = schema.command(name) else {
        return unknown();
    };
    // Measured: an operator typing `/op` (admin level) gets
    // `commands.generic.error.permissions` naming the command.
    if definition.permission > source.permission {
        return CommandOutput::failure(
            "commands.generic.error.permissions",
            vec![definition.name.clone()],
        );
    }
    let args = match parser::parse(schema, line) {
        Ok(args) => args,
        Err(ParseError::Unknown(_)) => return unknown(),
        Err(ParseError::Syntax(error)) => {
            return CommandOutput::failure("commands.generic.syntax", error.params());
        }
    };
    let Some(handler) = registry.handler(&definition.name) else {
        return CommandOutput::not_implemented(&definition.name);
    };
    let mut context = CommandContext::new(world, source, schema.clone());
    handler
        .run(&mut context, &args)
        .unwrap_or_else(|failure| failure)
}
