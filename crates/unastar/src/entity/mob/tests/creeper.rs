//! The creeper's `minecraft:target_nearby_sensor` replayed against BDS
//! 1.26.51 (`mob/ai/testdata/bds-1.26.51-creeper.json`, distilled by
//! `tools/bds-sweep/creeper_fixture.py` from `sweep.py creeper`).
//!
//! The creeper itself is not spawnable yet (it needs `minecraft:explode`
//! and `behavior.melee_attack`), so the sensor's decision is replayed on the
//! creeper's and the player's positions BDS sampled every tick: the fuse
//! lights on the tick the target comes inside the range and goes out on the
//! tick it leaves past the outside range.

use glam::DVec3;
use serde::Deserialize;
use unastar_data::actor_components::body;

use crate::entity::mob::components::sensors::compile_target_nearby;
use crate::entity::mob::components::{TargetNearbySensorConfig, TargetZone};
use crate::registry::collision::Aabb;

/// The creeper's and the player's collision box, both 0.6 × 1.8.
const WIDTH: f64 = 0.6;
const HEIGHT: f64 = 1.8;

#[derive(Debug, Deserialize)]
struct Fixture {
    items: Vec<Item>,
}

#[derive(Debug, Deserialize)]
struct Item {
    key: String,
    target: bool,
    actions: Vec<Action>,
    ticks: Vec<Sample>,
    events: Vec<Event>,
}

#[derive(Debug, Deserialize)]
struct Action {
    order: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Sample {
    t: u32,
    creeper: Option<[f64; 3]>,
    player: [f64; 3],
}

#[derive(Debug, Deserialize)]
struct Event {
    t: u32,
    event: String,
    #[serde(default)]
    set: Option<bool>,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("../ai/testdata/bds-1.26.51-creeper.json"))
        .expect("creeper fixture decodes")
}

/// The vanilla creeper's sensor, compiled from its definition.
fn creeper_sensor() -> TargetNearbySensorConfig {
    let creeper = unastar_data::actor_definitions::vanilla()
        .iter()
        .find(|definition| definition.identifier() == "minecraft:creeper")
        .expect("vanilla creeper");
    let json = &creeper.components["minecraft:target_nearby_sensor"];
    let body: body::TargetNearbySensor =
        unastar_data::actor_components::decode(json).expect("sensor body decodes");
    compile_target_nearby(body).expect("the creeper's sensor compiles")
}

fn gap(creeper: [f64; 3], player: [f64; 3]) -> f64 {
    let a = Aabb::from_feet(DVec3::from_array(creeper), WIDTH, HEIGHT);
    let b = Aabb::from_feet(DVec3::from_array(player), WIDTH, HEIGHT);
    (a.min - b.max).max(b.min - a.max).max(DVec3::ZERO).length()
}

/// The ticks BDS lit (`true`) and put out (`false`) the creeper's fuse.
fn bds_fuse(item: &Item) -> Vec<(u32, bool)> {
    item.events
        .iter()
        .filter(|event| event.event == "ignited")
        .map(|event| (event.t, event.set.unwrap_or(false)))
        .collect()
}

/// The ticks our sensor fires `start_exploding` (`true`) and
/// `stop_exploding` (`false`), from the sampled positions, with the target
/// in sight. Positions sampled on tick `t` are those the sensor reads that
/// tick; it reads the target from the tick after the creeper took it (BDS:
/// a creeper 2 blocks from the player lit one tick after its `TARGET`).
fn our_fuse(sensor: &TargetNearbySensorConfig, item: &Item, until: u32) -> Vec<(u32, bool)> {
    let targeted = item
        .events
        .iter()
        .find(|event| event.event == "target" && event.set == Some(true))
        .map_or(u32::MAX, |event| event.t);
    let mut previous: Option<TargetZone> = None;
    let mut out = Vec::new();
    for sample in item.ticks.iter().filter(|s| s.t > targeted && s.t < until) {
        let Some(creeper) = sample.creeper else {
            break;
        };
        let zone = sensor.zone(gap(creeper, sample.player), true);
        if let Some(event) = sensor.crossing(previous, zone) {
            let lit = sensor.on_inside_range.as_ref() == Some(event);
            // The first crossing (from no target) is the target being taken.
            if previous.is_some() || lit {
                out.push((sample.t, lit));
            }
        }
        previous = Some(zone);
    }
    out
}

#[test]
fn the_creeper_sensor_lights_and_puts_out_the_fuse_on_bds_ticks() {
    let sensor = creeper_sensor();
    assert_eq!((sensor.inside_range, sensor.outside_range), (2.5, 6.0));
    let fixture = fixture();
    let mut replayed = 0;
    // Items lit with flint and steel light without the sensor.
    for item in fixture
        .items
        .iter()
        .filter(|item| item.target && item.actions.iter().all(|a| a.order.is_none()))
    {
        // The lost-sight item puts a wall up, which these positions do not
        // carry; it is checked below.
        if item.key == "sight" {
            continue;
        }
        let bds = bds_fuse(item);
        let &(lit, true) = bds.first().expect("every sensor item lights") else {
            panic!("{}: put out before it lit", item.key);
        };
        // BDS's fuse runs until the explosion; replay up to it.
        let until = item
            .events
            .iter()
            .find(|event| event.event == "explosion")
            .map_or(u32::MAX, |event| event.t);
        let ours = our_fuse(&sensor, item, until);
        assert_eq!(ours, bds, "{}: first lit on {lit}", item.key);
        replayed += 1;
    }
    assert!(replayed >= 6, "{replayed} items replayed");
}

#[test]
fn losing_sight_inside_the_range_puts_the_fuse_out() {
    let sensor = creeper_sensor();
    let fixture = fixture();
    let item = fixture
        .items
        .iter()
        .find(|item| item.key == "sight")
        .expect("the sight item");
    // BDS: lit as the target is taken, out the tick after the wall went up
    // (on tick 15).
    assert_eq!(bds_fuse(item), [(1, true), (16, false)]);
    let inside = sensor.zone(0.5, true);
    assert_eq!(inside, TargetZone::Inside);
    let hidden = sensor.zone(0.5, false);
    assert_eq!(
        sensor.crossing(Some(inside), hidden),
        sensor.on_vision_lost_inside_range.as_ref()
    );
    assert!(sensor.on_vision_lost_inside_range.is_some());
    // Seen again inside the range, it lights again.
    assert_eq!(
        sensor.crossing(Some(hidden), inside),
        sensor.on_inside_range.as_ref()
    );
}

#[test]
fn the_forced_groups_switch_the_sensor_off() {
    let creeper = unastar_data::actor_definitions::vanilla()
        .iter()
        .find(|definition| definition.identifier() == "minecraft:creeper")
        .expect("vanilla creeper");
    let json =
        &creeper.component_groups["minecraft:forced_exploding"]["minecraft:target_nearby_sensor"];
    let body: body::TargetNearbySensor =
        unastar_data::actor_components::decode(json).expect("sensor body decodes");
    let sensor = compile_target_nearby(body).unwrap();
    assert!(sensor.on_inside_range.is_none());
    assert!(sensor.on_outside_range.is_none());
    assert!(sensor.on_vision_lost_inside_range.is_none());
}
