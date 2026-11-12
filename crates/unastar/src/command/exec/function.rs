//! `/function`, `/schedule` and `/reload`: function files.
//!
//! A function is a `.mcfunction` file of commands, one per line (`#`
//! comments and blank lines skipped), named by its path under a behaviour
//! pack's `functions/` without the extension (`a/b`). unastar reads them
//! from the world's behaviour packs (`worlds/<world>/behavior_packs/*/
//! functions/`, BDS's layout), at start and on `/reload`.
//!
//! Measured on BDS 1.26.51 (command capture, `functions` segment; its world
//! has no functions): every form naming a missing function answers
//! `commands.function.functionNameNotFound [name]`, clearing a ticking
//! area's schedule that has none `commands.schedule.tickingarea.clear.failure
//! [name]`, and `/reload` (admin level) answers
//! `commands.reload.success` on the console.
//!
//! [INFERENCE] from the lang: a run answers `commands.function.success
//! [entries]` (its commands run, a nested `/function` counting as one,
//! bounded by the `functionCommandLimit` rule); `/schedule delay add` answers
//! `commands.schedule.delay.functionQueued [name, ticks, gametime]` and runs
//! the function as its source that many ticks later (`replace`, the default,
//! drops earlier schedules of it); `on_area_loaded` queues it until every
//! chunk of the area is loaded (`commands.schedule.functionQueued`).

use crate::command::prelude::*;
use crate::command::{CommandSource, SourceKind};
use crate::ecs::resources::TickCounter;
use bevy_ecs::prelude::Resource;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Function files by name.
#[derive(Resource, Debug, Clone, Default)]
pub struct FunctionRegistry {
    /// Behaviour pack directories the functions come from.
    packs: Vec<PathBuf>,
    functions: BTreeMap<String, Vec<String>>,
}

impl FunctionRegistry {
    /// Functions of every behaviour pack directory under `packs_root`
    /// (`worlds/<world>/behavior_packs`).
    pub fn load(packs_root: &Path) -> Self {
        let packs = std::fs::read_dir(packs_root)
            .map(|entries| {
                let mut packs: Vec<PathBuf> = entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir())
                    .collect();
                packs.sort();
                packs
            })
            .unwrap_or_default();
        let mut registry = Self {
            packs,
            functions: BTreeMap::new(),
        };
        registry.reload();
        registry
    }

    /// Reads the packs' function files again.
    pub fn reload(&mut self) {
        self.functions.clear();
        for pack in self.packs.clone() {
            let root = pack.join("functions");
            let mut pending = vec![root.clone()];
            while let Some(dir) = pending.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for path in entries.flatten().map(|entry| entry.path()) {
                    if path.is_dir() {
                        pending.push(path);
                    } else if path.extension().is_some_and(|ext| ext == "mcfunction")
                        && let (Ok(relative), Ok(text)) =
                            (path.strip_prefix(&root), std::fs::read_to_string(&path))
                    {
                        let name = relative
                            .with_extension("")
                            .to_string_lossy()
                            .replace('\\', "/");
                        self.functions.insert(name, lines(&text));
                    }
                }
            }
        }
    }

    /// A function defined in code (tests, plugins).
    pub fn insert(&mut self, name: &str, text: &str) {
        self.functions.insert(name.to_owned(), lines(text));
    }

    pub fn get(&self, name: &str) -> Option<&[String]> {
        self.functions.get(name).map(Vec::as_slice)
    }
}

fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Functions waiting to run.
#[derive(Resource, Debug, Clone, Default)]
pub struct FunctionSchedule {
    delayed: Vec<Delayed>,
    on_area: Vec<OnArea>,
}

#[derive(Debug, Clone)]
struct Delayed {
    function: String,
    due: u64,
    source: CommandSource,
}

