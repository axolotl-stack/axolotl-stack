//! `/execute`: its subcommands turn the source into a list of execution
//! contexts (who runs, where, facing which way), conditions filter the list,
//! and `run` runs a command once per context, with the caller's permission.
//!
//! Measured on BDS 1.26.51 (scripted command captures):
//! - `as` changes who runs (`/execute as @e[type=cow] run say moo` says
//!   `[Cow] moo` once per cow) but not where; `at` moves to the entity's
//!   feet and rotation; `positioned`, `rotated`, `facing`, `align` and
//!   `anchored eyes` (local coordinates from the eyes) change the position
//!   and rotation of every context;
//! - a final condition answers `commands.execute.trueCondition` (`blocks`:
//!   `…trueConditionWithCount [blocks compared]`) or
//!   `commands.execute.falseCondition [if|unless, block|blocks|entity|score]`;
//!   a condition that leaves no context before `run` answers the same
//!   failure; a score condition on a holder without that score adds
//!   `commands.execute.ifUnlessScore.targets.empty [name]`;
//! - `run` answers with the nested command's output for every context, one
//!   after the other (success count: the sum); a run that fails adds
//!   `commands.execute.failed [command, executor]` after its own messages.
//!
//! Only the overworld exists in unastar. `in nether` and `in the_end` run
//! the chain in a dimension where nothing is loaded and no actor lives, as
//! the BDS capture (segment `execute_in`) answers there: `@s` and plain `@a`
//! still find the player, a selector that looks at positions finds nothing,
//! `setblock`, `testforblock` and `fill` answer their `outOfWorld`, `clone`
//! `commands.generic.outOfWorld`, `summon` `commands.summon.failed`, and
//! `if block` `commands.execute.outRangedDetectPosition [x, y, z]` before its
//! false condition. Every other command run there, and `if blocks`, answer
//! a literal "not implemented" failure rather than act on the overworld.
//! `at` moves the context back to the overworld with its actor.

use super::setblock::BlockSpec;
use super::testfor::compare_regions;
use crate::command::args::{Comparison, Overloads};
use crate::command::context::{CommandSource, Dimension, SourceKind, entity_name};
use crate::command::parser::parse_overloads;
use crate::command::prelude::*;
use crate::entity::components::Player;
use crate::server::game::blocks::block_at;
use crate::world::scoreboard::{ScoreHolder, Scoreboard};

arg_enum! {
    /// Captured `ActorLocation`.
    pub enum Anchor in "ActorLocation" {
        Eyes = "eyes",
        Feet = "feet",
    }
}

arg_enum! {
    /// Captured `Dimension`.
    pub enum DimensionName in "Dimension" {
        Overworld = "overworld",
        Nether = "nether",
        TheEnd = "the_end",
    }
}

arg_enum! {
    /// Captured `BlocksScanMode`.
    pub enum ScanMode in "BlocksScanMode" {
        Masked = "masked",
        All = "all",
    }
}

arg_enum! {
    /// Captured `Option_If_Unless`.
    pub enum Condition in "Option_If_Unless" {
        If = "if",
        Unless = "unless",
    }
}

