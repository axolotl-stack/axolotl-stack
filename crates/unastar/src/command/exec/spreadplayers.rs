//! `/spreadplayers <x> <z> <spreadDistance> <maxRange> <victim> [maxHeight]`.
//!
//! Measured on BDS 1.26.51 (command capture, `spreadplayers` segment):
//!
//! - `spreadDistance` below 0 answers `commands.generic.num.tooSmall
//!   [distance, 0.00]` and a `maxRange` below `spreadDistance + 1`
//!   `num.tooSmall [range, distance + 1]` (two decimals);
//! - each target lands on a random block column within `maxRange` of the
//!   centre, in its middle, with its feet two blocks above the highest solid
//!   block below `maxHeight` (the world's top without it; the top of the
//!   world over an unloaded column): the platform at y 199 puts the feet at
//!   201;
//! - the answer is `commands.spreadplayers.success.players [count, x, z]`
//!   with the centre floored to its block (`~ ~` at 8.5 9.5 answers 8.00,
//!   9.00); a player's `MovePlayer` has source actor type 0.
//!
//! Targets keep `spreadDistance` apart; [INFERENCE] a placement is retried
//! a bounded number of times before giving up with the lang's unsafe line.

use crate::command::prelude::*;
use crate::entity::item_drop::EntityRandom;
use crate::server::game::blocks::block_at;
use crate::world::chunk::blocks::AIR;
use rand::Rng;

overloads! {
    pub enum SpreadPlayersArgs for "spreadplayers" {
        0 => Spread {
            x: RelativeFloat,
            z: RelativeFloat,
            spread_distance: f32 = "spreadDistance",
            max_range: f32 = "maxRange",
            victim: Target,
            max_height: Option<RelativeFloat> = "maxHeight",
        },
    }
}

/// Attempts per target before the placement counts as unsafe.
const ATTEMPTS: usize = 1000;

pub fn run(ctx: &mut CommandContext<'_>, args: SpreadPlayersArgs) -> CommandResult {
    let SpreadPlayersArgs::Spread {
        x,
        z,
        spread_distance,
        max_range,
        victim,
        max_height,
    } = args;
    let two = |v: f32| format!("{v:.2}");
    if spread_distance < 0.0 {
        return failure!(
            "commands.generic.num.tooSmall",
            two(spread_distance),
            two(0.0)
        );
    }
    if max_range < spread_distance + 1.0 {
        return failure!(
            "commands.generic.num.tooSmall",
            two(max_range),
            two(spread_distance + 1.0)
        );
    }
    let targets = ctx.entities(Some(&victim))?;
    let center_x = x.resolve(ctx.source.position.x as f32).floor() as i32;
    let center_z = z.resolve(ctx.source.position.z as f32).floor() as i32;
    let top = crate::world::chunk::MIN_Y + i32::from(crate::world::chunk::SUBCHUNK_COUNT) * 16 - 1;
    let ceiling = max_height
        .map(|h| h.resolve(ctx.source.position.y as f32).floor() as i32)
        .unwrap_or(top)
        .min(top);
    let range = max_range.floor() as i32;
    let mut placed: Vec<(i32, i32)> = Vec::new();
    for &target in &targets {
        let mut spot = None;
        for _ in 0..ATTEMPTS {
            let (bx, bz) = {
                let mut random = ctx.world.get_resource_or_insert_with(EntityRandom::default);
                (
                    random.0.gen_range(center_x - range..=center_x + range),
                    random.0.gen_range(center_z - range..=center_z + range),
                )
            };
            let far_enough = placed.iter().all(|&(px, pz)| {
                let (dx, dz) = (f64::from(px - bx), f64::from(pz - bz));
                (dx * dx + dz * dz).sqrt() >= f64::from(spread_distance)
            });
            if far_enough {
                spot = Some((bx, bz));
                break;
            }
        }
        let Some((bx, bz)) = spot else {
            return failure!(
                "commands.spreadplayers.failure.players.unsafe",
                targets.len(),
                center_x,
                center_z,
                ATTEMPTS
            );
        };
        placed.push((bx, bz));
        let feet_y = surface(ctx.world, bx, bz, ceiling) + 2;
        let feet = DVec3::new(f64::from(bx) + 0.5, f64::from(feet_y), f64::from(bz) + 0.5);
        let rotation = ctx
            .world
            .get::<crate::entity::components::transform::Rotation>(target)
            .copied()
            .unwrap_or_default();
        super::teleport::move_entity_as(ctx.world, target, feet, rotation.yaw, rotation.pitch, 0);
    }
    Ok(CommandOutput::success(
        "commands.spreadplayers.success.players",
        vec![
            targets.len().to_string(),
            two(center_x as f32),
            two(center_z as f32),
        ],
    ))
}

/// The highest non-air block of a column at or below `ceiling` (the
/// ceiling itself over an unloaded or empty column).
fn surface(world: &World, x: i32, z: i32, ceiling: i32) -> i32 {
    let bottom = crate::world::chunk::MIN_Y;
    (bottom..=ceiling)
        .rev()
        .find(|&y| block_at(world, (x, y, z)).is_some_and(|block| block != *AIR))
        .unwrap_or(ceiling)
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::Replay;
    use jolyne::valentine::McpePacketData;

    #[test]
    fn spreadplayers_matches_the_bds_capture() {
        let steps = Replay::new().assert_segment("spreadplayers", &[]);
        // Landing spots are random: compare the height, and that the block
        // is within the range of the centre, with BDS's.
        for step in &steps {
            let moves = |packets: Vec<&McpePacketData>| -> Vec<(f32, f32, f32, i32)> {
                packets
                    .into_iter()
                    .filter_map(|p| match p {
                        McpePacketData::MovePlayerPacket(m) => Some((
                            m.position.x,
                            m.position.y,
                            m.position.z,
                            m.teleport_data.as_ref().map_or(-1, |t| t.source_actor_type),
                        )),
                        _ => None,
                    })
                    .collect()
            };
            let theirs = moves(step.captured.named("MovePlayerPacket"));
            let ours = moves(step.sent_named("MovePlayerPacket"));
            assert_eq!(ours.len(), theirs.len(), "{}", step.captured.command);
            for (ours, theirs) in ours.iter().zip(&theirs) {
                assert_eq!(
                    (ours.1, ours.3),
                    (theirs.1, theirs.3),
                    "{}",
                    step.captured.command
                );
                if step.captured.command.starts_with("/spreadplayers 8 8 ") {
                    let range = if step.captured.command.contains(" 1 2 ") {
                        2.0
                    } else {
                        1.0
                    };
                    for v in [ours.0, ours.2, theirs.0, theirs.2] {
                        assert!((v - 8.5).abs() <= range, "{}: {v}", step.captured.command);
                    }
                }
            }
        }
    }
}
