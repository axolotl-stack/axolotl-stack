//! Shields: blocking, wear and axe disabling.
//!
//! A player raises a shield by sneaking with one in the selected hotbar
//! slot or the offhand (the hotbar first), unless shields are disabled or
//! another item is being used; the raised shield sets the `BLOCKING` flag
//! (flag 72, in the second flag word) the player and its viewers receive.
//! After [`RAISE_TICKS`] a raised shield blocks melee, projectile, thorns
//! and explosion hits whose source is in front of the player
//! (horizontally, within 90° of the view direction) or that have no source
//! position at all. A blocked hit deals nothing, plays `item.shield.block`
//! at the player and wears the shield by `1 + floor(damage)` (the offhand is
//! resent in window 119). A blocked melee hit from an attacker holding an
//! axe still wears the shield, then disables every shield of the player for
//! [`DISABLE_TICKS`] (`PlayerStartItemCooldown` `minecraft:shield`), plays
//! `break` and lowers the shield.
//!
//! Measured on BDS 1.26.51 (client-diff `itemuses-shield`): the block
//! sound, the wear (+4 for a 3-damage zombie hit, +5 for 4 damage, +14 for
//! the vindicator's 13 through the axe disable), sourceless hits being
//! blocked, the disable cooldown and `break` sound. Public references
//! (Minecraft Wiki, Shield): the raise delay, the front arc, no wear below
//! [`MIN_WEAR_DAMAGE`] and the blockable causes. Not implemented: the
//! one-tick `TRANSITION_BLOCKING` and `BLOCKED_USING_SHIELD` flag pulses
//! BDS sends when raising, lowering and blocking.

use bevy_ecs::prelude::*;
use glam::{DVec2, DVec3};
use unastar_data::actor_components::body::ActorDamageCause;

use crate::entity::components::{
    GameMode, HeldSlot, MainInventory, OffhandSlot, Player, PlayerState, Position, Rotation,
};
use crate::entity::mob::combat::Hit;
use crate::item::ItemStack;
use crate::server::game::durability::{WearKind, wear};

pub const SHIELD: &str = "minecraft:shield";
/// Ticks a raised shield takes to start blocking.
pub const RAISE_TICKS: u64 = 5;
/// Ticks an axe hit disables shields for, and their cooldown category.
pub const DISABLE_TICKS: u32 = 100;
pub const COOLDOWN: &str = "minecraft:shield";
/// Blocked damage below this does not wear the shield.
const MIN_WEAR_DAMAGE: f32 = 3.0;

/// A raised shield, since this server tick.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct ShieldRaised {
    pub since: u64,
}

/// The hand a shield is held in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hand {
    Main(usize),
    Offhand,
}

fn now(world: &World) -> u64 {
    world
        .get_resource::<crate::ecs::resources::TickCounter>()
        .map_or(0, |t| t.current)
}

fn is_shield(stack: &ItemStack) -> bool {
    !stack.is_empty() && stack.item_id() == SHIELD
}

/// The hand holding a shield: the selected hotbar slot first.
fn shield_hand(world: &World, player: Entity) -> Option<Hand> {
    let slot = world.get::<HeldSlot>(player).map(|s| usize::from(s.0));
    if let Some(slot) = slot
        && world
            .get::<MainInventory>(player)
            .and_then(|inv| inv.0.item(slot))
            .is_some_and(is_shield)
    {
        return Some(Hand::Main(slot));
    }
    world
        .get::<OffhandSlot>(player)
        .is_some_and(|offhand| is_shield(&offhand.0))
        .then_some(Hand::Offhand)
}

/// Whether `player` holds a shield up this tick.
fn raises(world: &World, player: Entity) -> bool {
    world.get::<PlayerState>(player).is_some_and(|s| s.sneaking)
        && super::can_use(world, player)
        && world.get::<super::ItemUsing>(player).is_none()
        && !super::cooldown::cooling(world, player, COOLDOWN)
        && shield_hand(world, player).is_some()
}

/// System (exclusive): raise and lower shields, telling the player and its
/// viewers through the flag word.
pub fn tick_shields(
    world: &mut World,
    players: &mut QueryState<(Entity, Option<&ShieldRaised>), With<Player>>,
    mut changed: Local<Vec<(Entity, bool)>>,
) {
    changed.extend(players.iter(world).filter_map(|(player, raised)| {
        let wanted = raises(world, player);
        (wanted != raised.is_some()).then_some((player, wanted))
    }));
    let now = now(world);
    for (player, raise) in changed.drain(..) {
        if raise {
            world.entity_mut(player).insert(ShieldRaised { since: now });
        } else {
            world.entity_mut(player).remove::<ShieldRaised>();
        }
        super::broadcast_all_flags(world, player);
    }
}

