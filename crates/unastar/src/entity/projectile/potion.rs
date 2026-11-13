//! `on_hit` effects of thrown potions: `thrown_potion_effect` and
//! `douse_fire` (splash potions). The experience bottle's `grant_xp` is in
//! [`super::hit`].
//!
//! Measured on BDS 1.26.51 (captures of `tools/bds-sweep/scripts/throwables.txt`
//! and `throwables-2.txt`, replayed by `item_use/throw_tests.rs`):
//! - Every bottle breaks with `LevelEvent` 2002 at the hit point, its data the
//!   potion's colour ([`splash_colour`]); the experience bottle and water use
//!   the water colour.
//! - An experience bottle drops 3 to 10 experience, split into orbs at the
//!   hit point.
//! - A splash potion makes five item-crack particles of the splash potion
//!   item and gives its effects to the players around the hit point
//!   ([`splash_factor`]); instant effects act at once without a
//!   `MobEffect`.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;

use super::ProjectilePotion;
use super::flight::Target;
use crate::entity::EntityRandom;
use crate::entity::components::{EffectType, Player, Position};
use crate::entity::mob::constants::{
    ICONCRACK_ITEM_SHIFT, ICONCRACK_PARTICLE, POTION_SPLASH_LEVEL_EVENT,
    PROJECTILE_PARTICLE_LEVEL_EVENT, SPLASH_POTION_ICONCRACKS,
};
use crate::entity::mob::net::sound::level_event;
use crate::server::game::item_use::food::potion_effects;
use crate::server::game::types::ItemRegistryResource;

/// The splash colour (ARGB as the level event's data) of each potion data
/// value 0-46, as BDS 1.26.51 sent them (throwables-2 capture). Water and
/// the effectless potions use the water colour, the first entry.
const SPLASH_COLOURS: [i32; 47] = include!("splash_colours.in");

/// The splash colour of potion `data`.
pub fn splash_colour(data: i16) -> i32 {
    usize::try_from(data)
        .ok()
        .and_then(|index| SPLASH_COLOURS.get(index))
        .copied()
        .unwrap_or(SPLASH_COLOURS[0])
}

/// How much of a splash potion reaches a player whose box is `distance`
/// blocks from the hit point: `1 - distance / 4`, none from 4 blocks. BDS
/// 1.26.51 (throwables-2 capture) gave a second player whose box was 0.7,
/// 1.7, 2.7 and 3.6 blocks from the splash 743, 518, 293 and 90 of poison's
/// 900 ticks (the duration times the factor, rounded up), and nothing at
/// 4.2.
pub fn splash_factor(distance: f64) -> Option<f64> {
    (distance < 4.0).then(|| 1.0 - distance / 4.0)
}

/// The distance from `point` to the nearest point of `aabb` (0 inside).
fn distance_to_box(point: DVec3, aabb: &crate::registry::collision::Aabb) -> f64 {
    point.clamp(aabb.min, aabb.max).distance(point)
}

/// `thrown_potion_effect`: the splash, the item-crack particles and the
/// potion's effects on the players around `at` (a player hit directly gets
/// all of it).
pub(super) fn splash(
    world: &mut World,
    projectile: Entity,
    _owner: Option<Entity>,
    at: DVec3,
    target: Target,
) {
    let potion = world
        .get::<ProjectilePotion>(projectile)
        .map_or(0, |potion| potion.0);
    level_event(world, at, POTION_SPLASH_LEVEL_EVENT, splash_colour(potion));
    let item = world
        .get_resource::<ItemRegistryResource>()
        .and_then(|items| items.0.get_by_name("minecraft:splash_potion"))
        .map_or(0, |item| item.network_id());
    for _ in 0..SPLASH_POTION_ICONCRACKS {
        level_event(
            world,
            at,
            PROJECTILE_PARTICLE_LEVEL_EVENT,
            ICONCRACK_PARTICLE | (item << ICONCRACK_ITEM_SHIFT),
        );
    }
    let effects = potion_effects(potion);
    if effects.is_empty() {
        return;
    }
    let mut players: Vec<(Entity, f64)> = Vec::new();
    let mut query = world.query_filtered::<(Entity, &Position), With<Player>>();
    for (player, eye) in query.iter(world) {
        let aabb = crate::server::game::player_box(eye.0);
        let factor = if target == Target::Actor(player) {
            Some(1.0)
        } else {
            splash_factor(distance_to_box(at, &aabb))
        };
        if let Some(factor) = factor {
            players.push((player, factor));
        }
    }
    for (player, factor) in players {
        for &(effect, amplifier, duration) in effects {
            if matches!(
                effect,
                EffectType::INSTANT_HEALTH | EffectType::INSTANT_DAMAGE
            ) {
                crate::server::game::effects::apply_instant(
                    world, player, effect, amplifier, factor,
                );
                continue;
            }
            let ticks = (factor * f64::from(duration)).ceil() as u32;
            if ticks > 20 {
                // One more: the effect step of this tick, which already ran
                // for the player, takes one off before the client hears of
                // it (BDS sent the full 900 ticks of a poison splash).
                crate::server::game::effects::add_effect(
                    world,
                    player,
                    effect,
                    amplifier,
                    ticks + 1,
                );
            }
        }
    }
}

/// `douse_fire`: a water potion puts out the fire in the block it broke in
/// and in the four blocks beside it.
pub(super) fn douse_fire(world: &mut World, projectile: Entity, at: DVec3) {
    let potion = world
        .get::<ProjectilePotion>(projectile)
        .map_or(0, |potion| potion.0);
    if potion != 0 {
        return;
    }
    let centre = at.floor().as_ivec3();
    for offset in [IVec3::ZERO, IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z] {
        crate::server::game::blocks::extinguish_fire(world, centre + offset);
    }
}
