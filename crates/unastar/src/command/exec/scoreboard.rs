//! `/scoreboard objectives …` and `/scoreboard players …`, on the world
//! [`Scoreboard`].
//!
//! Measured on BDS 1.26.51 (scripted command captures):
//! - `objectives add` adds the name to the `ScoreboardObjectives` soft enum
//!   (`UpdateSoftEnum`) and answers `…objectives.add.success`; `dummy` is
//!   the only criteria; a taken name is `…objectives.add.alreadyExists`;
//! - `objectives list` answers `§a%commands.scoreboard.objectives.list.count`
//!   then one `…list.entry [name, display name, criteria]` per objective;
//! - `objectives setdisplay` broadcasts `SetDisplayObjective` (sort order 0
//!   ascending, 1 descending, the default) and answers `…successSet [slot,
//!   objective]`; without an objective it clears the slot (empty names,
//!   order 0) and answers `…successCleared [slot]`;
//! - `objectives remove` removes every score (`SetScore` remove), then the
//!   objective (`RemoveObjective`) and the soft enum value;
//! - `players set|add|remove|random|operation` answer
//!   `…players.set.success [objective, name, score]` (add and remove:
//!   `…players.add|remove.success [amount, objective, name, score]`);
//! - `players list` answers `§a%commands.scoreboard.players.list.count`
//!   followed by one message whose key is `%%s` per holder joined by `, `,
//!   with the holders' names as parameters; `players list <holder>` (and
//!   `*`, every holder) answers `§a%…list.player.count` and one
//!   `…list.player.entry [score, display name, objective]` per score, the
//!   success count being the number of scores; an empty listing fails;
//! - holders that are neither players nor fake players are named by their
//!   unique id; scores are only sent (`SetScore`) for objectives a slot
//!   shows, and showing what a slot shows sends nothing;
//! - `players test` answers `…test.success` / `…test.failed [score, min,
//!   max]`; `players reset` answers `…resetscore.success [objective, name]`,
//!   or `…list.player.empty [name]` for a holder without scores.
//!
//! A name that is not an online player is a fake player. The score shown
//! under player names (`belowname`) is not sent as actor data yet.

use crate::command::args::Operation;
use crate::command::prelude::*;
use crate::entity::components::Player;
use crate::world::scoreboard::{Display, DisplaySlot, ScoreHolder, Scoreboard, SortOrder};
use bevy_ecs::prelude::Mut;
use jolyne::valentine::types::{
    ActorUniqueId, ChangeEntityScorejson, ChangeFakePlayerScorejson, ChangePlayerScorejson,
    EnumsSoftEnumUpdateType, PlayerScoreboardId, RemoveScorejson, ScoreboardId,
    SetScorePacketScoreInfoItem,
};
use jolyne::valentine::{
    RemoveObjectivePacket, SetDisplayObjectivePacket, SetScorePacket, UpdateSoftEnumPacket,
};
use rand::Rng;

arg_enum! {
    /// Captured `ScoreboardSortOrder`.
    pub enum SortOrderArg in "ScoreboardSortOrder" {
        Ascending = "ascending",
        Descending = "descending",
    }
}

arg_enum! {
    /// Captured `ScoreboardDisplaySlotSortable`.
    pub enum SortableSlot in "ScoreboardDisplaySlotSortable" {
        List = "list",
        Sidebar = "sidebar",
    }
}

arg_enum! {
    /// Captured `ScoreboardPlayersNumAction`.
    pub enum NumAction in "ScoreboardPlayersNumAction" {
        Set = "set",
        Add = "add",
        Remove = "remove",
    }
}