#[derive(Debug, Clone)]
struct OnArea {
    function: String,
    /// Chunk coordinates, inclusive.
    min: (i32, i32),
    max: (i32, i32),
    /// The ticking area it waits for, if named so.
    ticking_area: Option<String>,
    source: CommandSource,
}

/// Runs a function's entries for `source`; returns how many commands ran.
///
/// `depth` is this call's `/function` nesting; nested calls take from the
/// same budget.
fn run_function(
    ctx: &mut CommandContext<'_>,
    source: &CommandSource,
    name: &str,
    budget: &mut i64,
    depth: u32,
) -> usize {
    let Some(entries) = ctx
        .world
        .get_resource::<FunctionRegistry>()
        .and_then(|registry| registry.get(name))
        .map(<[String]>::to_vec)
    else {
        return 0;
    };
    let mut ran = 0;
    for line in entries {
        if *budget <= 0 {
            break;
        }
        *budget -= 1;
        ran += 1;
        ctx.world.insert_resource(RunningFunction {
            budget: *budget,
            depth,
        });
        ctx.run(source.clone(), &line);
        if let Some(running) = ctx.world.remove_resource::<RunningFunction>() {
            *budget = running.budget;
        }
    }
    ran
}

/// The `functionCommandLimit` rule.
fn command_limit(world: &World) -> i64 {
    world
        .get_resource::<crate::world::rules::GameRules>()
        .and_then(|rules| rules.value("functionCommandLimit"))
        .and_then(|(_, value)| match value {
            unastar_data::game_rules::GameRuleValue::Int(limit) => Some(i64::from(limit)),
            _ => None,
        })
        .unwrap_or(10_000)
}

fn exists(world: &World, name: &str) -> bool {
    world
        .get_resource::<FunctionRegistry>()
        .is_some_and(|registry| registry.get(name).is_some())
}

fn not_found(name: &str) -> CommandOutput {
    CommandOutput::failure(
        "commands.function.functionNameNotFound",
        vec![name.to_owned()],
    )
}

overloads! {
    pub enum FunctionArgs for "function" {
        0 => Run { name: FilePath },
    }
}

/// Deepest `/function` nesting run. A safety cap until BDS's own limit is
/// measured: a function that calls itself would otherwise recurse until the
/// stack overflows. Deeper calls run nothing.
const MAX_FUNCTION_DEPTH: u32 = 64;

/// The running outermost `/function`: the `functionCommandLimit` budget its
/// nested calls share, and how deep they are.
#[derive(Resource)]
struct RunningFunction {
    budget: i64,
    depth: u32,
}

pub fn run_function_command(ctx: &mut CommandContext<'_>, args: FunctionArgs) -> CommandResult {
    let FunctionArgs::Run { name } = args;
    if !exists(ctx.world, &name) {
        return Err(not_found(&name));
    }
    let source = ctx.source.clone();
    let outer = ctx.world.remove_resource::<RunningFunction>();
    let (mut budget, depth) = match &outer {
        Some(outer) => (outer.budget, outer.depth + 1),
        None => (command_limit(ctx.world), 1),
    };
    let ran = if depth > MAX_FUNCTION_DEPTH {
        0
    } else {
        run_function(ctx, &source, &name, &mut budget, depth)
    };
    if let Some(outer) = outer {
        ctx.world.insert_resource(RunningFunction {
            budget,
            depth: outer.depth,
        });
    }
    success!("commands.function.success", ran)
}

arg_enum! {
    /// Captured `DelayMode`.
    pub enum DelayMode in "DelayMode" {
        Replace = "replace",
        Append = "append",
    }
}

