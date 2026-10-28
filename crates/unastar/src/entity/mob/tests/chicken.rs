//! The vanilla chicken end to end: compilation, native AddActor, wing-flap
//! glide, fall damage, egg laying, tempting, loot and persistence; and the
//! thrown egg: native AddActor, consumption, arc, impact and hatching.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;
use jolyne::valentine::{AddActorPacket, McpePacket, McpePacketData};
use tokio::sync::mpsc::Receiver;
use valentine::bedrock::codec::BedrockCodec;

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::Health;
use crate::entity::components::{
    DroppedItem, GameMode, HeldSlot, ItemEntityStack, MainInventory, OnFire, Player, PlayerSession,
    Projectile, RuntimeEntityId,
};
use crate::entity::mob::components::spawn_entity::{SpawnEntityTimers, SpawnTimer};
use crate::entity::mob::net::network::add_actor_packet;
use crate::entity::mob::*;
use crate::item::ItemStack;
use crate::server::game::{GameServer, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};

const FLOOR: i16 = 63;

struct Harness {
    server: GameServer,
    player: Entity,
    rx: Receiver<McpePacket>,
}

/// A stone floor at y = 63 over chunk (0, 0), viewed by one survival player.
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
            column.set_block(x, FLOOR, z, *blocks::STONE);
        }
    }
    let chunk = world
        .spawn((ChunkData::new(column), ChunkPosition::new(0, 0)))
        .id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(0, 0), chunk);
    let (tx, rx) = tokio::sync::mpsc::channel(8192);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(
                2.5,
                f64::from(FLOOR + 1) + crate::entity::PLAYER_EYE_HEIGHT,
                2.5,
            )),
            Rotation::default(),
            GameMode::Survival,
            MainInventory::default(),
            HeldSlot(0),
            Health::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "farmer".into(), None, None, tx),
        ))
        .id();
    let mut viewers = ChunkViewers::default();
    viewers.insert(player);
    world.entity_mut(chunk).insert(viewers);
    Harness { server, player, rx }
}

fn is_baby(world: &World, entity: Entity) -> bool {
    world
        .get::<ActorFlags>(entity)
        .unwrap()
        .get(actor_flags::BABY)
        .unwrap()
}

/// Spawn chickens until the spawn event picks the given age (95 % adult).
fn spawn(world: &mut World, identifier: &str, feet: DVec3, baby: bool) -> Entity {
    for _ in 0..200 {
        let mob = spawn_mob(world, identifier, feet, 0.0, SpawnCause::Spawned).unwrap();
        if is_baby(world, mob) == baby {
            return mob;
        }
        world.despawn(mob);
    }
    panic!("no {identifier} of the requested age in 200 spawns");
}

fn drain(rx: &mut Receiver<McpePacket>) -> Vec<McpePacketData> {
    std::iter::from_fn(|| rx.try_recv().ok().map(|p| p.data)).collect()
}

fn dropped(world: &mut World) -> Vec<(String, u8)> {
    world
        .query_filtered::<&ItemEntityStack, With<DroppedItem>>()
        .iter(world)
        .map(|stack| (stack.0.item_id.clone(), stack.0.count))
        .collect()
}

#[test]
fn vanilla_chicken_compiles_to_a_spawnable_archetype() {
    let server = harness(1).server;
    let registry = server.ecs.world().resource::<MobRegistry>();
    let chicken = registry.get("minecraft:chicken");
    assert!(
        chicken.is_some(),
        "chicken unsupported: {:?}",
        registry.unsupported_reason("minecraft:chicken")
    );
    let chicken = chicken.unwrap();
    assert_eq!(
        chicken.native.wing_flap_drag,
        Some(constants::WING_FLAP_VERTICAL_DRAG)
    );
    assert_eq!(
        registry.get("minecraft:cow").unwrap().native.wing_flap_drag,
        None
    );
}

