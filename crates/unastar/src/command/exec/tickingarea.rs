//! `/tickingarea add|remove|remove_all|list|preload`: the world's named
//! ticking areas.
//!
//! Measured on BDS 1.26.51 (command capture, `tickingarea` segment):
//!
//! - a box covers whole chunks and is reported from its first to its last
//!   block at y 0 (`0 0 0 1000 0 0` is `0, 0, 0` to `1007, 0, 15`); a circle
//!   keeps its centre block and a radius of at most 4 chunks
//!   (`commands.tickingarea-add.radiusfailure [4]`); at most 10 areas
//!   (`commands.tickingarea.inuse [n, 10]` follows every change); unnamed
//!   areas are `Area0`, `Area1`, …; names must be unique;
//! - lists and the areas a `remove`/`preload` touched are one message of
//!   literal lines (`- name: x y z %commands.tickingarea-list.to x y z`,
//!   circles `- name (%…type.circle): x y z %…circle.radius: r %…chunks`,
//!   ` %…preload` for preloaded areas) that the client translates; `list`
//!   prefixes a `§a` title and `all-dimensions` the dimension name;
//! - `remove <position>` removes every area containing the block,
//!   `preload` without a value only reports how many matching areas are
//!   marked.
//!
//! The areas are kept for the session; they do not yet keep chunks loaded or
//! ticking, and are not saved with the world.

use crate::command::prelude::*;
use bevy_ecs::prelude::Resource;

