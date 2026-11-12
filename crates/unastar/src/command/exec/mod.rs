//! One executor per vanilla command, each over its typed overloads
//! (declared with [`overloads!`](super::overloads) next to the executor).
//!
//! Captured commands without a `register` line answer
//! [`CommandOutput::not_implemented`](super::CommandOutput::not_implemented).

mod chat;
mod clear;
mod clone;
mod damage;
mod daylock;
mod difficulty;
mod effect;
mod enchant;
mod event;
#[cfg(test)]
pub(crate) mod evidence;
mod execute;
mod fill;
mod gamemode;
mod gamerule;
mod give;
mod help;
mod inputpermission;
mod kick;
mod kill;
mod mobevent;
mod music;
mod op;
mod replaceitem;
mod scoreboard;
mod scriptevent;
mod setblock;
mod sound;
mod spawnpoint;
mod stop;
mod summon;
mod tag;
mod teleport;
mod tellraw;
#[cfg(test)]
pub(crate) mod test_support;
mod testfor;
mod time;
mod title;
mod visuals;
mod weather;
mod xp;

pub use gamemode::set_game_mode;
pub use op::OperatorList;
pub use stop::StopRequested;
pub use teleport::move_entity;

use super::registry::CommandRegistry;

/// Registers every vanilla executor unastar has. Each `register` checks the
/// typed declaration against the captured syntax.
pub fn register_vanilla(registry: &mut CommandRegistry) {
    registry
        .register(visuals::run_camerashake)
        .register(clear::run)
        .register(spawnpoint::run_clearspawnpoint)
        .register(clone::run)
        .register(damage::run)
        .register(daylock::run)
        .register(op::run_deop)
        .register(difficulty::run)
        .register(effect::run)
        .register(enchant::run)
        .register(event::run)
        .register(execute::run)
        .register(fill::run)
        .register(visuals::run_fog)
        .register(gamemode::run)
        .register(gamerule::run)
        .register(give::run)
        .register(help::run)
        .register(visuals::run_hud)
        .register(inputpermission::run)
        .register(kick::run)
        .register(kill::run)
        .register(chat::run_list)
        .register(chat::run_me)
        .register(mobevent::run)
        .register(music::run)
        .register(op::run_op)
        .register(visuals::run_particle)
        .register(visuals::run_playanimation)
        .register(sound::run_playsound)
        .register(replaceitem::run)
        .register(chat::run_say)
        .register(scoreboard::run)
        .register(scriptevent::run)
        .register(setblock::run)
        .register(spawnpoint::run_setworldspawn)
        .register(spawnpoint::run_spawnpoint)
        .register(stop::run)
        .register(sound::run_stopsound)
        .register(summon::run)
        .register(tag::run)
        .register(teleport::run)
        .register(chat::run_tell)
        .register(tellraw::run)
        .register(testfor::run_testfor)
        .register(testfor::run_testforblock)
        .register(testfor::run_testforblocks)
        .register(time::run)
        .register(title::run)
        .register(title::run_raw)
        .register(weather::run_toggledownfall)
        .register(weather::run)
        .register(xp::run);
}

#[cfg(test)]
mod tests {
    use super::super::registry::CommandRegistry;

    /// Prints every captured command whose answer differs (development aid).
    #[test]
    #[ignore = "survey of the whole capture; run with --ignored --nocapture"]
    fn survey_the_bds_capture() {
        use super::evidence::{Replay, capture};
        let mut segments: Vec<&str> = Vec::new();
        for captured in capture() {
            if captured.segment != "prelude" && !segments.contains(&captured.segment.as_str()) {
                segments.push(&captured.segment);
            }
        }
        for name in segments {
            let mut replay = Replay::new();
            for step in replay.segment(name) {
                let theirs = step.captured.output().unwrap_or_default();
                if step.ours != theirs {
                    println!(
                        "[{name}] {}
    BDS: {:?}
    us:  {:?}",
                        step.captured.command, theirs, step.ours
                    );
                }
            }
        }
    }

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

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use super::evidence::Replay;

    /// Commands above the operator's level, unknown commands and syntax errors.
    #[test]
    fn permission_and_syntax_answers_match_the_bds_capture() {
        Replay::new().assert_segment("permissions", &[]);
    }
}
