//! Feeding, love mode, mating, growth and milking, driven through the real
//! item-use-on-actor path and the scheduled tick.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::McpePacketData;
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsActorEvent,
    EnumsItemUseOnActorInventoryTransactionActionType as ActionType,
    ItemUseOnActorInventoryTransaction,
};
use tokio::sync::mpsc;

use crate::entity::EntityRandom;
use crate::entity::components::{
    GameMode, HeldSlot, MainInventory, Player, PlayerSession, Position, Rotation, RuntimeEntityId,
    RuntimeId,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::components::{AgeableConfig, BreedableConfig};
use crate::entity::mob::constants;
use crate::entity::mob::life::ageing::BabyAge;
use crate::entity::mob::state::{BreedCooldown, InLove, MateRequest, Persistent};
use crate::entity::mob::{MobKind, SpawnCause, spawn_mob};
use crate::item::ItemStack;
use crate::server::game::{GameServer, ServerConfig, world_stack};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};

struct Harness {
    server: GameServer,
    player: Entity,
    rx: mpsc::Receiver<jolyne::valentine::McpePacket>,
}

/// A grass floor at y = 63 over chunk (0, 0), viewed by one player
/// standing on it.
fn harness(mode: GameMode) -> Harness {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(11));
    let mut column = Chunk::new(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            column.set_block(x, 63, z, *blocks::GRASS_BLOCK);
        }
    }
    let chunk = world.spawn(ChunkData::new(column)).id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(0, 0), chunk);
    let (tx, rx) = mpsc::channel(1024);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(
                3.5,
                64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                3.5,
            )),
            Rotation::default(),
            mode,
            MainInventory::default(),
            HeldSlot(0),
            RuntimeEntityId(1),
            PlayerSession::new(1, "feeder".into(), None, None, tx),
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

    /// The server tick systems see during the next `tick()`.
    fn now(&mut self) -> u64 {
        self.world()
            .resource::<crate::ecs::resources::TickCounter>()
            .current
    }

    /// Ticks until `calf` grows up, as of the next tick.
    fn remaining(&mut self, calf: Entity) -> Option<u64> {
        let now = self.now();
        self.world().get::<BabyAge>(calf).unwrap().remaining(now)
    }

    fn hold(&mut self, item: &str, count: u8) {
        let stack = world_stack(self.world(), item, count).expect("vanilla item");
        let player = self.player;
        self.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, stack)
            .unwrap();
    }

    fn slot(&mut self, slot: usize) -> ItemStack {
        let player = self.player;
        self.world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(slot)
            .unwrap()
            .clone()
    }

    fn interact(&mut self, target: Entity) {
        let runtime_id = self.world().get::<RuntimeId>(target).unwrap().0;
        let transaction = ItemUseOnActorInventoryTransaction {
            runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            action_type: ActionType::Interact,
            slot: 0,
            ..Default::default()
        };
        let player = self.player;
        crate::entity::mob::interaction::interact::player_interact(
            self.world(),
            player,
            target,
            &transaction,
        );
    }

    fn packets(&mut self) -> Vec<McpePacketData> {
        let mut packets = Vec::new();
        while let Ok(packet) = self.rx.try_recv() {
            packets.push(packet.data);
        }
        packets
    }

    /// An adult cow (the spawn event picks adult 95 % of the time).
    fn adult_cow(&mut self, x: f64, z: f64) -> Entity {
        for _ in 0..200 {
            let cow = spawn_mob(
                self.world(),
                "minecraft:cow",
                DVec3::new(x, 64.0, z),
                0.0,
                SpawnCause::Spawned,
            )
            .unwrap();
            if !is_baby(self.world(), cow) {
                return cow;
            }
            self.world().despawn(cow);
        }
        panic!("no adult cow in 200 spawns");
    }
}

fn is_baby(world: &World, entity: Entity) -> bool {
    world
        .get::<ActorFlags>(entity)
        .unwrap()
        .get(actor_flags::BABY)
        .unwrap()
}

fn in_love_flag(world: &World, entity: Entity) -> bool {
    world
        .get::<ActorFlags>(entity)
        .unwrap()
        .get(actor_flags::INLOVE)
        .unwrap()
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

/// Actor events about `runtime_id` with their data.
fn actor_event_data(packets: &[McpePacketData], runtime_id: i64) -> Vec<(EnumsActorEvent, i32)> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::ActorEventPacket(event)
                if event.target_runtime_id.actor_runtime_id == runtime_id as u64 =>
            {
                Some((event.event_id, event.data))
            }
            _ => None,
        })
        .collect()
}

/// `ActorEvent` Feed data for a held `item` with aux 0.
fn feed_data(world: &World, item: &str) -> i32 {
    let network_id = world
        .resource::<crate::server::game::types::ItemRegistryResource>()
        .0
        .get_by_name(item)
        .unwrap()
        .network_id;
    network_id << constants::FEED_EVENT_ITEM_SHIFT
}

