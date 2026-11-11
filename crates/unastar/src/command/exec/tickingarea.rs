//! `/tickingarea add|remove|remove_all|list|preload`: the world's named
//! ticking areas.
//!
//! Measured on BDS 1.26.51 (command capture, `tickingarea` segment):
//!
//! - a box covers whole chunks and is reported from its first to its last
//!   block at y 0 (`0 0 0 1000 0 0` is `0, 0, 0` to `1007, 0, 15`); a circle
//!   is centred on the middle of its centre block's chunk at y 0 (`circle
//!   100 70 100 2` is centred at `104, 0, 104`; level-data capture) with a
//!   radius of at most 4 chunks (`commands.tickingarea-add.radiusfailure
//!   [4]`); at most 10 areas over all dimensions
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
//! Areas are saved with the world, one `tickingarea_<uuid>` record each
//! (`crate::world::level_data`); after a restart BDS lists them in the order
//! of those keys, and so does unastar. Areas of other dimensions (from a world
//! BDS saved) are kept, counted and listed with `all-dimensions`. Areas do
//! not yet keep chunks loaded or ticking.

use crate::command::prelude::*;
use crate::storage::level::records::StoredTickingArea;
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
    /// The centre chunk and a radius in chunks.
    Circle { center: (i32, i32), radius: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickingArea {
    name: String,
    shape: Shape,
    preload: bool,
    /// 0 overworld, 1 nether, 2 the End.
    dimension: i32,
    /// The uuid of its `tickingarea_<uuid>` record.
    uuid: String,
}

/// The block a circle is reported centred on: its chunk's middle, at y 0.
fn circle_center(chunk: (i32, i32)) -> (i32, i32, i32) {
    (chunk.0 * 16 + 8, 0, chunk.1 * 16 + 8)
}

impl TickingArea {
    fn contains(&self, block: IVec3) -> bool {
        let chunk = (block.x >> 4, block.z >> 4);
        match self.shape {
            Shape::Box { min, max } => {
                (min.0..=max.0).contains(&chunk.0) && (min.1..=max.1).contains(&chunk.1)
            }
            Shape::Circle { center, radius } => {
                let (dx, dz) = (chunk.0 - center.0, chunk.1 - center.1);
                dx * dx + dz * dz <= radius * radius
            }
        }
    }

    /// The area as its `tickingarea_<uuid>` record stores it: the block
    /// corners of its first and last chunk (a circle's square around it).
    pub fn to_stored(&self) -> (String, StoredTickingArea) {
        let (circle, min, max) = match self.shape {
            Shape::Box { min, max } => (false, min, max),
            Shape::Circle { center, radius } => (
                true,
                (center.0 - radius, center.1 - radius),
                (center.0 + radius, center.1 + radius),
            ),
        };
        (
            self.uuid.clone(),
            StoredTickingArea {
                dimension: self.dimension,
                circle,
                min: (min.0 * 16, min.1 * 16),
                max: (max.0 * 16, max.1 * 16),
                name: self.name.clone(),
                preload: self.preload,
            },
        )
    }

    /// The area a record holds (the record checked it covers whole chunks).
    pub fn from_stored(uuid: &str, stored: &StoredTickingArea) -> Self {
        let min = (stored.min.0 >> 4, stored.min.1 >> 4);
        let max = (stored.max.0 >> 4, stored.max.1 >> 4);
        let shape = if stored.circle {
            let radius = (max.0 - min.0) / 2;
            Shape::Circle {
                center: (min.0 + radius, min.1 + radius),
                radius,
            }
        } else {
            Shape::Box { min, max }
        };
        Self {
            name: stored.name.clone(),
            shape,
            preload: stored.preload,
            dimension: stored.dimension,
            uuid: uuid.to_owned(),
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
            Shape::Circle { center, radius } => {
                let (x, y, z) = circle_center(center);
                format!(
                    "- {} (%commands.tickingarea-list.type.circle): {x} {y} {z} %commands.tickingarea-list.circle.radius: {radius} %commands.tickingarea-list.chunks{preload}",
                    self.name
                )
            }
        }
    }
}

/// The world's ticking areas: in creation order, then (after a restart) in
/// the order of their records' keys.
#[derive(Resource, Debug, Clone, Default)]
pub struct TickingAreas {
    areas: Vec<TickingArea>,
    next_unnamed: usize,
}

impl TickingAreas {
    /// Areas loaded from their records, in key order.
    pub fn from_stored(areas: Vec<TickingArea>) -> Self {
        Self {
            areas,
            next_unnamed: 0,
        }
    }

    /// Every area, all dimensions.
    pub fn areas(&self) -> &[TickingArea] {
        &self.areas
    }

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

/// The dimension commands manage areas in (unastar runs the overworld).
const OVERWORLD: i32 = 0;

/// Whether the area is in the dimension the command runs in.
fn here(area: &TickingArea) -> bool {
    area.dimension == OVERWORLD
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
            let block = ctx.block_position(Some(center));
            let center = (block.x >> 4, block.z >> 4);
            let preload = preload.unwrap_or(false);
            let key = if preload {
                "commands.tickingarea-add-circle.preload.success"
            } else {
                "commands.tickingarea-add-circle.success"
            };
            let (x, y, z) = circle_center(center);
            let params = vec![format!("{x}, {y}, {z}"), radius.to_string()];
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
            if !areas.areas.iter().any(here) {
                return failure!("commands.tickingarea.noneExist.currentDimension");
            }
            let (removed, kept): (Vec<_>, Vec<_>) = areas
                .areas
                .iter()
                .cloned()
                .partition(|a| here(a) && a.contains(block));
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
            if !areas.areas.iter().any(here) {
                return failure!("commands.tickingarea.noneExist.currentDimension");
            }
            let (removed, kept): (Vec<_>, Vec<_>) = areas
                .areas
                .iter()
                .cloned()
                .partition(|a| here(a) && a.name == *name);
            if removed.is_empty() {
                return failure!("commands.tickingarea-remove.byname.failure", &*name);
            }
            areas.areas = kept;
            removal(&areas, &removed)
        }
        TickingAreaArgs::RemoveAll { .. } => {
            if !areas.areas.iter().any(here) {
                return failure!("commands.tickingarea.noneExist.currentDimension");
            }
            let (removed, kept): (Vec<_>, Vec<_>) =
                std::mem::take(&mut areas.areas).into_iter().partition(here);
            areas.areas = kept;
            let refs: Vec<&TickingArea> = removed.iter().collect();
            let mut output =
                CommandOutput::success("commands.tickingarea-remove_all.success", vec![]);
            output.add_success(lines(&refs), vec![]);
            let (key, params) = areas.in_use();
            output.add_success(key, params);
            output
        }
        TickingAreaArgs::List { all_dimensions, .. } => {
            let listed = |dimension: i32| {
                areas
                    .areas
                    .iter()
                    .filter(|a| a.dimension == dimension)
                    .map(TickingArea::line)
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let mut output = if all_dimensions.is_some() {
                if areas.areas.is_empty() {
                    return failure!("commands.tickingarea.noneExist.currentDimension");
                }
                let mut output = CommandOutput::success(
                    "\u{a7}a%commands.tickingarea-list.success.allDimensions",
                    vec![],
                );
                // Measured (console): the areas of each dimension under its
                // name, overworld first. [INFERENCE] One message per dimension.
                for dimension in 0..=2 {
                    if areas.areas.iter().any(|a| a.dimension == dimension) {
                        output.add_success(
                            format!(
                                "%dimension.dimensionName{dimension}: \n{}",
                                listed(dimension)
                            ),
                            vec![],
                        );
                    }
                }
                output
            } else {
                if !areas.areas.iter().any(here) {
                    return failure!("commands.tickingarea.noneExist.currentDimension");
                }
                let list = listed(OVERWORLD);
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
                .filter(|&i| here(&areas.areas[i]) && areas.areas[i].contains(block))
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
                .filter(|&i| here(&areas.areas[i]) && areas.areas[i].name == *name)
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
        // [INFERENCE] After a restart the counter starts again and skips the
        // names of loaded areas.
        None => loop {
            let name = format!("Area{}", areas.next_unnamed);
            areas.next_unnamed += 1;
            if !areas.areas.iter().any(|a| a.name == name) {
                break name;
            }
        },
    };
    areas.areas.push(TickingArea {
        name,
        shape,
        preload,
        dimension: OVERWORLD,
        uuid: uuid::Uuid::new_v4().to_string(),
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
