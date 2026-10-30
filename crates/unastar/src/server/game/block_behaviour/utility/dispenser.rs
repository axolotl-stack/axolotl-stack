//! Dispensers and droppers.
//!
//! As BDS 1.26.51 does:
//! - **Placement.** `facing_direction` points at the placing player: up
//!   when looking down more than 45°, down when looking up more than 45°,
//!   else back along the horizontal look; `triggered_bit` starts clear.
//! - **Power.** When power appears and `triggered_bit` is clear, a tick is
//!   scheduled [`DISPENSE_DELAY`] ticks later and `triggered_bit` is set;
//!   when power is gone `triggered_bit` clears. The tick fires without
//!   checking power again, so a pulse shorter than the delay still fires
//!   once, and staying powered fires only once.
//! - **Firing.** One non-empty slot is chosen by reservoir sampling (the
//!   n-th non-empty slot replaces the choice with chance 1/n). With none,
//!   `LevelSoundEvent` `block.click.fail` plays at the block's corner with
//!   the block's wire id. Items leave from the dispense position
//!   `(x + 0.5 + 0.7·sx, y + 0.3 + 0.7·sy, z + 0.5 + 0.7·sz)`, `s` being
//!   the facing step. Every dispense that used an item sends `LevelEvent`
//!   2000 (smoke) there with data `sx + 4 + 3·sz`.
//! - **Ejecting** (anything without its own behaviour, and droppers): one
//!   item spawns at the dispense position lowered by 0.125 (facing up or
//!   down) or 0.15625, moving `(sx·v, 0.2, sz·v)` with `v = 0.2 + U·0.1`,
//!   each axis plus a gaussian × 0.045, with `block.click` (data: the wire
//!   id of the block at the dispense position).
//! - **Droppers** first try the container in front: a slot is chosen again
//!   and one item moves in as a hopper would push it (into a furnace's
//!   input from above, its fuel from a side); a container in front takes
//!   the turn even when it has no room. A composter in front takes the
//!   turn as well and, below a downward-facing dropper, composts one item.
//!   Otherwise the item is ejected, with a second `block.click` at the
//!   block's corner.
//! - **Dispensers** run the item's behaviour; a behaviour that cannot act
//!   falls back to ejecting:
//!   - arrows, snowballs, eggs, splash and lingering potions, bottles of
//!     enchanting and fire charges launch their projectile from the
//!     dispense position along the facing raised by 0.1, with
//!     `block.click` (data −1) and `LevelEvent` 1002 (data 78642);
//!   - bone meal grows the block in front (`block.click`, data −1) or
//!     fails with `block.click.fail` (data 1000) and dispenses nothing;
//!   - TNT spawns a primed TNT ([`super::tnt::dispense`]);
//!   - a water bucket empties a source into the cell in front (air or
//!     liquid there) with `block.click` data 1000 and becomes a bucket; an
//!     empty bucket takes a source in front back with `block.click`
//!     data −1.
//!
//! Provisional (Minecraft Wiki): the projectiles' speed is their
//! definition's `power` and each axis of their direction gets a gaussian
//! × 0.045; lava and powder snow buckets behave like water buckets; flint
//! and steel lights TNT in front or sets fire to air there (1 durability),
//! else fails like bone meal; armour goes onto a player in front with that
//! slot empty; spawn eggs spawn their mob standing in the cell in front;
//! minecart items place their minecart on a rail in front; glowstone
//! charges a respawn anchor in front. The other special behaviours that
//! acted play `block.click` (data −1) too.
//!
//! The contents live in the block's container actor
//! (`server::game::containers`): a player's use opens its window, breaking
//! it drops them, and every dispense runs the container's neighbour
//! updates so comparators read it again.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;

use super::super::{Place, bit, int, random, schedule_tick, with};
use crate::entity::components::{ArmourInventory, PlayerSession, Position, Rotation, Velocity};
use crate::entity::item_drop::PLAYER_EYE_HEIGHT;
use crate::entity::mob::constants::{
    CLIMATE_VARIANT_PROPERTY, PLAYER_BOX_HEIGHT, PLAYER_BOX_WIDTH,
};
use crate::entity::mob::net::sound::{level_event, play_level_sound};
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::entity::systems::BlockSource;
use crate::item::ItemStack;
use crate::registry::block::{BlockEntry, LiquidKind, StateValue};
use crate::server::game::containers;

/// Ticks between power appearing and the dispense.
pub const DISPENSE_DELAY: u64 = 4;
const FACING: &str = "facing_direction";
const TRIGGERED: &str = "triggered_bit";
/// `LevelEvent` of the smoke a dispense leaves.
pub const LEVEL_EVENT_DISPENSE_SMOKE: i32 = 2000;
/// `LevelEvent` of a launched projectile, and its data.
pub const LEVEL_EVENT_LAUNCH: i32 = 1002;
const LAUNCH_DATA: i32 = 78642;
/// Data of the failure click a bone meal that grew nothing plays.
const FERTILIZE_FAIL_DATA: i32 = 1000;
/// Distance of the dispense position from the block centre along the
/// facing, and its height above the block's bottom before that.
const DISPENSE_REACH: f64 = 0.7;
const DISPENSE_BASE_Y: f64 = 0.3;
/// How far an ejected item starts below the dispense position.
const EJECT_DROP_VERTICAL: f64 = 0.125;
const EJECT_DROP_SIDEWAYS: f64 = 0.15625;
/// An ejected item's speed along the facing is `BASE + U·RANGE`; it always
/// rises at `UP`; each axis gets a gaussian × `SPREAD`.
const EJECT_SPEED_BASE: f64 = 0.2;
const EJECT_SPEED_RANGE: f64 = 0.1;
const EJECT_UP: f64 = 0.2;
const EJECT_SPREAD: f64 = 0.0075 * 6.0;
/// A projectile's direction is the facing raised by this much.
const SHOT_LIFT: f64 = 0.1;
/// Gaussian spread per axis of a dispensed projectile's direction
/// (provisional: Minecraft Wiki).
const SHOT_SPREAD: f64 = 0.0075 * 6.0;
/// Data of the click a bucket emptied by a dispenser plays.
const BUCKET_EMPTIED_CLICK_DATA: i32 = 1000;

/// The unit step of `facing_direction` (0 down, 1 up, 2 north, 3 south,
/// 4 west, 5 east).
fn step(facing: i32) -> IVec3 {
    match facing.rem_euclid(6) {
        0 => IVec3::NEG_Y,
        1 => IVec3::Y,
        2 => IVec3::NEG_Z,
        3 => IVec3::Z,
        4 => IVec3::NEG_X,
        _ => IVec3::X,
    }
}

/// Where items leave a dispenser at `pos` facing along `step`.
fn dispense_position(pos: IVec3, step: IVec3) -> DVec3 {
    pos.as_dvec3() + DVec3::new(0.5, DISPENSE_BASE_Y, 0.5) + step.as_dvec3() * DISPENSE_REACH
}

