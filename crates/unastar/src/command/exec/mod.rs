//! One executor per vanilla command, each over its typed overloads
//! (declared with [`overloads!`](super::overloads) next to the executor).
//!
//! Captured commands without a `register` line answer
//! [`CommandOutput::not_implemented`](super::CommandOutput::not_implemented).

mod admin;
mod camera;
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
mod function;
mod gamemode;
mod gamerule;
mod gametest;
mod give;
mod help;
mod inputpermission;
mod kick;
mod kill;
mod loot;
mod mobevent;
mod music;
mod op;
mod place;
#[cfg(test)]
pub(crate) mod probe;
mod recipe;
mod replaceitem;
mod scoreboard;
mod scriptevent;
mod server_tools;
mod setblock;
mod sound;
mod spawnpoint;
mod spreadplayers;
mod stop;
mod structure;
mod summon;
mod tag;
mod teleport;
mod tellraw;
#[cfg(test)]
pub(crate) mod test_support;
mod testfor;
mod tickingarea;
mod time;
mod title;
mod visuals;
mod weather;
mod xp;

pub use admin::{
    Allowlist, PlayerLimit, SaveState, ServerFiles, ServerIdentity, WorldFiles, load_identity,
};
pub use camera::{CameraState, join_packets as camera_join_packets};
pub use function::{FunctionRegistry, run_due as run_scheduled_functions};
pub use gamemode::set_game_mode;
pub use kick::Kicked;
pub use mobevent::MobEvents;
pub use op::OperatorList;
pub use recipe::{UnlockedRecipes, join_packet as recipes_join_packet};
pub use scoreboard::join_packets as scoreboard_join_packets;
pub(crate) use setblock::box_loaded;
pub use stop::StopRequested;
pub use structure::{StructureTemplate, Structures};
pub use teleport::move_entity;
pub use tickingarea::{TickingArea, TickingAreas};

use super::registry::CommandRegistry;

/// Registers every vanilla executor unastar has. Each `register` checks the
/// typed declaration against the captured syntax.
pub fn register_vanilla(registry: &mut CommandRegistry) {
    registry
        .register(camera::run_aimassist)
        .register(admin::run_allowlist)
        .register_raw("camera", camera::run_camera)
        .register(visuals::run_camerashake)
        .register(clear::run)
        .register(spawnpoint::run_clearspawnpoint)
        .register(clone::run)
        .register(admin::run_changesetting)
        .register(camera::run_controlscheme)
        .register(damage::run)
        .register(daylock::run)
        .register(op::run_deop)
        .register(gametest::run_dialogue)
        .register(difficulty::run)
        .register(effect::run)
        .register(enchant::run)
        .register(event::run)
        .register(execute::run)
        .register(fill::run)
        .register(function::run_function_command)
        .register(visuals::run_fog)
        .register(gamemode::run)
        .register(gamerule::run)
        .register(gametest::run_gametest)
        .register(give::run)
        .register(help::run)
        .register(visuals::run_hud)
        .register(inputpermission::run)
        .register(kick::run)
        .register(kill::run)
        .register_raw("loot", loot::run)
        .register(chat::run_list)
        .register(chat::run_me)
        .register(mobevent::run)
        .register(music::run)
        .register(op::run_op)
        .register(op::run_permission)
        .register(place::run_place)
        .register(place::run_locate)
        .register(server_tools::run_reloadconfig)
        .register(server_tools::run_reloadpacketlimitconfig)
        .register(server_tools::run_script)
        .register(server_tools::run_wsserver)
        .register(function::run_reload)
        .register(visuals::run_particle)
        .register(visuals::run_playanimation)
        .register(sound::run_playsound)
        .register(recipe::run)
        .register(replaceitem::run)
        .register(chat::run_say)
        .register(admin::run_save)
        .register(function::run_schedule)
        .register(scoreboard::run)
        .register(scriptevent::run)
        .register(setblock::run)
        .register(admin::run_setmaxplayers)
        .register(admin::run_sendshowstoreoffer)
        .register(admin::run_serveridentity)
        .register(spawnpoint::run_setworldspawn)
        .register(spawnpoint::run_spawnpoint)
        .register(spreadplayers::run)
        .register(stop::run)
        .register(structure::run)
        .register(sound::run_stopsound)
        .register(summon::run)
        .register(tag::run)
        .register(teleport::run)
        .register(chat::run_tell)
        .register(tellraw::run)
        .register(testfor::run_testfor)
        .register(testfor::run_testforblock)
        .register(testfor::run_testforblocks)
        .register(tickingarea::run)
        .register(time::run)
        .register(title::run)
        .register(title::run_raw)
        .register(admin::run_transfer)
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
                if let Some(("console", _)) = step.captured.directive() {
                    if step.console != step.captured.console {
                        println!(
                            "[{name}] {}
    BDS: {:?}
    us:  {:?}",
                            step.captured.command, step.captured.console, step.console
                        );
                    }
                    continue;
                }
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

    /// A second player: commands naming or selecting it, what it receives
    /// (target messages, the operators' broadcast, transfer, store offer,
    /// kick) and the console's op/deop/permission changes of it.
    #[test]
    fn a_second_player_sees_what_bds_sends_it() {
        use super::evidence::assert_same_client_packets;
        let steps = Replay::new().assert_segment("players", &[]);
        assert_same_client_packets(
            &steps,
            "Alice",
            &[
                "TextPacket",
                "SetTitlePacket",
                "TransferPacket",
                "ShowStoreOfferPacket",
                "DisconnectPacket",
            ],
            &[],
        );
        assert_same_client_packets(&steps, "Bob", &["DisconnectPacket"], &[]);
    }
}
