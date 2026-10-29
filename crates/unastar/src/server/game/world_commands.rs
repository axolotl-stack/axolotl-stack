//! Applies the world and player commands (`/time`, `/difficulty`,
//! `/gamerule`, `/gamemode`, `/kill`) with world access, broadcasts what
//! they change the way BDS does, and replies to the executor.

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{ActorUniqueId, PlayerInputTick};
use jolyne::valentine::{
    GameRulesChangedPacket, McpePacket, SetDifficultyPacket, SetPlayerGameTypePacket,
    UpdatePlayerGameTypePacket,
};

use super::commands::send_output;
use super::join::{abilities_packet, game_type};
use crate::command::{
    CommandOutput, GameRuleRequest, PlayerTarget, TimeQuery, TimeRequest, WorldRequest,
};
use crate::ecs::resources::TickCounter;
use crate::entity::components::{GameMode, Player, PlayerName, PlayerSession, RuntimeEntityId};
use crate::world::clock;
use crate::world::rules::{Difficulty, GameRule, GameRules, WorldTime};

/// Apply `request` for `executor` and send it the outcome.
pub(super) fn apply_world_request(world: &mut World, executor: Entity, request: WorldRequest) {
    let output = match request {
        WorldRequest::Time(request) => apply_time(world, request),
        WorldRequest::Difficulty(difficulty) => apply_difficulty(world, difficulty),
        WorldRequest::GameRule(request) => apply_game_rule(world, request),
        WorldRequest::GameMode { mode, target } => apply_game_mode(world, executor, mode, &target),
        WorldRequest::Kill(target) => apply_kill(world, executor, &target),
    };
    if let Some(session) = world.get::<PlayerSession>(executor) {
        send_output(session, output);
    }
}

fn apply_time(world: &mut World, request: TimeRequest) -> CommandOutput {
    let mut output = CommandOutput::default();
    let now = *world.resource::<WorldTime>();
    if let TimeRequest::Query(query) = request {
        output.message(match query {
            TimeQuery::Daytime => format!("Daytime is {}", now.time_of_day()),
            TimeQuery::Gametime => {
                format!("Gametime is {}", world.resource::<TickCounter>().current)
            }
            TimeQuery::Day => format!("Day is {}", now.ticks / WorldTime::DAY_TICKS),
        });
        return output;
    }
    let Some(ticks) = request.resolve(now) else {
        output.error(format!(
            "The time must stay between 0 and {}",
            WorldTime::MAX
        ));
        return output;
    };
    clock::set_time(world, ticks);
    output.message(match request {
        TimeRequest::Add(delta) => format!("Added {delta} to the time"),
        _ => format!("Set the time to {ticks}"),
    });
    output
}

fn apply_difficulty(world: &mut World, difficulty: Difficulty) -> CommandOutput {
    *world.resource_mut::<Difficulty>() = difficulty;
    broadcast(
        world,
        McpePacket::from(SetDifficultyPacket {
            difficulty: difficulty.wire(),
        }),
    );
    let mut output = CommandOutput::default();
    output.message(format!(
        "Set game difficulty to {}",
        difficulty.name().to_ascii_uppercase()
    ));
    output
}

fn apply_game_rule(world: &mut World, request: GameRuleRequest) -> CommandOutput {
    let mut output = CommandOutput::default();
    let rules = world.resource::<GameRules>();
    match request {
        GameRuleRequest::List => output.message(
            GameRule::ALL
                .map(|rule| format!("{} = {}", rule.name(), rules.get(rule)))
                .join(", "),
        ),
        GameRuleRequest::Query(rule) => {
            output.message(format!("{} = {}", rule.name(), rules.get(rule)));
        }
        GameRuleRequest::Set(rule, value) => {
            let mut rules = world.resource_mut::<GameRules>();
            rules.set(rule, value);
            // BDS announces only the rule that changed.
            let packet = McpePacket::from(GameRulesChangedPacket {
                rule_data: jolyne::valentine::types::GameRulesChangedPacketData {
                    rules_list: vec![rules.wire(rule)],
                },
            });
            broadcast(world, packet);
            output.message(format!(
                "Game rule {} has been updated to {value}",
                rule.name()
            ));
        }
    }
    output
}