/// A standard normal sample (Box–Muller) from the block random.
fn gaussian(world: &mut World) -> f64 {
    let mut rng = random(world);
    let u: f64 = rng.r#gen::<f64>().max(f64::MIN_POSITIVE);
    let v: f64 = rng.r#gen();
    (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
}

/// A random non-empty slot: each non-empty slot replaces the choice with
/// chance 1/n, n counting the non-empty slots so far.
fn pick_slot(world: &mut World, slots: &crate::entity::components::Inventory) -> Option<usize> {
    let mut chosen = None;
    let mut seen = 0;
    for (index, stack) in slots.slots().iter().enumerate() {
        if stack.is_empty() {
            continue;
        }
        seen += 1;
        if random(world).gen_range(0..seen) == 0 {
            chosen = Some(index);
        }
    }
    chosen
}

/// The wire id of the block at `pos` (0 when unloaded).
fn wire_at(world: &World, pos: IVec3) -> i32 {
    world.block_at(pos).map_or(0, |state| {
        crate::server::game::types::block_network_ids(world).to_wire(state) as i32
    })
}

/// Eject one item `stack` from the dispense position `at` of a block
/// facing `facing`.
fn eject(world: &mut World, at: DVec3, facing: i32, stack: ItemStack) {
    let step = step(facing);
    let drop = if facing.rem_euclid(6) < 2 {
        EJECT_DROP_VERTICAL
    } else {
        EJECT_DROP_SIDEWAYS
    };
    let one = stack.with_count(1).with_net_id(0).into_slot();
    let item = crate::entity::item_drop::spawn_item_drop(world, at - DVec3::Y * drop, one, None);
    let speed = EJECT_SPEED_BASE + random(world).r#gen::<f64>() * EJECT_SPEED_RANGE;
    let mut velocity = DVec3::new(
        f64::from(step.x) * speed,
        EJECT_UP,
        f64::from(step.z) * speed,
    );
    velocity.x += gaussian(world) * EJECT_SPREAD;
    velocity.y += gaussian(world) * EJECT_SPREAD;
    velocity.z += gaussian(world) * EJECT_SPREAD;
    if let Some(item) = item {
        world.entity_mut(item).insert(Velocity(velocity));
    }
    let data = wire_at(world, at.floor().as_ivec3());
    play_level_sound(world, None, at, "block.click", data);
}

/// The smoke of a dispense at `at` facing along `step`.
fn smoke(world: &World, at: DVec3, step: IVec3) {
    level_event(
        world,
        at,
        LEVEL_EVENT_DISPENSE_SMOKE,
        step.x + 4 + step.z * 3,
    );
}

/// What a dispenser's item behaviour did.
enum Dispensed {
    /// It acted: the slot now holds `rest`, and `extra` (a filled bucket
    /// from a stack of empty ones) goes back into the dispenser.
    Used {
        rest: ItemStack,
        extra: Option<ItemStack>,
    },
    /// It could not act and nothing is dispensed.
    Failed,
    /// No behaviour of its own: the item is ejected.
    Eject,
}

impl Dispensed {
    fn one_used(stack: &ItemStack) -> Self {
        Self::Used {
            rest: stack.grow(-1),
            extra: None,
        }
    }
}

/// Launch the projectile actor `identifier` from `at` along `direction`;
/// `None` when it cannot spawn or is no projectile.
fn shoot(
    world: &mut World,
    identifier: &str,
    climate: Option<&str>,
    at: DVec3,
    direction: DVec3,
) -> Option<Entity> {
    let projectile = match spawn_mob(world, identifier, at, 0.0, SpawnCause::Spawned) {
        Ok(projectile) => projectile,
        Err(error) => {
            tracing::warn!(%identifier, %error, "dispensed projectile could not spawn");
            return None;
        }
    };
    let Some(power) = world
        .get::<crate::entity::projectile::ProjectileConfig>(projectile)
        .map(|config| config.0.power)
    else {
        world.despawn(projectile);
        return None;
    };
    if let Some(climate) = climate {
        use crate::entity::definition::properties::{ActorProperties, NamedPropertyValue};
        if let Some(mut properties) = world.get_mut::<ActorProperties>(projectile) {
            let _ =
                properties.set_named(CLIMATE_VARIANT_PROPERTY, NamedPropertyValue::Enum(climate));
        }
    }
    let mut direction = direction.normalize_or_zero();
    direction.x += gaussian(world) * SHOT_SPREAD;
    direction.y += gaussian(world) * SHOT_SPREAD;
    direction.z += gaussian(world) * SHOT_SPREAD;
    let velocity = direction * power;
    let horizontal = velocity.x.hypot(velocity.z);
    let yaw = velocity.x.atan2(velocity.z).to_degrees() as f32;
    let pitch = velocity.y.atan2(horizontal).to_degrees() as f32;
    world.entity_mut(projectile).insert((
        Position(at),
        Velocity(velocity),
        Rotation::new(yaw, pitch),
    ));
    Some(projectile)
}

/// The projectile actor, climate variant and carried potion (and whether
/// it is a tipped arrow's) a dispensed `stack` launches.
fn projectile_of(
    stack: &ItemStack,
) -> Option<(&'static str, Option<&'static str>, Option<(i16, bool)>)> {
    use unastar_data::actor_identifiers::ActorType;
    match stack.item_id() {
        "minecraft:arrow" => {
            let potion = (stack.damage > 0).then_some((stack.damage - 1, true));
            return Some(("minecraft:arrow", None, potion));
        }
        "minecraft:experience_bottle" => return Some(("minecraft:xp_bottle", None, None)),
        "minecraft:fire_charge" => return Some(("minecraft:small_fireball", None, None)),
        _ => {}
    }
    let (actor, climate) = crate::entity::projectile::thrown_by(stack.item_id())?;
    let potion = matches!(actor, ActorType::SplashPotion | ActorType::LingeringPotion)
        .then_some((stack.damage, false));
    Some((actor.name(), climate, potion))
}

/// Equip `stack` (one armour piece) onto a player whose box meets the cell
/// `front` and whose matching slot is empty.
fn equip_player(world: &mut World, front: IVec3, stack: &ItemStack) -> bool {
    let min = front.as_dvec3();
    let max = min + DVec3::ONE;
    let half = PLAYER_BOX_WIDTH / 2.0;
    let mut players =
        world.query_filtered::<(Entity, &Position, &ArmourInventory), With<PlayerSession>>();
    let target = players.iter(world).find_map(|(player, position, armour)| {
        let feet = position.0 - DVec3::Y * PLAYER_EYE_HEIGHT;
        let meets = feet.x - half < max.x
            && feet.x + half > min.x
            && feet.y < max.y
            && feet.y + PLAYER_BOX_HEIGHT > min.y
            && feet.z - half < max.z
            && feet.z + half > min.z;
        if !meets {
            return None;
        }
        (0..4)
            .find(|slot| {
                armour.slot(*slot).is_some_and(ItemStack::is_empty)
                    && crate::item::equipment::accepts_armour(stack, *slot)
            })
            .map(|slot| (player, slot))
    });
    let Some((player, slot)) = target else {
        return false;
    };
    let one = stack.clone().with_count(1).with_net_id(0).into_slot();
    if let Some(mut armour) = world.get_mut::<ArmourInventory>(player)
        && armour.set_slot(slot, one).is_ok()
    {
        crate::server::game::inventory::send_armour_slot(world, player, slot);
        return true;
    }
    false
}

/// Whether the block at `pos` can take a liquid from a bucket: air or a
/// liquid.
fn takes_liquid(world: &World, pos: IVec3) -> bool {
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    let registry = world.registry();
    registry.liquid_state(state).is_some()
        || registry
            .get_by_runtime_id(state)
            .is_some_and(|entry| entry.string_id == "minecraft:air")
}

/// A filled bucket empties into `front`.
fn empty_bucket(world: &mut World, front: IVec3, stack: &ItemStack) -> Dispensed {
    let block = match stack.item_id() {
        "minecraft:water_bucket" => "minecraft:water",
        "minecraft:lava_bucket" => "minecraft:lava",
        _ => "minecraft:powder_snow",
    };
    let Some(state) = world.registry().default_state_id_by_name(block) else {
        return Dispensed::Eject;
    };
    if !takes_liquid(world, front) {
        return Dispensed::Eject;
    }
    let Some(bucket) = crate::server::game::world_stack(world, "minecraft:bucket", 1) else {
        return Dispensed::Eject;
    };
    crate::server::game::blocks::set_block(world, front, state, false);
    Dispensed::Used {
        rest: bucket.into_slot(),
        extra: None,
    }
}

/// An empty bucket fills from a source (or powder snow) in `front`.
fn fill_bucket(world: &mut World, front: IVec3, stack: &ItemStack) -> Dispensed {
    let Some(state) = world.block_at(front) else {
        return Dispensed::Eject;
    };
    let registry = world.registry();
    let filled = match registry.liquid_state(state) {
        Some(liquid) if liquid.depth == 0 => match liquid.kind {
            LiquidKind::Water => "minecraft:water_bucket",
            LiquidKind::Lava => "minecraft:lava_bucket",
        },
        Some(_) => return Dispensed::Eject,
        None if registry
            .get_by_runtime_id(state)
            .is_some_and(|entry| entry.string_id == "minecraft:powder_snow") =>
        {
            "minecraft:powder_snow_bucket"
        }
        None => return Dispensed::Eject,
    };
    let air = super::super::air(registry);
    let Some(full) = crate::server::game::world_stack(world, filled, 1) else {
        return Dispensed::Eject;
    };
    crate::server::game::blocks::set_block(world, front, air, false);
    if stack.count == 1 {
        Dispensed::Used {
            rest: full.into_slot(),
            extra: None,
        }
    } else {
        Dispensed::Used {
            rest: stack.grow(-1),
            extra: Some(full),
        }
    }
}

/// Flint and steel lights TNT in `front` or sets fire to air there.
fn light(world: &mut World, front: IVec3, stack: &ItemStack) -> Dispensed {
    let lit = if world
        .block_at(front)
        .and_then(|state| world.registry().get_by_runtime_id(state))
        .is_some_and(super::tnt::is_tnt)
    {
        super::tnt::ignite(world, front)
    } else {
        let below = super::super::name_at(world, front - IVec3::Y);
        let fire = if matches!(below, Some("minecraft:soul_sand" | "minecraft:soul_soil")) {
            "minecraft:soul_fire"
        } else {
            "minecraft:fire"
        };
        let is_air = super::super::name_at(world, front) == Some("minecraft:air");
        match world.registry().default_state_id_by_name(fire) {
            Some(fire) if is_air && super::super::can_survive(world, front, fire) => {
                crate::server::game::blocks::set_block(world, front, fire, false);
                true
            }
            _ => false,
        }
    };
    if !lit {
        return Dispensed::Failed;
    }
    let worn = {
        let mut rng = random(world);
        crate::server::game::durability::wear(
            stack,
            1,
            crate::server::game::durability::WearKind::Tool,
            &mut *rng,
        )
    };
    Dispensed::Used {
        rest: worn.unwrap_or_else(|| stack.clone()),
        extra: None,
    }
}

/// The dispenser behaviour of `stack` at the dispense position `at`, the
/// cell `front` in front, facing along `step`.
fn behaviour(
    world: &mut World,
    front: IVec3,
    step: IVec3,
    at: DVec3,
    stack: &ItemStack,
) -> Dispensed {
    let id = stack.item_id();
    if let Some((actor, climate, potion)) = projectile_of(stack) {
        let direction = step.as_dvec3() + DVec3::Y * SHOT_LIFT;
        let Some(projectile) = shoot(world, actor, climate, at, direction) else {
            return Dispensed::Eject;
        };
        if let Some((potion, arrow)) = potion {
            crate::entity::projectile::carry_potion(world, projectile, potion, arrow);
        }
        if actor == "minecraft:arrow" {
            let one = stack.clone().with_count(1).with_net_id(0);
            world
                .entity_mut(projectile)
                .insert(crate::entity::projectile::ProjectileItem(one));
        }
        play_level_sound(world, None, at, "block.click", -1);
        level_event(world, at, LEVEL_EVENT_LAUNCH, LAUNCH_DATA);
        return Dispensed::one_used(stack);
    }
    let used = match id {
        "minecraft:tnt" | "minecraft:underwater_tnt" => {
            // The TNT path plays its own click and fuse sound.
            return if super::tnt::dispense(world, at) {
                Dispensed::one_used(stack)
            } else {
                Dispensed::Eject
            };
        }
        "minecraft:bone_meal" => {
            if !super::super::fertilize(world, front) {
                play_level_sound(world, None, at, "block.click.fail", FERTILIZE_FAIL_DATA);
                return Dispensed::Failed;
            }
            Dispensed::one_used(stack)
        }
        "minecraft:water_bucket" | "minecraft:lava_bucket" | "minecraft:powder_snow_bucket" => {
            empty_bucket(world, front, stack)
        }
        "minecraft:bucket" => fill_bucket(world, front, stack),
        "minecraft:flint_and_steel" => {
            let lit = light(world, front, stack);
            if matches!(lit, Dispensed::Failed) {
                play_level_sound(world, None, at, "block.click.fail", -1);
                return lit;
            }
            lit
        }
        "minecraft:glowstone"
            if super::super::name_at(world, front) == Some("minecraft:respawn_anchor") =>
        {
            if super::respawn_anchor::charge(world, front) {
                Dispensed::one_used(stack)
            } else {
                Dispensed::Eject
            }
        }
        _ if super::rails::minecart_actor(id).is_some() => {
            let actor = super::rails::minecart_actor(id).expect("checked");
            if super::rails::place_minecart(world, front, actor) {
                Dispensed::one_used(stack)
            } else {
                Dispensed::Eject
            }
        }
        _ if crate::entity::mob::interaction::interact::spawn_egg_actor(id).is_some() => {
            let actor = crate::entity::mob::interaction::interact::spawn_egg_actor(id)
                .expect("checked")
                .to_owned();
            let feet = front.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
            let yaw = random(world).gen_range(0.0f32..360.0);
            match spawn_mob(world, &actor, feet, yaw, SpawnCause::Spawned) {
                Ok(_) => Dispensed::one_used(stack),
                Err(error) => {
                    tracing::warn!(%actor, %error, "dispensed spawn egg could not spawn");
                    Dispensed::Eject
                }
            }
        }
        _ if stack
            .item()
            .is_some_and(|item| item.equipment.armour_slot.is_some()) =>
        {
            if equip_player(world, front, stack) {
                Dispensed::one_used(stack)
            } else {
                Dispensed::Eject
            }
        }
        _ => Dispensed::Eject,
    };
    if matches!(used, Dispensed::Used { .. }) {
        let emptied = matches!(
            id,
            "minecraft:water_bucket" | "minecraft:lava_bucket" | "minecraft:powder_snow_bucket"
        );
        let data = if emptied {
            BUCKET_EMPTIED_CLICK_DATA
        } else {
            -1
        };
        play_level_sound(world, None, at, "block.click", data);
    }
    used
}

/// A dropper's turn with the item in `slot`: into the container or
/// composter in front, else ejected.
fn drop_from(
    world: &mut World,
    pos: IVec3,
    state: u32,
    facing: i32,
    mut slots: crate::entity::components::Inventory,
    slot: usize,
) {
    let step = step(facing);
    let front = pos + step;
    let at = dispense_position(pos, step);
    if containers::contents(world, front).is_some() {
        let Some(chosen) = pick_slot(world, &slots) else {
            return;
        };
        let stack = slots.item(chosen).cloned().unwrap_or_else(ItemStack::empty);
        let from_above = facing.rem_euclid(6) == 0;
        if containers::insert_one_item(world, front, &stack, from_above) == Some(true) {
            let _ = slots.set_item(chosen, stack.grow(-1));
            containers::set_contents(world, pos, &slots);
        }
        return;
    }
    if super::super::name_at(world, front) == Some("minecraft:composter") {
        if facing.rem_euclid(6) == 0
            && let Some(chosen) = pick_slot(world, &slots)
        {
            let stack = slots.item(chosen).cloned().unwrap_or_else(ItemStack::empty);
            let added = super::composter::add_items(world, front, &stack, 1);
            if added > 0 {
                let _ = slots.set_item(chosen, stack.grow(-(added as i16)));
                containers::set_contents(world, pos, &slots);
            }
        }
        return;
    }
    let stack = slots.item(slot).cloned().unwrap_or_else(ItemStack::empty);
    eject(world, at, facing, stack.clone());
    smoke(world, at, step);
    let corner = crate::server::game::types::block_network_ids(world).to_wire(state) as i32;
    play_level_sound(world, None, pos.as_dvec3(), "block.click", corner);
    let _ = slots.set_item(slot, stack.grow(-1));
    containers::set_contents(world, pos, &slots);
}

/// Fire the dispenser or dropper `state` at `pos` once.
fn fire(world: &mut World, pos: IVec3, state: u32) {
    let Some(entry) = world.registry().get_by_runtime_id(state) else {
        return;
    };
    let dropper = entry.string_id == "minecraft:dropper";
    let facing = int(entry, state, FACING).unwrap_or(0);
    let Some(mut slots) = containers::contents(world, pos) else {
        return;
    };
    let Some(slot) = pick_slot(world, &slots) else {
        let wire = crate::server::game::types::block_network_ids(world).to_wire(state) as i32;
        play_level_sound(world, None, pos.as_dvec3(), "block.click.fail", wire);
        return;
    };
    if dropper {
        drop_from(world, pos, state, facing, slots, slot);
        return;
    }
    let step = step(facing);
    let front = pos + step;
    let at = dispense_position(pos, step);
    let stack = slots.item(slot).cloned().unwrap_or_else(ItemStack::empty);
    let (rest, extra) = match behaviour(world, front, step, at, &stack) {
        Dispensed::Failed => return,
        Dispensed::Used { rest, extra } => (rest, extra),
        Dispensed::Eject => {
            eject(world, at, facing, stack.clone());
            (stack.grow(-1), None)
        }
    };
    smoke(world, at, step);
    let _ = slots.set_item(slot, rest);
    if let Some(extra) = extra {
        let (_, leftover) = slots.add_item(extra.into_slot());
        if !leftover.is_empty() {
            eject(world, at, facing, leftover);
        }
    }
    containers::set_contents(world, pos, &slots);
}

/// The window opens through the container path, not as a block use.
pub fn is_usable(_entry: &BlockEntry, _state: u32) -> bool {
    false
}

/// Never reached: a use opens the container window instead.
pub fn use_block(_world: &mut World, _player: Entity, _pos: IVec3, _state: u32) -> bool {
    false
}

/// Facing the placing player, untriggered.
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let pitch = place.input.pitch;
    let facing = if pitch > 45.0 {
        1
    } else if pitch < -45.0 {
        0
    } else {
        crate::server::game::placement::Horizontal::from_yaw(place.input.yaw)
            .opposite()
            .facing()
    };
    let state = with(place.block, place.state, FACING, StateValue::Int(facing));
    Some(with(place.block, state, TRIGGERED, StateValue::Byte(0)))
}

