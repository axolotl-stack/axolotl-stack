//! The elytra: who may glide, and what gliding costs it.
//!
//! The client simulates the glide's movement; the server decides whether
//! the player glides. Measured on BDS 1.26.51 (`tools/bds-sweep/scripts/
//! elytra-2.txt` and `elytra-3.txt`, replayed by `elytra/tests.rs`):
//! - BDS starts a glide when the jump is pressed and held in the air with an
//!   elytra in the chest slot; unastar takes the client's `StartGliding` for
//!   that. Without an elytra with more than 1 durability left there is no
//!   glide.
//! - Gliding wears the elytra by 1 every 20 ticks: glides of 10, 19, 21, 39,
//!   41 and 100 ticks cost 0, 1, 1, 1, 2 and 5.
//! - A wear that finds 1 durability left stops the glide instead. An elytra
//!   with 12 left wore 11 times and stopped gliding at the next 20-tick mark
//!   (239 ticks after it started).
//! - No slot update is sent while gliding (the client predicts the wear); the
//!   chest slot shows it at the next inventory resync.
//! - Creative players do not wear it.

use bevy_ecs::prelude::*;

use crate::entity::components::{ArmourInventory, GameMode, PlayerState};
use crate::item::ItemStack;
use crate::server::game::durability::{WearKind, max_durability, wear};

const ELYTRA: &str = "minecraft:elytra";
/// Gliding ticks per point of wear.
pub const WEAR_INTERVAL: u32 = 20;

/// Ticks a player has been gliding.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct GlideTicks(pub u32);

/// Durability `stack` has left, if it is an elytra.
fn elytra_left(stack: &ItemStack) -> Option<i32> {
    if stack.item_id() != ELYTRA {
        return None;
    }
    let max = stack.item().map(max_durability)?;
    Some(max - crate::item::enchant::durability_damage(stack))
}

/// Whether `stack` lets its wearer glide.
pub fn can_glide_with(stack: &ItemStack) -> bool {
    elytra_left(stack).is_some_and(|left| left > 1)
}

/// Exclusive system, after movement: gliding players without a usable
/// elytra stop, and every 20th gliding tick wears the elytra or, with 1
/// durability left, ends the glide.
pub fn tick_gliding(world: &mut World, players: &mut QueryState<(Entity, &PlayerState)>) {
    let gliding: Vec<(Entity, bool)> = players
        .iter(world)
        .map(|(player, state)| (player, state.gliding))
        .collect();
    for (player, is_gliding) in gliding {
        if !is_gliding {
            world.entity_mut(player).remove::<GlideTicks>();
            continue;
        }
        let chest = world
            .get::<ArmourInventory>(player)
            .map(|armour| armour.chestplate().clone())
            .unwrap_or_else(ItemStack::empty);
        let Some(left) =
            elytra_left(&chest).filter(|left| *left > 1 || glide_ticks(world, player) > 0)
        else {
            stop(world, player);
            continue;
        };
        let ticks = glide_ticks(world, player) + 1;
        world.entity_mut(player).insert(GlideTicks(ticks));
        if !ticks.is_multiple_of(WEAR_INTERVAL) {
            continue;
        }
        if left <= 1 {
            stop(world, player);
            continue;
        }
        if world.get::<GameMode>(player) == Some(&GameMode::Creative) {
            continue;
        }
        let worn = wear(
            &chest,
            1,
            WearKind::Armour,
            &mut world
                .get_resource_or_init::<crate::entity::EntityRandom>()
                .0,
        );
        if let (Some(worn), Some(mut armour)) = (worn, world.get_mut::<ArmourInventory>(player)) {
            let _ = armour.set_chestplate(worn);
        }
    }
}

fn glide_ticks(world: &World, player: Entity) -> u32 {
    world.get::<GlideTicks>(player).map_or(0, |ticks| ticks.0)
}

fn stop(world: &mut World, player: Entity) {
    if let Some(mut state) = world.get_mut::<PlayerState>(player) {
        state.gliding = false;
    }
    world.entity_mut(player).remove::<GlideTicks>();
}

#[cfg(test)]
mod tests;
