//! `/teleport` (`/tp`).
//!
//! Relative destination coordinates and rotations resolve against the command
//! source, not each victim, as in Bedrock. Players are moved with a
//! `MovePlayer` teleport (their chunk loader follows); other actors get a new
//! position and rotation, which the actor sync broadcasts.

use super::summon::facing_yaw;
use crate::command::prelude::*;
use crate::ecs::resources::TickCounter;
use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{Player, PlayerSession, RuntimeEntityId};
use crate::world::ecs::ChunkLoader;
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsPlayerPositionModeComponentPositionMode, PlayerInputTick, Vec2, Vec3,
};
use jolyne::valentine::{MovePlayerPacket, MovePlayerTeleportData};

/// `MovePlayer` teleport cause for commands.
const TELEPORT_CAUSE_COMMAND: i32 = 3;
/// `source_actor_type` BDS 1.26.51 sends in command teleports (measured).
const SOURCE_ACTOR_TYPE: i32 = 1;

pub fn run(ctx: &mut CommandContext<'_>, args: &Args) -> CommandResult {
    let victims = ctx.entities(args.get("victim"))?;

    // Every overload has a `destination`: a position or a target.
    let (position, to_entity) = match args.get::<&CommandPosition>("destination") {
        Some(position) => (ctx.position(Some(position)), None),
        None => {
            let entity = ctx.entity(args.get("destination"))?;
            let feet = ctx.feet(entity).ok_or_else(CommandOutput::no_targets)?;
            (feet, Some(entity))
        }
    };
    let look_at = match (
        args.get::<&CommandPosition>("lookAtPosition"),
        args.get::<&Target>("lookAtEntity"),
    ) {
        (Some(p), _) => Some(ctx.position(Some(p))),
        (None, Some(target)) => {
            let entity = ctx.entities(Some(target))?[0];
            ctx.feet(entity).map(|f| f + eye_offset(ctx.world, entity))
        }
        (None, None) => None,
    };
    let explicit_rotation = (args.has("yRot") || args.has("xRot"))
        .then(|| ctx.rotation(args.get("yRot"), args.get("xRot")));
    let entity_rotation = to_entity
        .and_then(|e| ctx.world.get::<Rotation>(e))
        .map(|r| (r.yaw, r.pitch));

    if args.get::<bool>("checkForBlocks") == Some(true) && !space_is_clear(ctx.world, position) {
        return failure!(
            "commands.tp.safeTeleportFail",
            ctx.names(&victims),
            coordinates(position)
        );
    }

    for &victim in &victims {
        let current = ctx
            .world
            .get::<Rotation>(victim)
            .copied()
            .unwrap_or_default();
        let (yaw, pitch) = match look_at {
            Some(at) => facing(position + eye_offset(ctx.world, victim), at),
            None => explicit_rotation
                .or(entity_rotation)
                .unwrap_or((current.yaw, current.pitch)),
        };
        move_entity(ctx.world, victim, position, yaw, pitch);
        if to_entity.is_none() {
            ctx.notify(
                victim,
                "commands.tp.successVictim",
                vec![coordinates(position)],
            );
        }
    }

    let names = ctx.names(&victims);
    let output = match to_entity {
        Some(entity) => {
            CommandOutput::success("commands.tp.success", vec![names, ctx.name(entity)])
        }
        None => CommandOutput::success(
            "commands.tp.success.coordinates",
            vec![
                names,
                number(position.x),
                number(position.y),
                number(position.z),
            ],
        ),
    };
    Ok(output.with_count(victims.len()))
}

fn eye_offset(world: &World, entity: Entity) -> DVec3 {
    if world.get::<Player>(entity).is_some() {
        DVec3::Y * PLAYER_EYE_HEIGHT
    } else {
        DVec3::ZERO
    }
}

/// Yaw and pitch looking from `eye` to `at`.
fn facing(eye: DVec3, at: DVec3) -> (f32, f32) {
    let delta = at - eye;
    let horizontal = (delta.x * delta.x + delta.z * delta.z).sqrt();
    (
        facing_yaw(eye, at),
        (-delta.y.atan2(horizontal)).to_degrees() as f32,
    )
}

fn number(value: f64) -> String {
    format!("{value:.2}")
}

fn coordinates(p: DVec3) -> String {
    format!("{}, {}, {}", number(p.x), number(p.y), number(p.z))
}

/// Feet and head blocks at the destination do not block movement.
fn space_is_clear(world: &World, feet: DVec3) -> bool {
    let block = feet.floor().as_ivec3();
    [block, block + IVec3::Y].into_iter().all(|pos| {
        crate::server::game::blocks::block_at(world, (pos.x, pos.y, pos.z)).is_none_or(|id| {
            crate::registry::BlockRegistry::vanilla()
                .get_by_runtime_id(id)
                .is_none_or(|entry| !entry.data.blocks_motion)
        })
    })
}

/// Moves one entity; `position` is its new feet position.
pub fn move_entity(world: &mut World, entity: Entity, position: DVec3, yaw: f32, pitch: f32) {
    if let Some(mut rotation) = world.get_mut::<Rotation>(entity) {
        *rotation = Rotation::new(yaw, pitch);
    }
    if world.get::<Player>(entity).is_none() {
        if let Some(mut current) = world.get_mut::<Position>(entity) {
            current.0 = position;
        }
        return;
    }
    let eye = position + DVec3::Y * PLAYER_EYE_HEIGHT;
    if let Some(mut current) = world.get_mut::<Position>(entity) {
        current.0 = eye;
    }
    if let Some(mut loader) = world.get_mut::<ChunkLoader>(entity) {
        loader.move_to(
            (position.x / 16.0).floor() as i32,
            (position.z / 16.0).floor() as i32,
        );
    }
    let runtime_id = world.get::<RuntimeEntityId>(entity).map_or(0, |r| r.0);
    let tick = world.get_resource::<TickCounter>().map_or(0, |t| t.current);
    let packet = MovePlayerPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        position: Vec3 {
            x: eye.x as f32,
            y: eye.y as f32,
            z: eye.z as f32,
        },
        rotation: Vec2 { x: pitch, y: yaw },
        // Measured: BDS leaves head yaw 0 in command teleports, even with a yaw.
        y_head_rotation: 0.0,
        position_mode: EnumsPlayerPositionModeComponentPositionMode::Teleport,
        on_ground: false,
        riding_runtime_id: ActorRuntimeId {
            actor_runtime_id: 0,
        },
        teleport_data: Some(MovePlayerTeleportData {
            teleportation_cause: TELEPORT_CAUSE_COMMAND,
            source_actor_type: SOURCE_ACTOR_TYPE,
        }),
        tick: PlayerInputTick { inputtick: tick },
    };
    if let Some(session) = world.get::<PlayerSession>(entity) {
        let _ = session.send(packet.into());
    }
}
