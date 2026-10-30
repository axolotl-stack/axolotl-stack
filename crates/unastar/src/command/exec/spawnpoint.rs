//! `/setworldspawn [spawnPoint]` and `/spawnpoint [player] [spawnPos]`.
//!
//! Measured on BDS 1.26.51: both answer with the block position
//! (`commands.setworldspawn.success` x, y, z;
//! `commands.spawnpoint.success.single` name, x, y, z) and send
//! `SetSpawnPosition` (`Worldspawn` / `Playerrespawn`, dimension 0,
//! `spawn_block_pos` all `i32::MIN`).
//!
//! The world spawn is the [`WorldSpawn`] players respawn at; a player's spawn
//! point is its [`PlayerSpawnPoint`]. Both stand the player in the middle of
//! the block. Joining players keep the configured spawn rules, and spawn
//! points are not saved with the player.

use crate::command::prelude::*;
use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::player_life::respawn::{PlayerSpawnPoint, WorldSpawn};
use crate::server::game::ServerConfigResource;
use jolyne::valentine::SetSpawnPositionPacket;
use jolyne::valentine::types::{BlockPos, DimensionType, EnumsSpawnPositionType};

/// The Overworld, the only dimension spawns can be set in.
const OVERWORLD: i32 = 0;

overloads! {
    pub enum SetWorldSpawnArgs for "setworldspawn" {
        0 => Set { spawn_point: Option<CommandPosition> = "spawnPoint" },
    }
}

overloads! {
    pub enum SpawnPointArgs for "spawnpoint" {
        0 => Set {
            player: Option<Target>,
            spawn_pos: Option<CommandPosition> = "spawnPos",
        },
    }
}

pub fn run_setworldspawn(ctx: &mut CommandContext<'_>, args: SetWorldSpawnArgs) -> CommandResult {
    let SetWorldSpawnArgs::Set { spawn_point } = args;
    if dimension(ctx.world) != OVERWORLD {
        return failure!("commands.setworldspawn.wrongDimension");
    }
    let block = ctx.position(spawn_point).floor().as_ivec3();
    ctx.world.insert_resource(WorldSpawn(standing_in(block)));
    ctx.broadcast(spawn_packet(EnumsSpawnPositionType::Worldspawn, block));
    success!("commands.setworldspawn.success", block.x, block.y, block.z)
}

pub fn run_spawnpoint(ctx: &mut CommandContext<'_>, args: SpawnPointArgs) -> CommandResult {
    let SpawnPointArgs::Set { player, spawn_pos } = args;
    let players = ctx.players(player.as_ref())?;
    if dimension(ctx.world) != OVERWORLD {
        return failure!("commands.spawnpoint.wrongDimension");
    }
    let block = ctx.position(spawn_pos).floor().as_ivec3();
    for &player in &players {
        ctx.world
            .entity_mut(player)
            .insert(PlayerSpawnPoint(standing_in(block)));
        ctx.send(
            player,
            spawn_packet(EnumsSpawnPositionType::Playerrespawn, block),
        );
    }
    let [x, y, z] = [block.x, block.y, block.z].map(|c| c.to_string());
    let output = match players.as_slice() {
        [player] => CommandOutput::success(
            "commands.spawnpoint.success.single",
            vec![ctx.name(*player), x, y, z],
        ),
        // [INFERENCE] Several targets: the lang's multiple-target line.
        _ => CommandOutput::success(
            "commands.spawnpoint.success.multiple.specific",
            vec![ctx.names(&players), x, y, z],
        ),
    };
    Ok(output.with_count(players.len()))
}

fn dimension(world: &World) -> i32 {
    world
        .get_resource::<ServerConfigResource>()
        .map_or(OVERWORLD, |config| config.0.world.dimension)
}

/// A player standing in the middle of `block`, in the `Position` convention.
fn standing_in(block: IVec3) -> DVec3 {
    block.as_dvec3() + DVec3::new(0.5, PLAYER_EYE_HEIGHT, 0.5)
}

