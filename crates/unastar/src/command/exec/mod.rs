//! One executor per vanilla command, each over its typed overloads
//! (declared with [`overloads!`](super::overloads) next to the executor).
//!
//! Captured commands without a `register` line answer
//! [`CommandOutput::not_implemented`](super::CommandOutput::not_implemented).

mod difficulty;
mod gamemode;
mod gamerule;
mod kill;
mod stop;
mod summon;
mod teleport;
#[cfg(test)]
pub(crate) mod test_support;
mod time;

pub use gamemode::set_game_mode;
pub use stop::StopRequested;
pub use teleport::move_entity;

use super::registry::CommandRegistry;

/// Registers every vanilla executor unastar has. Each `register` checks the
/// typed declaration against the captured syntax.
pub fn register_vanilla(registry: &mut CommandRegistry) {
    registry
        .register(difficulty::run)
        .register(gamemode::run)
        .register(gamerule::run)
        .register(kill::run)
        .register(stop::run)
        .register(summon::run)
        .register(teleport::run)
        .register(time::run);
}

#[cfg(test)]
mod tests {
    use super::super::registry::CommandRegistry;

    #[test]
    fn every_declaration_matches_the_captured_syntax() {
        // `register` panics on a declaration that differs from the syntax.
        let registry = CommandRegistry::vanilla();
        assert!(registry.handler("teleport").is_some());
        assert!(
            registry
                .unimplemented()
                .all(|name| registry.handler(name).is_none())
        );
    }
}