overloads! {
    pub enum ScheduleArgs for "schedule" {
        0 => DelayTicks {
            mode: EnumValue,
            condition: EnumValue,
            function: FilePath,
            time: i32,
            delay_mode: Option<DelayMode> = "DelayMode",
        },
        1 => DelayT {
            mode: EnumValue,
            condition: EnumValue,
            function: FilePath,
            time: Postfixed,
            delay_mode: Option<DelayMode> = "DelayMode",
        },
        2 => DelayS {
            mode: EnumValue,
            condition: EnumValue,
            function: FilePath,
            time: Postfixed,
            delay_mode: Option<DelayMode> = "DelayMode",
        },
        3 => DelayD {
            mode: EnumValue,
            condition: EnumValue,
            function: FilePath,
            time: Postfixed,
            delay_mode: Option<DelayMode> = "DelayMode",
        },
        4 => DelayClear { mode: EnumValue, condition: EnumValue, function: FilePath },
        5 => Clear { mode: EnumValue, function: FilePath },
        6 => AreaBox {
            mode: EnumValue,
            condition: EnumValue,
            from: BlockPosition,
            to: BlockPosition,
            function: FilePath,
        },
        7 => AreaCircle {
            mode: EnumValue,
            condition: EnumValue,
            area: EnumValue = "type",
            center: BlockPosition,
            radius: i32,
            function: FilePath,
        },
        8 => AreaTicking {
            mode: EnumValue,
            condition: EnumValue,
            area: EnumValue = "type",
            name: Text,
            function: FilePath,
        },
        9 => AreaClearTicking {
            mode: EnumValue,
            condition: EnumValue,
            cleartype: EnumValue,
            name: Text,
            function: Option<FilePath>,
        },
        10 => AreaClearFunction {
            mode: EnumValue,
            condition: EnumValue,
            cleartype: EnumValue,
            function: FilePath,
        },
    }
}

pub fn run_schedule(ctx: &mut CommandContext<'_>, args: ScheduleArgs) -> CommandResult {
    let delay = |function: FilePath, ticks: i64, mode: Option<DelayMode>| (function, ticks, mode);
    let (function, ticks, mode) = match args {
        ScheduleArgs::DelayTicks {
            function,
            time,
            delay_mode,
            ..
        } => delay(function, i64::from(time), delay_mode),
        ScheduleArgs::DelayT {
            function,
            time,
            delay_mode,
            ..
        } => delay(function, i64::from(time.0), delay_mode),
        ScheduleArgs::DelayS {
            function,
            time,
            delay_mode,
            ..
        } => delay(function, i64::from(time.0) * 20, delay_mode),
        ScheduleArgs::DelayD {
            function,
            time,
            delay_mode,
            ..
        } => delay(function, i64::from(time.0) * 24_000, delay_mode),
        ScheduleArgs::DelayClear { function, .. } | ScheduleArgs::Clear { function, .. } => {
            return clear_delayed(ctx, &function);
        }
        ScheduleArgs::AreaBox {
            from, to, function, ..
        } => {
            let (from, to) = (ctx.block_position(Some(from)), ctx.block_position(Some(to)));
            let min = (from.x.min(to.x) >> 4, from.z.min(to.z) >> 4);
            let max = (from.x.max(to.x) >> 4, from.z.max(to.z) >> 4);
            return queue_on_area(ctx, &function, min, max, None);
        }
        ScheduleArgs::AreaCircle {
            center,
            radius,
            function,
            ..
        } => {
            let center = ctx.block_position(Some(center));
            let (cx, cz) = (center.x >> 4, center.z >> 4);
            return queue_on_area(
                ctx,
                &function,
                (cx.saturating_sub(radius), cz.saturating_sub(radius)),
                (cx.saturating_add(radius), cz.saturating_add(radius)),
                None,
            );
        }
        ScheduleArgs::AreaTicking { name, function, .. } => {
            return queue_on_area(ctx, &function, (0, 0), (0, 0), Some(name.0));
        }
        ScheduleArgs::AreaClearTicking { name, function, .. } => {
            if let Some(function) = &function
                && !exists(ctx.world, function)
            {
                return Err(not_found(function));
            }
            let mut schedule = ctx
                .world
                .get_resource_or_insert_with(FunctionSchedule::default);
            let before = schedule.on_area.len();
            schedule.on_area.retain(|queued| {
                queued.ticking_area.as_deref() != Some(&*name)
                    || function.as_ref().is_some_and(|f| queued.function != **f)
            });
            let removed = before - schedule.on_area.len();
            return match (removed, function) {
                (0, None) => failure!("commands.schedule.tickingarea.clear.failure", &*name),
                (0, Some(function)) => failure!(
                    "commands.schedule.tickingarea.func.clear.failure",
                    &*name,
                    &*function
                ),
                (n, None) => success!("commands.schedule.tickingarea.clear.success", n, &*name),
                (n, Some(function)) => success!(
                    "commands.schedule.tickingarea.func.clear.success",
                    n,
                    &*name,
                    &*function
                ),
            };
        }
        ScheduleArgs::AreaClearFunction { function, .. } => {
            if !exists(ctx.world, &function) {
                return Err(not_found(&function));
            }
            let mut schedule = ctx
                .world
                .get_resource_or_insert_with(FunctionSchedule::default);
            let before = schedule.on_area.len();
            schedule
                .on_area
                .retain(|queued| queued.function != *function);
            let removed = before - schedule.on_area.len();
            if removed == 0 {
                return failure!("commands.schedule.clear.failure", &*function);
            }
            return success!("commands.schedule.clear.success", removed, &*function);
        }
    };
    if !exists(ctx.world, &function) {
        return Err(not_found(&function));
    }
    if ticks < 0 {
        return failure!("commands.schedule.delay.negativeTime");
    }
    let now = ctx
        .world
        .get_resource::<TickCounter>()
        .map_or(0, |tick| tick.current);
    let due = now + ticks as u64;
    let source = ctx.source.clone();
    let mut schedule = ctx
        .world
        .get_resource_or_insert_with(FunctionSchedule::default);
    if mode.unwrap_or(DelayMode::Replace) == DelayMode::Replace {
        schedule
            .delayed
            .retain(|queued| queued.function != *function);
    }
    schedule.delayed.push(Delayed {
        function: function.0.clone(),
        due,
        source,
    });
    success!(
        "commands.schedule.delay.functionQueued",
        &*function,
        ticks,
        due
    )
}

