//! `/teleport` (`/tp`).
//!
//! Relative destination coordinates and rotations resolve against the command
//! source, not each victim, as in Bedrock. Players are moved with a
//! `MovePlayer` teleport (their chunk loader follows); other actors get a new
//! position and rotation, which the actor sync broadcasts.
//!
//! Measured (BDS 1.26.51): the victim gets `commands.tp.successVictim` with
//! the position; the source gets `commands.tp.success.coordinates [names, x, y,
//! z]` with two decimals; the `MovePlayer` has cause 3, source actor type 1 and
//! head yaw 0.

use crate::command::prelude::*;
use crate::ecs::resources::TickCounter;
use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{Player, PlayerSession, RuntimeEntityId};
use crate::entity::geometry::look_at;
use crate::world::ecs::ChunkLoader;
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsPlayerPositionModeComponentPositionMode, PlayerInputTick, Vec2, Vec3,
};
use jolyne::valentine::{MovePlayerPacket, MovePlayerTeleportData};

/// `MovePlayer` teleport cause for commands.
const TELEPORT_CAUSE_COMMAND: i32 = 3;
/// `source_actor_type` BDS 1.26.51 sends in command teleports (measured).
const SOURCE_ACTOR_TYPE: i32 = 1;

overloads! {
    /// One variant per captured overload; `victim` absent means the source.
    pub enum TeleportArgs for "teleport" {
        0 => ToPosition {
            destination: CommandPosition,
            check: Option<bool> = "checkForBlocks",
            force: Option<bool> = "forceProvidedPositionOnDimensionChange",
        },
        1 => ToPositionRotated {
            destination: CommandPosition,
            yaw: Option<RelativeFloat> = "yRot",
            pitch: Option<RelativeFloat> = "xRot",
            check: Option<bool> = "checkForBlocks",
            force: Option<bool> = "forceProvidedPositionOnDimensionChange",
        },
        2 => ToPositionFacingPosition {
            destination: CommandPosition,
            facing: EnumValue,
            at: CommandPosition = "lookAtPosition",
            check: Option<bool> = "checkForBlocks",
            force: Option<bool> = "forceProvidedPositionOnDimensionChange",
        },
        3 => ToPositionFacingEntity {
            destination: CommandPosition,
            facing: EnumValue,
            at: Target = "lookAtEntity",
            check: Option<bool> = "checkForBlocks",
            force: Option<bool> = "forceProvidedPositionOnDimensionChange",
        },
        4 => VictimToPositionRotated {
            victim: Target,
            destination: CommandPosition,
            yaw: Option<RelativeFloat> = "yRot",
            pitch: Option<RelativeFloat> = "xRot",
            check: Option<bool> = "checkForBlocks",
            force: Option<bool> = "forceProvidedPositionOnDimensionChange",
        },
        5 => VictimToPosition {
            victim: Target,
            destination: CommandPosition,
            check: Option<bool> = "checkForBlocks",
            force: Option<bool> = "forceProvidedPositionOnDimensionChange",
        },
        6 => VictimToPositionFacingPosition {
            victim: Target,
            destination: CommandPosition,
            facing: EnumValue,
            at: CommandPosition = "lookAtPosition",
            check: Option<bool> = "checkForBlocks",
            force: Option<bool> = "forceProvidedPositionOnDimensionChange",
        },
        7 => VictimToPositionFacingEntity {
            victim: Target,
            destination: CommandPosition,
            facing: EnumValue,
            at: Target = "lookAtEntity",
            check: Option<bool> = "checkForBlocks",
            force: Option<bool> = "forceProvidedPositionOnDimensionChange",
        },
        8 => ToEntity { destination: Target },
        9 => VictimToEntity {
            victim: Target,
            destination: Target,
            check: Option<bool> = "checkForBlocks",
            force: Option<bool> = "forceProvidedPositionOnDimensionChange",
        },
    }
}

/// Where the victims go.
enum Destination {
    Position(CommandPosition),
    Entity(Target),
}

/// Which way they face afterwards.
enum Facing {
    /// Keep their rotation (or take the destination entity's).
    Unchanged,
    Rotation(Option<RelativeFloat>, Option<RelativeFloat>),
    Position(CommandPosition),
    Entity(Target),
}

/// The overloads reduced to one plan. `force` only matters across
/// dimensions, which unastar does not have yet.
struct Plan {
    victim: Option<Target>,
    destination: Destination,
    facing: Facing,
    check: bool,
}