/// At most this many areas per dimension.
const MAX_AREAS: usize = 10;
/// A circle's largest radius, in chunks.
const MAX_RADIUS: i32 = 4;
/// [INFERENCE] A box's largest size, in chunks (the lang's
/// `chunkfailure` limit; 63 chunks were accepted).
const MAX_CHUNKS: i64 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Shape {
    /// Chunk coordinates, inclusive.
    Box { min: (i32, i32), max: (i32, i32) },
    /// The centre block and a radius in chunks.
    Circle {
        center: (i32, i32, i32),
        radius: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickingArea {
    name: String,
    shape: Shape,
    preload: bool,
}

impl TickingArea {
    fn contains(&self, block: IVec3) -> bool {
        let chunk = (block.x >> 4, block.z >> 4);
        match self.shape {
            Shape::Box { min, max } => {
                (min.0..=max.0).contains(&chunk.0) && (min.1..=max.1).contains(&chunk.1)
            }
            Shape::Circle { center, radius } => {
                let (dx, dz) = (chunk.0 - (center.0 >> 4), chunk.1 - (center.2 >> 4));
                dx * dx + dz * dz <= radius * radius
            }
        }
    }

    /// The area's line in a list.
    fn line(&self) -> String {
        let preload = if self.preload {
            " %commands.tickingarea-list.preload"
        } else {
            ""
        };
        match self.shape {
            Shape::Box { min, max } => format!(
                "- {}: {} 0 {} %commands.tickingarea-list.to {} 0 {}{preload}",
                self.name,
                min.0 * 16,
                min.1 * 16,
                max.0 * 16 + 15,
                max.1 * 16 + 15
            ),
            Shape::Circle { center, radius } => format!(
                "- {} (%commands.tickingarea-list.type.circle): {} {} {} %commands.tickingarea-list.circle.radius: {radius} %commands.tickingarea-list.chunks{preload}",
                self.name, center.0, center.1, center.2
            ),
        }
    }
}

/// The world's ticking areas, in creation order.
#[derive(Resource, Debug, Clone, Default)]
pub struct TickingAreas {
    areas: Vec<TickingArea>,
    next_unnamed: usize,
}

impl TickingAreas {
    fn in_use(&self) -> CommandMessageParams {
        (
            "commands.tickingarea.inuse",
            vec![self.areas.len().to_string(), MAX_AREAS.to_string()],
        )
    }
}

type CommandMessageParams = (&'static str, Vec<String>);

/// Lines of `areas`, each ending in a newline (as `remove` and `preload`
/// report them).
fn lines(areas: &[&TickingArea]) -> String {
    areas.iter().map(|a| format!("{}\n", a.line())).collect()
}

overloads! {
    pub enum TickingAreaArgs for "tickingarea" {
        0 => AddBox {
            mode: EnumValue,
            from: BlockPosition,
            to: BlockPosition,
            name: Option<Text>,
            preload: Option<bool>,
        },
        1 => AddCircle {
            mode: EnumValue,
            circle: EnumValue,
            center: BlockPosition,
            radius: i32,
            name: Option<Text>,
            preload: Option<bool>,
        },
        2 => RemoveAt { mode: EnumValue, position: BlockPosition },
        3 => RemoveNamed { mode: EnumValue, name: Text },
        4 => RemoveAll { mode: EnumValue },
        5 => List { mode: EnumValue, all_dimensions: Option<EnumValue> = "all-dimensions" },
        6 => PreloadAt { mode: EnumValue, position: BlockPosition, preload: Option<bool> },
        7 => PreloadNamed { mode: EnumValue, name: Text, preload: Option<bool> },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: TickingAreaArgs) -> CommandResult {
    let mut areas = ctx
        .world
        .get_resource_or_insert_with(TickingAreas::default)
        .clone();
    let output = match args {
        TickingAreaArgs::AddBox {
            from,
            to,
            name,
            preload,
            ..
        } => {
            let (from, to) = (ctx.block_position(Some(from)), ctx.block_position(Some(to)));
            let min = ((from.x.min(to.x)) >> 4, (from.z.min(to.z)) >> 4);
            let max = ((from.x.max(to.x)) >> 4, (from.z.max(to.z)) >> 4);
            let chunks = i64::from(max.0 - min.0 + 1) * i64::from(max.1 - min.1 + 1);
            if chunks > MAX_CHUNKS {
                return failure!("commands.tickingarea-add.chunkfailure", MAX_CHUNKS);
            }
            let preload = preload.unwrap_or(false);
            let key = if preload {
                "commands.tickingarea-add-bounds.preload.success"
            } else {
                "commands.tickingarea-add-bounds.success"
            };
            let params = vec![
                format!("{}, 0, {}", min.0 * 16, min.1 * 16),
                format!("{}, 0, {}", max.0 * 16 + 15, max.1 * 16 + 15),
            ];
            add(
                &mut areas,
                name,
                Shape::Box { min, max },
                preload,
                key,
                params,
            )?
        }
        TickingAreaArgs::AddCircle {
            center,
            radius,
            name,
            preload,
            ..
        } => {
            if radius > MAX_RADIUS {
                return failure!("commands.tickingarea-add.radiusfailure", MAX_RADIUS);
            }
            let center = ctx.block_position(Some(center));
            let preload = preload.unwrap_or(false);
            let key = if preload {
                "commands.tickingarea-add-circle.preload.success"
            } else {
                "commands.tickingarea-add-circle.success"
            };
            let params = vec![
                format!("{}, {}, {}", center.x, center.y, center.z),
                radius.to_string(),
            ];
            let center = (center.x, center.y, center.z);
            add(
                &mut areas,
                name,
                Shape::Circle { center, radius },
                preload,
                key,
                params,
            )?
        }
        TickingAreaArgs::RemoveAt { position, .. } => {
            let block = ctx.block_position(Some(position));
            if areas.areas.is_empty() {
                return failure!("commands.tickingarea.noneExist.currentDimension");
            }
            let (removed, kept): (Vec<_>, Vec<_>) =
                areas.areas.iter().cloned().partition(|a| a.contains(block));
            if removed.is_empty() {
                return failure!(
                    "commands.tickingarea-remove.failure",
                    format!("{}, {}, {}", block.x, block.y, block.z)
                );
            }
            areas.areas = kept;
            removal(&areas, &removed)
        }
        TickingAreaArgs::RemoveNamed { name, .. } => {
            if areas.areas.is_empty() {
                return failure!("commands.tickingarea.noneExist.currentDimension");
            }
            let (removed, kept): (Vec<_>, Vec<_>) =
                areas.areas.iter().cloned().partition(|a| a.name == *name);
            if removed.is_empty() {
                return failure!("commands.tickingarea-remove.byname.failure", &*name);
            }
            areas.areas = kept;
            removal(&areas, &removed)
        }
        TickingAreaArgs::RemoveAll { .. } => {
            if areas.areas.is_empty() {
                return failure!("commands.tickingarea.noneExist.currentDimension");
            }
            let removed = std::mem::take(&mut areas.areas);
            let refs: Vec<&TickingArea> = removed.iter().collect();
            let mut output =
                CommandOutput::success("commands.tickingarea-remove_all.success", vec![]);
            output.add_success(lines(&refs), vec![]);
            let (key, params) = areas.in_use();
            output.add_success(key, params);
            output
        }
        TickingAreaArgs::List { all_dimensions, .. } => {
            if areas.areas.is_empty() {
                return failure!("commands.tickingarea.noneExist.currentDimension");
            }
            let list = areas
                .areas
                .iter()
                .map(TickingArea::line)
                .collect::<Vec<_>>()
                .join("\n");
            let mut output = if all_dimensions.is_some() {
                let mut output = CommandOutput::success(
                    "\u{a7}a%commands.tickingarea-list.success.allDimensions",
                    vec![],
                );
                output.add_success(format!("%dimension.dimensionName0: \n{list}"), vec![]);
                output
            } else {
                let mut output = CommandOutput::success(
                    "\u{a7}a%commands.tickingarea-list.success.currentDimension",
                    vec![],
                );
                output.add_success(list, vec![]);
                output
            };
            let (key, params) = areas.in_use();
            output.add_success(key, params);
            output
        }
        TickingAreaArgs::PreloadAt {
            position, preload, ..
        } => {
            let block = ctx.block_position(Some(position));
            let indices: Vec<usize> = (0..areas.areas.len())
                .filter(|&i| areas.areas[i].contains(block))
                .collect();
            if indices.is_empty() {
                return failure!(
                    "commands.tickingarea-preload.byposition.failure",
                    format!("{}, {}, {}", block.x, block.y, block.z)
                );
            }
            preload_report(&mut areas, &indices, preload)
        }
        TickingAreaArgs::PreloadNamed { name, preload, .. } => {
            let indices: Vec<usize> = (0..areas.areas.len())
                .filter(|&i| areas.areas[i].name == *name)
                .collect();
            if indices.is_empty() {
                return failure!("commands.tickingarea-preload.byname.failure", &*name);
            }
            preload_report(&mut areas, &indices, preload)
        }
    };
    ctx.world.insert_resource(areas);
    // Measured: one success whatever the number of lines.
    Ok(output.with_count(1))
}

fn add(
    areas: &mut TickingAreas,
    name: Option<Text>,
    shape: Shape,
    preload: bool,
    key: &'static str,
    params: Vec<String>,
) -> Result<CommandOutput, CommandOutput> {
    if areas.areas.len() >= MAX_AREAS {
        return Err(CommandOutput::failure(
            "commands.tickingarea-add.failure",
            vec![MAX_AREAS.to_string()],
        ));
    }
    let name = match name {
        Some(name) => {
            if areas.areas.iter().any(|a| a.name == *name) {
                return Err(CommandOutput::failure(
                    "commands.tickingarea-add.conflictingname",
                    vec![name.0],
                ));
            }
            name.0
        }
        None => {
            let name = format!("Area{}", areas.next_unnamed);
            areas.next_unnamed += 1;
            name
        }
    };
    areas.areas.push(TickingArea {
        name,
        shape,
        preload,
    });
    let mut output = CommandOutput::success(key, params);
    let (key, params) = areas.in_use();
    output.add_success(key, params);
    Ok(output)
}

fn removal(areas: &TickingAreas, removed: &[TickingArea]) -> CommandOutput {
    let refs: Vec<&TickingArea> = removed.iter().collect();
    let mut output = CommandOutput::success("commands.tickingarea-remove.success", vec![]);
    output.add_success(lines(&refs), vec![]);
    let (key, params) = areas.in_use();
    output.add_success(key, params);
    output
}

/// `preload`: sets the matching areas' flag when a value is given, then
/// lists them and counts the preloaded ones.
fn preload_report(
    areas: &mut TickingAreas,
    indices: &[usize],
    preload: Option<bool>,
) -> CommandOutput {
    let mut output = CommandOutput::default();
    if let Some(preload) = preload {
        for &i in indices {
            areas.areas[i].preload = preload;
        }
        output.add_success("commands.tickingarea-preload.success", vec![]);
    }
    let matching: Vec<&TickingArea> = indices.iter().map(|&i| &areas.areas[i]).collect();
    let count = matching.iter().filter(|a| a.preload).count();
    output.add_success(lines(&matching), vec![]);
    output.add_success(
        "commands.tickingarea-preload.count",
        vec![count.to_string()],
    );
    output
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::Replay;

    #[test]
    fn tickingarea_matches_the_bds_capture() {
        Replay::new().assert_segment("tickingarea", &[]);
    }
}