fn clear_delayed(ctx: &mut CommandContext<'_>, function: &str) -> CommandResult {
    if !exists(ctx.world, function) {
        return Err(not_found(function));
    }
    let mut schedule = ctx
        .world
        .get_resource_or_insert_with(FunctionSchedule::default);
    let before = schedule.delayed.len();
    schedule
        .delayed
        .retain(|queued| queued.function != function);
    let removed = before - schedule.delayed.len();
    if removed == 0 {
        return failure!("commands.schedule.clear.failure", function);
    }
    success!("commands.schedule.clear.success", removed, function)
}

fn queue_on_area(
    ctx: &mut CommandContext<'_>,
    function: &str,
    min: (i32, i32),
    max: (i32, i32),
    ticking_area: Option<String>,
) -> CommandResult {
    if !exists(ctx.world, function) {
        return Err(not_found(function));
    }
    let source = ctx.source.clone();
    ctx.world
        .get_resource_or_insert_with(FunctionSchedule::default)
        .on_area
        .push(OnArea {
            function: function.to_owned(),
            min,
            max,
            ticking_area,
            source,
        });
    success!("commands.schedule.functionQueued")
}

overloads! {
    pub enum ReloadArgs for "reload" {
        0 => Reload { all: Option<EnumValue> },
    }
}

pub fn run_reload(ctx: &mut CommandContext<'_>, _args: ReloadArgs) -> CommandResult {
    ctx.world
        .get_resource_or_insert_with(FunctionRegistry::default)
        .reload();
    success!("commands.reload.success")
}