/// Dispensers stand anywhere.
pub fn can_survive(
    _blocks: &impl BlockSource,
    _pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    true
}

/// The container actor comes with the placement; power arrives through
/// [`power_changed`].
pub fn placed(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Neighbour changes reach dispensers only as power ([`power_changed`]).
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// The delayed dispense.
pub fn scheduled_tick(world: &mut World, pos: IVec3, state: u32) {
    fire(world, pos, state);
}

/// Dispensers have no random ticks.
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Rising power schedules a dispense and sets `triggered_bit`; lost power
/// clears it.
pub fn power_changed(world: &mut World, pos: IVec3, state: u32, powered: bool) {
    let Some(entry) = world.registry().get_by_runtime_id(state) else {
        return;
    };
    let triggered = bit(entry, state, TRIGGERED);
    if powered == triggered {
        return;
    }
    let new = with(entry, state, TRIGGERED, StateValue::Byte(u8::from(powered)));
    if powered {
        schedule_tick(world, pos, DISPENSE_DELAY);
    }
    crate::server::game::blocks::set_block(world, pos, new, false);
}

/// The contents drop through the container path when the block breaks.
pub fn breaking(_world: &mut World, _pos: IVec3, _state: u32) {}

#[cfg(test)]
mod tests {
    use bevy_ecs::prelude::*;
    use glam::IVec3;
    use jolyne::valentine::McpePacketData;

    use super::super::super::tests::{FLOOR, state};
    use super::super::tnt::tests::Server;
    use super::*;
    use crate::entity::components::{DroppedItem, ItemEntityStack};

    const AT: IVec3 = IVec3::new(2, FLOOR + 1, 2);
    const LEVER: IVec3 = IVec3::new(2, FLOOR + 1, 1);
    const FRONT: IVec3 = IVec3::new(3, FLOOR + 1, 2);

    /// A `block` facing east at [`AT`] holding `items`, with an off lever
    /// beside it.
    fn setup(block: &str, items: &[(&str, u8)]) -> Server {
        let mut s = Server::new();
        s.set(
            AT,
            state(block, &[("facing_direction", StateValue::Int(5))]),
        );
        let world = s.world();
        let mut slots = containers::contents(world, AT).expect("a container");
        assert_eq!(slots.size(), 9);
        for (slot, (item, count)) in items.iter().enumerate() {
            let stack = crate::server::game::world_stack(world, item, *count).unwrap();
            slots.set_item(slot, stack).unwrap();
        }
        containers::set_contents(world, AT, &slots);
        s.lever(LEVER);
        s.packets();
        s
    }

    /// `(item, count)` of every non-empty slot of the container at `pos`.
    fn contents(s: &mut Server, pos: IVec3) -> Vec<(String, u8)> {
        containers::contents(s.world(), pos)
            .unwrap()
            .slots()
            .iter()
            .filter(|stack| !stack.is_empty())
            .map(|stack| (stack.item_id().to_owned(), stack.count))
            .collect()
    }

    fn dropped(s: &mut Server) -> Vec<(String, u8)> {
        let world = s.world();
        world
            .query_filtered::<&ItemEntityStack, With<DroppedItem>>()
            .iter(world)
            .map(|stack| (stack.0.item_id().to_owned(), stack.0.count))
            .collect()
    }

    fn sounds(packets: &[McpePacketData]) -> Vec<(String, i32)> {
        packets
            .iter()
            .filter_map(|p| match p {
                McpePacketData::LevelSoundEventPacket(sound) => {
                    Some((sound.sound_event.to_string(), sound.data))
                }
                _ => None,
            })
            .collect()
    }

    fn events(packets: &[McpePacketData]) -> Vec<(i32, i32)> {
        packets
            .iter()
            .filter_map(|p| match p {
                McpePacketData::LevelEventPacket(event) => Some((event.event_id, event.data)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_rising_edge_fires_one_arrow_after_the_delay_and_not_again_while_powered() {
        let mut s = setup("minecraft:dispenser", &[("minecraft:arrow", 3)]);
        s.flip(LEVER);
        s.tick(2);
        assert_eq!(s.value(AT, TRIGGERED), Some(StateValue::Byte(1)));
        assert!(s.actors("minecraft:arrow").is_empty(), "the delay runs");
        s.tick(DISPENSE_DELAY + 1);
        assert_eq!(s.actors("minecraft:arrow").len(), 1);
        assert_eq!(contents(&mut s, AT), [("minecraft:arrow".to_owned(), 2)]);
        let packets = s.packets();
        assert!(events(&packets).contains(&(LEVEL_EVENT_LAUNCH, LAUNCH_DATA)));
        assert!(events(&packets).contains(&(LEVEL_EVENT_DISPENSE_SMOKE, 5)));
        assert!(sounds(&packets).contains(&("block.click".to_owned(), -1)));
        s.tick(20);
        assert_eq!(
            contents(&mut s, AT),
            [("minecraft:arrow".to_owned(), 2)],
            "staying powered fires nothing more"
        );
        s.flip(LEVER);
        s.tick(2);
        assert_eq!(s.value(AT, TRIGGERED), Some(StateValue::Byte(0)));
        s.flip(LEVER);
        s.tick(DISPENSE_DELAY + 3);
        assert_eq!(contents(&mut s, AT), [("minecraft:arrow".to_owned(), 1)]);
    }

    #[test]
    fn a_water_bucket_empties_into_the_cell_in_front() {
        let mut s = setup("minecraft:dispenser", &[("minecraft:water_bucket", 1)]);
        s.flip(LEVER);
        s.tick(DISPENSE_DELAY + 3);
        assert_eq!(s.name(FRONT), "minecraft:water");
        assert_eq!(contents(&mut s, AT), [("minecraft:bucket".to_owned(), 1)]);
        // The empty bucket takes the source back on the next pulse.
        s.flip(LEVER);
        s.tick(2);
        s.flip(LEVER);
        s.tick(DISPENSE_DELAY + 3);
        assert_eq!(s.name(FRONT), "minecraft:air");
        assert_eq!(
            contents(&mut s, AT),
            [("minecraft:water_bucket".to_owned(), 1)]
        );
    }

    #[test]
    fn an_item_without_a_behaviour_is_ejected() {
        let mut s = setup("minecraft:dispenser", &[("minecraft:dirt", 2)]);
        s.flip(LEVER);
        s.tick(DISPENSE_DELAY + 3);
        assert_eq!(dropped(&mut s), [("minecraft:dirt".to_owned(), 1)]);
        assert_eq!(contents(&mut s, AT), [("minecraft:dirt".to_owned(), 1)]);
        let packets = s.packets();
        assert!(events(&packets).contains(&(LEVEL_EVENT_DISPENSE_SMOKE, 5)));
        assert!(
            sounds(&packets)
                .iter()
                .any(|(name, _)| name == "block.click")
        );
    }

    #[test]
    fn an_empty_dispenser_clicks_and_fails() {
        let mut s = setup("minecraft:dispenser", &[]);
        s.flip(LEVER);
        s.tick(DISPENSE_DELAY + 3);
        let packets = s.packets();
        assert!(
            sounds(&packets)
                .iter()
                .any(|(name, _)| name == "block.click.fail")
        );
        assert!(
            !events(&packets)
                .iter()
                .any(|(id, _)| *id == LEVEL_EVENT_DISPENSE_SMOKE)
        );
        assert!(dropped(&mut s).is_empty());
    }

    #[test]
    fn a_dropper_moves_one_item_into_the_chest_in_front() {
        let mut s = setup("minecraft:dropper", &[("minecraft:dirt", 3)]);
        s.set(FRONT, state("minecraft:chest", &[]));
        s.flip(LEVER);
        s.tick(DISPENSE_DELAY + 3);
        assert_eq!(contents(&mut s, FRONT), [("minecraft:dirt".to_owned(), 1)]);
        assert_eq!(contents(&mut s, AT), [("minecraft:dirt".to_owned(), 2)]);
        assert!(dropped(&mut s).is_empty(), "nothing is dropped");
    }

    #[test]
    fn a_dropper_without_a_container_in_front_drops_the_item() {
        let mut s = setup("minecraft:dropper", &[("minecraft:arrow", 2)]);
        s.flip(LEVER);
        s.tick(DISPENSE_DELAY + 3);
        assert!(
            s.actors("minecraft:arrow").is_empty(),
            "droppers shoot nothing"
        );
        assert_eq!(dropped(&mut s), [("minecraft:arrow".to_owned(), 1)]);
    }

    #[test]
    fn dispensers_face_the_placing_player() {
        let entry = crate::registry::BlockRegistry::vanilla()
            .get_by_name("minecraft:dispenser")
            .unwrap();
        let facing = |yaw: f32, pitch: f32| {
            let input = crate::server::game::placement::PlacementInput {
                clicked: IVec3::ZERO,
                face: 1,
                click: glam::DVec3::ZERO,
                yaw,
                pitch,
            };
            let h = super::super::super::tests::Harness::new();
            let place = Place {
                blocks: &h.world,
                block: entry,
                state: entry.default_state_id,
                input: &input,
                position: IVec3::new(0, FLOOR + 1, 0),
            };
            let placed = placement(&place).unwrap();
            int(entry, placed, FACING).unwrap()
        };
        assert_eq!(facing(180.0, 0.0), 3, "looking north: faces south");
        assert_eq!(facing(0.0, 0.0), 2, "looking south: faces north");
        assert_eq!(facing(0.0, 60.0), 1, "looking down: faces up");
        assert_eq!(facing(0.0, -60.0), 0, "looking up: faces down");
    }
}

/// Replays the BDS 1.26.51 utility-block capture
/// (`testdata/bds_1_26_51_utility.json`) for dispensers, droppers, TNT,
/// note blocks and rails through a real server, comparing the watched
/// cells' states and their timing counted from each step's first change,
/// the actors that appeared, and the sounds and level events these blocks
/// send.
///
/// The capture's floor is at y = −61, the harness floor at [`FLOOR`], so
/// every position is shifted by the difference. A click lands immediately
/// here and one to two ticks after the order there, which is why timing
/// counts from the first change. Blocks the sweep built with untraced
/// commands (a watched cell that is air here but not there, the block a
/// lever is clicked onto) are put in place first; container contents are
/// what the steps imply (three arrows, a water bucket, two stone).
#[cfg(test)]
mod bds_parity {
    use std::collections::{BTreeMap, HashSet};

    use bevy_ecs::prelude::*;
    use glam::{DVec3, IVec3};
    use jolyne::valentine::McpePacketData;
    use serde_json::Value;

    use super::super::super::tests::FLOOR;
    use super::super::tnt::tests::Server;
    use crate::entity::components::{DroppedItem, MainInventory, Position, Rotation};
    use crate::entity::mob::MobKind;
    use crate::entity::systems::BlockSource;
    use crate::registry::block::StateValue;
    use crate::server::game::containers;

    const FIXTURE: &str = include_str!("../testdata/bds_1_26_51_utility.json");
    const BDS_FLOOR: i32 = -61;
    const DY: f64 = (FLOOR - BDS_FLOOR) as f64;

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture is JSON")
    }

    fn ivec(pos: &Value) -> IVec3 {
        let at = |i: usize| pos[i].as_f64().unwrap() as i32;
        IVec3::new(at(0), at(1) - BDS_FLOOR + FLOOR, at(2))
    }

    fn dvec(pos: &Value) -> DVec3 {
        let at = |i: usize| pos[i].as_f64().unwrap();
        DVec3::new(at(0), at(1) + DY, at(2))
    }

    /// A watched cell label: a named position, or `"x,y,z"`.
    fn label_pos(step: &Value, label: &str) -> IVec3 {
        if let Some(pos) = step["positions"].get(label) {
            return ivec(pos);
        }
        let parts: Vec<i64> = label.split(',').map(|p| p.parse().unwrap()).collect();
        ivec(&serde_json::json!(parts))
    }

    /// `name[k=v,...]` in the capture's notation (keys sorted, bytes as
    /// booleans, strings quoted), or the bare name without states.
    fn notation(state: u32) -> String {
        let registry = crate::registry::BlockRegistry::vanilla();
        let entry = registry.get_by_runtime_id(state).unwrap();
        let mut states: Vec<(String, String)> = entry
            .data
            .properties
            .iter()
            .map(|property| {
                let value = match entry.state_value(state, property.name).unwrap() {
                    StateValue::Byte(b) => (b != 0).to_string(),
                    StateValue::Int(i) => i.to_string(),
                    StateValue::Str(s) => format!("\"{s}\""),
                };
                (property.name.to_string(), value)
            })
            .collect();
        states.sort();
        if states.is_empty() {
            return entry.string_id.clone();
        }
        let joined: Vec<String> = states.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{}[{}]", entry.string_id, joined.join(","))
    }

    /// The runtime state of a notation string.
    fn parse(wanted: &str) -> u32 {
        let name = wanted.split('[').next().unwrap();
        let registry = crate::registry::BlockRegistry::vanilla();
        let entry = registry.get_by_name(name).unwrap();
        (entry.min_state_id..=entry.max_state_id)
            .find(|state| notation(*state) == wanted)
            .unwrap_or_else(|| panic!("no state {wanted}"))
    }

    /// What one replayed step did.
    #[derive(Default)]
    struct Run {
        /// `(tick, label, state)` of every watched cell change.
        changes: Vec<(u64, String, String)>,
        /// `(tick, actor, first position)`.
        actors: Vec<(u64, String, DVec3)>,
        packets: Vec<McpePacketData>,
    }

    impl Run {
        fn first_change(&self) -> u64 {
            self.changes.first().map_or(0, |c| c.0)
        }

        /// The changes of `labels`, timed from the step's first change.
        fn changes_of(&self, labels: &[&str]) -> Vec<(u64, String, String)> {
            let first = self.first_change();
            self.changes
                .iter()
                .filter(|(_, label, _)| labels.contains(&label.as_str()))
                .map(|(tick, label, state)| (tick - first, label.clone(), state.clone()))
                .collect()
        }

        /// The appeared actors, timed from the step's first change.
        fn actors(&self) -> Vec<(u64, String, DVec3)> {
            let first = self.first_change();
            self.actors
                .iter()
                .map(|(tick, actor, at)| (tick.saturating_sub(first), actor.clone(), *at))
                .collect()
        }
    }

    fn first_change(step: &Value) -> u64 {
        step["changes"]
            .as_array()
            .and_then(|c| c.first())
            .map_or(0, |c| c["tick"].as_u64().unwrap())
    }

    fn their_changes(step: &Value, labels: &[&str]) -> Vec<(u64, String, String)> {
        let first = first_change(step);
        let mut out = Vec::new();
        for change in step["changes"].as_array().unwrap() {
            let tick = change["tick"].as_u64().unwrap() - first;
            let cells: BTreeMap<&String, &Value> =
                change["cells"].as_object().unwrap().iter().collect();
            for (label, state) in cells {
                if labels.contains(&label.as_str()) {
                    out.push((tick, label.clone(), state.as_str().unwrap().to_owned()));
                }
            }
        }
        out
    }

    fn their_actors(step: &Value) -> Vec<(u64, String, DVec3)> {
        let first = first_change(step);
        step["actors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|a| a["event"] == "appeared")
            .map(|a| {
                (
                    a["tick"].as_u64().unwrap().saturating_sub(first),
                    a["type"].as_str().unwrap().to_owned(),
                    dvec(&a["position"]),
                )
            })
            .collect()
    }

    fn sorted(mut changes: Vec<(u64, String, String)>) -> Vec<(u64, String, String)> {
        changes.sort();
        changes
    }

    /// Every actor (mob or item) in the world.
    fn actors(world: &mut World) -> Vec<(Entity, String, DVec3)> {
        let mut out: Vec<(Entity, String, DVec3)> = world
            .query::<(Entity, &MobKind, &Position)>()
            .iter(world)
            .map(|(e, kind, p)| (e, kind.0.identifier.to_string(), p.0))
            .collect();
        out.extend(
            world
                .query_filtered::<(Entity, &Position), With<DroppedItem>>()
                .iter(world)
                .map(|(e, p)| (e, "minecraft:item".to_owned(), p.0)),
        );
        out
    }

    /// The click of a `use` step, as the client sends it; placements are
    /// committed as an approving plugin manager would.
    fn click(s: &mut Server, using: &Value) {
        use jolyne::valentine::types::{BlockPos, Vec3};
        let player = s.player;
        let feet = dvec(&using["player"]["feet"]);
        let world = s.world();
        world.get_mut::<Position>(player).unwrap().0 =
            feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
        *world.get_mut::<Rotation>(player).unwrap() = Rotation::new(
            using["player"]["yaw"].as_f64().unwrap() as f32,
            using["player"]["pitch"].as_f64().unwrap() as f32,
        );
        let stack = world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .cloned()
            .unwrap_or_else(crate::item::ItemStack::empty);
        let ids = crate::server::game::types::block_network_ids(world);
        let mut item =
            jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData::default();
        if let Some(entry) = stack.item() {
            item.id = i16::try_from(entry.network_id()).unwrap();
            item.stacksize = u16::from(stack.count);
            item.auxvalue = u32::try_from(stack.damage).unwrap();
            item.block_runtime_id = ids
                .item_block_to_wire(entry.network_id(), u32::try_from(stack.runtime_id).unwrap());
        }
        let clicked = ivec(&using["clicked"]);
        let at = using["click"].as_array().unwrap();
        let face = match using["face"].as_str().unwrap() {
            "down" => 0,
            "up" => 1,
            "north" => 2,
            "south" => 3,
            "west" => 4,
            _ => 5,
        };
        let data = jolyne::valentine::ItemUseInventoryTransaction {
            position: BlockPos {
                x: clicked.x,
                y: clicked.y,
                z: clicked.z,
            },
            face,
            slot: 0,
            item,
            click_position: Vec3 {
                x: at[0].as_f64().unwrap() as f32,
                y: at[1].as_f64().unwrap() as f32,
                z: at[2].as_f64().unwrap() as f32,
            },
            ..Default::default()
        };
        crate::server::game::blocks::handle_block_click(world, player, &data);
        let events = world
            .resource_mut::<crate::ecs::events::EventBuffer>()
            .drain();
        for event in events {
            if matches!(event, crate::ecs::events::ServerEvent::BlockPlace { .. }) {
                crate::server::game::commit_block_event(world, &event);
            }
        }
    }

    /// Run one of the sweep's commands.
    fn command(s: &mut Server, line: &str) {
        let line = line.replace('"', "");
        let words: Vec<&str> = line.split_whitespace().collect();
        let coords = |from: usize| {
            let at = |i: usize| words[from + i].parse::<f64>().unwrap();
            serde_json::json!([at(0), at(1), at(2)])
        };
        match words[0] {
            "setblock" => {
                let pos = ivec(&coords(1));
                if words.get(5) == Some(&"destroy") {
                    crate::server::game::blocks::destroy_block(s.world(), pos);
                } else {
                    s.set(pos, parse(&format!("minecraft:{}", words[4])));
                }
            }
            "summon" => {
                let feet = dvec(&coords(2));
                crate::entity::mob::spawn_mob(
                    s.world(),
                    &format!("minecraft:{}", words[1]),
                    feet,
                    0.0,
                    crate::entity::mob::SpawnCause::Spawned,
                )
                .unwrap();
            }
            "tp" => {
                let player = s.player;
                let feet = dvec(&coords(2));
                s.world().get_mut::<Position>(player).unwrap().0 =
                    feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
            }
            other => panic!("command {other}"),
        }
    }

    /// Replay `step` holding `held` (default: the step's held item
    /// afterwards), recording its watched cells and new actors every tick.
    fn replay(s: &mut Server, step: &Value, held: Option<&str>) -> Run {
        let name = step["step"].as_str().unwrap();
        let mut labels: Vec<(String, IVec3)> = Vec::new();
        for source in [&step["positions"], &step["before"]] {
            for label in source.as_object().into_iter().flatten().map(|(l, _)| l) {
                if !labels.iter().any(|(l, _)| l == label) {
                    labels.push((label.clone(), label_pos(step, label)));
                }
            }
        }
        // Cells the sweep built without a trace.
        for (label, pos) in &labels {
            let Some(theirs) = step["before"][label].as_str() else {
                continue;
            };
            let ours = notation(s.world().block_at(*pos).unwrap());
            if ours != theirs {
                assert_eq!(
                    ours, "minecraft:air",
                    "{name}: cell {label} before the step"
                );
                s.set(*pos, parse(theirs));
            }
        }
        if let Some(using) = step.get("use") {
            let clicked = ivec(&using["clicked"]);
            if notation(s.world().block_at(clicked).unwrap()) == "minecraft:air" {
                s.set(clicked, parse("minecraft:stone"));
            }
            let item = held.map(str::to_owned).or_else(|| {
                step["held_after"]
                    .as_array()
                    .map(|h| h[0].as_str().unwrap().to_owned())
            });
            let stack = match item.as_deref() {
                Some("minecraft:flint_and_steel") => {
                    crate::server::game::world_stack(s.world(), "minecraft:flint_and_steel", 1)
                        .unwrap()
                }
                Some(item) => crate::server::game::world_stack(s.world(), item, 64).unwrap(),
                None => crate::item::ItemStack::empty(),
            };
            let player = s.player;
            s.world()
                .get_mut::<MainInventory>(player)
                .unwrap()
                .0
                .set_item(0, stack)
                .unwrap();
        }
        s.packets();
        let mut seen: HashSet<Entity> = actors(s.world()).into_iter().map(|a| a.0).collect();
        let mut states: Vec<String> = labels
            .iter()
            .map(|(_, pos)| notation(s.world().block_at(*pos).unwrap()))
            .collect();
        let scheduled: Vec<(u64, String)> = step["scheduled"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                (
                    c["tick"].as_u64().unwrap(),
                    c["command"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        if let Some(using) = step.get("use") {
            click(s, using);
        }
        for line in step["commands"].as_array().into_iter().flatten() {
            command(s, line.as_str().unwrap());
        }
        let mut run = Run::default();
        let watched = step["watched_ticks"].as_u64().unwrap_or(10);
        for tick in 0..=watched {
            if tick > 0 {
                s.tick(1);
            }
            for (_, line) in scheduled.iter().filter(|(when, _)| *when == tick) {
                command(s, line);
            }
            for (index, (label, pos)) in labels.iter().enumerate() {
                let now = notation(s.world().block_at(*pos).unwrap());
                if now != states[index] {
                    run.changes.push((tick, label.clone(), now.clone()));
                    states[index] = now;
                }
            }
            for (entity, actor, position) in actors(s.world()) {
                if seen.insert(entity) {
                    run.actors.push((tick, actor, position));
                }
            }
            run.packets.extend(s.packets());
        }
        run
    }

    type Heard = Vec<(String, DVec3, i32)>;

    /// `(name, position, data)` of our sounds named `names`.
    fn our_sounds(run: &Run, names: &[&str]) -> Heard {
        run.packets
            .iter()
            .filter_map(|p| match p {
                McpePacketData::LevelSoundEventPacket(sound)
                    if names.contains(&&*sound.sound_event) =>
                {
                    let at = DVec3::new(
                        f64::from(sound.position.x),
                        f64::from(sound.position.y),
                        f64::from(sound.position.z),
                    );
                    Some((sound.sound_event.to_string(), at, sound.data))
                }
                _ => None,
            })
            .collect()
    }

    /// The capture's sounds named `names`; block states become wire ids.
    fn their_sounds(s: &mut Server, step: &Value, names: &[&str]) -> Heard {
        let ids = crate::server::game::types::block_network_ids(s.world());
        step["sounds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|sound| names.contains(&sound["sound"].as_str().unwrap()))
            .map(|sound| {
                let data = match &sound["data"] {
                    Value::String(state) => ids.to_wire(parse(state)) as i32,
                    other => other.as_i64().unwrap() as i32,
                };
                (
                    sound["sound"].as_str().unwrap().to_owned(),
                    dvec(&sound["position"]),
                    data,
                )
            })
            .collect()
    }

    type Events = Vec<(i32, DVec3, i32)>;

    fn our_events(run: &Run, ids: &[i32]) -> Events {
        run.packets
            .iter()
            .filter_map(|p| match p {
                McpePacketData::LevelEventPacket(e) if ids.contains(&e.event_id) => Some((
                    e.event_id,
                    DVec3::new(
                        f64::from(e.position.x),
                        f64::from(e.position.y),
                        f64::from(e.position.z),
                    ),
                    e.data,
                )),
                _ => None,
            })
            .collect()
    }

    fn their_events(step: &Value, ids: &[i32]) -> Events {
        step["level_events"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| ids.contains(&(e["event"].as_i64().unwrap() as i32)))
            .map(|e| {
                (
                    e["event"].as_i64().unwrap() as i32,
                    dvec(&e["position"]),
                    e["data"].as_i64().unwrap() as i32,
                )
            })
            .collect()
    }

    /// Same names and data in the same order, positions within 1e-3.
    fn assert_same<T: PartialEq + std::fmt::Debug>(
        name: &str,
        what: &str,
        ours: Vec<(T, DVec3, i32)>,
        theirs: Vec<(T, DVec3, i32)>,
    ) {
        let same = ours.len() == theirs.len()
            && ours
                .iter()
                .zip(&theirs)
                .all(|(a, b)| a.0 == b.0 && a.2 == b.2 && (a.1 - b.1).length() < 1e-3);
        assert!(same, "{name}: {what}\n ours   {ours:?}\n theirs {theirs:?}");
    }

    /// `(offset, type)` of the appeared actors of `types`.
    fn timing(actors: &[(u64, String, DVec3)], types: &[&str]) -> Vec<(u64, String)> {
        actors
            .iter()
            .filter(|a| types.contains(&a.1.as_str()))
            .map(|a| (a.0, a.1.clone()))
            .collect()
    }

    fn steps<'a>(fixture: &'a Value, group: &str, prefix: &str) -> Vec<&'a Value> {
        fixture[group]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["step"].as_str().unwrap().starts_with(prefix))
            .collect()
    }

    /// Put `count` of `item` into the container at `pos`.
    fn fill(s: &mut Server, pos: IVec3, item: &str, count: u8) {
        let world = s.world();
        let mut slots = containers::contents(world, pos).unwrap();
        let stack = crate::server::game::world_stack(world, item, count).unwrap();
        slots.set_item(0, stack).unwrap();
        containers::set_contents(world, pos, &slots);
    }

    const DISPENSER_SOUNDS: [&str; 2] = ["block.click", "block.click.fail"];
    const DISPENSER_EVENTS: [i32; 2] =
        [super::LEVEL_EVENT_LAUNCH, super::LEVEL_EVENT_DISPENSE_SMOKE];

    /// Liquid spreading is not the dispenser's: the source it places or
    /// takes is compared, not the flow around it.
    fn without_flow(changes: Vec<(u64, String, String)>) -> Vec<(u64, String, String)> {
        changes
            .into_iter()
            .filter(|c| {
                !c.2.starts_with("minecraft:water")
                    || (c.1 == "front" && c.2.contains("liquid_depth=0]"))
            })
            .collect()
    }

    /// One dispenser or dropper scenario (placing it, its lever, the lever
    /// and power steps; breaking it is the container path's), holding
    /// `count` of `item`; `shot` names the actors it should make appear.
    fn dispenser_scenario(s: &mut Server, prefix: &str, item: &str, count: u8, shot: &[&str]) {
        let fixture = fixture();
        let block = if prefix == "dropper_" {
            "minecraft:dropper"
        } else {
            "minecraft:dispenser"
        };
        for step in steps(&fixture, "dispensers", prefix) {
            let name = step["step"].as_str().unwrap();
            if name.ends_with("_destroyed") {
                continue;
            }
            let held = name
                .ends_with("_place")
                .then_some(if name.contains("lever") {
                    "minecraft:lever"
                } else {
                    block
                });
            let run = replay(s, step, held);
            let labels = ["placed", "lever", "dispenser", "dropper", "front", "side"];
            assert_eq!(
                sorted(without_flow(run.changes_of(&labels))),
                sorted(without_flow(their_changes(step, &labels))),
                "{name}: block changes"
            );
            assert_eq!(
                timing(&run.actors(), shot),
                timing(&their_actors(step), shot),
                "{name}: actors"
            );
            let theirs = their_sounds(s, step, &DISPENSER_SOUNDS);
            assert_same(name, "sounds", our_sounds(&run, &DISPENSER_SOUNDS), theirs);
            assert_same(
                name,
                "level events",
                our_events(&run, &DISPENSER_EVENTS),
                their_events(step, &DISPENSER_EVENTS),
            );
            if name.ends_with("_place") && !name.contains("lever") {
                fill(s, ivec(&step["positions"]["placed"]), item, count);
            }
        }
    }

    #[test]
    fn dispenser_arrows_match_the_bds_capture() {
        let mut s = Server::new();
        dispenser_scenario(&mut s, "arrow_", "minecraft:arrow", 3, &["minecraft:arrow"]);
    }

    #[test]
    fn dispenser_water_bucket_matches_the_bds_capture() {
        let mut s = Server::new();
        dispenser_scenario(&mut s, "water_bucket_", "minecraft:water_bucket", 1, &[]);
    }

    #[test]
    fn dispenser_ejecting_stone_matches_the_bds_capture() {
        let mut s = Server::new();
        dispenser_scenario(&mut s, "stone_", "minecraft:stone", 2, &["minecraft:item"]);
    }

    #[test]
    fn dropper_into_a_chest_matches_the_bds_capture() {
        let fixture = fixture();
        let mut s = Server::new();
        dispenser_scenario(
            &mut s,
            "dropper_",
            "minecraft:stone",
            2,
            &["minecraft:item"],
        );
        let chest =
            ivec(&steps(&fixture, "dispensers", "dropper_lever_on")[0]["positions"]["front"]);
        let moved: Vec<(String, u8)> = containers::contents(s.world(), chest)
            .unwrap()
            .slots()
            .iter()
            .filter(|stack| !stack.is_empty())
            .map(|stack| (stack.item_id().to_owned(), stack.count))
            .collect();
        assert_eq!(
            moved,
            [("minecraft:stone".to_owned(), 1)],
            "one stone moved in"
        );
    }

    /// The tick offset of the first change of `cell`.
    fn offset_of(changes: &[(u64, String, String)], cell: &str) -> Option<u64> {
        changes.iter().find(|c| c.1 == cell).map(|c| c.0)
    }

    /// The primed TNT that appeared, timed from the first change.
    fn primed(actors: Vec<(u64, String, DVec3)>) -> Vec<(u64, DVec3)> {
        actors
            .into_iter()
            .filter(|a| a.1 == super::super::tnt::PRIMED_TNT)
            .map(|a| (a.0, a.2))
            .collect()
    }

    #[test]
    fn tnt_matches_the_bds_capture() {
        let fixture = fixture();
        let mut s = Server::new();
        let fuse = [super::super::tnt::LEVEL_EVENT_FUSE];
        for step in steps(&fixture, "tnt", "") {
            let name = step["step"].as_str().unwrap();
            let run = replay(&mut s, step, None);
            let labels: Vec<String> = step["before"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            let ours = run.changes_of(&labels);
            let theirs = their_changes(step, &labels);
            if name == "lever_place" {
                assert_eq!(ours, theirs, "{name}");
                continue;
            }
            // The explosion's rays are random: the floor under the TNT,
            // the timing and the primed actors are compared.
            let under = "0,-61,0";
            assert_eq!(
                offset_of(&ours, under),
                offset_of(&theirs, under),
                "{name}: the explosion's tick"
            );
            assert_same(
                name,
                "fuse sound",
                our_events(&run, &fuse),
                their_events(step, &fuse),
            );
            let (ours_tnt, theirs_tnt) = (primed(run.actors()), primed(their_actors(step)));
            if name == "primed_by_explosion" {
                // Timed from the summon: the summoned TNT explodes, priming
                // the TNT block beside it in the same tick.
                assert_eq!(run.first_change(), first_change(step), "{name}: explosion");
                let chained = theirs_tnt.iter().find(|t| t.1.x > 1.0).unwrap();
                assert!(
                    ours_tnt
                        .iter()
                        .any(|t| t.0 == chained.0 && (t.1 - chained.1).length() < 1e-3),
                    "{name}: chain-primed TNT {ours_tnt:?} {theirs_tnt:?}"
                );
                continue;
            }
            assert_eq!(offset_of(&ours, "0,-60,0"), Some(0), "{name}: lit first");
            assert_eq!(ours_tnt.len(), 1, "{name}: {ours_tnt:?}");
            assert_eq!(ours_tnt[0].0, theirs_tnt[0].0, "{name}: primed at");
            assert!(
                (ours_tnt[0].1 - theirs_tnt[0].1).length() < 1e-3,
                "{name}: {ours_tnt:?} {theirs_tnt:?}"
            );
        }
    }

    #[test]
    fn note_block_use_matches_the_bds_capture() {
        let fixture = fixture();
        let mut s = Server::new();
        for step in steps(&fixture, "note_block_use", "") {
            let name = step["step"].as_str().unwrap();
            let run = replay(&mut s, step, None);
            let labels = ["lever", "note_block", "under"];
            assert_eq!(
                run.changes_of(&labels),
                their_changes(step, &labels),
                "{name}: block changes"
            );
            let theirs = their_sounds(&mut s, step, &["note"]);
            assert_same(name, "note sounds", our_sounds(&run, &["note"]), theirs);
            let ours: Vec<(i32, i32)> = run
                .packets
                .iter()
                .filter_map(|p| match p {
                    McpePacketData::BlockEventPacket(e) => Some((e.event_type, e.event_value)),
                    _ => None,
                })
                .collect();
            let theirs: Vec<(i32, i32)> = step["block_events"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|e| {
                    (
                        e["event_type"].as_i64().unwrap() as i32,
                        e["event_value"].as_i64().unwrap() as i32,
                    )
                })
                .collect();
            assert_eq!(ours, theirs, "{name}: block events");
            assert!(
                !run.packets
                    .iter()
                    .any(|p| matches!(p, McpePacketData::BlockActorDataPacket(_))),
                "{name}: no actor data"
            );
        }
    }

    #[test]
    fn rails_match_the_bds_capture() {
        let fixture = fixture();
        let mut s = Server::new();
        for step in steps(&fixture, "rails", "") {
            let name = step["step"].as_str().unwrap();
            let held = (name == "minecart").then_some("minecraft:minecart");
            let run = replay(&mut s, step, held);
            let labels: Vec<String> = step["positions"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            assert_eq!(
                sorted(run.changes_of(&labels)),
                sorted(their_changes(step, &labels)),
                "{name}: block changes"
            );
            if name == "minecart" {
                let ours = run.actors();
                let theirs = their_actors(step);
                assert_eq!(ours.len(), 1, "{name}: {ours:?}");
                assert_eq!(ours[0].1, theirs[0].1);
                assert!(
                    (ours[0].2 - theirs[0].2).length() < 1e-3,
                    "{name}: {ours:?} {theirs:?}"
                );
            }
        }
    }
}
