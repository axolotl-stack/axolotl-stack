//! Actual potion splash recipients and water-bottle block effects.

use crate::entity::components::{GameMode, Health, PhysicsBody, Player, Position};
use crate::entity::mob::state::Dead;
use crate::entity::systems::physics::BlockSource;
use crate::registry::block::StateValue;
use crate::registry::collision::Aabb;
use crate::server::game::effects::add_effect_scaled_from;
use crate::server::game::potions::{PotionProfile, ProjectilePotion, is_instant};
use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};

/// Snapshot living feet positions, excluding spectators, dead and removed actors.
pub(super) fn recipients(world: &mut World) -> Vec<(Entity, DVec3, Aabb)> {
    world
        .query_filtered::<(
            Entity,
            &Position,
            &Health,
            Option<&Player>,
            Option<&PhysicsBody>,
            Option<&GameMode>,
        ), Without<Dead>>()
        .iter(world)
        .filter(|(_, _, health, _, _, mode)| {
            !health.is_dead() && *mode != Some(&GameMode::Spectator)
        })
        .filter_map(|(actor, position, _, player, body, _)| {
            if player.is_some() {
                Some((
                    actor,
                    position.0 - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
                    crate::server::game::player_box(position.0),
                ))
            } else {
                let body = body?;
                Some((
                    actor,
                    position.0,
                    Aabb::from_feet(position.0, body.width, body.height),
                ))
            }
        })
        .collect()
}

/// Apply a profile with separate duration and instantaneous intensity scaling.
pub(super) fn apply(
    world: &mut World,
    target: Entity,
    source: Entity,
    profile: &PotionProfile,
    duration_scale: f64,
    potency: f32,
) -> bool {
    let mut accepted = false;
    for effect in &profile.effects {
        let duration = if is_instant(effect.effect) {
            0
        } else {
            (f64::from(effect.duration_ticks) * duration_scale).round() as u32
        };
        if !is_instant(effect.effect) && duration < 20 {
            continue;
        }
        accepted |= add_effect_scaled_from(
            world,
            target,
            Some(source),
            effect.effect,
            effect.amplifier,
            duration,
            potency,
        );
    }
    accepted
}

/// Splash near the actual collision point, giving a directly struck actor full intensity.
pub(crate) fn splash(
    world: &mut World,
    projectile: Entity,
    potion: &ProjectilePotion,
    at: DVec3,
    direct: Option<Entity>,
    duration_scale: f64,
) {
    for (actor, feet, _) in recipients(world) {
        let distance = feet.distance(at);
        if distance > 4.0 {
            continue;
        }
        let factor = if Some(actor) == direct {
            1.0
        } else {
            1.0 - distance / 4.0
        };
        apply(
            world,
            actor,
            projectile,
            &potion.profile,
            duration_scale * factor,
            factor as f32,
        );
    }
}

/// Extinguish affected fire and campfires independently through their block hook.
pub(crate) fn douse(world: &mut World, projectile: Entity, at: DVec3) {
    let direction = world
        .get::<crate::entity::components::Velocity>(projectile)
        .map_or(DVec3::ZERO, |velocity| velocity.0.normalize_or_zero());
    let center = (at - direction * 1e-7).floor().as_ivec3();
    let hit = (at + direction * 1e-7).floor().as_ivec3();
    let registry = world
        .resource::<crate::server::game::types::BlockRegistryResource>()
        .0
        .clone();
    // The cell immediately before impact is outside the touched face. Water
    // reaches its four horizontal neighbors and the directly touched campfire.
    let mut positions = Vec::new();
    for offset in [IVec3::ZERO, IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z] {
        positions.push(center + offset);
    }
    if !positions.contains(&hit) {
        positions.push(hit);
    }
    for pos in positions {
        let Some(state) = world.block_at(pos) else {
            continue;
        };
        let Some(block) = registry.get_by_runtime_id(state) else {
            continue;
        };
        let next = match block.string_id.as_str() {
            "minecraft:fire" | "minecraft:soul_fire" => Some(*crate::world::chunk::blocks::AIR),
            "minecraft:campfire" | "minecraft:soul_campfire" => {
                block.with_state_value(state, "extinguished", StateValue::Byte(1))
            }
            _ => None,
        };
        if let Some(next) = next.filter(|next| *next != state)
            && super::hooks::block_change(world, projectile, pos, next)
        {
            crate::server::game::blocks::set_block(world, pos, next, false);
        }
    }
}

/// Extinguish each nearby burning actor after its own native authorization.
pub(super) fn douse_actors(world: &mut World, source: Entity, at: DVec3) {
    for (actor, feet, _) in recipients(world) {
        let burning = world
            .get::<crate::entity::player_life::environment::Burning>(actor)
            .is_some()
            || world
                .get::<crate::entity::mob::combat::fire::Burning>(actor)
                .is_some()
            || world
                .get::<crate::entity::components::OnFire>(actor)
                .is_some_and(|fire| fire.is_on_fire());
        if feet.distance(at) > 4.0 || !burning || !super::hooks::extinguish(world, source, actor) {
            continue;
        }
        if let Ok(mut actor) = world.get_entity_mut(actor) {
            actor.remove::<crate::entity::player_life::environment::Burning>();
            if let Some(mut fire) = actor.get_mut::<crate::entity::components::OnFire>() {
                fire.extinguish();
            }
        }
        if world.get::<crate::entity::mob::MobKind>(actor).is_some() {
            crate::entity::mob::combat::fire::extinguish(world, actor);
        }
    }
}
