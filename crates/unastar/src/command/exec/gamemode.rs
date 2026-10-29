//! `/gamemode <mode> [player]`.
//!
//! Measured on BDS 1.26.51: each target gets `UpdatePlayerGameType` and a
//! `gameMode.changed` message; the source gets `commands.gamemode.success.self`
//! (or `.other`). BDS sends no `UpdateAbilities`; the client derives them.

use crate::command::prelude::*;
use crate::entity::components::{GameMode, Player, RuntimeEntityId};
use crate::server::game::ServerConfigResource;
use jolyne::valentine::UpdatePlayerGameTypePacket;
use jolyne::valentine::types::{ActorUniqueId, EnumsGameType, PlayerInputTick};

arg_enum! {
    /// The captured `GameMode` enum.
    pub enum GameModeArg in "GameMode" {
        Survival = "survival" | "s",
        Creative = "creative" | "c",
        Adventure = "adventure" | "a",
        Spectator = "spectator",
        Default = "default" | "d",
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: &Args) -> CommandResult {
    let requested = match args.choice::<GameModeArg>("gameMode") {
        Some(mode) => mode,
        None => {
            let id = args.get::<i32>("gameMode").expect("int overload");
            match id {
                0 => GameModeArg::Survival,
                1 => GameModeArg::Creative,
                2 => GameModeArg::Adventure,
                5 => GameModeArg::Default,
                6 => GameModeArg::Spectator,
                _ => return failure!("commands.gamemode.fail.invalid", id),
            }
        }
    };
    let mode = match requested {
        GameModeArg::Survival => GameMode::Survival,
        GameModeArg::Creative => GameMode::Creative,
        GameModeArg::Adventure => GameMode::Adventure,
        GameModeArg::Spectator => GameMode::Spectator,
        GameModeArg::Default => ctx
            .world
            .get_resource::<ServerConfigResource>()
            .map_or(GameMode::Survival, |config| config.0.default_gamemode),
    };
    let mode_name = format!("%createWorldScreen.gameMode.{}", mode_key(mode));

    let players = ctx.players(args.get("player"))?;
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

/// Changes a player's game mode and tells their client.
pub fn set_game_mode(world: &mut World, player: Entity, mode: GameMode) {
    if world.get::<Player>(player).is_none() {
        return;
    }
    world.entity_mut(player).insert(mode);
    let unique_id = world.get::<RuntimeEntityId>(player).map_or(0, |id| id.0);
    let packet = UpdatePlayerGameTypePacket {
        player_game_type: match mode {
            GameMode::Survival => EnumsGameType::Survival,
            GameMode::Creative => EnumsGameType::Creative,
            GameMode::Adventure => EnumsGameType::Adventure,
            GameMode::Spectator => EnumsGameType::Spectator,
        },
        targetplayer: ActorUniqueId {
            actor_unique_id: unique_id,
        },
        tick: PlayerInputTick { inputtick: 0 },
    };
    if let Some(session) = world.get::<crate::entity::components::PlayerSession>(player) {
        let _ = session.send(packet.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;
    use jolyne::valentine::McpePacketData;

    #[test]
    fn game_mode_enum_covers_the_captured_values() {
        crate::command::args::assert_covers::<GameModeArg>();
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
        assert!(
            matches!(&sent[0], McpePacketData::UpdatePlayerGameTypePacket(p)
            if p.player_game_type == EnumsGameType::Creative)
        );
        assert!(matches!(&sent[1], McpePacketData::TextPacket(t)
            if format!("{t:?}").contains("gameMode.changed")));
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
