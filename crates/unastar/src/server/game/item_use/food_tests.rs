//! Every vanilla food against BDS 1.26.51.
//!
//! `testdata/bds-foods.json` is what eating each food did on BDS
//! (`tools/bds-sweep/food_table.py` over a capture of
//! `tools/bds-sweep/scripts/foods.txt` played against plain BDS 1.26.51 at
//! easy difficulty, 2026-11-11; suspicious stew for each data value 0-13).
//! `testdata/bds-foods-2.json` (`scripts/foods-2.txt`) eats the chance foods
//! again (both outcomes of each), dried kelp and the Saturation stew from a
//! low food level.
//! Each food is eaten here, at full health, from BDS's food level,
//! saturation and exhaustion at the start of the tick its eat finished in
//! (natural regeneration while BDS's player ate is in them; Poison on for the
//! honey bottle, as the script did), and the replies are compared: the food
//! level and saturation after it, the effects added and removed (an effect
//! the food gave running out is not one), `CompletedUsingItem`, the level
//! sounds and what hotbar slot 0 holds afterwards. Where the food gave
//! Saturation, the food level and saturation at the script's closing
//! `/effect @s clear` too.
//!
//! An effect a food gives only by chance (raw chicken, rotten flesh,
//! poisonous potato) is not compared with the one roll BDS made: on each side
//! it must be there with the food document's values or not there
//! (`docs/solutions/testing/replays-compare-random-rolls-with-measured-outcomes.md`).

use serde::Deserialize;

use jolyne::valentine::McpePacketData;
use jolyne::valentine::types::EnumsMobEffectPacketPayloadEvent;

use super::{ItemUseQueue, UseRequest};
use crate::entity::components::{ActiveEffect, EffectType, Hunger, MainInventory};
use crate::server::game::capture_replay::Replay;
use crate::server::game::effects;

/// The Saturation effect's id.
const SATURATION: i32 = 23;

const BDS: [&str; 2] = [
    include_str!("testdata/bds-foods.json"),
    include_str!("testdata/bds-foods-2.json"),
];

#[derive(Deserialize)]
struct Table {
    foods: Vec<Food>,
}

#[derive(Deserialize)]
struct Food {
    item: String,
    data: i16,
    /// `None` where BDS did not let it be eaten (the food level was full).
    before: Option<Before>,
    after: Option<After>,
    /// At the script's closing `/effect @s clear`.
    end: Option<After>,
    added: Vec<(i32, i32, i32, bool)>,
    removed: Vec<i32>,
    completed: Option<(i32, i32)>,
    slot: Option<i32>,
    sounds: Vec<String>,
}

#[derive(Deserialize)]
struct Before {
    hunger: f32,
    saturation: f32,
    exhaustion: f32,
}

#[derive(Deserialize, Debug, PartialEq)]
struct After {
    hunger: f32,
    saturation: f32,
}

/// What eating one food did, as compared.
#[derive(Debug, Default, PartialEq)]
struct Outcome {
    hunger: i32,
    /// Saturation in thousandths (BDS sends f32 sums such as 7.2000003).
    saturation: i32,
    /// Food level and saturation once the effects the food gave have acted
    /// (compared where it gave Saturation).
    end: Option<(i32, i32)>,
    added: Vec<(i32, i32, i32, bool)>,
    removed: Vec<i32>,
    completed: Option<(i32, i32)>,
    slot: i32,
    sounds: Vec<String>,
}

fn thousandths(value: f32) -> i32 {
    (value * 1000.0).round() as i32
}

/// The effects `item` gives, with the chance of each (its food document).
fn food_effects(item: &str) -> Vec<(i32, i32, i32, f32)> {
    let stack = crate::item::test_support::stack(&format!("minecraft:{item}"), 1);
    let Some(food) = stack
        .item()
        .and_then(|entry| entry.components().food.clone())
    else {
        return Vec::new();
    };
    food.effects
        .iter()
        .filter_map(|effect| {
            let id = effects::effect_by_name(&effect.name)?;
            Some((
                i32::from(id.0),
                effect.amplifier,
                effect.duration * 20,
                effect.chance,
            ))
        })
        .collect()
}

/// Takes the food's chance effects out of `outcome`; wrong where one is
/// there with other values than the food document gives it.
fn take_chance_effects(outcome: &mut Outcome, item: &str) -> Vec<String> {
    let chance: Vec<(i32, i32, i32)> = food_effects(item)
        .into_iter()
        .filter(|(.., chance)| *chance < 1.0)
        .map(|(id, amplifier, duration, _)| (id, amplifier, duration))
        .collect();
    let mut wrong = Vec::new();
    outcome.added.retain(|(id, amplifier, duration, _)| {
        let Some(documented) = chance.iter().find(|(chance_id, ..)| chance_id == id) else {
            return true;
        };
        if *documented != (*id, *amplifier, *duration) {
            wrong.push(format!(
                "chance effect {id} {amplifier} {duration}, documented {documented:?}"
            ));
        }
        false
    });
    outcome
        .removed
        .retain(|id| !chance.iter().any(|(chance_id, ..)| chance_id == id));
    wrong
}