#[test]
fn spawned_adult_chicken_add_actor_matches_native_summon() {
    let mut h = harness(7);
    let world = h.server.ecs.world_mut();
    let chicken = spawn(
        world,
        "minecraft:chicken",
        DVec3::new(105.5, 136.0, -25.5),
        false,
    );
    let bytes = include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/chicken.bin"
    );
    assert_eq!(bytes[0], 0x0d);
    let mut input = &bytes[1..];
    let native = AddActorPacket::decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    let rotation = *world.get::<Rotation>(chicken).unwrap();
    let ours = add_actor_packet(
        "minecraft:chicken",
        world.get::<RuntimeId>(chicken).unwrap().0,
        world.get::<Position>(chicken).unwrap().0,
        DVec3::new(0.0, f64::from(native.velocity.y), 0.0),
        &rotation,
        world.get::<EntityMetadata>(chicken).unwrap(),
        world.get::<ActorProperties>(chicken).unwrap(),
        world.get::<ActorAttributes>(chicken).unwrap(),
    );
    assert_eq!(ours.actor_type, native.actor_type);
    assert_eq!(ours.position, native.position);
    assert_eq!(ours.attributes_list, native.attributes_list);
    assert_eq!(ours.actor_data, native.actor_data);
    assert_eq!(ours.actor_links, native.actor_links);
    // climate_variant (slot 0) is the default; sound_variant (slot 1) is a
    // fair coin in the spawn event.
    let props = &ours.synched_properties;
    assert!(props.float_entries_list.is_empty());
    assert_eq!(props.int_entries_list.len(), 2);
    assert_eq!(
        props.int_entries_list[0],
        native.synched_properties.int_entries_list[0]
    );
    assert_eq!(props.int_entries_list[1].property_index, 1);
    assert!((0..=1).contains(&props.int_entries_list[1].data));
}

/// Ticks until `mob` stands on the floor, and its health then.
fn drop_from_ten_blocks(identifier: &str) -> (u32, f32, f32) {
    let mut h = harness(3);
    let world = h.server.ecs.world_mut();
    let feet = DVec3::new(8.5, f64::from(FLOOR + 11), 8.5);
    let mob = spawn(world, identifier, feet, false);
    let start = world.get::<Health>(mob).unwrap().current;
    for tick in 1..=200 {
        h.server.tick();
        let world = h.server.ecs.world_mut();
        if world.get::<OnGround>(mob).unwrap().0 {
            // The landing resolves in the same tick.
            let health = world.get::<Health>(mob).map_or(0.0, |h| h.current);
            return (tick, start, health);
        }
    }
    panic!("{identifier} never landed");
}

#[test]
fn chicken_glides_down_slower_than_a_cow_and_takes_no_fall_damage() {
    let (cow_ticks, cow_start, cow_health) = drop_from_ten_blocks("minecraft:cow");
    let (chicken_ticks, chicken_start, chicken_health) = drop_from_ten_blocks("minecraft:chicken");
    // A cow falls 10 blocks in about 25 ticks; the chicken's terminal speed
    // under wing-flap drag is (0.08 × 0.98 × 0.6) / (1 − 0.98 × 0.6) ≈ 0.11
    // blocks per tick, so it needs several times as long.
    assert!(cow_ticks < 30, "cow took {cow_ticks} ticks");
    assert!(
        chicken_ticks > 2 * cow_ticks,
        "chicken {chicken_ticks} vs cow {cow_ticks} ticks"
    );
    // 10 blocks: ceil(10 − 3) = 7 damage to the cow, none to the chicken
    // (its native fall multiplier is 0 and its damage sensor cancels falls).
    assert_eq!(cow_start - cow_health, 7.0);
    assert_eq!(chicken_health, chicken_start);
}

#[test]
fn chicken_terminal_glide_matches_the_native_drag() {
    let mut h = harness(5);
    let world = h.server.ecs.world_mut();
    // High enough to keep gliding for 60 ticks, near enough to the player
    // not to despawn.
    let chicken = spawn(
        world,
        "minecraft:chicken",
        DVec3::new(8.5, f64::from(FLOOR + 90), 8.5),
        false,
    );
    for _ in 0..60 {
        h.server.tick();
    }
    let world = h.server.ecs.world_mut();
    let vy = world.get::<Velocity>(chicken).unwrap().0.y;
    let factor = 0.98 * (1.0 - constants::WING_FLAP_VERTICAL_DRAG);
    let terminal = -(constants::LIVING_GRAVITY * factor) / (1.0 - factor);
    assert!(
        (vy - terminal).abs() < 1e-6,
        "vy {vy} vs terminal {terminal}"
    );
}