overloads! {
    pub enum ScoreboardArgs for "scoreboard" {
        0 => AddObjective {
            category: EnumValue,
            action: EnumValue,
            objective: SoftEnumValue,
            criteria: EnumValue,
            display_name: Option<Text> = "displayName",
        },
        1 => RemoveObjective { category: EnumValue, action: EnumValue, objective: SoftEnumValue },
        2 => ListObjectives { category: EnumValue, action: EnumValue },
        3 => SetDisplaySortable {
            category: EnumValue,
            action: EnumValue,
            slot: SortableSlot = "displaySlot",
            objective: Option<SoftEnumValue>,
            order: Option<SortOrderArg> = "sortOrder",
        },
        4 => SetDisplayBelowName {
            category: EnumValue,
            action: EnumValue,
            slot: EnumValue = "displaySlot",
            objective: Option<SoftEnumValue>,
        },
        5 => ListPlayers {
            category: EnumValue,
            action: EnumValue,
            holder: Option<WildcardTarget> = "playername",
        },
        6 => Reset {
            category: EnumValue,
            action: EnumValue,
            holder: WildcardTarget = "player",
            objective: Option<SoftEnumValue>,
        },
        7 => Test {
            category: EnumValue,
            action: EnumValue,
            holder: WildcardTarget = "player",
            objective: SoftEnumValue,
            min: WildcardInt,
            max: Option<WildcardInt>,
        },
        8 => Random {
            category: EnumValue,
            action: EnumValue,
            holder: WildcardTarget = "player",
            objective: SoftEnumValue,
            min: i32,
            max: i32,
        },
        9 => Change {
            category: EnumValue,
            action: NumAction,
            holder: WildcardTarget = "player",
            objective: SoftEnumValue,
            count: i32,
        },
        10 => Operation {
            category: EnumValue,
            action: EnumValue,
            target: WildcardTarget = "targetName",
            target_objective: SoftEnumValue = "targetObjective",
            operation: Operation,
            source: WildcardTarget = "selector",
            objective: SoftEnumValue,
        },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: ScoreboardArgs) -> CommandResult {
    ctx.world.get_resource_or_insert_with(Scoreboard::default);
    use ScoreboardArgs as S;
    match args {
        S::AddObjective {
            objective,
            criteria,
            display_name,
            ..
        } => add_objective(ctx, &objective, &criteria, display_name.as_deref()),
        S::RemoveObjective { objective, .. } => remove_objective(ctx, &objective),
        S::ListObjectives { .. } => list_objectives(ctx),
        S::SetDisplaySortable {
            slot,
            objective,
            order,
            ..
        } => {
            let slot = match slot {
                SortableSlot::List => DisplaySlot::List,
                SortableSlot::Sidebar => DisplaySlot::Sidebar,
            };
            let order = match order {
                Some(SortOrderArg::Ascending) => SortOrder::Ascending,
                _ => SortOrder::Descending,
            };
            set_display(ctx, slot, objective.as_deref(), order)
        }
        S::SetDisplayBelowName { objective, .. } => set_display(
            ctx,
            DisplaySlot::BelowName,
            objective.as_deref(),
            SortOrder::Descending,
        ),
        S::ListPlayers { holder, .. } => match holder {
            Some(holder) => {
                let holders = holders(ctx, &holder)?;
                list_holder_scores(ctx, &holders)
            }
            None => list_holders(ctx),
        },
        S::Reset {
            holder, objective, ..
        } => reset(ctx, &holder, objective.as_deref()),
        S::Test {
            holder,
            objective,
            min,
            max,
            ..
        } => test(ctx, &holder, &objective, min, max),
        S::Random {
            holder,
            objective,
            min,
            max,
            ..
        } => {
            // Measured: `random … 4 4` is an invalid range too.
            if min >= max {
                return failure!("commands.scoreboard.players.random.invalidRange", min, max);
            }
            let objective = existing(ctx, &objective)?;
            let holders = holders(ctx, &holder)?;
            let mut output = CommandOutput::default();
            for holder in holders {
                let value = ctx
                    .world
                    .resource_mut::<crate::entity::EntityRandom>()
                    .0
                    .gen_range(min..=max);
                set_score(ctx, &holder, &objective, value);
                let name = holder_name(ctx, &holder);
                output.add_success(
                    "commands.scoreboard.players.set.success",
                    vec![objective.clone(), name, value.to_string()],
                );
            }
            Ok(output)
        }
        S::Change {
            action,
            holder,
            objective,
            count,
            ..
        } => change(ctx, action, &holder, &objective, count),
        S::Operation {
            target,
            target_objective,
            operation,
            source,
            objective,
            ..
        } => operation_(
            ctx,
            &target,
            &target_objective,
            &operation,
            &source,
            &objective,
        ),
    }
}

fn board<'a>(ctx: &'a CommandContext<'_>) -> &'a Scoreboard {
    ctx.world.resource::<Scoreboard>()
}

fn board_mut<'a>(ctx: &'a mut CommandContext<'_>) -> Mut<'a, Scoreboard> {
    ctx.world.resource_mut::<Scoreboard>()
}

/// The objective's name if it exists (`commands.scoreboard.objectiveNotFound`).
fn existing(ctx: &CommandContext<'_>, objective: &str) -> Result<String, CommandOutput> {
    match board(ctx).objective(objective) {
        Some(objective) => Ok(objective.name.clone()),
        None => failure!("commands.scoreboard.objectiveNotFound", objective),
    }
}

fn add_objective(
    ctx: &mut CommandContext<'_>,
    name: &str,
    criteria: &str,
    display_name: Option<&str>,
) -> CommandResult {
    if !board_mut(ctx).add_objective(name, display_name.unwrap_or(name), criteria) {
        return failure!("commands.scoreboard.objectives.add.alreadyExists", name);
    }
    ctx.broadcast(soft_enum(EnumsSoftEnumUpdateType::Add, name));
    success!("commands.scoreboard.objectives.add.success", name)
}

fn remove_objective(ctx: &mut CommandContext<'_>, name: &str) -> CommandResult {
    let was_shown = board(ctx).objective(name).is_some() && shown(ctx, name);
    let Some(removed) = board_mut(ctx).remove_objective(name) else {
        return failure!("commands.scoreboard.objectiveNotFound", name);
    };
    let removals: Vec<SetScorePacketScoreInfoItem> = removed
        .scores()
        .iter()
        .map(|(id, _)| remove_entry(*id, name))
        .collect();
    if was_shown && !removals.is_empty() {
        ctx.broadcast(SetScorePacket {
            score_info: removals,
        });
    }
    ctx.broadcast(RemoveObjectivePacket {
        objective_name: name.into(),
    });
    ctx.broadcast(soft_enum(EnumsSoftEnumUpdateType::Remove, name));
    success!("commands.scoreboard.objectives.remove.success", name)
}

/// Measured: listed by name, success count the number of objectives; none
/// is a failure.
fn list_objectives(ctx: &mut CommandContext<'_>) -> CommandResult {
    let mut objectives = board(ctx).objectives().to_vec();
    if objectives.is_empty() {
        return failure!("commands.scoreboard.objectives.list.empty");
    }
    objectives.sort_by(|a, b| a.name.cmp(&b.name));
    let mut output = CommandOutput::success(
        "§a%commands.scoreboard.objectives.list.count",
        vec![objectives.len().to_string()],
    );
    for objective in &objectives {
        output.add_message(
            "commands.scoreboard.objectives.list.entry",
            vec![
                objective.name.clone(),
                objective.display_name.clone(),
                objective.criteria.clone(),
            ],
        );
    }
    Ok(output.with_count(objectives.len()))
}

fn set_display(
    ctx: &mut CommandContext<'_>,
    slot: DisplaySlot,
    objective: Option<&str>,
    order: SortOrder,
) -> CommandResult {
    let Some(name) = objective else {
        board_mut(ctx).set_display(slot, None);
        ctx.broadcast(cleared_display(slot));
        return success!(
            "commands.scoreboard.objectives.setdisplay.successCleared",
            slot.name()
        );
    };
    let Some(objective) = board(ctx).objective(name).cloned() else {
        return failure!("commands.scoreboard.objectiveNotFound", name);
    };
    let display = Display {
        objective: objective.name.clone(),
        order,
    };
    // Measured: showing what the slot shows sends nothing; replacing another
    // display clears the slot first.
    let shown = board(ctx).display(slot).cloned();
    if shown.as_ref() == Some(&display) {
        return success!(
            "commands.scoreboard.objectives.setdisplay.successSet",
            slot.name(),
            objective.name
        );
    }
    if shown.is_some() {
        ctx.broadcast(cleared_display(slot));
    }
    board_mut(ctx).set_display(slot, Some(display));
    ctx.broadcast(SetDisplayObjectivePacket {
        display_slot_name: slot.name().into(),
        objective_name: objective.name.as_str().into(),
        objective_display_name: objective.display_name.as_str().into(),
        criteria_name: objective.criteria.as_str().into(),
        sort_order: order.wire(),
    });
    success!(
        "commands.scoreboard.objectives.setdisplay.successSet",
        slot.name(),
        objective.name
    )
}

/// The holders a target names: `*` every holder with a score, a selector
/// its entities, a name the online player of that name or a fake player.
fn holders(
    ctx: &mut CommandContext<'_>,
    target: &WildcardTarget,
) -> Result<Vec<ScoreHolder>, CommandOutput> {
    match &target.0 {
        None => Ok(board(ctx)
            .scored_holders()
            .into_iter()
            .map(|(_, holder)| holder.clone())
            .collect()),
        Some(Target::Name(name)) => Ok(vec![
            ctx.entities(Some(&Target::Name(name.clone())))
                .ok()
                .and_then(|found| found.first().copied())
                .map_or_else(|| ScoreHolder::Fake(name.clone()), ScoreHolder::Entity),
        ]),
        Some(selector) => Ok(ctx
            .entities(Some(selector))?
            .into_iter()
            .map(ScoreHolder::Entity)
            .collect()),
    }
}

/// Players by name, fake players by their name, other actors by their
/// unique id (measured: a cow is listed as "-4294967276").
fn holder_name(ctx: &CommandContext<'_>, holder: &ScoreHolder) -> String {
    match holder {
        ScoreHolder::Entity(entity) if ctx.world.get::<Player>(*entity).is_some() => {
            ctx.name(*entity)
        }
        ScoreHolder::Entity(entity) => super::visuals::runtime_id(ctx.world, *entity)
            .unwrap_or_default()
            .to_string(),
        ScoreHolder::Fake(name) => name.clone(),
    }
}

/// Sets a score and sends it when the objective is shown.
fn set_score(ctx: &mut CommandContext<'_>, holder: &ScoreHolder, objective: &str, value: i32) {
    let id = board_mut(ctx).set_score(holder, objective, value);
    if shown(ctx, objective) {
        let entry = change_entry(ctx.world, id, holder, objective, value);
        ctx.broadcast(SetScorePacket {
            score_info: vec![entry],
        });
    }
}

fn change(
    ctx: &mut CommandContext<'_>,
    action: NumAction,
    holder: &WildcardTarget,
    objective: &str,
    count: i32,
) -> CommandResult {
    let objective = existing(ctx, objective)?;
    let holders = holders(ctx, holder)?;
    let mut output = CommandOutput::default();
    for holder in holders {
        let current = board(ctx).score(&holder, &objective).unwrap_or(0);
        let value = match action {
            NumAction::Set => count,
            NumAction::Add => current.wrapping_add(count),
            NumAction::Remove => current.wrapping_sub(count),
        };
        set_score(ctx, &holder, &objective, value);
        let name = holder_name(ctx, &holder);
        match action {
            NumAction::Set => output.add_success(
                "commands.scoreboard.players.set.success",
                vec![objective.clone(), name, value.to_string()],
            ),
            NumAction::Add => output.add_success(
                "commands.scoreboard.players.add.success",
                vec![
                    count.to_string(),
                    objective.clone(),
                    name,
                    value.to_string(),
                ],
            ),
            NumAction::Remove => output.add_success(
                "commands.scoreboard.players.remove.success",
                vec![
                    count.to_string(),
                    objective.clone(),
                    name,
                    value.to_string(),
                ],
            ),
        }
    }
    Ok(output)
}

fn operation_(
    ctx: &mut CommandContext<'_>,
    target: &WildcardTarget,
    target_objective: &str,
    operation: &str,
    source: &WildcardTarget,
    source_objective: &str,
) -> CommandResult {
    let target_objective = existing(ctx, target_objective)?;
    let source_objective = existing(ctx, source_objective)?;
    let targets = holders(ctx, target)?;
    let sources = holders(ctx, source)?;
    let mut output = CommandOutput::default();
    for target in &targets {
        for source in &sources {
            // Measured: a source without a score answers like an empty
            // listing, then `allMatchesFailed` when nothing was updated.
            let Some(theirs) = board(ctx).score(source, &source_objective) else {
                let name = holder_name(ctx, source);
                output.add_failure("commands.scoreboard.players.list.player.empty", vec![name]);
                continue;
            };
            let ours = board(ctx).score(target, &target_objective).unwrap_or(0);
            let value = match operation {
                "=" => theirs,
                "+=" => ours.wrapping_add(theirs),
                "-=" => ours.wrapping_sub(theirs),
                "*=" => ours.wrapping_mul(theirs),
                // [INFERENCE] Division and remainder by zero leave the score.
                "/=" => ours.checked_div(theirs).unwrap_or(ours),
                "%=" => ours.checked_rem(theirs).unwrap_or(ours),
                "<" => ours.min(theirs),
                ">" => ours.max(theirs),
                "><" => {
                    set_score(ctx, source, &source_objective, ours);
                    theirs
                }
                _ => unreachable!("the parser only accepts the captured operators"),
            };
            set_score(ctx, target, &target_objective, value);
            let name = holder_name(ctx, target);
            output.add_success(
                "commands.scoreboard.players.set.success",
                vec![target_objective.clone(), name, value.to_string()],
            );
        }
    }
    if output.success_count == 0 {
        output.add_failure("commands.scoreboard.allMatchesFailed", vec![]);
        return Err(output);
    }
    Ok(output)
}

fn test(
    ctx: &mut CommandContext<'_>,
    holder: &WildcardTarget,
    objective: &str,
    min: WildcardInt,
    max: Option<WildcardInt>,
) -> CommandResult {
    let objective = existing(ctx, objective)?;
    let min = min.0.unwrap_or(i32::MIN);
    let max = max.and_then(|m| m.0).unwrap_or(i32::MAX);
    let holders = holders(ctx, holder)?;
    let mut output = CommandOutput::default();
    for holder in holders {
        let Some(score) = board(ctx).score(&holder, &objective) else {
            let name = holder_name(ctx, &holder);
            output.add_failure(
                "commands.scoreboard.players.score.notFound",
                vec![objective.clone(), name],
            );
            continue;
        };
        let params = vec![score.to_string(), min.to_string(), max.to_string()];
        if (min..=max).contains(&score) {
            output.add_success("commands.scoreboard.players.test.success", params);
        } else {
            output.add_failure("commands.scoreboard.players.test.failed", params);
        }
    }
    if output.success_count == 0 {
        return Err(output);
    }
    Ok(output)
}

fn reset(
    ctx: &mut CommandContext<'_>,
    holder: &WildcardTarget,
    objective: Option<&str>,
) -> CommandResult {
    let objective = objective.map(|o| existing(ctx, o)).transpose()?;
    let holders = holders(ctx, holder)?;
    let mut output = CommandOutput::default();
    for holder in holders {
        let name = holder_name(ctx, &holder);
        let names: Vec<String> = match &objective {
            Some(objective) => vec![objective.clone()],
            None => board(ctx)
                .scores_of(&holder)
                .into_iter()
                .map(|(objective, _)| objective.name.clone())
                .collect(),
        };
        let id = board(ctx).id_of(&holder);
        let mut removed = Vec::new();
        for objective in names {
            if board_mut(ctx).reset_score(&holder, &objective) {
                removed.push(objective);
            }
        }
        let Some(id) = id.filter(|_| !removed.is_empty()) else {
            output.add_failure("commands.scoreboard.players.list.player.empty", vec![name]);
            continue;
        };
        let entries: Vec<SetScorePacketScoreInfoItem> = removed
            .iter()
            .filter(|o| shown(ctx, o))
            .map(|o| remove_entry(id, o))
            .collect();
        if !entries.is_empty() {
            ctx.broadcast(SetScorePacket {
                score_info: entries,
            });
        }
        match &objective {
            Some(objective) => output.add_success(
                "commands.scoreboard.players.resetscore.success",
                vec![objective.clone(), name],
            ),
            None => output.add_success("commands.scoreboard.players.reset.success", vec![name]),
        }
    }
    if output.success_count == 0 {
        return Err(output);
    }
    Ok(output)
}

fn list_holders(ctx: &mut CommandContext<'_>) -> CommandResult {
    let holders: Vec<ScoreHolder> = board(ctx)
        .scored_holders()
        .into_iter()
        .map(|(_, holder)| holder.clone())
        .collect();
    if holders.is_empty() {
        return failure!("commands.scoreboard.players.list.empty");
    }
    let names: Vec<String> = holders.iter().map(|h| holder_name(ctx, h)).collect();
    let mut output = CommandOutput::success(
        "§a%commands.scoreboard.players.list.count",
        vec![names.len().to_string()],
    );
    output.add_message(vec!["%%s"; names.len()].join(", "), names);
    Ok(output)
}

fn list_holder_scores(ctx: &mut CommandContext<'_>, holders: &[ScoreHolder]) -> CommandResult {
    let mut output = CommandOutput::default();
    for holder in holders {
        let name = holder_name(ctx, holder);
        let scores: Vec<(String, String, i32)> = board(ctx)
            .scores_of(holder)
            .into_iter()
            .map(|(o, score)| (o.name.clone(), o.display_name.clone(), score))
            .collect();
        if scores.is_empty() {
            output.add_failure("commands.scoreboard.players.list.player.empty", vec![name]);
            continue;
        }
        // Measured: the success count is the number of scores listed.
        output.add_message(
            "§a%commands.scoreboard.players.list.player.count",
            vec![scores.len().to_string(), name],
        );
        output.success_count += scores.len() as u32;
        for (objective, display_name, score) in scores {
            output.add_message(
                "commands.scoreboard.players.list.player.entry",
                vec![score.to_string(), display_name, objective],
            );
        }
    }
    if output.success_count == 0 {
        return Err(output);
    }
    Ok(output)
}

fn cleared_display(slot: DisplaySlot) -> SetDisplayObjectivePacket {
    SetDisplayObjectivePacket {
        display_slot_name: slot.name().into(),
        objective_name: "".into(),
        objective_display_name: "".into(),
        criteria_name: "".into(),
        sort_order: 0,
    }
}

/// Whether any slot shows the objective: clients are only sent the scores of
/// shown objectives (measured).
fn shown(ctx: &CommandContext<'_>, objective: &str) -> bool {
    !board(ctx).slots_showing(objective).is_empty()
}

fn soft_enum(update_type: EnumsSoftEnumUpdateType, value: &str) -> UpdateSoftEnumPacket {
    UpdateSoftEnumPacket {
        enum_name: "ScoreboardObjectives".into(),
        values: vec![value.into()],
        update_type,
    }
}

fn remove_entry(id: i64, objective: &str) -> SetScorePacketScoreInfoItem {
    SetScorePacketScoreInfoItem::RemoveScore(RemoveScorejson {
        action: "remove".into(),
        scoreboard_id: ScoreboardId { scoreboard_id: id },
        objective_name: Some(objective.into()),
    })
}

/// A `SetScore` change entry: players by unique id, other actors by actor
/// id, fake players by name.
fn change_entry(
    world: &World,
    id: i64,
    holder: &ScoreHolder,
    objective: &str,
    value: i32,
) -> SetScorePacketScoreInfoItem {
    let scoreboard_id = ScoreboardId { scoreboard_id: id };
    match holder {
        ScoreHolder::Fake(name) => {
            SetScorePacketScoreInfoItem::ChangeFakePlayerScore(ChangeFakePlayerScorejson {
                action: "changefakeplayer".into(),
                scoreboard_id,
                objective_name: objective.into(),
                score_value: value,
                fake_player_name: name.as_str().into(),
            })
        }
        ScoreHolder::Entity(entity) => {
            let unique_id = super::visuals::runtime_id(world, *entity).unwrap_or(-1);
            if world.get::<Player>(*entity).is_some() {
                SetScorePacketScoreInfoItem::ChangePlayerScore(ChangePlayerScorejson {
                    action: "changeplayer".into(),
                    scoreboard_id,
                    objective_name: objective.into(),
                    score_value: value,
                    player_unique_id: PlayerScoreboardId {
                        player_unique_id: unique_id,
                    },
                })
            } else {
                SetScorePacketScoreInfoItem::ChangeEntityScore(ChangeEntityScorejson {
                    action: "changeentity".into(),
                    scoreboard_id,
                    objective_name: objective.into(),
                    score_value: value,
                    actor_id: ActorUniqueId {
                        actor_unique_id: unique_id,
                    },
                })
            }
        }
    }
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{Replay, assert_same_packets, packet_name};
    use jolyne::valentine::McpePacketData;
    use jolyne::valentine::types::SetScorePacketScoreInfoItem;

    /// The capture's unique ids: the player, then the cow the segment scores.
    const BDS_PLAYER: i64 = -4_294_967_295;
    const BDS_COW: i64 = -4_294_967_276;

    /// A `SetScore` with unique ids replaced through `ids`.
    fn mapped(packet: &McpePacketData, ids: &[(i64, i64)]) -> McpePacketData {
        let McpePacketData::SetScorePacket(packet) = packet else {
            return packet.clone();
        };
        let map = |id: i64| {
            ids.iter()
                .find(|(from, _)| *from == id)
                .map_or(id, |(_, to)| *to)
        };
        let mut packet = packet.clone();
        for item in &mut packet.score_info {
            match item {
                SetScorePacketScoreInfoItem::ChangePlayerScore(change) => {
                    change.player_unique_id.player_unique_id =
                        map(change.player_unique_id.player_unique_id);
                }
                SetScorePacketScoreInfoItem::ChangeEntityScore(change) => {
                    change.actor_id.actor_unique_id = map(change.actor_id.actor_unique_id);
                }
                _ => {}
            }
        }
        McpePacketData::SetScorePacket(packet)
    }

    #[test]
    fn scoreboard_matches_the_bds_capture() {
        // A cow is named by its unique id, which differs between servers;
        // these answers are compared below with the id mapped.
        let cow_lines = [
            "/scoreboard players set @e[type=cow,r=16] kills 1",
            "/scoreboard players list",
            "/scoreboard players list @e[type=cow,r=16]",
        ];
        let steps = Replay::new().assert_segment("scoreboard", &cow_lines);
        // unastar's name for the cow: the holder its score was set for.
        let cow: i64 = steps
            .iter()
            .find(|s| s.captured.command == cow_lines[0])
            .and_then(|s| s.ours.messages.first())
            .and_then(|m| m.params.get(1))
            .and_then(|id| id.parse().ok())
            .expect("the cow's score was set");
        let ids = [(BDS_PLAYER, 1), (BDS_COW, cow)];
        for step in steps
            .iter()
            .filter(|s| cow_lines.contains(&s.captured.command.as_str()))
        {
            let mut theirs = step.captured.output().expect("an answer");
            for message in &mut theirs.messages {
                for param in &mut message.params {
                    if *param == BDS_COW.to_string() {
                        *param = cow.to_string();
                    }
                }
            }
            assert_eq!(step.ours, theirs, "{}", step.captured.command);
        }
        assert_same_packets(
            &steps,
            &[
                "SetDisplayObjectivePacket",
                "RemoveObjectivePacket",
                "UpdateSoftEnumPacket",
            ],
            &[],
        );
        for step in &steps {
            let theirs: Vec<McpePacketData> = step
                .captured
                .named("SetScorePacket")
                .into_iter()
                .map(|p| mapped(p, &ids))
                .collect();
            let ours: Vec<McpePacketData> = step
                .sent
                .iter()
                .filter(|p| packet_name(p) == "SetScorePacket")
                .cloned()
                .collect();
            assert_eq!(ours, theirs, "{}: SetScore", step.captured.command);
        }
    }
}
