//! A zombie's spawn bonuses across a save and load, against BDS 1.26.51
//! (`testdata/bds-1.26.51-zombie-reload.json`: three zombies hit before a
//! BDS stop and after the next start, and the attributes BDS saved for each;
//! `testdata/bds-1.26.51-spawn-bonus.json`: the bonuses BDS saved for 1,740
//! mobs of 13 kinds).

use bevy_ecs::prelude::*;
use glam::{DVec2, DVec3};
use serde_json::Value;
use zuri_nbt::{NBTTag, encoding::LittleEndian, tag};

use super::{encode_for_tests, restore_mob};
use crate::entity::EntityRandom;
use crate::entity::mob::combat::knockback::KnockbackResistance;
use crate::entity::mob::combat::knockback_velocity;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::constants;
use crate::entity::mob::spawn_bonus::FollowRangeBonus;
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
    saved_attribute(record, "minecraft:knockback_resistance")
}

/// The attribute compound `wanted` of a saved record.
fn saved_attribute(record: &[u8], wanted: &str) -> NBTTag {
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
                    if name.0 == wanted))
        })
        .cloned()
        .unwrap_or_else(|| panic!("the zombie's {wanted} is saved"))
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

/// The `RandomSpawnBonus` amount of a saved BDS attribute, if it has one.
fn bonus_amount(attribute: &Value) -> Option<f32> {
    attribute["Modifiers"]
        .as_array()
        .and_then(|modifiers| modifiers.first())
        .map(|modifier| modifier["Amount"]["Float"].as_f64().unwrap() as f32)
}

fn set_bonus(world: &mut World, zombie: Entity, bonus: Option<f32>) {
    match bonus {
        Some(amount) => {
            world.entity_mut(zombie).insert(FollowRangeBonus(amount));
        }
        None => {
            world.entity_mut(zombie).remove::<FollowRangeBonus>();
        }
    }
}

#[test]
fn a_zombie_saves_its_follow_range_as_bds_does() {
    let mut server = server();
    let world = server.ecs.world_mut();
    for row in fixture()["zombies"].as_array().unwrap() {
        let saved = &row["saved_attributes"]["minecraft:follow_range"];
        let zombie = zombie(world);
        set_bonus(world, zombie, bonus_amount(saved));
        let record = encode_for_tests(world, zombie);
        assert_eq!(
            saved_attribute(&record, "minecraft:follow_range"),
            nbt(saved),
            "{}",
            row["key"]
        );
        world.despawn(zombie);
    }
}

#[test]
fn a_loaded_zombie_keeps_the_follow_range_bonus_it_spawned_with() {
    let mut server = server();
    let world = server.ecs.world_mut();
    for bonus in [Some(1.2716), None] {
        let zombie = zombie(world);
        set_bonus(world, zombie, bonus);
        let record = encode_for_tests(world, zombie);
        world.despawn(zombie);
        for _ in 0..3 {
            let loaded = restore_mob(world, &record).unwrap();
            assert_eq!(
                world.get::<FollowRangeBonus>(loaded).map(|kept| kept.0),
                bonus,
                "restored, not re-rolled"
            );
            world.despawn(loaded);
        }
    }
}

fn spawn_bonus_fixture() -> Value {
    serde_json::from_str(include_str!("testdata/bds-1.26.51-spawn-bonus.json")).unwrap()
}

/// The largest gap between the empirical distribution functions of two
/// samples (the two-sample Kolmogorov-Smirnov statistic).
fn ks_distance(a: &mut [f32], b: &mut [f32]) -> f64 {
    a.sort_by(f32::total_cmp);
    b.sort_by(f32::total_cmp);
    let (mut i, mut j, mut gap) = (0, 0, 0.0f64);
    while i < a.len() && j < b.len() {
        if a[i] <= b[j] {
            i += 1;
        } else {
            j += 1;
        }
        gap = gap.max((i as f64 / a.len() as f64 - j as f64 / b.len() as f64).abs());
    }
    gap
}

/// Which kinds roll the two spawn bonuses, how often and how big: the
/// zombie, husk, drowned and zombified piglin always save a knockback
/// resistance below 0.05 and about a third of them a follow-range
/// multiplier between 1 and 1.5; the zombie villager and the other kinds
/// neither (easy and hard alike).
#[test]
fn the_zombie_classes_roll_the_spawn_bonuses_bds_saved() {
    use crate::entity::mob::native::NativeTraits;
    use rand::SeedableRng;
    use unastar_data::actor_identifiers::ActorType;
    let fixture = spawn_bonus_fixture();
    let (mut follow, mut resistance, mut rolling) = (Vec::new(), Vec::new(), 0usize);
    for run in fixture["runs"].as_array().unwrap() {
        for (identifier, row) in run["classes"].as_object().unwrap() {
            let native = NativeTraits::of(ActorType::from_name(identifier).unwrap());
            let count = row["count"].as_u64().unwrap() as usize;
            let amounts = |key: &str| -> Vec<f32> {
                row[key]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|amount| amount.as_f64().unwrap() as f32)
                    .collect()
            };
            let (range, knockback) = (amounts("follow_range"), amounts("knockback_resistance"));
            assert_eq!(native.follow_range_bonus, !range.is_empty(), "{identifier}");
            assert_eq!(
                native.knockback_resistance_bonus.is_some(),
                knockback.len() == count,
                "{identifier}"
            );
            if native.follow_range_bonus {
                rolling += count;
                follow.extend(range);
                resistance.extend(knockback);
            }
        }
    }
    let mut rng = rand::rngs::StdRng::seed_from_u64(5);
    let draws = 30_000;
    let mut ours: Vec<f32> = (0..draws)
        .filter_map(|_| FollowRangeBonus::roll(&mut rng).map(|bonus| bonus.0))
        .collect();
    let (bds_share, our_share) = (
        follow.len() as f64 / rolling as f64,
        ours.len() as f64 / draws as f64,
    );
    // Three standard deviations of BDS's share over its 1,160 mobs.
    let deviation = (bds_share * (1.0 - bds_share) / rolling as f64).sqrt();
    assert!(
        (bds_share - our_share).abs() < 3.0 * deviation,
        "bonus share: BDS {bds_share}, ours {our_share}"
    );
    // The 95 % two-sample Kolmogorov-Smirnov bound for the amounts.
    let bound = |n: usize, m: usize| 1.36 * ((n + m) as f64 / (n * m) as f64).sqrt();
    let gap = ks_distance(&mut follow, &mut ours);
    assert!(
        gap < bound(follow.len(), ours.len()),
        "follow-range amounts: KS {gap}"
    );
    let mut resisted: Vec<f32> = (0..draws)
        .map(|_| {
            KnockbackResistance::roll(&mut rng, constants::ZOMBIE_KNOCKBACK_RESISTANCE_BONUS).0
                as f32
        })
        .collect();
    let gap = ks_distance(&mut resistance, &mut resisted);
    assert!(
        gap < bound(resistance.len(), resisted.len()),
        "knockback resistance: KS {gap}"
    );
}