impl From<TeleportArgs> for Plan {
    fn from(args: TeleportArgs) -> Self {
        use TeleportArgs as T;
        let plan = |victim, destination, facing, check: Option<bool>| Plan {
            victim,
            destination,
            facing,
            check: check.unwrap_or(false),
        };
        match args {
            T::ToPosition {
                destination, check, ..
            } => plan(
                None,
                Destination::Position(destination),
                Facing::Unchanged,
                check,
            ),
            T::ToPositionRotated {
                destination,
                yaw,
                pitch,
                check,
                ..
            } => plan(
                None,
                Destination::Position(destination),
                Facing::Rotation(yaw, pitch),
                check,
            ),
            T::ToPositionFacingPosition {
                destination,
                at,
                check,
                ..
            } => plan(
                None,
                Destination::Position(destination),
                Facing::Position(at),
                check,
            ),
            T::ToPositionFacingEntity {
                destination,
                at,
                check,
                ..
            } => plan(
                None,
                Destination::Position(destination),
                Facing::Entity(at),
                check,
            ),
            T::VictimToPositionRotated {
                victim,
                destination,
                yaw,
                pitch,
                check,
                ..
            } => plan(
                Some(victim),
                Destination::Position(destination),
                Facing::Rotation(yaw, pitch),
                check,
            ),
            T::VictimToPosition {
                victim,
                destination,
                check,
                ..
            } => plan(
                Some(victim),
                Destination::Position(destination),
                Facing::Unchanged,
                check,
            ),
            T::VictimToPositionFacingPosition {
                victim,
                destination,
                at,
                check,
                ..
            } => plan(
                Some(victim),
                Destination::Position(destination),
                Facing::Position(at),
                check,
            ),
            T::VictimToPositionFacingEntity {
                victim,
                destination,
                at,
                check,
                ..
            } => plan(
                Some(victim),
                Destination::Position(destination),
                Facing::Entity(at),
                check,
            ),
            T::ToEntity { destination } => plan(
                None,
                Destination::Entity(destination),
                Facing::Unchanged,
                None,
            ),
            T::VictimToEntity {
                victim,
                destination,
                check,
                ..
            } => plan(
                Some(victim),
                Destination::Entity(destination),
                Facing::Unchanged,
                check,
            ),
        }
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: TeleportArgs) -> CommandResult {
    let plan = Plan::from(args);
    let victims = ctx.entities(plan.victim.as_ref())?;
    let (position, to_entity) = match &plan.destination {
        Destination::Position(p) => (ctx.position(Some(*p)), None),
        Destination::Entity(target) => {
            let entity = ctx.entity(Some(target))?;
            let feet = ctx.feet(entity).ok_or_else(CommandOutput::no_targets)?;
            (feet, Some(entity))
        }
    };
    let look_target = match &plan.facing {
        Facing::Position(p) => Some(ctx.position(Some(*p))),
        Facing::Entity(target) => {
            let entity = ctx.entities(Some(target))?[0];
            ctx.eye(entity)
        }
        Facing::Unchanged | Facing::Rotation(..) => None,
    };

    if plan.check && !space_is_clear(ctx.world, position) {
        return failure!(
            "commands.tp.safeTeleportFail",
            ctx.names(&victims),
            format::position(position)
        );
    }

    for &victim in &victims {
        let current = ctx
            .world
            .get::<Rotation>(victim)
            .copied()
            .unwrap_or_default();
        let (yaw, pitch) = match (&plan.facing, look_target) {
            (_, Some(at)) => {
                let eye =
                    position + DVec3::Y * crate::entity::geometry::eye_height(ctx.world, victim);
                look_at(eye, at)
            }
            (Facing::Rotation(yaw, pitch), _) => ctx.rotation(*yaw, *pitch),
            _ => to_entity
                .and_then(|e| ctx.world.get::<Rotation>(e))
                .map_or((current.yaw, current.pitch), |r| (r.yaw, r.pitch)),
        };
        move_entity(ctx.world, victim, position, yaw, pitch);
        if to_entity.is_none() {
            ctx.notify(
                victim,
                "commands.tp.successVictim",
                vec![format::position(position)],
            );
        }
    }

    let names = ctx.names(&victims);
    let output = match to_entity {
        Some(entity) => {
            CommandOutput::success("commands.tp.success", vec![names, ctx.name(entity)])
        }
        None => {
            let [x, y, z] = format::position_params(position);
            CommandOutput::success("commands.tp.success.coordinates", vec![names, x, y, z])
        }
    };
    Ok(output.with_count(victims.len()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;
    use jolyne::valentine::McpePacketData;

    #[test]
    fn teleport_to_coordinates_matches_the_bds_capture() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        assert_eq!(
            h.run(op, "/tp 0 100 0"),
            CommandOutput::success(
                "commands.tp.success.coordinates",
                vec!["Op".into(), "0.50".into(), "100.00".into(), "0.50".into()]
            )
        );
        let sent = h.sent(op);
        let McpePacketData::MovePlayerPacket(moved) = &sent[0] else {
            panic!("{sent:?}");
        };
        assert_eq!(
            (moved.position.x, moved.position.y, moved.position.z),
            (0.5, 101.62, 0.5)
        );
        assert!(matches!(&sent[1], McpePacketData::TextPacket(t)
            if format!("{t:?}").contains("commands.tp.successVictim")
                && format!("{t:?}").contains("0.50, 100.00, 0.50")));

        h.run(op, "/tp @s ~ ~5 ~ 90 0");
        let moved = h
            .sent(op)
            .into_iter()
            .find_map(|p| match p {
                McpePacketData::MovePlayerPacket(p) => Some(p),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            (moved.rotation.x, moved.rotation.y, moved.y_head_rotation),
            (0.0, 90.0, 0.0)
        );
        assert_eq!(moved.position.y, 106.62_f32);
    }

    #[test]
    fn unknown_names_match_no_target_and_entities_are_destinations() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        let _bob = h.player_at("Bob", false, DVec3::new(20.5, 70.0, 20.5));
        assert_eq!(h.run(op, "/tp NoSuchPlayer"), CommandOutput::no_targets());
        assert_eq!(
            h.run(op, "/tp Bob"),
            CommandOutput::success("commands.tp.success", vec!["Op".into(), "Bob".into()])
        );
        assert_eq!(h.world().get::<Position>(op).unwrap().0.x, 20.5);
    }
}
