use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::AddActorPacket;
use jolyne::valentine::actor_flags;
use valentine::bedrock::codec::BedrockCodec;

use crate::entity::EntityRandom;
use crate::entity::components::Health;
use crate::entity::mob::net::network::add_actor_packet;
use crate::entity::mob::*;
use crate::server::game::{GameServer, ServerConfig};

fn server() -> GameServer {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    server
        .ecs
        .world_mut()
        .insert_resource(EntityRandom::seeded(7));
    server
}

fn native(bytes: &[u8]) -> AddActorPacket {
    assert_eq!(bytes[0], 0x0d);
    let mut input = &bytes[1..];
    let packet = AddActorPacket::decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    packet
}

fn is_baby(world: &World, entity: Entity) -> bool {
    world
        .get::<ActorFlags>(entity)
        .unwrap()
        .get(actor_flags::BABY)
        .unwrap()
}

/// Spawn cows until the spawn event picks the given age (95 % adult).
fn spawn_cow(world: &mut World, baby: bool) -> Entity {
    for _ in 0..200 {
        let cow = spawn_mob(
            world,
            "minecraft:cow",
            DVec3::new(105.5, 136.0, -25.5),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        if is_baby(world, cow) == baby {
            return cow;
        }
        world.despawn(cow);
    }
    panic!("no cow of the requested age in 200 spawns");
}

#[test]
fn vanilla_cow_compiles_to_a_spawnable_archetype() {
    let server = server();
    let registry = server.ecs.world().resource::<MobRegistry>();
    assert!(
        registry.get("minecraft:cow").is_some(),
        "cow unsupported: {:?}",
        registry.unsupported_reason("minecraft:cow")
    );
}

#[test]
fn spawned_adult_cow_add_actor_matches_native_summon() {
    let mut server = server();
    let world = server.ecs.world_mut();
    let cow = spawn_cow(world, false);
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/cow.bin"
    ));
    let rotation = *world.get::<Rotation>(cow).unwrap();
    let ours = add_actor_packet(
        "minecraft:cow",
        world.get::<RuntimeId>(cow).unwrap().0,
        world.get::<Position>(cow).unwrap().0,
        DVec3::new(0.0, f64::from(native.velocity.y), 0.0),
        &rotation,
        world.get::<EntityMetadata>(cow).unwrap(),
        world.get::<ActorProperties>(cow).unwrap(),
        world.get::<ActorAttributes>(cow).unwrap(),
    );
    assert_eq!(ours.actor_type, native.actor_type);
    assert_eq!(ours.position, native.position);
    assert_eq!(ours.attributes_list, native.attributes_list);
    assert_eq!(ours.actor_data, native.actor_data);
    assert_eq!(ours.actor_links, native.actor_links);
    // climate_variant (slot 0) is the default; sound_variant (slot 1) is a
    // fair coin in the spawn event, so only its slot and range are fixed.
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

#[test]
fn baby_cow_is_half_scale_and_flagged() {
    let mut server = server();
    let world = server.ecs.world_mut();
    let cow = spawn_cow(world, true);
    let body = world.get::<PhysicsBody>(cow).unwrap();
    assert!((body.width - 0.45).abs() < 1e-6 && (body.height - 0.65).abs() < 1e-6);
    let metadata = world.get::<EntityMetadata>(cow).unwrap();
    use jolyne::valentine::actor_data_i_ds as ids;
    use jolyne::valentine::types::DataItemEntryPayload;
    let Some(DataItemEntryPayload::DataItemFloatPayload(scale)) = metadata.get(ids::RESERVED_038)
    else {
        panic!("scale missing");
    };
    assert_eq!(scale.value, 0.5);
    assert!(world.get::<components::FollowParentGoal>(cow).is_some());
    assert!(world.get::<components::BreedableConfig>(cow).is_none());
    assert!(world.get::<components::LootConfig>(cow).is_none());
}

#[test]
fn adult_cow_carries_authored_health_goals_and_loot() {
    let mut server = server();
    let world = server.ecs.world_mut();
    let cow = spawn_cow(world, false);
    let health = world.get::<Health>(cow).unwrap();
    assert_eq!((health.current, health.max), (10.0, 10.0));
    assert_eq!(
        world
            .get::<components::LootConfig>(cow)
            .unwrap()
            .table
            .as_ref(),
        "entities/cow"
    );
    let stroll = world.get::<components::RandomStrollGoal>(cow).unwrap();
    assert_eq!(
        (stroll.base.priority, stroll.base.speed_multiplier),
        (6, 0.8)
    );
    let tempt = world.get::<components::TemptGoal>(cow).unwrap();
    assert_eq!(tempt.within_radius, 16.0);
    assert_eq!(tempt.items[0].identifier.as_ref(), "minecraft:wheat");
    let look = world.get::<components::LookAtPlayerGoal>(cow).unwrap();
    assert_eq!(look.look_ticks, (40, 80));
}
