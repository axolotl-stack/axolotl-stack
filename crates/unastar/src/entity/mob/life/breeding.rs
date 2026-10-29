//! Mating and love-mode expiry.
//!
//! The breed goal raises a [`MateRequest`] when two partners in love touch;
//! [`tick_breeding`] re-checks the pair, spawns the offspring at the first
//! parent through the `entity_born` path, copies the inherited properties
//! from a random parent, clears love mode on both, starts their
//! `breed_cooldown` and spawns the `experience_reward.on_bred` experience.
//! Love mode ends [`constants::LOVE_TICKS`] after feeding. Both timers are
//! deadlines on `TickCounter::current`; only love expiry has a visible
//! effect (the hearts flag), so only `InLove` is removed when it runs out.

use bevy_ecs::message::{MessageCursor, Messages};
use bevy_ecs::prelude::*;
use rand::Rng;
use tracing::warn;

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::Position;
use crate::entity::definition::properties::ActorProperties;
use crate::entity::mob::components::{BreedableConfig, ExperienceRewardConfig, OffspringConfig};
use crate::entity::mob::interaction::interact::set_love_flag;
use crate::entity::mob::state::{BreedCooldown, Dead, InLove, MateRequest};
use crate::entity::mob::{MobKind, SpawnCause, constants, spawn_mob};

/// System (exclusive, `EntityLogicSet`): end expired love mode, then mate
/// the requested pairs. This is the only reader of [`MateRequest`] and
/// advances its message buffers once per tick.
pub fn tick_breeding(world: &mut World, mut cursor: Local<MessageCursor<MateRequest>>) {
    let now = world.resource::<TickCounter>().current;
    let expired: Vec<Entity> = world
        .query::<(Entity, &InLove)>()
        .iter(world)
        .filter(|(_, love)| love.until_tick <= now)
        .map(|(entity, _)| entity)
        .collect();
    for entity in expired {
        world.entity_mut(entity).remove::<InLove>();
        set_love_flag(world, entity, false);
    }
    let Some(messages) = world.get_resource::<Messages<MateRequest>>() else {
        return;
    };
    let requests: Vec<MateRequest> = cursor.read(messages).copied().collect();
    for request in requests {
        mate(world, request, now);
    }
    world.resource_mut::<Messages<MateRequest>>().update();
}

/// A living mob in love that can breed: its identifier and breed config.
fn lover(world: &World, entity: Entity, now: u64) -> Option<(&str, &BreedableConfig)> {
    if world.get::<Dead>(entity).is_some()
        || world
            .get::<InLove>(entity)
            .is_none_or(|love| love.until_tick <= now)
    {
        return None;
    }
    Some((
        world.get::<MobKind>(entity)?.0.identifier.as_ref(),
        world.get::<BreedableConfig>(entity)?,
    ))
}

fn mate(world: &mut World, MateRequest { a, b }: MateRequest, now: u64) {
    if a == b {
        return;
    }
    let (Some((id_a, config_a)), Some((id_b, config_b))) =
        (lover(world, a, now), lover(world, b, now))
    else {
        return;
    };
    let accepts = |config: &BreedableConfig, mate: &str| {
        config.breeds_with.iter().any(|kind| kind.as_ref() == mate)
    };
    if !accepts(config_a, id_b) || !accepts(config_b, id_a) {
        return;
    }
    let offspring = world.get::<OffspringConfig>(a);
    let baby_id = offspring
        .and_then(|config| config.pairs.iter().find(|(mate, _)| mate.as_ref() == id_b))
        .map_or(id_a, |(_, baby)| baby.as_ref())
        .to_owned();
    let inherited = offspring
        .map(|config| std::sync::Arc::clone(&config.property_inheritance))
        .unwrap_or_default();
    let extra_baby_chance = config_a.extra_baby_chance;
    let cooldowns = [config_a.breed_cooldown, config_b.breed_cooldown];
    let Some(position) = world.get::<Position>(a).map(|p| p.0) else {
        return;
    };

    let extra = world.resource_mut::<EntityRandom>().0.r#gen::<f32>() < extra_baby_chance;
    for _ in 0..1 + usize::from(extra) {
        let baby = match spawn_mob(world, &baby_id, position, 0.0, SpawnCause::Born) {
            Ok(baby) => baby,
            Err(error) => {
                warn!(baby = %baby_id, %error, "offspring could not be spawned");
                return;
            }
        };
        let parent = if world.resource_mut::<EntityRandom>().0.gen_bool(0.5) {
            a
        } else {
            b
        };
        inherit_properties(world, parent, baby, &inherited);
    }
    for (parent, cooldown) in [a, b].into_iter().zip(cooldowns) {
        world
            .entity_mut(parent)
            .remove::<InLove>()
            .insert(BreedCooldown {
                until_tick: now + (cooldown.max(0.0) * constants::TICKS_PER_SECOND) as u64,
            });
        set_love_flag(world, parent, false);
    }
    let on_bred = world
        .get::<ExperienceRewardConfig>(a)
        .and_then(|reward| reward.on_bred.clone());
    if let Some(expr) = on_bred {
        let amount = crate::entity::mob::combat::death::experience_reward(world, a, &expr);
        crate::entity::xp_orb::spawn_experience(world, position, amount);
    }
}

/// Copy the named properties from `parent` to `baby` (both must declare
/// them; the cow's offspring is its own kind).
fn inherit_properties(
    world: &mut World,
    parent: Entity,
    baby: Entity,
    names: &[std::sync::Arc<str>],
) {
    if names.is_empty() {
        return;
    }
    let Ok([parent, mut baby]) = world.get_entity_mut([parent, baby]) else {
        return;
    };
    let (Some(from), Some(mut to)) = (
        parent.get::<ActorProperties>(),
        baby.get_mut::<ActorProperties>(),
    ) else {
        return;
    };
    for (name, value) in from.named_values() {
        if names.iter().any(|wanted| wanted.as_ref() == name)
            && let Err(error) = to.set_named(name, value)
        {
            warn!(property = name, %error, "inherited property does not fit the offspring");
        }
    }
}
