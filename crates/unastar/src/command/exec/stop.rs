//! `/stop` (owner level: the server console). The runtime loop saves players
//! and chunks and exits after the tick that inserted [`StopRequested`].

use crate::command::prelude::*;

/// Set by `/stop`; the server loop shuts down when it sees it.
#[derive(bevy_ecs::prelude::Resource, Debug, Default)]
pub struct StopRequested;

overloads! {
    pub enum StopArgs for "stop" {
        0 => Stop {},
    }
}

pub fn run(ctx: &mut CommandContext<'_>, _args: StopArgs) -> CommandResult {
    ctx.world.insert_resource(StopRequested);
    success!("commands.stop.start")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandSource, execute};

    #[test]
    fn console_stop_requests_shutdown() {
        let mut world = World::new();
        let output = execute(&mut world, CommandSource::server(), "stop");
        assert_eq!(output.to_text(), ["Stopping the server"]);
        assert!(world.contains_resource::<StopRequested>());
    }
}
