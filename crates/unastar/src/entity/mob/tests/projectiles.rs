//! Projectile actors replayed against BDS captures: the bottle o'
//! enchanting (`minecraft:xp_bottle`) from its native AddActor to the orbs
//! it breaks into, and the invisible first tick every projectile shares.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;
use jolyne::valentine::{AddActorPacket, McpePacketData};
use valentine::bedrock::codec::BedrockCodec;

use super::facets::{Arena, SURFACE, floor};
use crate::entity::components::{
    GameMode, PhysicsBody, Position, Projectile, Rotation, RuntimeId, Velocity,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::net::network::add_actor_packet;
use crate::entity::mob::*;
use crate::entity::xp_orb::XpOrb;
use crate::world::rules::Difficulty;

const XP_BOTTLE: &str = "minecraft:xp_bottle";

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("../testdata/bds-1.26.51-xp-bottles.json")).unwrap()
}

fn flag(world: &World, actor: Entity, index: u32) -> bool {
    world.get::<ActorFlags>(actor).unwrap().get(index).unwrap()
}

fn bottle(a: &mut Arena, feet: DVec3) -> Entity {
    spawn_mob(a.world(), XP_BOTTLE, feet, 0.0, SpawnCause::Spawned).unwrap()
}

fn orb_values(world: &mut World) -> Vec<(DVec3, u32)> {
    world
        .query::<(&Position, &XpOrb)>()
        .iter(world)
        .map(|(position, orb)| (position.0, orb.value))
        .collect()
}