overloads! {
    pub enum ExecuteArgs for "execute" {
        0 => As { subcommand: EnumValue, origin: Target, chained: Chained = "chainedCommand" },
        1 => At { subcommand: EnumValue, origin: Target, chained: Chained = "chainedCommand" },
        2 => In { subcommand: EnumValue, dimension: DimensionName, chained: Chained = "chainedCommand" },
        3 => Positioned {
            subcommand: EnumValue,
            position: CommandPosition,
            chained: Chained = "chainedCommand",
        },
        4 => PositionedAs {
            subcommand: EnumValue,
            secondary: EnumValue = "secondary subcommand",
            origin: Target,
            chained: Chained = "chainedCommand",
        },
        5 => Rotated {
            subcommand: EnumValue,
            yaw: RelativeFloat,
            pitch: RelativeFloat,
            chained: Chained = "chainedCommand",
        },
        6 => RotatedAs {
            subcommand: EnumValue,
            secondary: EnumValue = "secondary subcommand",
            origin: Target,
            chained: Chained = "chainedCommand",
        },
        7 => Facing { subcommand: EnumValue, position: CommandPosition, chained: Chained = "chainedCommand" },
        8 => FacingEntity {
            subcommand: EnumValue,
            secondary: EnumValue = "secondary subcommand",
            origin: Target,
            anchor: Anchor,
            chained: Chained = "chainedCommand",
        },
        9 => Align { subcommand: EnumValue, axes: Text, chained: Chained = "chainedCommand" },
        10 => Anchored { subcommand: EnumValue, anchored: Anchor, chained: Chained = "chainedCommand" },
        11 => IfBlock {
            condition: Condition = "subcommand",
            secondary: EnumValue = "secondary subcommand",
            position: BlockPosition,
            block: EnumValue,
            chained: Option<Chained> = "chainedCommand",
        },
        12 => IfBlockStates {
            condition: Condition = "subcommand",
            secondary: EnumValue = "secondary subcommand",
            position: BlockPosition,
            block: EnumValue,
            block_states: BlockStates = "blockStates",
            chained: Option<Chained> = "chainedCommand",
        },
        13 => IfBlocks {
            condition: Condition = "subcommand",
            secondary: EnumValue = "secondary subcommand",
            begin: BlockPosition,
            end: BlockPosition,
            destination: BlockPosition,
            scan_mode: ScanMode = "scan mode",
            chained: Option<Chained> = "chainedCommand",
        },
        14 => IfEntity {
            condition: Condition = "subcommand",
            secondary: EnumValue = "secondary subcommand",
            target: Target,
            chained: Option<Chained> = "chainedCommand",
        },
        15 => IfScoreCompare {
            condition: Condition = "subcommand",
            secondary: EnumValue = "secondary subcommand",
            target: Target,
            objective: SoftEnumValue,
            operation: Comparison,
            source: Target,
            source_objective: SoftEnumValue = "objective",
            chained: Option<Chained> = "chainedCommand",
        },
        16 => IfScoreMatches {
            condition: Condition = "subcommand",
            secondary: EnumValue = "secondary subcommand",
            target: Target,
            objective: SoftEnumValue,
            matches: EnumValue,
            range: IntRange,
            chained: Option<Chained> = "chainedCommand",
        },
        17 => Run { subcommand: EnumValue, command: CommandLine },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: ExecuteArgs) -> CommandResult {
    let contexts = vec![ctx.source.clone()];
    step(ctx, contexts, args)
}

/// One subcommand over every context, then the rest of the chain.
fn step(
    ctx: &mut CommandContext<'_>,
    contexts: Vec<CommandSource>,
    args: ExecuteArgs,
) -> CommandResult {
    use ExecuteArgs as E;
    match args {
        E::As {
            origin, chained, ..
        } => {
            let next = expand(ctx, &contexts, &origin, |world, context, entity| {
                let mut context = context.clone();
                context.kind = if world.get::<Player>(entity).is_some() {
                    SourceKind::Player(entity)
                } else {
                    SourceKind::Actor(entity)
                };
                context.name = entity_name(world, entity);
                context
            })?;
            chain(ctx, next, &chained)
        }
        E::At {
            origin, chained, ..
        } => {
            let next = expand(ctx, &contexts, &origin, |world, context, entity| {
                let mut context = context.clone();
                move_to(world, &mut context, entity);
                // Every actor lives in the overworld.
                context.dimension = Dimension::Overworld;
                let rotation = world
                    .get::<crate::entity::components::transform::Rotation>(entity)
                    .copied()
                    .unwrap_or_default();
                context.yaw = rotation.yaw;
                context.pitch = rotation.pitch;
                context
            })?;
            chain(ctx, next, &chained)
        }
        E::In {
            dimension, chained, ..
        } => {
            let dimension = match dimension {
                DimensionName::Overworld => Dimension::Overworld,
                DimensionName::Nether => Dimension::Nether,
                DimensionName::TheEnd => Dimension::TheEnd,
            };
            let next = contexts
                .into_iter()
                .map(|mut context| {
                    context.dimension = dimension;
                    context
                })
                .collect();
            chain(ctx, next, &chained)
        }
        E::Positioned {
            position, chained, ..
        } => {
            let next = contexts
                .into_iter()
                .map(|mut context| {
                    context.position = resolve(ctx, &context, position);
                    context.anchor = 0.0;
                    context
                })
                .collect();
            chain(ctx, next, &chained)
        }
        E::PositionedAs {
            origin, chained, ..
        } => {
            let next = expand(ctx, &contexts, &origin, |world, context, entity| {
                let mut context = context.clone();
                move_to(world, &mut context, entity);
                context
            })?;
            chain(ctx, next, &chained)
        }
        E::Rotated {
            yaw,
            pitch,
            chained,
            ..
        } => {
            let next = contexts
                .into_iter()
                .map(|mut context| {
                    context.yaw = yaw.resolve(context.yaw);
                    context.pitch = pitch.resolve(context.pitch);
                    context
                })
                .collect();
            chain(ctx, next, &chained)
        }
        E::RotatedAs {
            origin, chained, ..
        } => {
            let next = expand(ctx, &contexts, &origin, |world, context, entity| {
                let mut context = context.clone();
                let rotation = world
                    .get::<crate::entity::components::transform::Rotation>(entity)
                    .copied()
                    .unwrap_or_default();
                context.yaw = rotation.yaw;
                context.pitch = rotation.pitch;
                context
            })?;
            chain(ctx, next, &chained)
        }
        E::Facing {
            position, chained, ..
        } => {
            let next = contexts
                .into_iter()
                .map(|mut context| {
                    let at = resolve(ctx, &context, position);
                    face(&mut context, at);
                    context
                })
                .collect();
            chain(ctx, next, &chained)
        }
        E::FacingEntity {
            origin,
            anchor,
            chained,
            ..
        } => {
            let next = expand(ctx, &contexts, &origin, |world, context, entity| {
                let mut context = context.clone();
                let at = match anchor {
                    Anchor::Eyes => crate::entity::geometry::eye_position(world, entity),
                    Anchor::Feet => crate::entity::geometry::feet_position(world, entity),
                };
                if let Some(at) = at {
                    face(&mut context, at);
                }
                context
            })?;
            chain(ctx, next, &chained)
        }
        E::Align { axes, chained, .. } => {
            let axes = &*axes;
            if axes.is_empty()
                || !axes.chars().all(|c| matches!(c, 'x' | 'y' | 'z'))
                || axes.len()
                    != axes
                        .chars()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
            {
                return failure!("commands.execute.align.invalidInput");
            }
            let next = contexts
                .into_iter()
                .map(|mut context| {
                    let p = &mut context.position;
                    if axes.contains('x') {
                        p.x = p.x.floor();
                    }
                    if axes.contains('y') {
                        p.y = p.y.floor();
                    }
                    if axes.contains('z') {
                        p.z = p.z.floor();
                    }
                    context
                })
                .collect();
            chain(ctx, next, &chained)
        }
        E::Anchored {
            anchored, chained, ..
        } => {
            let next = contexts
                .into_iter()
                .map(|mut context| {
                    context.anchor = match anchored {
                        Anchor::Feet => 0.0,
                        Anchor::Eyes => eye_height(ctx.world, &context),
                    };
                    context
                })
                .collect();
            chain(ctx, next, &chained)
        }
        E::IfBlock {
            condition,
            position,
            block,
            chained,
            ..
        } => {
            let spec = BlockSpec::resolve(&block, None)?;
            test(
                ctx,
                contexts,
                condition,
                "block",
                chained,
                |ctx, context| {
                    let pos = resolve(ctx, context, position.0).floor().as_ivec3();
                    detect_block(ctx, context, pos, |state| spec.matches(state))
                },
            )
        }
        E::IfBlockStates {
            condition,
            position,
            block,
            block_states,
            chained,
            ..
        } => {
            let spec = BlockSpec::resolve(&block, Some(&block_states))?;
            test(
                ctx,
                contexts,
                condition,
                "block",
                chained,
                |ctx, context| {
                    let pos = resolve(ctx, context, position.0).floor().as_ivec3();
                    detect_block(ctx, context, pos, |state| spec.matches(state))
                },
            )
        }
        E::IfBlocks { .. } if contexts.iter().any(|c| c.dimension != Dimension::Overworld) => {
            Err(other_dimension())
        }
        E::IfBlocks {
            condition,
            begin,
            end,
            destination,
            scan_mode,
            chained,
            ..
        } => test(
            ctx,
            contexts,
            condition,
            "blocks",
            chained,
            |ctx, context| {
                let [begin, end, destination] = [begin, end, destination]
                    .map(|p| resolve(ctx, context, p.0).floor().as_ivec3());
                match compare_regions(
                    ctx.world,
                    begin,
                    end,
                    destination,
                    scan_mode == ScanMode::Masked,
                ) {
                    Ok(compared) => Check {
                        holds: true,
                        count: Some(compared),
                        error: None,
                    },
                    Err(_) => Check::holds(false),
                }
            },
        ),
        E::IfEntity {
            condition,
            target,
            chained,
            ..
        } => test(
            ctx,
            contexts,
            condition,
            "entity",
            chained,
            |ctx, context| {
                Check::holds(with_source(ctx, context, |ctx| {
                    ctx.entities(Some(&target)).is_ok()
                }))
            },
        ),
        E::IfScoreCompare {
            condition,
            target,
            objective,
            operation,
            source,
            source_objective,
            chained,
            ..
        } => test(
            ctx,
            contexts,
            condition,
            "score",
            chained,
            |ctx, context| {
                let ours = match score(ctx, context, &target, &objective) {
                    Ok(score) => score,
                    Err(missing) => return missing,
                };
                let theirs = match score(ctx, context, &source, &source_objective) {
                    Ok(score) => score,
                    Err(missing) => return missing,
                };
                Check::holds(match &*operation {
                    "<" => ours < theirs,
                    "<=" => ours <= theirs,
                    "=" => ours == theirs,
                    ">=" => ours >= theirs,
                    ">" => ours > theirs,
                    _ => false,
                })
            },
        ),
        E::IfScoreMatches {
            condition,
            target,
            objective,
            range,
            chained,
            ..
        } => test(
            ctx,
            contexts,
            condition,
            "score",
            chained,
            |ctx, context| match score(ctx, context, &target, &objective) {
                Ok(score) => Check::holds(range.contains(score)),
                Err(missing) => missing,
            },
        ),
        E::Run { command, .. } => {
            let name = crate::command::parser::command_name(&command)
                .trim_start_matches('/')
                .to_owned();
            let mut output = CommandOutput::default();
            for context in contexts {
                let executor = context.name.clone();
                let result = if context.dimension == Dimension::Overworld
                    || RUN_IN_EMPTY_DIMENSIONS.contains(&name.as_str())
                {
                    ctx.run(context, &command)
                } else {
                    other_dimension()
                };
                let failed = result.success_count == 0 && !result.messages.is_empty();
                output.success_count += result.success_count;
                output.messages.extend(result.messages);
                if failed {
                    output.add_failure("commands.execute.failed", vec![name.clone(), executor]);
                }
            }
            if output.success_count == 0 && !output.messages.is_empty() {
                return Err(output);
            }
            Ok(output)
        }
    }
}

/// Parses the rest of the chain (validated with the whole line) and runs it.
fn chain(
    ctx: &mut CommandContext<'_>,
    contexts: Vec<CommandSource>,
    chained: &str,
) -> CommandResult {
    let definition = ctx
        .schema
        .command("execute")
        .expect("/execute is in the captured syntax");
    let args = parse_overloads(&ctx.schema, definition, chained, 0)
        .expect("the parser validated the chain with the whole line");
    step(ctx, contexts, ExecuteArgs::from_args(&args))
}

/// Commands whose answers in a dimension where nothing is loaded are
/// measured and come out of the usual checks ([`Dimension`]).
const RUN_IN_EMPTY_DIMENSIONS: [&str; 6] = [
    "testfor",
    "setblock",
    "testforblock",
    "fill",
    "clone",
    "summon",
];

/// What unastar answers for anything else in the nether or the end.
fn other_dimension() -> CommandOutput {
    CommandOutput::failure(
        "/execute in another dimension is not implemented in unastar yet",
        vec![],
    )
}

/// `if block`: the block at `pos` in the context's dimension. Measured in
/// the nether: where nothing is loaded the test cannot be judged
/// (`commands.execute.outRangedDetectPosition [x, y, z]`, then the false
/// condition); [INFERENCE] an unloaded overworld chunk answers the same.
/// Outside the world's height the block is air, as `testforblock` measured.
fn detect_block(
    ctx: &CommandContext<'_>,
    context: &CommandSource,
    pos: IVec3,
    matches: impl Fn(u32) -> bool,
) -> Check {
    if !super::setblock::in_height(pos.y) {
        return Check::holds(matches(*crate::world::chunk::blocks::AIR));
    }
    let found = if context.dimension == Dimension::Overworld {
        block_at(ctx.world, (pos.x, pos.y, pos.z))
    } else {
        None
    };
    match found {
        Some(state) => Check::holds(matches(state)),
        None => Check {
            holds: false,
            count: None,
            error: Some((
                "commands.execute.outRangedDetectPosition".into(),
                vec![pos.x.to_string(), pos.y.to_string(), pos.z.to_string()],
            )),
        },
    }
}

/// What a condition found in one context.
struct Check {
    holds: bool,
    /// Blocks compared (`if blocks`), shown when the test ends the chain.
    count: Option<u64>,
    /// A context the test could not judge, with BDS's message.
    error: Option<(String, Vec<String>)>,
}

impl Check {
    fn holds(holds: bool) -> Self {
        Self {
            holds,
            count: None,
            error: None,
        }
    }
}

/// Applies a condition to every context; at the end of the chain answers
/// whether it held.
fn test(
    ctx: &mut CommandContext<'_>,
    contexts: Vec<CommandSource>,
    condition: Condition,
    kind: &str,
    chained: Option<Chained>,
    mut check: impl FnMut(&mut CommandContext<'_>, &CommandSource) -> Check,
) -> CommandResult {
    let wanted = condition == Condition::If;
    let mut passed = Vec::new();
    let mut errors = CommandOutput::default();
    let mut count = None;
    for context in contexts {
        let found = check(ctx, &context);
        if let Some((key, params)) = found.error {
            errors.add_failure(key, params);
            continue;
        }
        if found.holds == wanted {
            count = found.count.or(count);
            passed.push(context);
        }
    }
    let word = match condition {
        Condition::If => "if",
        Condition::Unless => "unless",
    };
    if passed.is_empty() {
        errors.add_failure(
            "commands.execute.falseCondition",
            vec![word.to_owned(), kind.to_owned()],
        );
        return Err(errors);
    }
    match chained {
        Some(chained) => chain(ctx, passed, &chained),
        None => Ok(match count {
            Some(count) if wanted => CommandOutput::success(
                "commands.execute.trueConditionWithCount",
                vec![count.to_string()],
            ),
            _ => CommandOutput::success("commands.execute.trueCondition", vec![]),
        }),
    }
}

/// One new context per entity `origin` matches from each context.
fn expand(
    ctx: &mut CommandContext<'_>,
    contexts: &[CommandSource],
    origin: &Target,
    mut make: impl FnMut(&World, &CommandSource, Entity) -> CommandSource,
) -> Result<Vec<CommandSource>, CommandOutput> {
    let mut next = Vec::new();
    for context in contexts {
        let entities = with_source(ctx, context, |ctx| ctx.entities(Some(origin)))?;
        for entity in entities {
            next.push(make(ctx.world, context, entity));
        }
    }
    Ok(next)
}

/// Runs `f` with `source` as the context's source.
fn with_source<T>(
    ctx: &mut CommandContext<'_>,
    source: &CommandSource,
    f: impl FnOnce(&mut CommandContext<'_>) -> T,
) -> T {
    let saved = std::mem::replace(&mut ctx.source, source.clone());
    let out = f(ctx);
    ctx.source = saved;
    out
}

fn resolve(
    ctx: &mut CommandContext<'_>,
    context: &CommandSource,
    position: CommandPosition,
) -> DVec3 {
    with_source(ctx, context, |ctx| ctx.position(Some(position)))
}

fn move_to(world: &World, context: &mut CommandSource, entity: Entity) {
    if let Some(feet) = crate::entity::geometry::feet_position(world, entity) {
        context.position = feet;
    }
}

/// Turns a context to look from its anchor at `at`.
fn face(context: &mut CommandSource, at: DVec3) {
    let from = context.position + DVec3::Y * context.anchor;
    let (yaw, pitch) = crate::entity::geometry::look_at(from, at);
    context.yaw = yaw;
    context.pitch = pitch;
}

/// Measured: `anchored eyes` puts a player's eyes 1.52 above its feet
/// (`/execute anchored eyes positioned ^ ^ ^1 run summon pig ~ ~ ~` from feet
/// at y 200 summons at y 201.52), not at the 1.62 its movement reports.
const PLAYER_ANCHOR_EYES: f64 = 1.52;

/// Height of the context executor's `eyes` anchor (0 for the console).
/// [INFERENCE] Other actors anchor at their eye height.
fn eye_height(world: &World, context: &CommandSource) -> f64 {
    context
        .entity()
        .and_then(|entity| {
            if world.get::<Player>(entity).is_some() {
                return Some(PLAYER_ANCHOR_EYES);
            }
            let eye = crate::entity::geometry::eye_position(world, entity)?;
            let feet = crate::entity::geometry::feet_position(world, entity)?;
            Some(eye.y - feet.y)
        })
        .unwrap_or(0.0)
}

/// The score of the one holder `target` names; a holder without one fails
/// the test with `commands.execute.ifUnlessScore.targets.empty [name]`
/// (measured).
fn score(
    ctx: &mut CommandContext<'_>,
    context: &CommandSource,
    target: &Target,
    objective: &str,
) -> Result<i32, Check> {
    let found = with_source(ctx, context, |ctx| {
        ctx.entities(Some(target))
            .ok()
            .and_then(|found| found.first().copied())
    });
    let (holder, name) = match (target, found) {
        (_, Some(entity)) => (ScoreHolder::Entity(entity), ctx.name(entity)),
        (Target::Name(name), None) => (ScoreHolder::Fake(name.clone()), name.clone()),
        (Target::Selector(_), None) => return Err(Check::holds(false)),
    };
    ctx.world
        .get_resource::<Scoreboard>()
        .and_then(|board| board.score(&holder, objective))
        .ok_or(Check {
            holds: false,
            count: None,
            error: Some((
                "commands.execute.ifUnlessScore.targets.empty".to_owned(),
                vec![name],
            )),
        })
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{Captured, Replay, assert_same_packets, segment};
    use crate::entity::components::transform::Rotation;
    use crate::entity::mob::MobKind;
    use bevy_ecs::prelude::Entity;
    use glam::DVec3;
    use jolyne::valentine::McpePacketData;
    use jolyne::valentine::types::Vec3;

    /// Teleports that turn the player like a cow. BDS's cows had wandered and
    /// turned by then (their AI runs between the scripted lines; the replay's
    /// cows stand still), so these `MovePlayer`s match BDS's but for rotation.
    const TURNED_BY_A_COW: [&str; 3] = [
        "/execute rotated as @e[type=cow,r=16,c=1] run tp @s ~ ~ ~ ~ ~",
        "/execute facing entity @e[type=cow,r=16,c=1] feet run tp @s ~ ~ ~ ~ ~",
        "/execute as @s at @s run tp @s ~ ~1 ~",
    ];

    /// The nether and the end, where nothing is loaded: every answer of the
    /// segment, and no block or actor changed in the overworld.
    #[test]
    fn execute_in_matches_the_bds_capture() {
        let mut replay = Replay::new();
        let cows = |replay: &mut Replay| {
            let world = replay.h.world();
            world
                .query_filtered::<(), bevy_ecs::prelude::With<MobKind>>()
                .iter(world)
                .count()
        };
        let before = cows(&mut replay);
        let steps = replay.assert_segment("execute_in", &[]);
        assert_eq!(steps.len(), 13);
        assert_eq!(
            cows(&mut replay),
            before,
            "no cow summoned in the overworld"
        );
        let stone = crate::registry::BlockRegistry::vanilla()
            .default_state_id_by_name("minecraft:stone")
            .unwrap();
        for pos in [(8, 100, 8), (9, 100, 9)] {
            assert_ne!(
                crate::server::game::blocks::block_at(replay.h.world(), pos),
                Some(stone),
                "{pos:?}"
            );
        }
    }

    #[test]
    fn other_commands_in_another_dimension_are_not_implemented() {
        let mut replay = Replay::new();
        for line in [
            "/execute in nether run tp @s 8 100 8",
            "/execute in the_end if blocks 0 0 0 0 0 0 1 1 1 all",
        ] {
            let (output, _) = replay.run(line);
            assert!(!output.is_success(), "{line}");
            assert!(
                output.messages[0].key.contains("not implemented"),
                "{line}: {output:?}"
            );
        }
        // `at` brings the context back to the overworld.
        let (output, _) = replay.run("/execute in nether at @s run testfor @e[r=1]");
        assert!(output.is_success(), "{output:?}");
    }

    #[test]
    fn execute_matches_the_bds_capture() {
        let steps = Replay::new().assert_segment("execute", &[]);
        assert_same_packets(&steps, &["TextPacket"], &[]);
        assert_same_packets(&steps, &["MovePlayerPacket"], &TURNED_BY_A_COW);
        for command in TURNED_BY_A_COW {
            let step = steps
                .iter()
                .find(|s| s.captured.command == command)
                .expect("the command is in the segment");
            assert_eq!(
                positions(step.sent_named("MovePlayerPacket")),
                positions(step.captured.named("MovePlayerPacket")),
                "{command}"
            );
        }
    }

    fn positions(packets: Vec<&McpePacketData>) -> Vec<Vec3> {
        packets
            .into_iter()
            .filter_map(|packet| match packet {
                McpePacketData::MovePlayerPacket(moved) => Some(moved.position.clone()),
                _ => None,
            })
            .collect()
    }

    /// The replay player turns as BDS's did (see [`TURNED_BY_A_COW`]).
    fn turn_like_bds(replay: &mut Replay, captured: &Captured) {
        for packet in captured.named("MovePlayerPacket") {
            if let McpePacketData::MovePlayerPacket(moved) = packet
                && let Some(mut rotation) = replay.h.world().get_mut::<Rotation>(replay.player)
            {
                *rotation = Rotation::new(moved.rotation.y, moved.rotation.x);
            }
        }
    }

    /// Every actor the segment summons (`align`, `anchored eyes` with local
    /// coordinates) stands where BDS's `AddActor` put it, the player facing
    /// where BDS's faced.
    #[test]
    fn execute_summons_where_bds_does() {
        let mut replay = Replay::new();
        let mut checked = 0;
        for captured in segment("execute") {
            let before = mobs(&mut replay);
            replay.run(&captured.command);
            turn_like_bds(&mut replay, captured);
            for packet in captured.named("AddActorPacket") {
                let McpePacketData::AddActorPacket(added) = packet else {
                    continue;
                };
                let theirs = DVec3::new(
                    f64::from(added.position.x),
                    f64::from(added.position.y),
                    f64::from(added.position.z),
                );
                let summoned: Vec<Entity> = mobs(&mut replay)
                    .into_iter()
                    .filter(|e| !before.contains(e))
                    .collect();
                let [mob] = summoned[..] else {
                    panic!("{}: summoned {summoned:?}", captured.command);
                };
                let ours = crate::entity::geometry::feet_position(replay.h.world(), mob).unwrap();
                assert!(
                    ours.distance(theirs) < 1e-3,
                    "{}: unastar {ours}, BDS {theirs}",
                    captured.command
                );
                checked += 1;
            }
        }
        // Two cows, the aligned pig and the pig in front of the eyes.
        assert_eq!(checked, 4);
    }

    fn mobs(replay: &mut Replay) -> Vec<Entity> {
        let world = replay.h.world();
        let mut query = world.query_filtered::<Entity, bevy_ecs::prelude::With<MobKind>>();
        query.iter(world).collect()
    }
}