#[test]
fn wheat_puts_adult_cows_in_love_and_mating_breeds_a_baby() {
    let mut h = harness(GameMode::Survival);
    let a = h.adult_cow(6.5, 8.5);
    let b = h.adult_cow(9.5, 8.5);
    h.server.tick();
    h.packets();
    h.hold("minecraft:wheat", 3);

    h.interact(a);
    let packets = h.packets();
    let runtime_a = h.world().get::<RuntimeId>(a).unwrap().0;
    let wheat = feed_data(h.world(), "minecraft:wheat");
    assert_eq!(
        actor_event_data(&packets, runtime_a),
        [(EnumsActorEvent::Feed, wheat)]
    );
    assert!(
        packets
            .iter()
            .any(|p| matches!(p, McpePacketData::InventorySlotPacket(_))),
        "the used wheat is resent"
    );
    let now = h.now();
    let world = h.world();
    assert_eq!(
        world.get::<InLove>(a).map(|love| love.until_tick),
        Some(now + u64::from(constants::LOVE_TICKS))
    );
    assert!(in_love_flag(world, a));
    assert!(world.get::<Persistent>(a).is_some());
    assert_eq!(h.slot(0).count, 2);

    // A mob already in love refuses more food.
    h.interact(a);
    assert_eq!(h.slot(0).count, 2);
    h.interact(b);
    assert_eq!(h.slot(0).count, 1);

    let before: Vec<Entity> = {
        let world = h.world();
        world
            .query_filtered::<Entity, With<MobKind>>()
            .iter(world)
            .collect()
    };
    h.world().write_message(MateRequest { a, b });
    let mating_tick = h.now();
    h.server.tick();

    let world = h.world();
    let babies: Vec<Entity> = world
        .query_filtered::<Entity, With<MobKind>>()
        .iter(world)
        .filter(|entity| !before.contains(entity))
        .collect();
    assert_eq!(babies.len(), 1, "exactly one calf");
    let calf = babies[0];
    assert_eq!(
        world.get::<MobKind>(calf).unwrap().0.identifier.as_ref(),
        "minecraft:cow"
    );
    assert!(is_baby(world, calf));
    assert!(world.get::<AgeableConfig>(calf).is_some());
    for parent in [a, b] {
        assert!(world.get::<InLove>(parent).is_none());
        assert!(!in_love_flag(world, parent));
        assert_eq!(
            world.get::<BreedCooldown>(parent).map(|c| c.until_tick),
            Some(mating_tick + 1200)
        );
    }
    assert!(
        world
            .query::<&crate::entity::xp_orb::XpOrb>()
            .iter(world)
            .next()
            .is_some(),
        "breeding experience spawned"
    );

    // Cooling down: wheat is refused until the cooldown ends.
    h.interact(a);
    assert_eq!(h.slot(0).count, 1);
}

#[test]
fn love_mode_expires() {
    let mut h = harness(GameMode::Creative);
    let cow = h.adult_cow(8.5, 8.5);
    h.hold("minecraft:wheat", 1);
    h.interact(cow);
    assert_eq!(h.slot(0).count, 1, "creative keeps the item");
    // Love ends at its deadline: still in love on the tick before it.
    let now = h.now();
    h.world().get_mut::<InLove>(cow).unwrap().until_tick = now + 1;
    h.server.tick();
    assert!(h.world().get::<InLove>(cow).is_some());
    h.server.tick();
    let world = h.world();
    assert!(world.get::<InLove>(cow).is_none());
    assert!(!in_love_flag(world, cow));
}

#[test]
fn calf_grows_up_after_its_duration_and_wheat_speeds_it() {
    let mut h = harness(GameMode::Survival);
    let calf = spawn_mob(
        h.world(),
        "minecraft:cow",
        DVec3::new(8.5, 64.0, 8.5),
        0.0,
        SpawnCause::Born,
    )
    .unwrap();
    assert!(is_baby(h.world(), calf));
    let spawned = h.now();
    assert_eq!(
        h.world().get::<BabyAge>(calf),
        Some(&BabyAge::Growing {
            grow_tick: spawned + 1200 * 20
        }),
        "1200 s of growth"
    );
    h.server.tick();
    h.packets();
    let start = h.remaining(calf).unwrap();
    assert_eq!(start, 1200 * 20 - 1);

    // Each wheat removes a tenth of the duration.
    h.hold("minecraft:wheat", 64);
    h.interact(calf);
    let runtime_id = h.world().get::<RuntimeId>(calf).unwrap().0;
    assert_eq!(
        actor_events(&h.packets(), runtime_id),
        [EnumsActorEvent::Feed, EnumsActorEvent::BabyAge]
    );
    assert_eq!(h.remaining(calf), Some(start - 2400));
    assert_eq!(h.slot(0).count, 63);

    // Wheat brings it to the last ticks; the remaining duration elapses.
    for _ in 0..8 {
        h.interact(calf);
    }
    assert_eq!(h.remaining(calf), Some(start - 9 * 2400));
    let now = h.now();
    h.world()
        .entity_mut(calf)
        .insert(BabyAge::Growing { grow_tick: now + 3 });
    for _ in 0..3 {
        h.server.tick();
        assert!(is_baby(h.world(), calf));
    }
    h.server.tick();
    h.server.tick();
    let world = h.world();
    assert!(!is_baby(world, calf), "grown into an adult");
    assert!(world.get::<AgeableConfig>(calf).is_none());
    assert!(world.get::<BabyAge>(calf).is_none());
    assert!(world.get::<BreedableConfig>(calf).is_some());
}