#[test]
fn adult_chicken_lays_an_egg_within_the_max_wait_and_a_baby_never_does() {
    let mut h = harness(11);
    let world = h.server.ecs.world_mut();
    let floor = f64::from(FLOOR + 1);
    let adult = spawn(
        world,
        "minecraft:chicken",
        DVec3::new(4.5, floor, 4.5),
        false,
    );
    let baby = spawn(
        world,
        "minecraft:chicken",
        DVec3::new(12.5, floor, 12.5),
        true,
    );
    for mob in [adult, baby] {
        // Keep both in place so their eggs are attributable.
        world.entity_mut(mob).remove::<ai::MobBrain>();
    }
    assert!(world.get::<SpawnEntityTimers>(baby).is_none());
    h.server.tick();
    let world = h.server.ecs.world_mut();
    // The counter advances at the end of a tick: this is the tick that ran.
    let rolled = world.resource::<TickCounter>().current - 1;
    let SpawnTimer::Due(due) = world.get::<SpawnEntityTimers>(adult).unwrap().0[0] else {
        panic!("the temperate rule rolled no wait");
    };
    // A wait of [300, 600) s, the rolling tick counting as the first.
    assert!((rolled + 300 * 20 - 1..rolled + 600 * 20 - 1).contains(&due));
    drain(&mut h.rx);
    let mut laid_at = None;
    for _ in 0..600 * 20 {
        h.server.tick();
        let world = h.server.ecs.world_mut();
        if !dropped(world).is_empty() {
            laid_at = Some(world.resource::<TickCounter>().current - 1);
            break;
        }
    }
    assert_eq!(laid_at, Some(due));
    let world = h.server.ecs.world_mut();
    assert_eq!(dropped(world), vec![("minecraft:egg".to_owned(), 1)]);
    let egg = world
        .query_filtered::<&Position, With<DroppedItem>>()
        .single(world)
        .unwrap()
        .0;
    assert!((egg.x - 4.5).abs() < 0.2 && (egg.z - 4.5).abs() < 0.2);
    let plops = drain(&mut h.rx)
        .into_iter()
        .filter(|packet| {
            matches!(packet, McpePacketData::LevelSoundEventPacket(sound)
                if &*sound.sound_event == "plop")
        })
        .count();
    assert_eq!(plops, 1);
}

#[test]
fn chicken_follows_a_player_holding_wheat_seeds() {
    let mut h = harness(13);
    let player = h.player;
    let world = h.server.ecs.world_mut();
    world
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, ItemStack::new("minecraft:wheat_seeds", 1))
        .unwrap();
    let chicken = spawn(
        world,
        "minecraft:chicken",
        DVec3::new(10.5, f64::from(FLOOR + 1), 10.5),
        false,
    );
    let start = world.get::<Position>(chicken).unwrap().0;
    let target = DVec3::new(2.5, start.y, 2.5);
    for _ in 0..200 {
        h.server.tick();
    }
    let world = h.server.ecs.world_mut();
    let end = world.get::<Position>(chicken).unwrap().0;
    assert!(
        end.distance(target) < start.distance(target) - 4.0,
        "chicken stayed at {end}"
    );
}

#[test]
fn killed_chicken_drops_raw_or_cooked_chicken_and_feathers() {
    for burning in [false, true] {
        let mut h = harness(17);
        let world = h.server.ecs.world_mut();
        let chicken = spawn(
            world,
            "minecraft:chicken",
            DVec3::new(4.5, f64::from(FLOOR + 1), 8.5),
            false,
        );
        if burning {
            world.entity_mut(chicken).insert(OnFire::new(200));
        }
        combat::hurt_mob(
            world,
            chicken,
            combat::Hit {
                damage: 100.0,
                cause: unastar_data::actor_components::body::ActorDamageCause::EntityAttack,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: components::KnockbackPower::DEFAULT,
            },
        );
        for _ in 0..30 {
            h.server.tick();
        }
        let world = h.server.ecs.world_mut();
        let drops = dropped(world);
        let meat = if burning {
            "minecraft:cooked_chicken"
        } else {
            "minecraft:chicken"
        };
        assert!(
            drops.iter().any(|(id, count)| id == meat && *count >= 1),
            "{drops:?}"
        );
        assert!(
            drops
                .iter()
                .all(|(id, _)| id == meat || id == "minecraft:feather"),
            "{drops:?}"
        );
    }
}