/// Switch each target's game mode: the player's own `SetPlayerGameType`,
/// `UpdatePlayerGameType` to every player (as BDS broadcasts it) and the
/// abilities of the new mode.
fn apply_game_mode(
    world: &mut World,
    executor: Entity,
    mode: GameMode,
    target: &PlayerTarget,
) -> CommandOutput {
    let mut output = CommandOutput::default();
    let targets = match targets(world, executor, target) {
        Ok(targets) => targets,
        Err(error) => {
            output.error(error);
            return output;
        }
    };
    let tick = world.resource::<TickCounter>().current;
    for player in targets {
        let Some(runtime_id) = world.get::<RuntimeEntityId>(player).map(|id| id.0) else {
            continue;
        };
        if let Some(mut current) = world.get_mut::<GameMode>(player) {
            *current = mode;
        }
        if let Some(session) = world.get::<PlayerSession>(player) {
            let _ = session.send(McpePacket::from(SetPlayerGameTypePacket {
                player_game_type: game_type(mode),
            }));
        }
        // Players are announced with one id for the unique and runtime id.
        broadcast(
            world,
            McpePacket::from(UpdatePlayerGameTypePacket {
                player_game_type: game_type(mode),
                targetplayer: ActorUniqueId {
                    actor_unique_id: runtime_id,
                },
                tick: PlayerInputTick { inputtick: tick },
            }),
        );
        broadcast(world, McpePacket::from(abilities_packet(runtime_id, mode)));
        output.message(if player == executor {
            format!("Set own game mode to {}", mode_name(mode))
        } else {
            format!(
                "Set {}'s game mode to {}",
                name(world, player),
                mode_name(mode)
            )
        });
    }
    output
}

fn apply_kill(world: &mut World, executor: Entity, target: &PlayerTarget) -> CommandOutput {
    let mut output = CommandOutput::default();
    let targets = match targets(world, executor, target) {
        Ok(targets) => targets,
        Err(error) => {
            output.error(error);
            return output;
        }
    };
    for player in targets {
        let name = name(world, player);
        if crate::entity::player_life::kill_player(world, player) {
            output.message(format!("Killed {name}"));
        }
    }
    if output.is_empty() {
        output.error("No targets matched selector");
    }
    output
}

/// The players `target` selects for `executor`.
fn targets(
    world: &mut World,
    executor: Entity,
    target: &PlayerTarget,
) -> Result<Vec<Entity>, String> {
    let mut players = world.query_filtered::<(Entity, &PlayerName), With<Player>>();
    let found: Vec<Entity> = match target {
        PlayerTarget::Executor => vec![executor],
        PlayerTarget::All => players.iter(world).map(|(entity, _)| entity).collect(),
        PlayerTarget::Named(wanted) => players
            .iter(world)
            .filter(|(_, name)| name.0.eq_ignore_ascii_case(wanted))
            .map(|(entity, _)| entity)
            .collect(),
    };
    if found.is_empty() {
        return Err("No targets matched selector".into());
    }
    Ok(found)
}

fn name(world: &World, player: Entity) -> String {
    world
        .get::<PlayerName>(player)
        .map_or_else(String::new, |name| name.0.clone())
}

fn mode_name(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Survival => "Survival",
        GameMode::Creative => "Creative",
        GameMode::Adventure => "Adventure",
        GameMode::Spectator => "Spectator",
    }
}

/// Send `packet` to every player.
fn broadcast(world: &mut World, packet: McpePacket) {
    let mut sessions = world.query::<&PlayerSession>();
    for session in sessions.iter(world) {
        let _ = session.send(packet.clone());
    }
}

#[cfg(test)]
mod tests;
