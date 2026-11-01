//! Sessionless sources use real native collision and finite terrain residency, without client input.

use super::*;
use crate::entity::components::{OnGround, PhysicsBody, PlayerState};
use crate::entity::systems::{BlockSource, WorldBlocks, step_body_sensed};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Resource, Default)]
struct Residents(BTreeMap<Entity, BTreeSet<(i32, i32)>>);

/// Keep only each real source's physical cells loaded, and release its references after retirement.
pub(super) fn update(world: &mut World) {
    let bodies: Vec<_> = world
        .query::<(
            Entity,
            &ManagedSource,
            Option<&Position>,
            Option<&Velocity>,
            Option<&SourceBody>,
        )>()
        .iter(world)
        .filter_map(|(actor, _, position, velocity, held)| {
            let eyes = position.map(|position| position.0).or_else(|| {
                held.and_then(|held| {
                    quarantine::source_snapshot(world, &held.0)
                        .map(|raw| DVec3::from_array(raw.data.position))
                })
            })?;
            let feet = eyes - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
            Some((
                actor,
                feet,
                feet + velocity.map_or(DVec3::ZERO, |velocity| velocity.0),
            ))
        })
        .collect();
    let mut desired = BTreeMap::new();
    for (actor, feet, next) in bodies {
        let cells = desired.entry(actor).or_insert_with(BTreeSet::new);
        for position in [feet, next] {
            if !position.is_finite() {
                continue;
            }
            let min = (position - DVec3::new(0.3, 0.0, 0.3)).floor().as_ivec3();
            let max = (position + DVec3::new(0.3, 0.0, 0.3)).floor().as_ivec3();
            for x in min.x >> 4..=max.x >> 4 {
                for z in min.z >> 4..=max.z >> 4 {
                    cells.insert((x, z));
                }
            }
            let _ = crate::server::worlds::prepare_chunks_with_anchor(world, position, actor);
        }
    }
    let old = world.get_resource_or_init::<Residents>();
    let previous = std::mem::replace(&mut old.into_inner().0, desired.clone());
    for (actor, cells) in previous {
        for cell in cells {
            if desired
                .get(&actor)
                .is_some_and(|cells| cells.contains(&cell))
            {
                continue;
            }
            let chunk = world
                .resource::<crate::world::ChunkManager>()
                .get_by_coords(cell.0, cell.1);
            if let Some(chunk) = chunk
                && let Some(mut viewers) = world.get_mut::<crate::world::ecs::ChunkViewers>(chunk)
            {
                viewers.remove(actor);
            }
            let mut manager = world.resource_mut::<crate::world::ChunkManager>();
            if let Some(viewers) = manager.pending_viewers.get_mut(&cell) {
                viewers.retain(|viewer| *viewer != actor);
            }
            if let Some(viewers) = manager.pending_generation.get_mut(&cell) {
                viewers.retain(|viewer| *viewer != actor);
            }
        }
    }
}

/// Source motion uses the existing living-body computer and the native player's box/eye geometry.
#[allow(clippy::type_complexity)]
pub(super) fn motion(
    blocks: WorldBlocks,
    rules: Res<crate::entity::player_life::PlayerRules>,
    mut bodies: Query<
        (
            &ManagedSource,
            &mut Position,
            &mut Velocity,
            &Age,
            &mut PlayerFall,
            &GameMode,
            &PlayerState,
            Option<&mut OnGround>,
        ),
        (
            With<crate::server::game::player_events::DetachedPlayer>,
            Without<PlayerSession>,
            Without<Dead>,
        ),
    >,
) {
    let body = PhysicsBody::living(rules.width, rules.height);
    for (managed, mut position, mut velocity, age, mut fall, mode, state, ground) in &mut bodies {
        if managed.record.phase != SourcePhase::Active {
            continue;
        }
        let feet = position.0 - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
        if blocks.block_at(feet.floor().as_ivec3()).is_none() {
            continue;
        }
        let mut movement = velocity.0;
        let mut grounded = ground.as_ref().is_some_and(|ground| ground.0);
        let flying = state.flying || *mode == GameMode::Spectator;
        let next = if flying {
            feet + movement
        } else {
            step_body_sensed(&blocks, &body, feet, &mut movement, &mut grounded, age.0).0
        };
        fall.after_move(next.y - feet.y, grounded, flying);
        if next != feet {
            position.0 = next + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
        }
        if velocity.0 != movement {
            velocity.0 = movement;
        }
        if let Some(mut ground) = ground
            && ground.0 != grounded
        {
            ground.0 = grounded;
        }
    }
}