#[test]
fn chicken_round_trips_through_storage_with_its_egg_timer() {
    let mut h = harness(19);
    let world = h.server.ecs.world_mut();
    let chicken = spawn(
        world,
        "minecraft:chicken",
        DVec3::new(4.5, f64::from(FLOOR + 1), 8.5),
        false,
    );
    h.server.tick();
    let world = h.server.ecs.world_mut();
    let now = world.resource::<TickCounter>().current;
    let SpawnTimer::Due(due) = world.get::<SpawnEntityTimers>(chicken).unwrap().0[0] else {
        panic!("no egg timer");
    };
    let chunk = world
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    let snapshot = persistence::snapshot_chunks(world, &[chunk], false);
    let column = snapshot[0].column.as_ref().unwrap();
    let nbt = column.actors[0].nbt.clone();
    world.despawn(chicken);
    let restored = persistence::restore_mob(world, &nbt).unwrap();
    assert!(!is_baby(world, restored));
    // Saved and restored between the same two ticks: the same firing tick.
    let SpawnTimer::Due(restored_due) = world.get::<SpawnEntityTimers>(restored).unwrap().0[0]
    else {
        panic!("egg timer lost");
    };
    assert_eq!(restored_due, due);
    assert!(restored_due >= now);
    // BDS's saved count includes the tick the egg is laid on.
    let zuri_nbt::NBTTag::List(entries) =
        SpawnEntityTimers(Box::new([SpawnTimer::Due(due)])).to_nbt(now)
    else {
        panic!("entries list");
    };
    let zuri_nbt::NBTTag::Compound(entry) = &entries.0[0] else {
        panic!("entry compound");
    };
    assert!(matches!(
        entry.0.get("SpawnTimer"),
        Some(zuri_nbt::NBTTag::Int(n)) if i64::from(n.0) == (due - now + 1) as i64
    ));
}

#[test]
fn vanilla_egg_compiles_to_a_projectile_archetype() {
    let server = harness(1).server;
    let registry = server.ecs.world().resource::<MobRegistry>();
    let egg = registry.get("minecraft:egg");
    assert!(
        egg.is_some(),
        "egg unsupported: {:?}",
        registry.unsupported_reason("minecraft:egg")
    );
    assert!(egg.unwrap().projectile);
}

#[test]
fn summoned_egg_add_actor_matches_native_summon() {
    let mut h = harness(7);
    let world = h.server.ecs.world_mut();
    let native = {
        let bytes = include_bytes!(
            "../../../../../bds-extractor/captures/1.26.51/operator/summons/egg.bin"
        );
        assert_eq!(bytes[0], 0x0d);
        let mut input = &bytes[1..];
        let packet = AddActorPacket::decode(&mut input, ()).unwrap();
        assert!(input.is_empty());
        packet
    };
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let egg = spawn_mob(world, "minecraft:egg", feet, 0.0, SpawnCause::Spawned).unwrap();
    assert!(world.get::<Projectile>(egg).is_some());
    assert!(world.get::<PhysicsBody>(egg).is_none());
    let ours = add_actor_packet(
        "minecraft:egg",
        world.get::<RuntimeId>(egg).unwrap().0,
        world.get::<Position>(egg).unwrap().0,
        world.get::<Velocity>(egg).unwrap().0,
        world.get::<Rotation>(egg).unwrap(),
        world.get::<EntityMetadata>(egg).unwrap(),
        world.get::<ActorProperties>(egg).unwrap(),
        world.get::<ActorAttributes>(egg).unwrap(),
    );
    assert_eq!(ours.actor_type, native.actor_type);
    assert_eq!(ours.position, native.position);
    assert_eq!(ours.velocity, native.velocity);
    assert_eq!(ours.rotation, native.rotation);
    assert_eq!(ours.y_head_rotation, native.y_head_rotation);
    assert_eq!(ours.y_body_rotation, native.y_body_rotation);
    assert_eq!(ours.attributes_list, native.attributes_list);
    assert_eq!(ours.actor_data, native.actor_data);
    assert_eq!(ours.synched_properties, native.synched_properties);
    assert_eq!(ours.actor_links, native.actor_links);
}