/// Where a hit comes from: its knockback source, else its attacker.
fn source_of(world: &World, hit: &Hit) -> Option<DVec3> {
    hit.knockback_from.or_else(|| {
        hit.attacker
            .and_then(|attacker| world.get::<Position>(attacker).map(|p| p.0))
    })
}

/// Whether `from` is in front of `player` (horizontally).
fn in_front(world: &World, player: Entity, from: DVec3) -> bool {
    let (Some(at), Some(rotation)) = (
        world.get::<Position>(player).map(|p| p.0),
        world.get::<Rotation>(player),
    ) else {
        return false;
    };
    let yaw = f64::from(rotation.yaw).to_radians();
    let facing = DVec2::new(-yaw.sin(), yaw.cos());
    facing.dot(DVec2::new(from.x - at.x, from.z - at.z)) > 0.0
}

fn blockable(cause: ActorDamageCause) -> bool {
    matches!(
        cause,
        ActorDamageCause::EntityAttack
            | ActorDamageCause::Projectile
            | ActorDamageCause::Thorns
            | ActorDamageCause::BlockExplosion
            | ActorDamageCause::EntityExplosion
            | ActorDamageCause::Fireworks
    )
}

/// The hurt hook: whether `target`'s raised shield blocks `hit`. A blocked
/// hit wears the shield, plays the block sound and, from an axe, disables
/// the target's shields.
pub fn block(world: &mut World, target: Entity, hit: &Hit) -> bool {
    let Some(raised) = world.get::<ShieldRaised>(target).copied() else {
        return false;
    };
    if now(world) < raised.since + RAISE_TICKS || !blockable(hit.cause) {
        return false;
    }
    // A hit without a source position is blocked (measured).
    let from = source_of(world, hit);
    let Some(hand) = shield_hand(world, target) else {
        return false;
    };
    if from.is_some_and(|from| !in_front(world, target, from)) {
        return false;
    }
    if let Some(at) = world.get::<Position>(target).map(|p| p.0) {
        crate::entity::mob::net::sound::play_level_sound(
            world,
            Some(target),
            at,
            "item.shield.block",
            -1,
        );
    }
    if hit.damage >= MIN_WEAR_DAMAGE {
        wear_shield(world, target, hand, 1 + hit.damage.floor() as i32);
    }
    let axe = hit.cause == ActorDamageCause::EntityAttack
        && hit
            .attacker
            .and_then(|attacker| crate::entity::mob::events::held_item(world, attacker))
            .and_then(ItemStack::item)
            .is_some_and(|entry| entry.has_tag("minecraft:is_axe"));
    if axe {
        disable(world, target);
    }
    true
}

/// Disable every shield of `player` and lower the raised one.
pub fn disable(world: &mut World, player: Entity) {
    super::cooldown::start_cooldown(world, player, COOLDOWN, DISABLE_TICKS);
    if let Some(at) = world.get::<Position>(player).map(|p| p.0) {
        crate::entity::mob::net::sound::play_level_sound(world, Some(player), at, "break", -1);
    }
    if world.entity_mut(player).take::<ShieldRaised>().is_some() {
        super::broadcast_all_flags(world, player);
    }
}

/// Wear the shield in `hand` by `points` (creative shields never wear).
fn wear_shield(world: &mut World, player: Entity, hand: Hand, points: i32) {
    if world.get::<GameMode>(player) == Some(&GameMode::Creative) {
        return;
    }
    let stack = match hand {
        Hand::Main(slot) => world
            .get::<MainInventory>(player)
            .and_then(|inv| inv.0.item(slot).cloned()),
        Hand::Offhand => world.get::<OffhandSlot>(player).map(|o| o.0.clone()),
    };
    let Some(stack) = stack else {
        return;
    };
    let Some(worn) = wear(
        &stack,
        points,
        WearKind::Tool,
        &mut world
            .get_resource_or_init::<crate::entity::EntityRandom>()
            .0,
    ) else {
        return;
    };
    let broke = worn.is_empty();
    match hand {
        Hand::Main(slot) => {
            if let Some(mut inv) = world.get_mut::<MainInventory>(player) {
                let _ = inv.0.set_item(slot, worn.clone());
            }
            crate::server::game::send_slot(world, player, slot, &worn);
        }
        Hand::Offhand => {
            if let Some(mut offhand) = world.get_mut::<OffhandSlot>(player) {
                offhand.0 = worn;
            }
            crate::server::game::inventory::send_offhand(world, player);
        }
    }
    if broke && let Some(at) = world.get::<Position>(player).map(|p| p.0) {
        crate::entity::mob::net::sound::play_level_sound(world, Some(player), at, "break", -1);
    }
}
