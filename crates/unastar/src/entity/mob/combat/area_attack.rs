//! `minecraft:area_attack`: contact damage (the slime classes').
//!
//! Every tick a living mob with the component hurts each actor that its
//! filter accepts (`other` = that actor) and whose box touches the mob's
//! box grown by `damage_range` on every side: `damage_per_tick` of its
//! cause, knocking the actor away from the mob. After a hit the mob waits
//! `damage_cooldown` before it hurts anyone again; the target's own hurt
//! immunity still applies. Players in creative or spectator are never hurt
//! (`hurt_player` refuses them).

use bevy_ecs::prelude::*;

use super::{Hit, hurt_mob, knockback_power_of};
use crate::ecs::resources::TickCounter;
use crate::entity::components::{PhysicsBody, Player, Position};
use crate::entity::item_drop::PLAYER_EYE_HEIGHT;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::AreaAttackConfig;
use crate::entity::mob::constants::{PLAYER_BOX_HEIGHT, PLAYER_BOX_WIDTH};
use crate::entity::mob::events::MobFacts;
use crate::entity::mob::state::Dead;
use crate::entity::player_life::hurt_player;
use crate::registry::collision::Aabb;

/// The server tick before which an area attacker hurts no one.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AreaAttackCooldown(pub u64);

/// System (exclusive): every area attacker hurts what touches it.
#[allow(clippy::type_complexity)]
pub fn area_attacks(
    world: &mut World,
    attackers: &mut QueryState<
        (Entity, &AreaAttackConfig, Option<&AreaAttackCooldown>),
        (With<MobKind>, Without<Dead>),
    >,
    actors: &mut QueryState<(Entity, &Position, Option<&PhysicsBody>, Has<Player>), Without<Dead>>,
    mut due: Local<Vec<(Entity, AreaAttackConfig)>>,
    mut touching: Local<Vec<(Entity, bool)>>,
) {
    let now = world.resource::<TickCounter>().current;
    due.extend(
        attackers
            .iter(world)
            .filter(|(_, _, cooldown)| cooldown.is_none_or(|c| now >= c.0))
            .map(|(entity, config, _)| (entity, config.clone())),
    );
    for (attacker, config) in due.drain(..) {
        let (Some(feet), Some(body)) = (
            world.get::<Position>(attacker).map(|p| p.0),
            world.get::<PhysicsBody>(attacker),
        ) else {
            continue;
        };
        let reach = Aabb::from_feet(feet, body.width, body.height)
            .inflate(glam::DVec3::splat(config.range));
        touching.clear();
        touching.extend(
            actors
                .iter(world)
                .filter(|(entity, ..)| *entity != attacker)
                .filter_map(|(entity, position, body, player)| {
                    let aabb = if player {
                        Aabb::from_feet(
                            position.0 - glam::DVec3::Y * PLAYER_EYE_HEIGHT,
                            PLAYER_BOX_WIDTH,
                            PLAYER_BOX_HEIGHT,
                        )
                    } else {
                        let body = body?;
                        Aabb::from_feet(position.0, body.width, body.height)
                    };
                    reach.intersects(&aabb).then_some((entity, player))
                }),
        );
        let mut hurt_any = false;
        for &(target, player) in touching.iter() {
            let accepted = config.filter.evaluate(&MobFacts {
                world,
                entity: attacker,
                other: Some(target),
            });
            if !accepted {
                continue;
            }
            let hit = Hit {
                damage: config.damage,
                cause: config.cause,
                attacker: Some(attacker),
                by_player: false,
                knockback_from: Some(feet),
                knockback_power: knockback_power_of(world, attacker),
            };
            let dealt = if player {
                hurt_player(world, target, &hit)
            } else {
                hurt_mob(world, target, hit)
            };
            hurt_any |= dealt > 0.0;
        }
        if hurt_any && config.cooldown_ticks > 0 {
            world
                .entity_mut(attacker)
                .insert(AreaAttackCooldown(now + u64::from(config.cooldown_ticks)));
        }
    }
}
