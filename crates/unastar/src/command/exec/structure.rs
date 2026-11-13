//! `/structure save|load|delete`: block structures by name.
//!
//! Measured on BDS 1.26.51 (command capture, `structure` segment):
//!
//! - names without a namespace get `mystructure:` (`s1` is
//!   `mystructure:s1`); save and delete answer the full name
//!   (`commands.structure.save.success`, `delete.success`, `notFound`), load
//!   the name as typed (`load.success [s1]`);
//! - a structure is at most 64 × 384 × 64 (`size.tooBig [64, 384, 64, x, y,
//!   z]`); a box below the world saves (as air);
//! - a load places every block of the box, air included, from `to` on:
//!   `mirror x` flips z, `mirror z` flips x, then the rotation turns the box
//!   clockwise (`90_degrees` maps (x, z) to (depth − 1 − z, x));
//!   `includeBlocks false` places none; a box below the world answers
//!   `y.tooLow [-64]`.
//!
//! Structures live for the session in [`Structures`]: a `disk` save is
//! marked as one but nothing writes structures with the world yet. Block
//! states are not rotated with the box, block entities and entities are not
//! saved, and integrity drops blocks at random from the seed.

use crate::command::prelude::*;
use crate::server::game::blocks::{block_at, set_block};
use crate::world::chunk::blocks::AIR;
use bevy_ecs::prelude::Resource;
use rand::{Rng, SeedableRng};
use std::collections::BTreeMap;

/// The largest structure (blocks per axis).
const MAX_SIZE: IVec3 = IVec3::new(64, 384, 64);

/// One saved box of blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureTemplate {
    pub size: IVec3,
    /// Block states, z fastest, then y, then x (the `.mcstructure` order).
    pub blocks: Vec<u32>,
    /// Saved in `disk` mode (to be kept with the world; not written yet).
    pub disk: bool,
}

impl StructureTemplate {
    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        ((x * self.size.y + y) * self.size.z + z) as usize
    }
}

/// Saved structures by full name (`mystructure:s1`).
#[derive(Resource, Debug, Clone, Default)]
pub struct Structures {
    pub templates: BTreeMap<String, StructureTemplate>,
}

/// `mystructure:` unless the name has a namespace.
fn full_name(name: &str) -> String {
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("mystructure:{name}")
    }
}

arg_enum! {
    /// Captured `StructureSaveMode`.
    pub enum SaveMode in "StructureSaveMode" {
        Disk = "disk",
        Memory = "memory",
    }
}

arg_enum! {
    /// Captured `Rotation`.
    pub enum Rotation in "Rotation" {
        R0 = "0_degrees",
        R90 = "90_degrees",
        R180 = "180_degrees",
        R270 = "270_degrees",
    }
}

arg_enum! {
    /// Captured `Mirror`.
    pub enum Mirror in "Mirror" {
        X = "x",
        Z = "z",
        None = "none",
        Xz = "xz",
    }
}

arg_enum! {
    /// Captured `StructureAnimationMode`.
    pub enum AnimationMode in "StructureAnimationMode" {
        BlockByBlock = "block_by_block",
        LayerByLayer = "layer_by_layer",
    }
}

overloads! {
    pub enum StructureArgs for "structure" {
        0 => Save {
            action: EnumValue,
            name: Text,
            from: BlockPosition,
            to: BlockPosition,
            save_mode: Option<SaveMode> = "saveMode",
        },
        1 => SaveWith {
            action: EnumValue,
            name: Text,
            from: BlockPosition,
            to: BlockPosition,
            include_entities: Option<bool> = "includeEntities",
            save_mode: Option<SaveMode> = "saveMode",
            include_blocks: Option<bool> = "includeBlocks",
        },
        2 => Delete { action: EnumValue, name: Text },
        3 => Load {
            action: EnumValue,
            name: Text,
            to: BlockPosition,
            rotation: Option<Rotation>,
            mirror: Option<Mirror>,
            include_entities: Option<bool> = "includeEntities",
            include_blocks: Option<bool> = "includeBlocks",
            waterlogged: Option<bool>,
            integrity: Option<f32>,
            seed: Option<Text>,
        },
        4 => LoadAnimated {
            action: EnumValue,
            name: Text,
            to: BlockPosition,
            rotation: Option<Rotation>,
            mirror: Option<Mirror>,
            animation: Option<AnimationMode> = "animationMode",
            animation_seconds: Option<f32> = "animationSeconds",
            include_entities: Option<bool> = "includeEntities",
            include_blocks: Option<bool> = "includeBlocks",
            waterlogged: Option<bool>,
            integrity: Option<f32>,
            seed: Option<Text>,
        },
    }
}