fn eat(h: &mut Replay, index: usize, food: &Food) -> Outcome {
    let player = h.player;
    h.clear();
    let stack = crate::item::test_support::stack(&format!("minecraft:{}", food.item), 1)
        .with_damage(food.data);
    h.put(0, stack);
    h.select(0);
    effects::clear_effects(h.world(), player);
    if food.item == "honey_bottle" {
        // `/effect @s poison 20 0 true`.
        let mut poison = ActiveEffect::new(0, 400);
        poison.show_particles = false;
        effects::add_effect_with(h.world(), player, EffectType::POISON, poison);
    }
    // Full health: BDS's values at the tick the eat completed already hold
    // what natural regeneration took while eating.
    if let Some(mut health) = h
        .world()
        .get_mut::<crate::entity::components::Health>(player)
    {
        health.current = health.max;
    }
    let before = food.before.as_ref().expect("an eaten food");
    *h.world().get_mut::<Hunger>(player).unwrap() = Hunger {
        food_level: before.hunger as i32,
        saturation: before.saturation,
        exhaustion: before.exhaustion,
    };
    if food.item == "chorus_fruit" {
        // BDS's superflat ground reaches wherever the fruit sends the player.
        for x in -12..=12 {
            for z in -12..=12 {
                h.set(glam::IVec3::new(x, -61, z), "minecraft:stone");
            }
        }
    }
    h.world()
        .insert_resource(crate::entity::EntityRandom::seeded(index as u64));
    h.tick();
    h.drain();
    h.world()
        .resource_mut::<ItemUseQueue>()
        .0
        .push((player, UseRequest::UseAir));
    let mut outcome = Outcome::default();
    for _ in 0..48 {
        h.tick();
        for packet in h.drain() {
            match packet {
                McpePacketData::MobEffectPacket(effect) => match effect.event_id {
                    EnumsMobEffectPacketPayloadEvent::Add => outcome.added.push((
                        effect.effect_id,
                        effect.effect_amplifier,
                        effect.effect_duration_ticks,
                        effect.show_particles,
                    )),
                    EnumsMobEffectPacketPayloadEvent::Remove => {
                        outcome.removed.push(effect.effect_id)
                    }
                    _ => {}
                },
                McpePacketData::CompletedUsingItemPacket(completed) => {
                    outcome.completed =
                        Some((i32::from(completed.item_id), completed.item_use_method));
                    // The food level and saturation as the eat left them.
                    let hunger = h.world().get::<Hunger>(player).unwrap();
                    (outcome.hunger, outcome.saturation) =
                        (hunger.food_level, thousandths(hunger.saturation));
                }
                McpePacketData::LevelSoundEventPacket(sound) if sound.sound_event != "pop" => {
                    outcome.sounds.push(sound.sound_event.to_string());
                }
                _ => {}
            }
        }
    }
    if food.added.iter().any(|(id, ..)| *id == SATURATION) {
        let hunger = h.world().get::<Hunger>(player).unwrap();
        outcome.end = Some((hunger.food_level, thousandths(hunger.saturation)));
    }
    let held = h
        .world()
        .get::<MainInventory>(player)
        .unwrap()
        .hotbar(0)
        .cloned();
    outcome.slot = held
        .filter(|stack| !stack.is_empty())
        .and_then(|stack| stack.item().map(|entry| entry.network_id()))
        .unwrap_or(0);
    outcome
}

#[test]
fn every_food_answers_like_bds() {
    let foods: Vec<Food> = BDS
        .iter()
        .flat_map(|table| serde_json::from_str::<Table>(table).unwrap().foods)
        .collect();
    let mut h = Replay::with_difficulty(crate::world::rules::Difficulty::Easy);
    let mut wrong = Vec::new();
    let mut compared = 0;
    for (index, food) in foods.iter().enumerate() {
        let name = format!("{} {}", food.item, food.data);
        let Some(after) = &food.after else {
            continue;
        };
        compared += 1;
        let mut ours = eat(&mut h, index, food);
        let mut bds = Outcome {
            hunger: after.hunger as i32,
            saturation: thousandths(after.saturation),
            end: food
                .end
                .as_ref()
                .filter(|_| food.added.iter().any(|(id, ..)| *id == SATURATION))
                .map(|end| (end.hunger as i32, thousandths(end.saturation))),
            added: food.added.clone(),
            removed: food.removed.clone(),
            completed: food.completed,
            slot: food.slot.unwrap_or(0),
            sounds: food.sounds.clone(),
        };
        // Saturation's first pulse lands in the tick the eat finished or the
        // next one (both seen on BDS), so for those foods the food level is
        // compared once the pulses are done (`end`).
        if food.added.iter().any(|(id, ..)| *id == SATURATION) {
            ours.hunger = 0;
            bds.hunger = 0;
        }
        // A chance effect: on each side there with the documented values or
        // not there, then compared without it. An effect the food added
        // running out is not the food's removal (BDS's window may end first).
        for outcome in [&mut ours, &mut bds] {
            let added: Vec<i32> = outcome.added.iter().map(|(id, ..)| *id).collect();
            outcome.removed.retain(|id| !added.contains(id));
        }
        for (who, outcome) in [("BDS", &mut bds), ("ours", &mut ours)] {
            for problem in take_chance_effects(outcome, &food.item) {
                wrong.push(format!("{name}, {who}: {problem}"));
            }
        }
        if ours != bds {
            wrong.push(format!("{name}:\n  BDS  {bds:?}\n  ours {ours:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert!(compared >= 69, "{compared} foods compared");
}