/// Runs the scheduled functions that are due (delays that ran out, areas
/// that finished loading). Called once a tick after the commands.
pub fn run_due(world: &mut World) {
    let Some(mut schedule) = world.get_resource::<FunctionSchedule>().cloned() else {
        return;
    };
    let now = world
        .get_resource::<TickCounter>()
        .map_or(0, |tick| tick.current);
    let mut due: Vec<(String, CommandSource)> = Vec::new();
    schedule.delayed.retain(|queued| {
        if queued.due <= now {
            due.push((queued.function.clone(), queued.source.clone()));
            false
        } else {
            true
        }
    });
    let loaded = |world: &World, min: (i32, i32), max: (i32, i32)| {
        world
            .get_resource::<crate::world::ecs::ChunkManager>()
            .is_some_and(|chunks| {
                (min.0..=max.0).all(|x| (min.1..=max.1).all(|z| chunks.contains_coords(x, z)))
            })
    };
    schedule.on_area.retain(|queued| {
        // Areas named by ticking area wait until that area exists.
        if queued.ticking_area.is_some() || !loaded(world, queued.min, queued.max) {
            return true;
        }
        due.push((queued.function.clone(), queued.source.clone()));
        false
    });
    world.insert_resource(schedule);
    for (function, source) in due {
        // A source whose entity is gone runs as the server.
        let source = match source.kind {
            SourceKind::Player(entity) | SourceKind::Actor(entity)
                if world.get_entity(entity).is_err() =>
            {
                CommandSource::server()
            }
            _ => source,
        };
        let _ = crate::command::execute(world, source, &format!("function {function}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;

    #[test]
    fn functions_run_their_entries_and_schedules_run_them_later() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        let mut registry = FunctionRegistry::default();
        registry.insert(
            "demo/hello",
            "# a comment\n\nscoreboard objectives add f dummy\nscoreboard players set @s f 7\n",
        );
        registry.insert(
            "demo/outer",
            "function demo/hello\nscoreboard players add @s f 1\n",
        );
        h.world().insert_resource(registry);
        assert_eq!(
            h.run(op, "/function demo/hello"),
            CommandOutput::success("commands.function.success", vec!["2".into()])
        );
        assert_eq!(
            h.run(op, "/function demo/outer"),
            CommandOutput::success("commands.function.success", vec!["2".into()])
        );
        assert_eq!(
            h.run(op, "/scoreboard players test @s f 8 8").success_count,
            1,
            "the nested function ran, then the add"
        );
        assert_eq!(
            h.run(op, "/function nosuch"),
            CommandOutput::failure(
                "commands.function.functionNameNotFound",
                vec!["nosuch".into()]
            )
        );

        h.world()
            .get_resource_or_insert_with(TickCounter::default)
            .current = 100;
        h.run(op, "/scoreboard players set @s f 0");
        assert_eq!(
            h.run(op, "/schedule delay add demo/outer 5"),
            CommandOutput::success(
                "commands.schedule.delay.functionQueued",
                vec!["demo/outer".into(), "5".into(), "105".into()]
            )
        );
        run_due(h.world());
        assert_eq!(
            h.run(op, "/scoreboard players test @s f 0 0").success_count,
            1,
            "not due yet"
        );
        h.world().resource_mut::<TickCounter>().current = 105;
        run_due(h.world());
        assert_eq!(
            h.run(op, "/scoreboard players test @s f 8 8").success_count,
            1
        );

        h.run(op, "/schedule delay add demo/outer 5s");
        assert_eq!(
            h.run(op, "/schedule clear demo/outer"),
            CommandOutput::success(
                "commands.schedule.clear.success",
                vec!["1".into(), "demo/outer".into()]
            )
        );
        assert_eq!(
            h.run(op, "/schedule clear demo/outer"),
            CommandOutput::failure("commands.schedule.clear.failure", vec!["demo/outer".into()])
        );
    }
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::Replay;

    #[test]
    fn functions_and_schedules_match_the_bds_capture() {
        Replay::new().assert_segment("functions", &[]);
    }
}