/// `collision_box`, `physics`, `type_family`: the summoned bottle's
/// AddActor equals the native one (box, gravity and collision flags,
/// invisible), and it belongs to its two families.
#[test]
fn summoned_xp_bottle_add_actor_matches_native_summon() {
    let mut a = floor(7, Difficulty::Normal, GameMode::Creative);
    let native = {
        let bytes = include_bytes!(
            "../../../../../bds-extractor/captures/1.26.51/operator/summons/xp_bottle.bin"
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
    let world = a.world();
    let bottle = spawn_mob(world, XP_BOTTLE, feet, 0.0, SpawnCause::Spawned).unwrap();
    assert!(world.get::<Projectile>(bottle).is_some());
    assert!(world.get::<PhysicsBody>(bottle).is_none());
    for family in ["projectile", "xp_bottle"] {
        assert!(crate::entity::mob::events::is_family(world, bottle, family));
    }
    assert!(!crate::entity::mob::events::is_family(world, bottle, "mob"));
    let ours = add_actor_packet(
        XP_BOTTLE,
        world.get::<RuntimeId>(bottle).unwrap().0,
        world.get::<Position>(bottle).unwrap().0,
        world.get::<Velocity>(bottle).unwrap().0,
        world.get::<Rotation>(bottle).unwrap(),
        world.get::<EntityMetadata>(bottle).unwrap(),
        world.get::<ActorProperties>(bottle).unwrap(),
        world.get::<ActorAttributes>(bottle).unwrap(),
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

/// A level sound as sent: name, position, actor identifier, unique id, data.
type Sound = (String, DVec3, String, i64, i32);

/// What one tick sent about the bottle.
#[derive(Default)]
struct Seen {
    added: bool,
    removed: bool,
    /// AddActor positions and values of experience orbs.
    orbs: Vec<(DVec3, i32)>,
    sounds: Vec<Sound>,
    events: Vec<(i32, DVec3, i32)>,
}

fn watch(packets: Vec<McpePacketData>, unique_id: i64) -> Seen {
    let mut seen = Seen::default();
    for packet in packets {
        match packet {
            McpePacketData::AddActorPacket(add) if &*add.actor_type == XP_BOTTLE => {
                seen.added = true;
            }
            McpePacketData::AddActorPacket(add) if &*add.actor_type == "minecraft:xp_orb" => {
                let value = add
                    .actor_data
                    .data
                    .iter()
                    .find(|item| item.id == jolyne::valentine::actor_data_i_ds::VALUE)
                    .and_then(|item| match &item.payload {
                        jolyne::valentine::types::DataItemEntryPayload::DataItemIntPayload(v) => {
                            Some(v.value)
                        }
                        _ => None,
                    })
                    .expect("orb value");
                seen.orbs.push((
                    DVec3::new(
                        f64::from(add.position.x),
                        f64::from(add.position.y),
                        f64::from(add.position.z),
                    ),
                    value,
                ));
            }
            McpePacketData::RemoveActorPacket(remove)
                if remove.target_actor_id.actor_unique_id == unique_id =>
            {
                seen.removed = true;
            }
            McpePacketData::LevelSoundEventPacket(sound) => seen.sounds.push((
                sound.sound_event.to_string(),
                DVec3::new(
                    f64::from(sound.position.x),
                    f64::from(sound.position.y),
                    f64::from(sound.position.z),
                ),
                sound.actor_identifier.to_string(),
                sound.actor_unique_id,
                sound.data,
            )),
            McpePacketData::LevelEventPacket(event) => seen.events.push((
                event.event_id,
                DVec3::new(
                    f64::from(event.position.x),
                    f64::from(event.position.y),
                    f64::from(event.position.z),
                ),
                event.data,
            )),
            _ => {}
        }
    }
    seen
}

fn close(a: DVec3, b: DVec3) -> bool {
    a.distance(b) < 1e-3
}

/// `projectile`: a bottle summoned with no velocity six blocks above the
/// floor replays every bottle of the capture. Invisible with
/// `disappeared` at the centre of its box, visible (`CAN_SHOW_NAME`) one
/// tick after its AddActor with `reappeared` a tick later and 0.05 lower;
/// the potion-splash level event in the default potion colour at the floor
/// top sixteen ticks after its AddActor, orbs worth 3 to 10 at the same
/// point, and `glass` there one tick later attributed to no actor.
///
/// Measured difference kept: BDS sends the bottle's AddActor and its
/// RemoveActor (and the orbs' AddActor) one tick after they happen, this
/// server on the tick itself, so here `disappeared` arrives one batch after
/// the AddActor instead of in the same batch, and the removal and the orbs
/// in the splash event's batch instead of the next one. Counted from the
/// summon and the hit, both servers send every packet on the same tick.
#[test]
fn an_xp_bottle_breaks_into_orbs_like_the_native_bottles() {
    let fixture = fixture();
    let rows = fixture["bottles"].as_array().unwrap();
    let drop = fixture["drop"].as_f64().unwrap();
    let row = &rows[0];
    for row in rows {
        for key in ["disappeared", "reappeared", "glass"] {
            assert_eq!(row[key], rows[0][key], "{key}: every bottle alike");
        }
        assert_eq!(row["level_event"]["batch"], rows[0]["level_event"]["batch"]);
        assert_eq!(row["visible_batch"], rows[0]["visible_batch"]);
    }
    let batch = |value: &serde_json::Value| value.as_u64().unwrap();
    let above = |value: &serde_json::Value| value["y_above_floor"].as_f64().unwrap();

    let mut a = floor(11, Difficulty::Normal, GameMode::Creative);
    let at = DVec3::new(0.5, SURFACE, 0.5);
    let bottle = bottle(&mut a, at + DVec3::Y * drop);
    let unique_id = a.world().get::<RuntimeId>(bottle).unwrap().0;
    {
        let world = a.world();
        assert!(flag(world, bottle, actor_flags::INVISIBLE));
        assert!(!flag(world, bottle, actor_flags::CAN_SHOW_NAME));
    }
    a.packets();

    // Batch 0 is the tick that sends the AddActor.
    let mut ticks: Vec<Seen> = Vec::new();
    let mut visible_at = None;
    while a.world().get_entity(bottle).is_ok() || ticks.len() <= 25 {
        a.ticks(1);
        let alive = a.world().get_entity(bottle).is_ok();
        if alive && visible_at.is_none() && !flag(a.world(), bottle, actor_flags::INVISIBLE) {
            assert!(flag(a.world(), bottle, actor_flags::CAN_SHOW_NAME));
            visible_at = Some(ticks.len() as u64);
        }
        let packets = a.packets();
        ticks.push(watch(packets, unique_id));
        assert!(ticks.len() < 40, "the bottle never broke");
    }
    assert!(ticks[0].added, "AddActor on the spawn tick");
    assert_eq!(visible_at, Some(batch(&row["visible_batch"])));

    let sound_at = |name: &str| -> Vec<(u64, &Sound)> {
        ticks
            .iter()
            .enumerate()
            .flat_map(|(i, seen)| seen.sounds.iter().map(move |s| (i as u64, s)))
            .filter(|(_, s)| s.0 == name)
            .collect()
    };
    let disappeared = sound_at("disappeared");
    assert_eq!(disappeared.len(), 1);
    let (tick, (_, position, identifier, id, data)) = disappeared[0];
    assert_eq!(
        tick,
        batch(&row["disappeared"]["batch"]) + 1,
        "one batch late"
    );
    assert!(close(*position, at + DVec3::Y * above(&row["disappeared"])));
    assert_eq!(
        identifier,
        row["disappeared"]["actor_identifier"].as_str().unwrap()
    );
    assert_eq!(*id, unique_id);
    assert_eq!(
        i64::from(*data),
        row["disappeared"]["data"].as_i64().unwrap()
    );

    let reappeared = sound_at("reappeared");
    assert_eq!(reappeared.len(), 1);
    let (tick, (_, position, identifier, id, data)) = reappeared[0];
    assert_eq!(tick, batch(&row["reappeared"]["batch"]));
    assert!(
        close(*position, at + DVec3::Y * above(&row["reappeared"])),
        "{position}"
    );
    assert_eq!(
        identifier,
        row["reappeared"]["actor_identifier"].as_str().unwrap()
    );
    assert_eq!(*id, unique_id);
    assert_eq!(
        i64::from(*data),
        row["reappeared"]["data"].as_i64().unwrap()
    );

    let event = &row["level_event"];
    let splash: Vec<(u64, &(i32, DVec3, i32))> = ticks
        .iter()
        .enumerate()
        .flat_map(|(i, seen)| seen.events.iter().map(move |e| (i as u64, e)))
        .collect();
    assert_eq!(splash.len(), 1, "one level event");
    let (hit_tick, (event_id, position, data)) = splash[0];
    assert_eq!(hit_tick, batch(&event["batch"]));
    assert_eq!(i64::from(*event_id), event["event_id"].as_i64().unwrap());
    assert_eq!(i64::from(*data), event["data"].as_i64().unwrap());
    assert!(close(*position, at + DVec3::Y * above(event)), "{position}");
    assert!(ticks[hit_tick as usize].removed, "removed on the hit tick");

    let glass = sound_at("glass");
    assert_eq!(glass.len(), 1);
    let (tick, (_, position, identifier, id, data)) = glass[0];
    assert_eq!(tick, hit_tick + 1, "one batch after the splash event");
    assert_eq!(
        tick,
        batch(&row["glass"]["batch"]),
        "the batch BDS sends it in"
    );
    assert!(close(*position, at + DVec3::Y * above(&row["glass"])));
    assert_eq!(
        identifier,
        row["glass"]["actor_identifier"].as_str().unwrap()
    );
    assert_eq!(*id, row["glass"]["unique_id"].as_i64().unwrap());
    assert_eq!(i64::from(*data), row["glass"]["data"].as_i64().unwrap());

    // The orbs go out with the removal, at the hit point moved by their
    // first tick of flight (BDS sends them a tick later, unmoved, at
    // `orb_y_above_floor`).
    let orbs = &ticks[hit_tick as usize].orbs;
    let total: i32 = orbs.iter().map(|(_, value)| value).sum();
    assert!((3..=10).contains(&total), "{total}");
    let mut values: Vec<u32> = orbs.iter().map(|(_, v)| *v as u32).collect();
    values.sort_unstable();
    let mut split: Vec<u32> = crate::entity::xp_orb::orb_values(total as u32).collect();
    split.sort_unstable();
    assert_eq!(values, split);
    assert_eq!(row["orb_y_above_floor"].as_f64(), Some(0.0));
    for (position, _) in orbs {
        let moved = *position - at;
        assert!(
            moved.x.abs() < 0.2 && moved.z.abs() < 0.2 && (0.0..0.4).contains(&moved.y),
            "{moved}"
        );
    }
    assert_eq!(
        ticks.iter().map(|seen| seen.orbs.len()).sum::<usize>(),
        orbs.len()
    );
}

/// `projectile` (`on_hit.grant_xp`): 100 bottles grant every amount from
/// `minXP` (3) up to but not including `maxXP` (11), as the 30 native
/// bottles (3 to 10) do, split into orbs the way BDS splits them.
#[test]
fn xp_bottles_grant_min_to_max_exclusive_experience() {
    let fixture = fixture();
    let native: Vec<u64> = fixture["bottles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["total"].as_u64().unwrap())
        .collect();
    assert!(native.iter().all(|total| (3..=10).contains(total)));
    for row in fixture["bottles"].as_array().unwrap() {
        // The same orbs, in whatever order BDS sends them.
        let mut orbs: Vec<u32> = serde_json::from_value(row["orbs"].clone()).unwrap();
        orbs.sort_unstable();
        let mut split: Vec<u32> = crate::entity::xp_orb::orb_values(orbs.iter().sum()).collect();
        split.sort_unstable();
        assert_eq!(orbs, split, "native split");
    }

    let mut a = floor(13, Difficulty::Normal, GameMode::Creative);
    // Out of the bottles' way.
    let player = a.player;
    a.world().get_mut::<Position>(player).unwrap().0 =
        DVec3::new(25.5, SURFACE + crate::entity::PLAYER_EYE_HEIGHT, 25.5);
    let cells: Vec<DVec3> = (0..10)
        .flat_map(|i| {
            (0..10).map(move |j| {
                DVec3::new(-13.5 + 3.0 * f64::from(i), 0.0, -13.5 + 3.0 * f64::from(j))
            })
        })
        .collect();
    for cell in &cells {
        bottle(&mut a, *cell + DVec3::Y * (SURFACE + 6.0));
    }
    a.ticks(18);
    let orbs = orb_values(a.world());
    let mut totals = vec![0u32; cells.len()];
    for (position, value) in orbs {
        let (cell, _) = cells
            .iter()
            .enumerate()
            .map(|(i, c)| (i, (position - *c).with_y(0.0).length()))
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .unwrap();
        totals[cell] += value;
    }
    let mut seen = totals.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen, (3..=10).collect::<Vec<u32>>(), "{totals:?}");
}