/// What a load places.
struct Placement {
    rotation: Rotation,
    mirror: Mirror,
    include_blocks: bool,
    integrity: f32,
    seed: Option<String>,
}

pub fn run(ctx: &mut CommandContext<'_>, args: StructureArgs) -> CommandResult {
    match args {
        StructureArgs::Save {
            name,
            from,
            to,
            save_mode,
            ..
        } => save(ctx, &name, from, to, save_mode, true),
        StructureArgs::SaveWith {
            name,
            from,
            to,
            save_mode,
            include_blocks,
            ..
        } => save(
            ctx,
            &name,
            from,
            to,
            save_mode,
            include_blocks.unwrap_or(true),
        ),
        StructureArgs::Delete { name, .. } => {
            let full = full_name(&name);
            let removed = ctx
                .world
                .get_resource_or_insert_with(Structures::default)
                .templates
                .remove(&full);
            match removed {
                Some(_) => success!("commands.structure.delete.success", full),
                None => failure!("commands.structure.notFound", full),
            }
        }
        StructureArgs::Load {
            name,
            to,
            rotation,
            mirror,
            include_blocks,
            integrity,
            seed,
            ..
        } => load(
            ctx,
            &name,
            to,
            Placement {
                rotation: rotation.unwrap_or(Rotation::R0),
                mirror: mirror.unwrap_or(Mirror::None),
                include_blocks: include_blocks.unwrap_or(true),
                integrity: integrity.unwrap_or(100.0),
                seed: seed.map(|s| s.0),
            },
        ),
        StructureArgs::LoadAnimated {
            name,
            to,
            rotation,
            mirror,
            animation_seconds,
            include_blocks,
            integrity,
            seed,
            ..
        } => {
            if animation_seconds.is_some_and(|s| s < 0.0) {
                return failure!("commands.structure.invalid.animationTime");
            }
            // The animation is not played: the structure appears at once.
            load(
                ctx,
                &name,
                to,
                Placement {
                    rotation: rotation.unwrap_or(Rotation::R0),
                    mirror: mirror.unwrap_or(Mirror::None),
                    include_blocks: include_blocks.unwrap_or(true),
                    integrity: integrity.unwrap_or(100.0),
                    seed: seed.map(|s| s.0),
                },
            )
        }
    }
}

fn save(
    ctx: &mut CommandContext<'_>,
    name: &str,
    from: BlockPosition,
    to: BlockPosition,
    mode: Option<SaveMode>,
    include_blocks: bool,
) -> CommandResult {
    let (from, to) = (ctx.block_position(Some(from)), ctx.block_position(Some(to)));
    let min = from.min(to);
    // In i64: corners from the command line can be far apart.
    let size = (from.as_i64vec3().max(to.as_i64vec3()) - min.as_i64vec3()) + glam::I64Vec3::ONE;
    if size.cmpgt(MAX_SIZE.as_i64vec3()).any() {
        return failure!(
            "commands.structure.size.tooBig",
            MAX_SIZE.x,
            MAX_SIZE.y,
            MAX_SIZE.z,
            size.x,
            size.y,
            size.z
        );
    }
    let size = size.as_ivec3();
    let mut blocks = Vec::with_capacity((size.x * size.y * size.z) as usize);
    for x in 0..size.x {
        for y in 0..size.y {
            for z in 0..size.z {
                let at = min + IVec3::new(x, y, z);
                let block = if include_blocks {
                    block_at(ctx.world, (at.x, at.y, at.z)).unwrap_or(*AIR)
                } else {
                    *AIR
                };
                blocks.push(block);
            }
        }
    }
    let full = full_name(name);
    ctx.world
        .get_resource_or_insert_with(Structures::default)
        .templates
        .insert(
            full.clone(),
            StructureTemplate {
                size,
                blocks,
                disk: mode == Some(SaveMode::Disk),
            },
        );
    success!("commands.structure.save.success", full)
}

