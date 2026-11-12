//! A zombie's spawn knockback resistance across a save and load, against
//! BDS 1.26.51 (`testdata/bds-1.26.51-zombie-reload.json`: three zombies hit
//! before a BDS stop and after the next start, and the attribute BDS saved
//! for each).

use bevy_ecs::prelude::*;
use glam::{DVec2, DVec3};
use serde_json::Value;
use zuri_nbt::{NBTTag, encoding::LittleEndian, tag};

use super::{encode_for_tests, restore_mob};
use crate::entity::EntityRandom;
use crate::entity::mob::combat::knockback::KnockbackResistance;
use crate::entity::mob::combat::knockback_velocity;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::server::game::{GameServer, ServerConfig};

fn fixture() -> Value {
    serde_json::from_str(include_str!("testdata/bds-1.26.51-zombie-reload.json")).unwrap()
}

/// The fixture's typed NBT (`{"Float": x}`, `{"Int": n}`, `{"Long": n}`,
/// strings, lists, compounds) as a tag.
fn nbt(value: &Value) -> NBTTag {
    match value {
        Value::String(text) => NBTTag::String(tag::String(text.clone())),
        Value::Array(items) => NBTTag::List(tag::List(items.iter().map(nbt).collect())),
        Value::Object(fields) => {
            if let Some(float) = fields.get("Float") {
                return NBTTag::Float(tag::Float(float.as_f64().unwrap() as f32));
            }
            if let Some(int) = fields.get("Int") {
                return NBTTag::Int(tag::Int(int.as_i64().unwrap() as i32));
            }
            if let Some(long) = fields.get("Long") {
                return NBTTag::Long(tag::Long(long.as_i64().unwrap()));
            }
            NBTTag::Compound(
                fields
                    .iter()
                    .map(|(name, value)| (name.clone(), nbt(value)))
                    .collect::<std::collections::HashMap<_, _>>()
                    .into(),
            )
        }
        other => panic!("unexpected fixture value {other}"),
    }
}

fn server() -> GameServer {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    server
        .ecs
        .world_mut()
        .insert_resource(EntityRandom::seeded(11));
    server
}

fn zombie(world: &mut World) -> Entity {
    spawn_mob(
        world,
        "minecraft:zombie",
        DVec3::new(8.5, 16.0, 8.5),
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap()
}

/// The `minecraft:knockback_resistance` compound of a saved record.
fn saved_resistance(record: &[u8]) -> NBTTag {
    let mut input = record;
    let NBTTag::Compound(root) = NBTTag::read(&mut input, LittleEndian).unwrap() else {
        panic!("actor root is a compound");
    };
    let Some(NBTTag::List(attributes)) = root.0.get("Attributes") else {
        panic!("no Attributes");
    };
    attributes
        .0
        .iter()
        .find(|attribute| {
            matches!(attribute, NBTTag::Compound(fields)
                if matches!(fields.0.get("Name"), Some(NBTTag::String(name))
                    if name.0 == "minecraft:knockback_resistance"))
        })
        .cloned()
        .expect("the zombie's knockback resistance is saved")
}

#[test]
fn a_zombie_saves_its_spawn_knockback_resistance_as_bds_does() {
    let mut server = server();
    let world = server.ecs.world_mut();
    for row in fixture()["zombies"].as_array().unwrap() {
        let bds = nbt(&row["saved_attributes"]["minecraft:knockback_resistance"]);
        let NBTTag::Compound(fields) = &bds else {
            panic!("a compound");
        };
        let Some(NBTTag::Float(current)) = fields.0.get("Current") else {
            panic!("Current");
        };
        let zombie = zombie(world);
        world
            .entity_mut(zombie)
            .insert(KnockbackResistance(f64::from(current.0)));
        let record = encode_for_tests(world, zombie);
        assert_eq!(saved_resistance(&record), bds, "{}", row["key"]);
        world.despawn(zombie);
    }
}

#[test]
fn a_loaded_zombie_keeps_the_knockback_resistance_it_spawned_with() {
    let mut server = server();
    let world = server.ecs.world_mut();
    for row in fixture()["zombies"].as_array().unwrap() {
        // BDS pushed each zombie the same before the save and after the load.
        let push = |name: &str| {
            row[name][0]
                .as_f64()
                .unwrap()
                .hypot(row[name][2].as_f64().unwrap())
        };
        assert_eq!(push("push_before_save"), push("push_after_load"));
        let zombie = zombie(world);
        let rolled = world.get::<KnockbackResistance>(zombie).unwrap().0;
        let record = encode_for_tests(world, zombie);
        world.despawn(zombie);
        for _ in 0..3 {
            let loaded = restore_mob(world, &record).unwrap();
            let kept = world.get::<KnockbackResistance>(loaded).unwrap().0;
            assert_eq!(kept, f64::from(rolled as f32), "{}", row["key"]);
            world.despawn(loaded);
        }
        // And BDS's own value gives BDS's push after the load.
        let bds = nbt(&row["saved_attributes"]["minecraft:knockback_resistance"]);
        let NBTTag::Compound(fields) = &bds else {
            panic!("a compound");
        };
        let Some(NBTTag::Float(current)) = fields.0.get("Current") else {
            panic!("Current");
        };
        let resisted = KnockbackResistance(f64::from(current.0)).resist(KnockbackPower::DEFAULT);
        let pushed = knockback_velocity(DVec3::ZERO, DVec2::X, resisted);
        assert!(
            (pushed.x - push("push_after_load")).abs() < 1e-6,
            "{}: {pushed}",
            row["key"]
        );
    }
}

#[test]
fn a_zombie_saved_without_the_attribute_loads_without_resistance() {
    let mut server = server();
    let world = server.ecs.world_mut();
    let zombie = zombie(world);
    world.entity_mut(zombie).remove::<KnockbackResistance>();
    let record = encode_for_tests(world, zombie);
    world.despawn(zombie);
    let loaded = restore_mob(world, &record).unwrap();
    assert!(world.get::<KnockbackResistance>(loaded).is_none());
}
