//! Hurting mobs: player melee, `minecraft:hurt_on_condition`, the hurt
//! timer and knockback. Death (loot, experience, removal) is in
//! [`super::death`].
//!
//! Every damage source goes through [`hurt_mob`], which applies the native
//! hurt rule: a successful hit starts a [`constants::HURT_TICKS`] hurt
//! timer; while it runs a new hit only deals what exceeds the damage that
//! started it, and nothing when it does not exceed it (override and
//! self-destruct damage ignore the timer). A full hit shows the hurt
//! animation to viewers (`ActorEvent` hurt) and, when it has a source
//! position, knocks the mob back. The changed `Health` reaches viewers as
//! an `UpdateAttributes` through `derive_mob_state`.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::{DVec2, DVec3};
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsActorEvent, ItemUseOnActorInventoryTransaction,
};
use jolyne::valentine::{ActorEventPacket, McpePacket};
use rand::Rng;
use unastar_data::actor_components::body::ActorDamageCause;

use super::config::{HurtCondition, HurtOnConditionConfig};
use super::events::{MobFacts, held_item};
use super::state::{Dead, LastHurt};
use super::{MobKind, constants, death};
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{
    ActorViewers, GameMode, Health, PhysicsBody, Player, PlayerSession, Position, RuntimeId,
    Velocity,
};
use crate::registry::collision::Aabb;

/// The hurt timer of a mob: the server tick it runs out at and the damage
/// of the hit that started it.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
#[component(storage = "SparseSet")]
pub struct HurtTimer {
    pub until_tick: u64,
    pub last_damage: f32,
}

/// One instance of damage dealt to a mob.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub damage: f32,
    pub cause: ActorDamageCause,
    pub attacker: Option<Entity>,
    pub by_player: bool,
    /// Position the knockback pushes away from (horizontally); `None` for
    /// damage without knockback.
    pub knockback_from: Option<DVec3>,
}

/// A player attacks `target` with whatever the server has in the player's
/// selected hotbar slot; the item and positions the client reports in the
/// transaction are not trusted.
///
/// Spectators, dead players and players whose eye is farther than
/// [`constants::MELEE_REACH`] from the mob's box cannot hit. Creative
/// players deal normal melee damage.
pub fn player_attack(
    world: &mut World,
    player: Entity,
    target: Entity,
    _transaction: &ItemUseOnActorInventoryTransaction,
) {
    let Some(eye) = attacker_eye(world, player) else {
        return;
    };
    let (Some(feet), Some(body)) = (
        world.get::<Position>(target).map(|p| p.0),
        world.get::<PhysicsBody>(target),
    ) else {
        return;
    };
    let mob_box = Aabb::from_feet(feet, body.width, body.height);
    if eye.distance_squared(eye.clamp(mob_box.min, mob_box.max))
        > constants::MELEE_REACH * constants::MELEE_REACH
    {
        return;
    }
    let damage = unastar_data::melee::attack_damage(
        held_item(world, player).map(|stack| stack.item_id.as_str()),
    );
    hurt_mob(
        world,
        target,
        Hit {
            damage,
            cause: ActorDamageCause::EntityAttack,
            attacker: Some(player),
            by_player: true,
            knockback_from: Some(eye),
        },
    );
}

/// The eye position of a player that may attack this tick.
fn attacker_eye(world: &World, player: Entity) -> Option<DVec3> {
    world.get::<Player>(player)?;
    if *world.get::<GameMode>(player)? == GameMode::Spectator
        || world.get::<Health>(player).is_some_and(Health::is_dead)
    {
        return None;
    }
    let eye = world.get::<Position>(player)?.0;
    eye.is_finite().then_some(eye)
}