#[test]
fn feeding_a_calf_sends_the_feed_and_baby_age_events_to_viewers() {
    let mut h = harness(GameMode::Survival);
    let calf = spawn_mob(
        h.world(),
        "minecraft:cow",
        DVec3::new(8.5, 64.0, 8.5),
        0.0,
        SpawnCause::Born,
    )
    .unwrap();
    h.server.tick();
    h.packets();
    let runtime_id = h.world().get::<RuntimeId>(calf).unwrap().0;
    let wheat = feed_data(h.world(), "minecraft:wheat");
    assert_ne!(wheat, 0);
    h.hold("minecraft:wheat", 2);

    // The calf eats the wheat (crumbs and eating sound at its mouth), then
    // shows the growth sparkles.
    h.interact(calf);
    assert_eq!(
        actor_event_data(&h.packets(), runtime_id),
        [
            (EnumsActorEvent::Feed, wheat),
            (EnumsActorEvent::BabyAge, 0)
        ]
    );

    // A calf whose growth is paused still eats, without the sparkles.
    h.world()
        .entity_mut(calf)
        .insert(BabyAge::Paused { remaining: 20_000 });
    h.interact(calf);
    assert_eq!(
        actor_event_data(&h.packets(), runtime_id),
        [(EnumsActorEvent::Feed, wheat)]
    );
    assert_eq!(
        h.world().get::<BabyAge>(calf),
        Some(&BabyAge::Paused {
            remaining: 20_000 - 2400
        })
    );
}

#[test]
fn feeding_wheat_alone_grows_a_calf() {
    let mut h = harness(GameMode::Survival);
    let calf = spawn_mob(
        h.world(),
        "minecraft:cow",
        DVec3::new(8.5, 64.0, 8.5),
        0.0,
        SpawnCause::Born,
    )
    .unwrap();
    h.hold("minecraft:wheat", 64);
    for _ in 0..10 {
        h.interact(calf);
    }
    assert_eq!(h.remaining(calf), Some(0));
    h.server.tick();
    h.server.tick();
    assert!(!is_baby(h.world(), calf));
}

#[test]
fn milking_a_cow_with_an_empty_bucket_gives_milk() {
    let mut h = harness(GameMode::Survival);
    let cow = h.adult_cow(8.5, 8.5);
    h.server.tick();
    h.packets();
    h.hold("minecraft:bucket", 1);
    h.interact(cow);
    assert_eq!(h.slot(0).item_id(), "minecraft:milk_bucket");
    assert_eq!(h.slot(0).count, 1);
    let packets = h.packets();
    assert!(packets.iter().any(|p| matches!(p,
        McpePacketData::LevelSoundEventPacket(sound) if sound.sound_event == "milk")));
    assert!(packets.iter().any(|p| matches!(p,
        McpePacketData::InventorySlotPacket(slot) if slot.slot == 0)));
    assert!(h.world().get::<Persistent>(cow).is_some());

    // From a stack, one bucket is used and the milk goes to a free slot.
    h.hold("minecraft:bucket", 3);
    h.interact(cow);
    assert_eq!(
        (h.slot(0).item_id(), h.slot(0).count),
        ("minecraft:bucket", 2)
    );
    assert_eq!(h.slot(1).item_id(), "minecraft:milk_bucket");

    // A milk bucket does not milk again.
    h.hold("minecraft:milk_bucket", 1);
    h.interact(cow);
    assert_eq!(h.slot(0).item_id(), "minecraft:milk_bucket");
    assert_eq!(h.slot(2), ItemStack::empty());
}

#[test]
fn cow_spawn_egg_on_a_cow_spawns_a_calf() {
    let mut h = harness(GameMode::Survival);
    let cow = h.adult_cow(8.5, 8.5);
    h.hold("minecraft:cow_spawn_egg", 2);
    h.interact(cow);
    assert_eq!(h.slot(0).count, 1);
    let world = h.world();
    let calves: Vec<Entity> = world
        .query_filtered::<Entity, With<MobKind>>()
        .iter(world)
        .filter(|entity| *entity != cow)
        .collect();
    assert_eq!(calves.len(), 1);
    assert!(is_baby(world, calves[0]));
    assert_eq!(
        world.get::<Position>(calves[0]).unwrap().0,
        world.get::<Position>(cow).unwrap().0
    );
}