fn spawn_packet(kind: EnumsSpawnPositionType, block: IVec3) -> SetSpawnPositionPacket {
    SetSpawnPositionPacket {
        spawn_position_type: kind,
        block_position: BlockPos {
            x: block.x,
            y: block.y,
            z: block.z,
        },
        dimensiontype: DimensionType { value: OVERWORLD },
        spawn_block_pos: BlockPos {
            x: i32::MIN,
            y: i32::MIN,
            z: i32::MIN,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;
    use crate::entity::player_life::respawn::respawn_position;
    use jolyne::valentine::McpePacketData;

    fn spawn_packets(h: &mut Harness, player: Entity) -> Vec<SetSpawnPositionPacket> {
        h.sent(player)
            .into_iter()
            .filter_map(|p| match p {
                McpePacketData::SetSpawnPositionPacket(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn setworldspawn_moves_the_world_spawn_like_bds() {
        let mut h = Harness::new();
        let alice = h.player_at("Alice", true, DVec3::new(10.5, 80.0, 10.5));
        let bob = h.player("Bob", false);

        let output = h.run(alice, "/setworldspawn 5 70 5");
        assert_eq!(
            output,
            CommandOutput::success(
                "commands.setworldspawn.success",
                vec!["5".into(), "70".into(), "5".into()]
            )
        );
        assert_eq!(
            h.world().resource::<WorldSpawn>().0,
            DVec3::new(5.5, 70.0 + PLAYER_EYE_HEIGHT, 5.5)
        );
        for player in [alice, bob] {
            let packets = spawn_packets(&mut h, player);
            assert_eq!(packets.len(), 1);
            assert_eq!(
                packets[0],
                spawn_packet(EnumsSpawnPositionType::Worldspawn, IVec3::new(5, 70, 5))
            );
            assert_eq!(packets[0].spawn_block_pos.x, i32::MIN);
        }
        assert_eq!(
            respawn_position(h.world(), bob),
            DVec3::new(5.5, 70.0 + PLAYER_EYE_HEIGHT, 5.5)
        );

        let output = h.run(alice, "/setworldspawn");
        assert_eq!(output.messages[0].params, ["10", "80", "10"]);
    }

    #[test]
    fn spawnpoint_sets_the_player_respawn_point_like_bds() {
        let mut h = Harness::new();
        let alice = h.player_at("Alice", true, DVec3::new(10.5, 80.0, 10.5));
        let bob = h.player("Bob", false);

        let output = h.run(alice, "/spawnpoint");
        assert_eq!(
            output,
            CommandOutput::success(
                "commands.spawnpoint.success.single",
                vec!["Alice".into(), "10".into(), "80".into(), "10".into()]
            )
        );
        let packets = spawn_packets(&mut h, alice);
        assert_eq!(
            packets,
            [spawn_packet(
                EnumsSpawnPositionType::Playerrespawn,
                IVec3::new(10, 80, 10)
            )]
        );

        let output = h.run(alice, "/spawnpoint @s 1 70 1");
        assert_eq!(output.messages[0].params, ["Alice", "1", "70", "1"]);
        assert_eq!(
            respawn_position(h.world(), alice),
            DVec3::new(1.5, 70.0 + PLAYER_EYE_HEIGHT, 1.5)
        );
        // Bob still respawns at the world spawn and got no packet.
        let world_spawn = h.world().resource::<WorldSpawn>().0;
        assert_eq!(respawn_position(h.world(), bob), world_spawn);
        assert!(spawn_packets(&mut h, bob).is_empty());

        let output = h.run(alice, "/spawnpoint @a 2 65 2");
        assert_eq!(output.success_count, 2);
        assert_eq!(
            output.messages[0].key,
            "commands.spawnpoint.success.multiple.specific"
        );
        let mut names: Vec<&str> = output.messages[0].params[0].split(", ").collect();
        names.sort_unstable();
        assert_eq!(names, ["Alice", "Bob"]);
        assert_eq!(output.messages[0].params[1..], ["2", "65", "2"]);
        assert_eq!(spawn_packets(&mut h, bob).len(), 1);
    }
}