/// Apply `hit` to `target` under the hurt rules; returns the health
/// actually removed (0 when the hit was ignored). A hit that empties the
/// mob's health starts its death.
pub fn hurt_mob(world: &mut World, target: Entity, hit: Hit) -> f32 {
    if hit.damage <= 0.0 || world.get::<Dead>(target).is_some() {
        return 0.0;
    }
    if world.get::<Health>(target).is_none_or(Health::is_dead) {
        return 0.0;
    }
    let ignores_timer = matches!(
        hit.cause,
        ActorDamageCause::Override | ActorDamageCause::SelfDestruct
    );
    let now = world.resource::<TickCounter>().current;
    let timer = world.get::<HurtTimer>(target).copied();
    let (applied, until_tick, full) = match timer {
        Some(timer) if now < timer.until_tick && !ignores_timer => {
            if hit.damage <= timer.last_damage {
                return 0.0;
            }
            (hit.damage - timer.last_damage, timer.until_tick, false)
        }
        _ => (hit.damage, now + u64::from(constants::HURT_TICKS), true),
    };
    let dead = {
        let mut entity = world.entity_mut(target);
        entity.insert((
            HurtTimer {
                until_tick,
                last_damage: hit.damage,
            },
            LastHurt {
                tick: now,
                cause: hit.cause,
                attacker: hit.attacker,
                by_player: hit.by_player,
            },
        ));
        let mut health = entity.get_mut::<Health>().expect("health checked above");
        health.damage(applied);
        health.is_dead()
    };
    if full {
        send_actor_event(world, target, EnumsActorEvent::Hurt, 0);
        if let Some(from) = hit.knockback_from {
            apply_knockback(world, target, from);
        }
    }
    if dead {
        death::begin_death(world, target);
    }
    applied
}

/// Push `target` away from `from` (horizontally).
fn apply_knockback(world: &mut World, target: Entity, from: DVec3) {
    let Some(feet) = world.get::<Position>(target).map(|p| p.0) else {
        return;
    };
    let mut away = DVec2::new(feet.x - from.x, feet.z - from.z);
    if away.length_squared() < 1.0e-8 {
        // Attacker straight above or below: a random horizontal direction.
        let angle = world
            .get_resource_or_init::<EntityRandom>()
            .0
            .gen_range(0.0..std::f64::consts::TAU);
        away = DVec2::from_angle(angle);
    }
    if let Some(mut velocity) = world.get_mut::<Velocity>(target) {
        velocity.0 = knockback_velocity(velocity.0, away.normalize());
    }
}

/// Native knockback: existing velocity × [`constants::KNOCKBACK_RETAIN`],
/// plus [`constants::KNOCKBACK_HORIZONTAL`] along `away` (a unit vector in
/// x/z) and `KNOCKBACK_VERTICAL × KNOCKBACK_VERTICAL_SCALE` upwards, the
/// vertical velocity capped at [`constants::KNOCKBACK_VERTICAL_CAP`].
pub fn knockback_velocity(velocity: DVec3, away: DVec2) -> DVec3 {
    let mut out = velocity * constants::KNOCKBACK_RETAIN;
    out.x += away.x * constants::KNOCKBACK_HORIZONTAL;
    out.z += away.y * constants::KNOCKBACK_HORIZONTAL;
    out.y = (out.y + constants::KNOCKBACK_VERTICAL * constants::KNOCKBACK_VERTICAL_SCALE)
        .min(constants::KNOCKBACK_VERTICAL_CAP);
    out
}

/// Send `ActorEvent(event, data)` about `entity` to its viewers.
pub fn send_actor_event(world: &World, entity: Entity, event: EnumsActorEvent, data: i32) {
    let (Some(viewers), Some(runtime_id)) = (
        world.get::<ActorViewers>(entity),
        world.get::<RuntimeId>(entity),
    ) else {
        return;
    };
    let packet = McpePacket::from(ActorEventPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        },
        event_id: event,
        data,
        fire_at_position: None,
    });
    for viewer in &viewers.viewers {
        if let Some(session) = world.get::<PlayerSession>(*viewer) {
            let _ = session.send(packet.clone());
        }
    }
}

