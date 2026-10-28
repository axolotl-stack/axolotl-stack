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
                .get::<crate::entity::mob::components::LootConfig>(cow)
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
        let stack = item.map_or_else(ItemStack::empty, |id| crate::item::test_support::stack(id, 1));
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
            .map(|stack| (stack.0.item_id().to_owned(), stack.0.count))
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
    let v = knockback_velocity(
        DVec3::new(0.2, -0.0784, -0.1),
        DVec2::new(0.6, 0.8),
        KnockbackPower::DEFAULT,
    );
    assert!((v.x - (0.1 + 0.6 * 0.4)).abs() < 1e-12);
    assert!((v.z - (-0.05 + 0.8 * 0.4)).abs() < 1e-12);
    assert_eq!(v.y, constants::KNOCKBACK_VERTICAL_CAP);
    // A mob already flying fast upwards keeps half its lift plus the push,
    // capped at the same 0.4.
    let v = knockback_velocity(DVec3::new(0.0, 1.0, 0.0), DVec2::X, KnockbackPower::DEFAULT);
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
