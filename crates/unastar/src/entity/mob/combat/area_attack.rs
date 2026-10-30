//! `minecraft:area_attack`: contact damage.
//!
//! Every tick each living mob with the component hurts every living actor
//! (mob or player, never itself) whose box touches its own box grown by
//! `damage_range` on every side and that passes `entity_filter` (`self` is
//! the mob, `other` the actor). The hit deals `damage_per_tick` with the
//! authored cause and the mob as attacker, so the hurt rules, armour and
//! knockback apply as for any attack. With a `damage_cooldown` the mob hurts
//! the same actor again only once the cooldown ran out, whether the hit hurt
//! or not; with `play_attack_sound` every hit plays the `attack` level sound.
//! This matches BDS 1.26.51.

use bevy_ecs::prelude::*;
use glam::DVec3;

use super::{Hit, hurt_mob, knockback_power_of};
use crate::ecs::resources::TickCounter;
use crate::entity::components::{PhysicsBody, Player, Position};
use crate::entity::item_drop::PLAYER_EYE_HEIGHT;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::area_attack::AreaAttackConfig;
use crate::entity::mob::constants::{AREA_ATTACK_SOUND, PLAYER_BOX_HEIGHT, PLAYER_BOX_WIDTH};
use crate::entity::mob::events::MobFacts;
use crate::entity::mob::net::sound::play_level_sound;
use crate::entity::mob::state::Dead;
use crate::entity::player_life::hurt_player;
use crate::registry::collision::Aabb;

/// The actors an area-attacking mob hurt recently: each with the server
/// tick its cooldown runs out at. Only kept while the cooldown is positive.
#[derive(Component, Debug, Default, Clone, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct AreaAttackCooldowns(Vec<(Entity, u64)>);

impl AreaAttackCooldowns {
    fn ready(&self, actor: Entity, now: u64) -> bool {
        self.0
            .iter()
            .find(|(other, _)| *other == actor)
            .is_none_or(|&(_, until)| now >= until)
    }

    fn start(&mut self, actor: Entity, now: u64, until: u64) {
        // Expired entries (and actors gone since) are dropped here.
        self.0
            .retain(|&(other, deadline)| other != actor && now < deadline);
        self.0.push((actor, until));
    }
}

/// Reused buffers of [`area_attack`].
#[derive(Default)]
pub struct AreaAttackScratch {
    attackers: Vec<(Entity, Aabb)>,
    /// Every living actor with its box.
    actors: Vec<(Entity, Aabb, bool)>,
}

/// System (exclusive): run every mob's `minecraft:area_attack` (module docs).
#[allow(clippy::type_complexity)]
pub fn area_attack(
    world: &mut World,
    attackers: &mut QueryState<
        (Entity, &Position, &PhysicsBody),
        (With<MobKind>, With<AreaAttackConfig>, Without<Dead>),
    >,
    mobs: &mut QueryState<(Entity, &Position, &PhysicsBody), (With<MobKind>, Without<Dead>)>,
    players: &mut QueryState<(Entity, &Position), (With<Player>, Without<Dead>)>,
    mut scratch: Local<AreaAttackScratch>,
) {
    let scratch = &mut *scratch;
    scratch.attackers.clear();
    scratch
        .attackers
        .extend(attackers.iter(world).map(|(entity, position, body)| {
            (entity, Aabb::from_feet(position.0, body.width, body.height))
        }));
    if scratch.attackers.is_empty() {
        return;
    }
    scratch.actors.clear();
    scratch
        .actors
        .extend(mobs.iter(world).map(|(entity, position, body)| {
            (
                entity,
                Aabb::from_feet(position.0, body.width, body.height),
                false,
            )
        }));
    scratch
        .actors
        .extend(players.iter(world).map(|(entity, position)| {
            let feet = position.0 - DVec3::Y * PLAYER_EYE_HEIGHT;
            (
                entity,
                Aabb::from_feet(feet, PLAYER_BOX_WIDTH, PLAYER_BOX_HEIGHT),
                true,
            )
        }));
    let now = world.resource::<TickCounter>().current;
    for &(owner, own_box) in &scratch.attackers {
        let Some(config) = world.get::<AreaAttackConfig>(owner).cloned() else {
            continue;
        };
        let reach = own_box.inflate(DVec3::splat(config.range));
        for &(actor, actor_box, is_player) in &scratch.actors {
            if actor == owner || !reach.intersects(&actor_box) {
                continue;
            }
            // An earlier hit this tick may have killed the owner.
            if world.get::<Dead>(owner).is_some() {
                break;
            }
            let passes = config.filter.evaluate(&MobFacts {
                world,
                entity: owner,
                other: Some(actor),
            });
            if !passes
                || world
                    .get::<AreaAttackCooldowns>(owner)
                    .is_some_and(|cooldowns| !cooldowns.ready(actor, now))
            {
                continue;
            }
            if config.cooldown_ticks > 0 {
                world
                    .entity_mut(owner)
                    .entry::<AreaAttackCooldowns>()
                    .or_default()
                    .get_mut()
                    .start(actor, now, now + config.cooldown_ticks);
            }
            let feet = world.get::<Position>(owner).map_or(DVec3::ZERO, |p| p.0);
            let hit = Hit {
                damage: config.damage,
                cause: config.cause,
                attacker: Some(owner),
                by_player: false,
                knockback_from: Some(feet),
                knockback_power: knockback_power_of(world, owner),
            };
            if is_player {
                hurt_player(world, actor, &hit);
            } else {
                hurt_mob(world, actor, hit);
            }
            if config.play_attack_sound {
                let centre = (own_box.min + own_box.max) * 0.5;
                play_level_sound(world, Some(owner), centre, AREA_ATTACK_SOUND, -1);
            }
        }
    }
}