/// System (exclusive): `minecraft:hurt_on_condition`. Every tick each
/// condition whose filter passes deals its `damage_per_tick` with its cause,
/// under the hurt rules and without knockback.
#[allow(clippy::type_complexity)]
pub fn hurt_on_condition(
    world: &mut World,
    mobs: &mut QueryState<(Entity, &HurtOnConditionConfig), (With<MobKind>, Without<Dead>)>,
    mut due: Local<Vec<(Entity, Arc<[HurtCondition]>)>>,
) {
    due.extend(
        mobs.iter(world)
            .map(|(entity, config)| (entity, Arc::clone(&config.conditions))),
    );
    for (entity, conditions) in due.drain(..) {
        for condition in conditions.iter() {
            if world.get::<Dead>(entity).is_some() {
                break;
            }
            let passes = condition.filter.evaluate(&MobFacts {
                world,
                entity,
                other: None,
            });
            if passes {
                hurt_mob(
                    world,
                    entity,
                    Hit {
                        damage: condition.damage_per_tick as f32,
                        cause: condition.cause.unwrap_or(ActorDamageCause::None),
                        attacker: None,
                        by_player: false,
                        knockback_from: None,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::prelude::*;
    use glam::DVec3;
    use jolyne::valentine::types::EnumsActorEvent;
    use jolyne::valentine::{McpePacket, McpePacketData};
    use tokio::sync::mpsc::Receiver;

    use super::*;
    use crate::entity::components::{
        DroppedItem, Experience, HeldSlot, ItemEntityStack, MainInventory, OnFire, Rotation,
        RuntimeEntityId,
    };
    use crate::entity::mob::state::Dead;
    use crate::entity::mob::{SpawnCause, spawn_mob};
    use crate::entity::xp_orb::XpOrb;
    use crate::item::ItemStack;
    use crate::server::game::{GameServer, ServerConfig};
    use crate::world::ChunkManager;
    use crate::world::chunk::{Chunk, blocks};
    use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};

    struct Harness {
        server: GameServer,
        player: Entity,
        rx: Receiver<McpePacket>,
    }

    /// A stone floor at y = 63 over chunk (0, 0), viewed by one survival
    /// player standing at (4.5, 64, 8.5).
    fn harness(seed: u64) -> Harness {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let world = server.ecs.world_mut();
        world.insert_resource(EntityRandom::seeded(seed));
        let mut column = Chunk::new(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                column.set_block(x, 63, z, *blocks::STONE);
            }
        }
        let chunk = world.spawn(ChunkData::new(column)).id();
        world
            .resource_mut::<ChunkManager>()
            .insert(ChunkPosition::new(0, 0), chunk);
        let (tx, rx) = tokio::sync::mpsc::channel(4096);
        let player = world
            .spawn((
                Player,
                Position(DVec3::new(
                    4.5,
                    64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                    8.5,
                )),
                Rotation::default(),
                GameMode::Survival,
                MainInventory::default(),
                HeldSlot(0),
                Health::default(),
                Experience::default(),
                RuntimeEntityId(1),
                PlayerSession::new(1, "attacker".into(), None, None, tx),
            ))
            .id();
        let mut viewers = ChunkViewers::default();
        viewers.insert(player);
        world.entity_mut(chunk).insert(viewers);
        Harness { server, player, rx }
    }

    impl Harness {
        fn world(&mut self) -> &mut World {
            self.server.ecs.world_mut()
        }

        /// An adult cow with its feet at `position`, already sent to the player.
        fn cow(&mut self, position: DVec3) -> Entity {
            let world = self.world();
            let cow = loop {
                let cow =
                    spawn_mob(world, "minecraft:cow", position, 0.0, SpawnCause::Spawned).unwrap();
                if world
                    .get::<crate::entity::mob::config::LootConfig>(cow)
                    .is_some()
                {
                    break cow;
                }
                world.despawn(cow);
            };
            self.server.tick();
            self.drain();
            cow
        }

        fn hold(&mut self, item: Option<&str>) {
            let player = self.player;
            let stack = item.map_or_else(ItemStack::empty, |id| ItemStack::new(id, 1));
            self.world()
                .get_mut::<MainInventory>(player)
                .unwrap()
                .0
                .set_item(0, stack)
                .unwrap();
        }

        /// Stand two blocks west of `cow` (facing it) and attack it.
        fn attack(&mut self, cow: Entity) {
            let player = self.player;
            let world = self.world();
            let feet = world.get::<Position>(cow).unwrap().0;
            world.get_mut::<Position>(player).unwrap().0 = DVec3::new(
                feet.x - 2.0,
                feet.y + crate::entity::PLAYER_EYE_HEIGHT,
                feet.z,
            );
            player_attack(world, player, cow, &Default::default());
        }

        fn drain(&mut self) -> Vec<McpePacketData> {
            std::iter::from_fn(|| self.rx.try_recv().ok().map(|p| p.data)).collect()
        }

        fn health(&mut self, cow: Entity) -> f32 {
            self.world().get::<Health>(cow).unwrap().current
        }

        fn drops(&mut self) -> Vec<(String, u8)> {
            let world = self.world();
            world
                .query_filtered::<&ItemEntityStack, With<DroppedItem>>()
                .iter(world)
                .map(|stack| (stack.0.item_id.clone(), stack.0.count))
                .collect()
        }

        fn orbs(&mut self) -> Vec<u32> {
            let world = self.world();
            world
                .query::<&XpOrb>()
                .iter(world)
                .map(|orb| orb.value)
                .collect()
        }
    }

    fn actor_events(packets: &[McpePacketData], runtime_id: i64) -> Vec<EnumsActorEvent> {
        packets
            .iter()
            .filter_map(|packet| match packet {
                McpePacketData::ActorEventPacket(event)
                    if event.target_runtime_id.actor_runtime_id == runtime_id as u64 =>
                {
                    Some(event.event_id)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn knockback_retains_half_the_velocity_and_caps_the_lift() {
        let v = knockback_velocity(DVec3::new(0.2, -0.0784, -0.1), DVec2::new(0.6, 0.8));
        assert!((v.x - (0.1 + 0.6 * 0.4)).abs() < 1e-12);
        assert!((v.z - (-0.05 + 0.8 * 0.4)).abs() < 1e-12);
        assert_eq!(v.y, constants::KNOCKBACK_VERTICAL_CAP);
        // A mob already flying fast upwards keeps half its lift plus the push,
        // capped at the same 0.4.
        let v = knockback_velocity(DVec3::new(0.0, 1.0, 0.0), DVec2::X);
        assert_eq!(v.y, 0.4);
    }

    #[test]
    fn a_sword_hit_hurts_flashes_and_knocks_the_cow_back() {
        let mut h = harness(1);
        let cow = h.cow(DVec3::new(8.5, 64.0, 8.5));
        h.hold(Some("minecraft:diamond_sword"));
        let before = h.world().get::<Velocity>(cow).unwrap().0;
        h.attack(cow);

        assert_eq!(h.health(cow), 10.0 - 8.0);
        let after = h.world().get::<Velocity>(cow).unwrap().0;
        // The player is due west: pushed east (+x) by 0.4, lifted to the cap.
        assert!((after.x - (before.x * 0.5 + 0.4)).abs() < 1e-9, "{after}");
        assert!((after.z - before.z * 0.5).abs() < 1e-9, "{after}");
        assert_eq!(after.y, 0.4);
        let hurt = h.world().get::<LastHurt>(cow).copied().unwrap();
        assert_eq!(
            (hurt.cause, hurt.attacker, hurt.by_player),
            (ActorDamageCause::EntityAttack, Some(h.player), true)
        );

        let runtime_id = h.world().get::<RuntimeId>(cow).unwrap().0;
        let packets = h.drain();
        assert_eq!(actor_events(&packets, runtime_id), [EnumsActorEvent::Hurt]);
        h.server.tick();
        let packets = h.drain();
        assert!(packets.iter().any(|p| matches!(p,
            McpePacketData::UpdateAttributesPacket(update)
                if update.target_runtime_id.actor_runtime_id == runtime_id as u64
                    && update.attribute_list.iter().any(|a|
                        &*a.name == "minecraft:health" && a.current_value == 2.0))));
        assert!(packets.iter().any(|p| matches!(p,
            McpePacketData::SetActorMotionPacket(motion)
                if motion.target_runtime_id.actor_runtime_id == runtime_id as u64)));
    }

    #[test]
    fn hits_inside_the_hurt_timer_only_deal_the_excess() {
        let mut h = harness(2);
        let cow = h.cow(DVec3::new(8.5, 64.0, 8.5));
        h.world().get_mut::<Health>(cow).unwrap().current = 10.0;
        h.hold(Some("minecraft:wooden_sword"));
        h.attack(cow);
        assert_eq!(h.health(cow), 10.0 - 5.0);
        let runtime_id = h.world().get::<RuntimeId>(cow).unwrap().0;
        h.drain();

        for _ in 0..3 {
            h.server.tick();
        }
        // Weaker or equal hits are ignored entirely.
        h.hold(None);
        h.attack(cow);
        h.hold(Some("minecraft:wooden_sword"));
        h.attack(cow);
        assert_eq!(h.health(cow), 5.0);
        // A stronger hit deals only what exceeds the first one, without a
        // new hurt flash or knockback.
        h.world().get_mut::<Velocity>(cow).unwrap().0 = DVec3::ZERO;
        h.hold(Some("minecraft:stone_sword"));
        h.attack(cow);
        assert_eq!(h.health(cow), 5.0 - (6.0 - 5.0));
        assert_eq!(h.world().get::<Velocity>(cow).unwrap().0, DVec3::ZERO);
        assert!(actor_events(&h.drain(), runtime_id).is_empty());

        // Ten ticks after the first hit the timer has run out: full damage.
        for _ in 3..10 {
            h.server.tick();
        }
        h.hold(None);
        h.attack(cow);
        assert_eq!(h.health(cow), 4.0 - 1.0);
        assert_eq!(
            actor_events(&h.drain(), runtime_id),
            [EnumsActorEvent::Hurt]
        );
    }

    #[test]
    fn spectators_and_players_out_of_reach_cannot_hit() {
        let mut h = harness(3);
        let cow = h.cow(DVec3::new(8.5, 64.0, 8.5));
        let player = h.player;
        *h.world().get_mut::<GameMode>(player).unwrap() = GameMode::Spectator;
        h.attack(cow);
        assert_eq!(h.health(cow), 10.0);
        *h.world().get_mut::<GameMode>(player).unwrap() = GameMode::Creative;
        let world = h.world();
        world.get_mut::<Position>(player).unwrap().0 = DVec3::new(0.5, 65.62, 8.5);
        player_attack(world, player, cow, &Default::default());
        assert_eq!(
            h.health(cow),
            10.0,
            "7.6 blocks from the box is out of reach"
        );
        h.attack(cow);
        assert_eq!(h.health(cow), 9.0, "creative players deal melee damage");
    }

    /// Kill `cow` with a fist hit after leaving it one health point.
    fn kill(h: &mut Harness, cow: Entity) {
        h.world().get_mut::<Health>(cow).unwrap().current = 1.0;
        h.hold(None);
        h.attack(cow);
    }

    #[test]
    fn a_killed_cow_dies_drops_loot_and_after_twenty_ticks_leaves_experience() {
        let mut h = harness(4);
        let mut leather = Vec::new();
        let mut beef = Vec::new();
        for round in 0..6 {
            let cow = h.cow(DVec3::new(8.5, 64.0, 8.5));
            let runtime_id = h.world().get::<RuntimeId>(cow).unwrap().0;
            kill(&mut h, cow);
            assert!(h.world().get::<Dead>(cow).is_some());
            assert_eq!(
                actor_events(&h.drain(), runtime_id),
                [EnumsActorEvent::Hurt, EnumsActorEvent::Death]
            );
            // Loot appears at the moment of death.
            let drops = h.drops();
            let count = |id: &str| -> u32 {
                drops
                    .iter()
                    .filter(|(item, _)| item == id)
                    .map(|(_, n)| u32::from(*n))
                    .sum()
            };
            leather.push(count("minecraft:leather"));
            beef.push(count("minecraft:beef"));
            assert_eq!(count("minecraft:cooked_beef"), 0);
            assert!(h.orbs().is_empty());

            // The kill landed before the next tick, which is the death tick;
            // the body goes DEATH_TICKS ticks after it.
            for _ in 0..constants::DEATH_TICKS {
                h.server.tick();
            }
            assert!(h.world().get_entity(cow).is_ok(), "death animation runs");
            h.server.tick();
            assert!(h.world().get_entity(cow).is_err(), "body removed");
            let packets = h.drain();
            assert!(packets.iter().any(|p| matches!(p,
                McpePacketData::RemoveActorPacket(remove)
                    if remove.target_actor_id.actor_unique_id == runtime_id)));
            let xp: u32 = h.orbs().iter().sum();
            assert!((1..=3).contains(&xp), "round {round}: {xp} experience");

            // Clear the ground for the next cow.
            let world = h.world();
            let leftovers: Vec<Entity> = world
                .query_filtered::<Entity, Or<(With<DroppedItem>, With<XpOrb>)>>()
                .iter(world)
                .collect();
            for entity in leftovers {
                world.despawn(entity);
            }
            h.drain();
        }
        assert!(leather.iter().all(|n| *n <= 2), "{leather:?}");
        assert!(beef.iter().all(|n| (1..=3).contains(n)), "{beef:?}");
    }

    #[test]
    fn a_burning_cow_drops_cooked_beef() {
        let mut h = harness(5);
        let cow = h.cow(DVec3::new(8.5, 64.0, 8.5));
        h.world().entity_mut(cow).insert(OnFire::new(100));
        kill(&mut h, cow);
        let drops = h.drops();
        assert!(
            drops
                .iter()
                .any(|(item, _)| item == "minecraft:cooked_beef"),
            "{drops:?}"
        );
        assert!(!drops.iter().any(|(item, _)| item == "minecraft:beef"));
    }

    #[test]
    fn a_cow_standing_in_lava_burns_to_death_without_experience() {
        let mut h = harness(6);
        let world = h.world();
        let chunk = world
            .resource::<ChunkManager>()
            .get_by_coords(0, 0)
            .unwrap();
        {
            let mut data = world.get_mut::<ChunkData>(chunk).unwrap();
            for x in 7..=9 {
                for z in 7..=9 {
                    for y in 64..=66 {
                        let block = if (x, z) == (8, 8) && y < 66 {
                            *blocks::LAVA
                        } else if (x, z) == (8, 8) {
                            continue;
                        } else {
                            *blocks::STONE
                        };
                        data.inner.set_block(x, y, z, block);
                    }
                }
            }
        }
        let cow = h.cow(DVec3::new(8.5, 64.0, 8.5));
        let mut died_at = None;
        for tick in 0..60 {
            h.server.tick();
            if died_at.is_none() && h.world().get::<Dead>(cow).is_some() {
                died_at = Some(tick);
                let hurt = h.world().get::<LastHurt>(cow).copied().unwrap();
                assert_eq!(
                    (hurt.cause, hurt.by_player),
                    (ActorDamageCause::Lava, false)
                );
            }
            if h.world().get_entity(cow).is_err() {
                break;
            }
        }
        // 4 damage every 10 ticks: dead on the third burn.
        assert!(died_at.is_some_and(|tick| tick <= 22), "{died_at:?}");
        assert!(h.world().get_entity(cow).is_err(), "body removed");
        assert!(h.orbs().is_empty(), "no experience without a player kill");
    }
}
