//! One hand-written executor per vanilla command, registered by captured name.
//!
//! Each module exposes `pub fn run(&mut CommandContext, &Args) -> CommandResult`.
//! Captured commands without a `register` line answer
//! [`CommandOutput::not_implemented`](super::CommandOutput::not_implemented).

mod gamemode;
mod stop;
mod summon;
mod teleport;
#[cfg(test)]
pub(crate) mod test_support;

pub use stop::StopRequested;

use super::registry::CommandRegistry;

/// Registers every vanilla executor unastar has.
pub fn register_vanilla(registry: &mut CommandRegistry) {
    registry
        .register("gamemode", gamemode::run)
        .register("stop", stop::run)
        .register("summon", summon::run)
        .register("teleport", teleport::run);
}

#[cfg(test)]
mod tests {
    use super::super::registry::CommandRegistry;

    #[test]
    fn vanilla_registry_builds_against_the_captured_syntax() {
        // `register` panics for names the captured syntax does not have.
        let registry = CommandRegistry::vanilla();
        assert!(registry.handler("teleport").is_some());
        assert!(
            registry
                .unimplemented()
                .all(|name| registry.handler(name).is_none())
        );
    }
}
