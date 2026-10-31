//! `/gamemode <mode> [player]`.
//!
//! Measured on BDS 1.26.51: `UpdatePlayerGameType` for each target and a
//! `gameMode.changed` message to it; the source gets
//! `commands.gamemode.success.self` (or `.other`).

use crate::command::prelude::*;
use crate::entity::components::{GameMode, Player, PlayerSession, RuntimeEntityId};
use crate::server::game::ServerConfigResource;
use crate::server::game::join::{abilities_packet, game_type};
use jolyne::valentine::UpdatePlayerGameTypePacket;
use jolyne::valentine::types::ActorUniqueId;

arg_enum! {
    /// The captured `GameMode` enum.
    pub enum GameModeName in "GameMode" {
        Survival = "survival" | "s",
        Creative = "creative" | "c",
        Adventure = "adventure" | "a",
        Spectator = "spectator",
        Default = "default" | "d",
    }
}

overloads! {
    pub enum GameModeArgs for "gamemode" {
        0 => Named { mode: GameModeName = "gameMode", player: Option<Target> },
        1 => Numbered { mode: i32 = "gameMode", player: Option<Target> },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: GameModeArgs) -> CommandResult {
    let (name, player) = match args {
        GameModeArgs::Named { mode, player } => (mode, player),
        GameModeArgs::Numbered { mode, player } => {
            let name = match mode {
                0 => GameModeName::Survival,
                1 => GameModeName::Creative,
                2 => GameModeName::Adventure,
                5 => GameModeName::Default,
                6 => GameModeName::Spectator,
                _ => return failure!("commands.gamemode.fail.invalid", mode),
            };
            (name, player)
        }
    };
    let mode = match name {
        GameModeName::Survival => GameMode::Survival,
        GameModeName::Creative => GameMode::Creative,
        GameModeName::Adventure => GameMode::Adventure,
        GameModeName::Spectator => GameMode::Spectator,
        GameModeName::Default => ctx
            .world
            .get_resource::<ServerConfigResource>()
            .map_or(GameMode::Survival, |config| config.0.default_gamemode),
    };
    let mode_name = format!("%createWorldScreen.gameMode.{}", mode_key(mode));

    let players = ctx.players(player.as_ref())?;
    let mut output = CommandOutput::default();
    for &player in &players {
        set_game_mode(ctx.world, player, mode);
        ctx.notify(player, "gameMode.changed", vec![mode_name.clone()]);
        if Some(player) == ctx.source.entity() {
            output.add_success("commands.gamemode.success.self", vec![mode_name.clone()]);
        } else {
            output.add_success(
                "commands.gamemode.success.other",
                vec![mode_name.clone(), ctx.name(player)],
            );
        }
    }
    Ok(output)
}

fn mode_key(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Survival => "survival",
        GameMode::Creative => "creative",
        GameMode::Adventure => "adventure",
        GameMode::Spectator => "spectator",
    }
}

/// Changes a player's game mode: `UpdatePlayerGameType` to every player (as
/// BDS does), and the new mode's abilities. BDS sends no abilities update, but
/// unastar's join abilities depend on the game mode (flight, instant build),
/// so they are resent rather than left stale.
pub fn set_game_mode(world: &mut World, player: Entity, mode: GameMode) {
    if world.get::<Player>(player).is_none() {
        return;
    }
    world.entity_mut(player).insert(mode);
    let runtime_id = world.get::<RuntimeEntityId>(player).map_or(0, |id| id.0);
    let operator = world.get::<crate::command::Operator>(player).is_some();
    let game_type = UpdatePlayerGameTypePacket {
        player_game_type: game_type(mode),
        // Players use one id as unique and runtime id.
        targetplayer: ActorUniqueId {
            actor_unique_id: runtime_id,
        },
        tick: crate::entity::components::input_tick_of(world, player),
    };
    let abilities = abilities_packet(runtime_id, mode, operator);
    let mut sessions = world.query::<(Entity, &PlayerSession)>();
    for (viewer, session) in sessions.iter(world) {
        if crate::server::broadcast::player_views::can_receive_actor(world, player, viewer) {
            let _ = session.send(game_type.clone().into());
            let _ = session.send(abilities.clone().into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;
    use jolyne::valentine::McpePacketData;
    use jolyne::valentine::types::EnumsGameType;

    #[test]
    fn game_mode_enum_covers_the_captured_values() {
        crate::command::args::assert_covers::<GameModeName>();
    }

    #[test]
    fn self_change_matches_the_bds_capture() {
        let mut h = Harness::new();
        let alice = h.player("Alice", true);
        let output = h.run(alice, "/gamemode creative");
        assert_eq!(
            output,
            CommandOutput::success(
                "commands.gamemode.success.self",
                vec!["%createWorldScreen.gameMode.creative".into()]
            )
        );
        assert_eq!(h.world().get::<GameMode>(alice), Some(&GameMode::Creative));
        let sent = h.sent(alice);
        assert!(sent.iter().any(
            |p| matches!(p, McpePacketData::UpdatePlayerGameTypePacket(p)
            if p.player_game_type == EnumsGameType::Creative)
        ));
        assert!(
            sent.iter()
                .any(|p| matches!(p, McpePacketData::TextPacket(t)
            if format!("{t:?}").contains("gameMode.changed")))
        );
    }

    #[test]
    fn numeric_modes_other_targets_and_invalid_ids() {
        let mut h = Harness::new();
        let alice = h.player("Alice", true);
        let bob = h.player("Bob", false);
        let output = h.run(alice, "/gamemode 1 Bob");
        assert_eq!(output.success_count, 1);
        assert_eq!(output.messages[0].key, "commands.gamemode.success.other");
        assert_eq!(output.messages[0].params[1], "Bob");
        assert_eq!(h.world().get::<GameMode>(bob), Some(&GameMode::Creative));
        assert_eq!(h.world().get::<GameMode>(alice), Some(&GameMode::Survival));

        let output = h.run(alice, "/gamemode 4");
        assert_eq!(
            output,
            CommandOutput::failure("commands.gamemode.fail.invalid", vec!["4".into()])
        );
        let output = h.run(alice, "/gamemode creative @a[m=spectator]");
        assert_eq!(output, CommandOutput::no_targets());
    }
}
