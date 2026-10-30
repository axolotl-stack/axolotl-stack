//! Gliding with an elytra.
//!
//! The client starts and stops gliding with the `StartGliding` /
//! `StopGliding` input flags of `PlayerAuthInput`. The server counts a
//! player as gliding only while its chest slot holds an elytra with more
//! than one durability point left; otherwise the glide is dropped. When
//! gliding starts or stops, the player and its viewers get the flag word
//! (`GLIDING`) and the box height (0.6 gliding; measured on BDS 1.26.51,
//! client-diff `itemuses-elytra-2`). While gliding outside creative the
//! elytra wears by one point every [`WEAR_INTERVAL`] ticks (Unbreaking
//! spares points like a tool's) and stops at one point left instead of
//! breaking. The wear rate is a public reference (Minecraft Wiki, Elytra)
//! until measured.

use bevy_ecs::prelude::*;

use crate::entity::components::{ArmourInventory, GameMode, Player, PlayerState, armor_slots};
use crate::item::ItemStack;
use crate::item::enchant::durability_damage;
use crate::server::game::durability::{WearKind, max_durability, wear};

pub const ELYTRA: &str = "minecraft:elytra";
/// Ticks of gliding per point of elytra wear.
pub const WEAR_INTERVAL: u64 = 20;

/// A gliding player, since this server tick.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct Gliding {
    pub since: u64,
}

/// Whether `stack` is an elytra that can still glide.
pub fn usable(stack: &ItemStack) -> bool {
    !stack.is_empty()
        && stack.item_id() == ELYTRA
        && stack
            .item()
            .is_some_and(|entry| durability_damage(stack) < max_durability(entry) - 1)
}

fn chest(world: &World, player: Entity) -> Option<ItemStack> {
    world
        .get::<ArmourInventory>(player)
        .map(|armour| armour.chestplate().clone())
}

/// System (exclusive): track gliding players, drop glides without a usable
/// elytra and wear the elytra.
pub fn tick_gliding(
    world: &mut World,
    players: &mut QueryState<(Entity, &PlayerState, Option<&Gliding>), With<Player>>,
    mut due: Local<Vec<(Entity, bool, Option<Gliding>)>>,
) {
    due.extend(
        players
            .iter(world)
            .map(|(player, state, gliding)| (player, state.gliding, gliding.copied())),
    );
    let now = world
        .get_resource::<crate::ecs::resources::TickCounter>()
        .map_or(0, |t| t.current);
    for (player, mut wants, gliding) in due.drain(..) {
        if wants && !chest(world, player).is_some_and(|stack| usable(&stack)) {
            if let Some(mut state) = world.get_mut::<PlayerState>(player) {
                state.gliding = false;
            }
            wants = false;
        }
        match (wants, gliding) {
            (true, None) => {
                world.entity_mut(player).insert(Gliding { since: now });
                super::broadcast_glide_flags(world, player);
            }
            (false, Some(_)) => {
                world.entity_mut(player).remove::<Gliding>();
                super::broadcast_glide_flags(world, player);
            }
            (true, Some(glide)) => {
                let ticks = now.saturating_sub(glide.since);
                if ticks > 0 && ticks.is_multiple_of(WEAR_INTERVAL) {
                    wear_elytra(world, player);
                }
            }
            (false, None) => {}
        }
    }
}

/// One point of glide wear on the worn elytra (none in creative).
fn wear_elytra(world: &mut World, player: Entity) {
    if world.get::<GameMode>(player) == Some(&GameMode::Creative) {
        return;
    }
    let Some(stack) = chest(world, player).filter(usable) else {
        return;
    };
    let Some(worn) = wear(
        &stack,
        1,
        WearKind::Tool,
        &mut world
            .get_resource_or_init::<crate::entity::EntityRandom>()
            .0,
    ) else {
        return;
    };
    if let Some(mut armour) = world.get_mut::<ArmourInventory>(player) {
        let _ = armour.set_slot(armor_slots::CHESTPLATE, worn);
    }
    crate::server::game::inventory::send_armour_slot(world, player, armor_slots::CHESTPLATE);
}