fn load(
    ctx: &mut CommandContext<'_>,
    name: &str,
    to: BlockPosition,
    placement: Placement,
) -> CommandResult {
    let full = full_name(name);
    let Some(template) = ctx
        .world
        .get_resource::<Structures>()
        .and_then(|s| s.templates.get(&full))
        .cloned()
    else {
        return failure!("commands.structure.notFound", full);
    };
    if !(0.0..=100.0).contains(&placement.integrity) {
        return failure!("commands.structure.invalid.integrity");
    }
    let origin = ctx.block_position(Some(to));
    let bottom = crate::world::chunk::MIN_Y;
    let top = bottom + i32::from(crate::world::chunk::SUBCHUNK_COUNT) * 16;
    if origin.y < bottom {
        return failure!("commands.structure.y.tooLow", bottom);
    }
    if origin.y + template.size.y > top {
        return failure!("commands.structure.y.tooHigh", top - template.size.y);
    }
    if !placement.include_blocks {
        return success!("commands.structure.load.success", name);
    }
    let mut random = rand::rngs::StdRng::seed_from_u64(seed_value(placement.seed.as_deref()));
    let size = template.size;
    for x in 0..size.x {
        for y in 0..size.y {
            for z in 0..size.z {
                if placement.integrity < 100.0
                    && random.gen_range(0.0..100.0) >= placement.integrity
                {
                    continue;
                }
                let block = template.blocks[template.index(x, y, z)];
                let (mx, mz) = match placement.mirror {
                    Mirror::None => (x, z),
                    Mirror::X => (x, size.z - 1 - z),
                    Mirror::Z => (size.x - 1 - x, z),
                    Mirror::Xz => (size.x - 1 - x, size.z - 1 - z),
                };
                let (rx, rz) = match placement.rotation {
                    Rotation::R0 => (mx, mz),
                    Rotation::R90 => (size.z - 1 - mz, mx),
                    Rotation::R180 => (size.x - 1 - mx, size.z - 1 - mz),
                    Rotation::R270 => (mz, size.x - 1 - mx),
                };
                let at = origin + IVec3::new(rx, y, rz);
                if block_at(ctx.world, (at.x, at.y, at.z)).is_some_and(|old| old != block) {
                    set_block(ctx.world, at, block, false);
                }
            }
        }
    }
    success!("commands.structure.load.success", name)
}

/// The integrity seed: a number as written, any other text hashed.
fn seed_value(seed: Option<&str>) -> u64 {
    match seed {
        None | Some("") => 0,
        Some(text) => text.parse::<i64>().map_or_else(
            |_| {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                text.hash(&mut hasher);
                hasher.finish()
            },
            |n| n as u64,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::args::assert_covers;

    #[test]
    fn enums_cover_the_captured_values() {
        assert_covers::<SaveMode>();
        assert_covers::<Rotation>();
        assert_covers::<Mirror>();
        assert_covers::<AnimationMode>();
    }
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{Replay, segment};
    use crate::registry::block::BlockNetworkIds;
    use crate::server::game::blocks::block_at;
    use jolyne::valentine::McpePacketData;

    #[test]
    fn structure_matches_the_bds_capture() {
        Replay::new().assert_segment("structure", &[]);
    }

    /// Every block BDS placed for a load (its first `UpdateSubChunkBlocks`,
    /// flags 2) is the block unastar's world holds after the same load.
    #[test]
    fn loads_place_the_blocks_bds_placed() {
        let mut replay = Replay::new();
        let mut compared = 0;
        for captured in segment("structure") {
            replay.step(&captured.command);
            let placed: Vec<((i32, i32, i32), u32)> = captured
                .named("UpdateSubChunkBlocksPacket")
                .into_iter()
                .filter_map(|p| match p {
                    McpePacketData::UpdateSubChunkBlocksPacket(p) => Some(p),
                    _ => None,
                })
                .flat_map(|p| p.blocks_changed.blocks_changed_standards.clone())
                .filter(|b| b.update_flags == 2)
                .map(|b| ((b.pos.x, b.pos.y, b.pos.z), b.runtime_id))
                .collect();
            for (pos, network_id) in placed {
                let ours = block_at(replay.h.world(), pos).expect("a loaded block");
                assert_eq!(
                    BlockNetworkIds::Hashed.to_wire(ours),
                    network_id,
                    "{} at {pos:?}",
                    captured.command
                );
                compared += 1;
            }
        }
        assert!(compared >= 20, "{compared} blocks compared");
    }
}