fn the_egg(world: &mut World) -> Option<Entity> {
    world
        .query_filtered::<Entity, With<Projectile>>()
        .iter(world)
        .next()
}

fn climate(world: &World, entity: Entity) -> String {
    let properties = world.get::<ActorProperties>(entity).unwrap();
    let slot = properties
        .layout()
        .slot(constants::CLIMATE_VARIANT_PROPERTY)
        .unwrap();
    let value = properties.value(&slot).unwrap();
    properties
        .layout()
        .enum_value(&slot, value)
        .unwrap()
        .to_owned()
}

/// The player looks 10° down along +z and holds `count` of `item`.
fn ready_to_throw(h: &mut Harness, item: &str, count: u8, mode: GameMode) {
    let world = h.server.ecs.world_mut();
    let mut player = world.entity_mut(h.player);
    player.insert((Rotation::new(0.0, 10.0), mode));
    player
        .get_mut::<MainInventory>()
        .unwrap()
        .0
        .set_item(0, ItemStack::new(item, count))
        .unwrap();
}

#[test]
fn thrown_egg_follows_its_arc_breaks_on_the_floor_and_uses_one_egg() {
    let mut h = harness(23);
    ready_to_throw(&mut h, "minecraft:blue_egg", 3, GameMode::Survival);
    let player = h.player;
    let eye = h.server.ecs.world().get::<Position>(player).unwrap().0;
    h.server
        .ecs
        .world_mut()
        .resource_mut::<crate::entity::projectile::ThrowQueue>()
        .0
        .push(player);
    h.server.tick();
    let world = h.server.ecs.world_mut();
    assert_eq!(
        world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        2
    );
    let egg = the_egg(world).expect("an egg in flight");
    assert_eq!(climate(world, egg), "cold");
    let packets = drain(&mut h.rx);
    assert!(packets.iter().any(|p| matches!(p,
        McpePacketData::InventorySlotPacket(slot) if slot.slot == 0 && slot.item.stacksize == 2)));
    assert!(packets.iter().any(|p| matches!(p,
        McpePacketData::LevelSoundEventPacket(sound) if &*sound.sound_event == "throw")));
    let McpePacketData::AddActorPacket(add) = packets
        .iter()
        .find(|p| matches!(p, McpePacketData::AddActorPacket(add) if &*add.actor_type == "minecraft:egg"))
        .unwrap()
    else {
        unreachable!()
    };
    // Invisible on its first tick.
    let flags = world.get::<ActorFlags>(egg).unwrap();
    assert!(flags.get(actor_flags::INVISIBLE).unwrap());
    assert_eq!(add.synched_properties.int_entries_list[0].data, 2);

    // 1.5 blocks per tick along the view, no thrower velocity; one tick
    // flown: the position moved by it, the velocity kept 0.99 and lost 0.03
    // (the definition's f32 values).
    let (inertia, gravity) = (f64::from(0.99f32), f64::from(0.03f32));
    let pitch = 10f64.to_radians();
    let mut velocity = DVec3::new(0.0, -pitch.sin(), pitch.cos()) * 1.5;
    let mut expected = eye + velocity;
    velocity = velocity * inertia - DVec3::Y * gravity;
    let mut flown = 1;
    loop {
        let world = h.server.ecs.world_mut();
        let position = world.get::<Position>(egg).unwrap().0;
        assert!(
            position.distance(expected) < 1e-9,
            "tick {flown}: {position} vs {expected}"
        );
        assert!(world.get::<Velocity>(egg).unwrap().0.distance(velocity) < 1e-9);
        h.server.tick();
        flown += 1;
        let world = h.server.ecs.world_mut();
        if flown == 2 {
            let flags = world.get::<ActorFlags>(egg).unwrap();
            assert!(
                !flags.get(actor_flags::INVISIBLE).unwrap(),
                "visible from tick 2"
            );
        }
        if world.get_entity(egg).is_err() {
            break;
        }
        expected += velocity;
        velocity = velocity * inertia - DVec3::Y * gravity;
        assert!(flown < 40, "the egg never landed");
    }
    // It broke on the tick its next move would have crossed the floor top.
    assert!(expected.y > f64::from(FLOOR + 1));
    assert!(expected.y + velocity.y <= f64::from(FLOOR + 1) + 1e-9);
    let packets = drain(&mut h.rx);
    let blue_egg = h
        .server
        .ecs
        .world()
        .resource::<crate::server::game::types::ItemRegistryResource>()
        .0
        .get_by_name("minecraft:blue_egg")
        .unwrap()
        .network_id;
    let crumbs = packets
        .iter()
        .filter(|p| {
            matches!(p, McpePacketData::LevelEventPacket(event)
                if event.event_id == 2009 && event.data == 14 | (blue_egg << 16))
        })
        .count();
    assert_eq!(crumbs, 6);
    assert!(
        packets
            .iter()
            .any(|p| matches!(p, McpePacketData::RemoveActorPacket(_)))
    );
}

