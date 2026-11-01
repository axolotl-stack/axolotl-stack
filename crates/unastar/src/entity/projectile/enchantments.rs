//! Bow enchantments captured by the accepted shot, independent of its owner.

use bevy_ecs::prelude::*;
use jolyne::valentine::{actor_data_i_ds as ids, actor_flags};

use crate::ecs::resources::TickCounter;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::systems::physics::living_liquid;
use crate::item::ItemStack;
use crate::item::enchant::{Enchantment, enchantment_level};
use crate::registry::block::LiquidKind;

const FLAME_TICKS: u64 = 100;

/// A shot's own weapon enchantments; changing or removing its owner cannot
/// replace these facts with a later weapon's enchantments.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub(crate) struct BowEnchantments {
    power: u8,
    flame_until: Option<u64>,
}

/// Capture the actual bow at the launch boundary, after item authorization.
pub(crate) fn capture(world: &mut World, projectile: Entity, bow: &ItemStack) {
    let now = world.resource::<TickCounter>().current;
    let shot = BowEnchantments {
        power: enchantment_level(bow, Enchantment::Power),
        flame_until: (enchantment_level(bow, Enchantment::Flame) > 0)
            .then(|| now.saturating_add(FLAME_TICKS)),
    };
    world.entity_mut(projectile).insert(shot);
    if shot.flame_until.is_some() {
        set_fire_flag(world, projectile, true);
    }
}

/// The extra speed-damage coefficient: zero without Power, otherwise
/// `(level + 1) / 2`, applied before the existing ceiling and critical bonus.
pub(crate) fn power_bonus(world: &World, projectile: Entity) -> f32 {
    world
        .get::<BowEnchantments>(projectile)
        .filter(|shot| shot.power > 0)
        .map_or(0.0, |shot| (f32::from(shot.power) + 1.0) * 0.5)
}

/// Accepted hits from a still-burning Flame arrow ignite for five seconds.
pub(crate) fn flame_ticks(world: &World, projectile: Entity) -> u64 {
    let now = world.resource::<TickCounter>().current;
    world
        .get::<BowEnchantments>(projectile)
        .and_then(|shot| shot.flame_until)
        .filter(|until| now < *until)
        .map_or(0, |_| FLAME_TICKS)
}

/// Expire or extinguish the shot's fire even when it is stuck in a block.
pub(super) fn tick(world: &mut World, projectile: Entity) {
    let Some(until) = world
        .get::<BowEnchantments>(projectile)
        .and_then(|shot| shot.flame_until)
    else {
        return;
    };
    let expired = world.resource::<TickCounter>().current >= until;
    let in_water = crate::entity::geometry::collision_box(world, projectile)
        .is_some_and(|bounds| living_liquid(world, bounds) == Some(LiquidKind::Water));
    if expired || in_water {
        world
            .get_mut::<BowEnchantments>(projectile)
            .expect("shot exists")
            .flame_until = None;
        set_fire_flag(world, projectile, false);
    }
}

/// Publish the shot's burning flag through the ordinary actor metadata lane.
fn set_fire_flag(world: &mut World, projectile: Entity, burning: bool) {
    let Some(mut flags) = world.get_mut::<ActorFlags>(projectile) else {
        return;
    };
    flags
        .update(actor_flags::ONFIRE, burning)
        .expect("ONFIRE is a known actor flag");
    let words = *flags.snapshot().words();
    if let Some(mut metadata) = world.get_mut::<EntityMetadata>(projectile) {
        metadata.set_long(ids::RESERVED_0, words[0] as i64);
    }
}
