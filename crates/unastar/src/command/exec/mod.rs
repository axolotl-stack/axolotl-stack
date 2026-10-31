//! One executor per vanilla command, each over its typed overloads
//! (declared with [`overloads!`](super::overloads) next to the executor).
//!
//! Captured commands without a `register` line answer
//! [`CommandOutput::not_implemented`](super::CommandOutput::not_implemented).

mod chat;
mod clear;
mod clone;
mod difficulty;
mod effect;
mod enchant;
mod fill;
mod gamemode;
mod gamerule;
mod give;
mod help;
mod kill;
mod op;
mod setblock;
mod spawnpoint;
mod stop;
mod summon;
mod teleport;
#[cfg(test)]
pub(crate) mod test_support;
mod time;
mod weather;
mod xp;

pub(crate) use gamemode::restore_game_mode;
pub use gamemode::{base_game_mode, mode_override, set_game_mode, set_mode_override};
pub use op::OperatorList;
pub use stop::StopRequested;
pub use teleport::move_entity;

use super::registry::CommandRegistry;

/// Registers every vanilla executor unastar has. Each `register` checks the
/// typed declaration against the captured syntax.
pub fn register_vanilla(registry: &mut CommandRegistry) {
    registry
        .register(clear::run)
        .register(clone::run)
        .register(op::run_deop)
        .register(difficulty::run)
        .register(effect::run)
        .register(enchant::run)
        .register(fill::run)
        .register(gamemode::run)
        .register(gamerule::run)
        .register(give::run)
        .register(help::run)
        .register(kill::run)
        .register(chat::run_list)
        .register(chat::run_me)
        .register(op::run_op)
        .register(chat::run_say)
        .register(setblock::run)
        .register(spawnpoint::run_setworldspawn)
        .register(spawnpoint::run_spawnpoint)
        .register(stop::run)
        .register(summon::run)
        .register(teleport::run)
        .register(chat::run_tell)
        .register(time::run)
        .register(weather::run)
        .register(xp::run);
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