#[test]
fn creative_throws_keep_the_egg_and_hatch_chicks_at_the_native_rates() {
    const THROWS: usize = 2000;
    let mut h = harness(29);
    ready_to_throw(&mut h, "minecraft:blue_egg", 1, GameMode::Creative);
    let player = h.player;
    let world = h.server.ecs.world_mut();
    world
        .resource_mut::<crate::entity::projectile::ThrowQueue>()
        .0
        .extend(std::iter::repeat_n(player, THROWS));
    // Every egg breaks on the same tick; the chicks then stand where they
    // hatched until their first AI tick.
    for _ in 0..40 {
        h.server.tick();
        if the_egg(h.server.ecs.world_mut()).is_none() {
            break;
        }
    }
    let world = h.server.ecs.world_mut();
    assert!(the_egg(world).is_none(), "every egg broke");
    assert_eq!(
        world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        1
    );
    let chicks: Vec<Entity> = world
        .query_filtered::<(Entity, &MobKind), Without<Projectile>>()
        .iter(world)
        .filter(|(_, kind)| &*kind.0.identifier == "minecraft:chicken")
        .map(|(entity, _)| entity)
        .collect();
    // Per egg: 1/8 for one chick, and of those 1/32 for four instead:
    // 0.1523 chicks per egg, standard deviation 0.40.
    assert!(
        (233..=377).contains(&chicks.len()),
        "{} chicks from {THROWS} eggs",
        chicks.len()
    );
    for &chick in &chicks {
        assert!(is_baby(world, chick));
        assert_eq!(climate(world, chick), "cold");
        let feet = world.get::<Position>(chick).unwrap().0;
        assert!(
            feet.x == 2.5 && feet.y > f64::from(FLOOR + 1) && (8.0..12.0).contains(&feet.z),
            "chick at {feet}"
        );
    }
}

#[test]
fn egg_spawn_rolls_match_the_native_chances() {
    use rand::SeedableRng;
    let spawn = crate::entity::projectile::SpawnChance {
        first_chance: 0.125,
        first_count: 1,
        second_chance: 0.03125,
        second_count: 4,
        baby: true,
        definition: unastar_data::actor_identifiers::ActorType::Chicken,
        on_spawn: Box::new([]),
    };
    let mut rng = rand::rngs::StdRng::seed_from_u64(31);
    let mut counts = [0u32; 5];
    const ROLLS: u32 = 200_000;
    for _ in 0..ROLLS {
        counts[crate::entity::projectile::spawn_count(&spawn, &mut rng) as usize] += 1;
    }
    assert_eq!(counts[2] + counts[3], 0);
    // Expected 175 000 / 24 219 / 781; four standard deviations.
    let near = |got: u32, p: f64| {
        let mean = f64::from(ROLLS) * p;
        (f64::from(got) - mean).abs() < 4.0 * (mean * (1.0 - p)).sqrt()
    };
    assert!(near(counts[0], 0.875), "{counts:?}");
    assert!(near(counts[1], 0.125 * 31.0 / 32.0), "{counts:?}");
    assert!(near(counts[4], 0.125 / 32.0), "{counts:?}");
}
