//! Game rules changed with `/gamerule` act on the next tick, as on BDS
//! 1.26.51 (`../testdata/bds-1.26.51-player-data.json`, phases
//! `regeneration_toggle`, `drowning_damage_off`, `fire_damage_off`,
//! `damage_command_rules`).

use jolyne::valentine::types::{DataItemEntryPayload, GameRuleRuleValue};

use super::*;
use crate::command::{CommandSource, execute};
use crate::entity::components::Hunger;

/// Run `line` as the server console does.
fn console(h: &mut Harness, line: &str) {
    let output = execute(h.world(), CommandSource::server(), line);
    assert!(output.success_count > 0, "{line}: {output:?}");
}

fn fixture() -> &'static serde_json::Value {
    static FIXTURE: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-player-data.json")).unwrap()
    })
}

/// The packets BDS sent in `phase` (see `actor_data::decode`).
fn bds(phase: &str) -> Vec<McpePacketData> {
    fixture()["phases"][phase]["packets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hex| {
            let hex = hex.as_str().unwrap();
            let bytes: Vec<u8> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect();
            let mut framed = Vec::with_capacity(bytes.len() + 5);
            let mut length = bytes.len() as u32;
            while length >= 0x80 {
                framed.push(length as u8 | 0x80);
                length >>= 7;
            }
            framed.push(length as u8);
            framed.extend_from_slice(&bytes);
            McpePacketData::decode_inner_bytes(&mut bytes::Bytes::from(framed), McpePacketArgs)
                .unwrap()
                .1
        })
        .collect()
}

/// What a phase shows of the rules and the player: rule changes, hurt
/// events, health and air updates and the `ONFIRE` flag, in order.
#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Rule(String, bool),
    Hurt(i32),
    Health(f32),
    Air(i16),
    OnFire(bool),
}

fn seen(packets: &[McpePacketData], runtime_id: u64) -> Vec<Seen> {
    let mut out = Vec::new();
    for packet in packets {
        match packet {
            McpePacketData::GameRulesChangedPacket(rules) => {
                for rule in &rules.rule_data.rules_list {
                    if let GameRuleRuleValue::Bool(value) = rule.rule_value {
                        out.push(Seen::Rule(rule.rule_name.to_string(), value));
                    }
                }
            }
            McpePacketData::ActorEventPacket(event)
                if event.target_runtime_id.actor_runtime_id == runtime_id
                    && event.event_id == EnumsActorEvent::Hurt =>
            {
                out.push(Seen::Hurt(event.data));
            }
            McpePacketData::UpdateAttributesPacket(update)
                if update.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                out.extend(
                    update
                        .attribute_list
                        .iter()
                        .filter(|attribute| &*attribute.name == "minecraft:health")
                        .map(|attribute| Seen::Health(attribute.current_value)),
                );
            }
            McpePacketData::SetActorDataPacket(data)
                if data.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                for row in &data.actor_data.data {
                    match &row.payload {
                        DataItemEntryPayload::DataItemShortPayload(air) if row.id == 7 => {
                            out.push(Seen::Air(air.value));
                        }
                        DataItemEntryPayload::DataItemInt64Payload(flags) if row.id == 0 => {
                            out.push(Seen::OnFire(flags.value & 1 != 0));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The index of the first rule change of `name` to `value`.
fn rule_at(seen: &[Seen], name: &str, value: bool) -> usize {
    seen.iter()
        .position(|seen| *seen == Seen::Rule(name.into(), value))
        .unwrap_or_else(|| panic!("no {name} = {value}"))
}

/// `naturalregeneration` read every tick: food 20 heals 1 every 80 ticks,
/// nothing while the rule is off, and the first heal comes 80 ticks after
/// it is back on (the timer starts again).
#[test]
fn natural_regeneration_follows_the_live_game_rule() {
    // BDS: heals 10 → 11 → 12, rule off, rule on, then 13, 14, 15.
    let bds = seen(&bds("regeneration_toggle"), 1);
    let off = rule_at(&bds, "naturalRegeneration", false);
    let on = rule_at(&bds, "naturalRegeneration", true);
    let heals = |seen: &[Seen]| -> Vec<f32> {
        seen.iter()
            .filter_map(|seen| match seen {
                Seen::Health(health) => Some(*health),
                _ => None,
            })
            .collect()
    };
    assert_eq!(heals(&bds[..off]), [10.0, 11.0, 12.0]);
    assert!(heals(&bds[off..on]).is_empty());
    assert_eq!(heals(&bds[on..])[..3], [13.0, 14.0, 15.0]);

    let mut h = harness(Difficulty::Easy);
    let player = h.player;
    h.set_health(10.0);
    let heals_after = |h: &mut Harness, ticks: usize| {
        let mut at = Vec::new();
        for tick in 1..=ticks {
            h.tick();
            if !health_updates(&h.drain()).is_empty() {
                at.push(tick);
            }
        }
        at
    };
    // `set_health` ran the damage tick: the heals land 80 and 160 ticks
    // after it.
    assert_eq!(heals_after(&mut h, 170), [79, 159]);
    console(&mut h, "gamerule naturalregeneration false");
    assert!(heals_after(&mut h, 200).is_empty());
    console(&mut h, "gamerule naturalregeneration true");
    assert_eq!(heals_after(&mut h, 170), [80, 160]);
    assert_eq!(h.world().get::<Health>(player).unwrap().current, 14.0);
}

/// `drowningdamage false`: the air still cycles from -19 back to 0 every
/// 20 ticks, without hits; `true` again: the next cycle hits.
#[test]
fn drowning_damage_off_keeps_the_air_cycle_without_hits() {
    let bds = seen(&bds("drowning_damage_off"), 1);
    let on = rule_at(&bds, "drowningDamage", true);
    let resets = |seen: &[Seen]| {
        seen.windows(2)
            .filter(|pair| pair[0] == Seen::Air(-19) && pair[1] == Seen::Air(0))
            .count()
    };
    assert!(resets(&bds[..on]) >= 3);
    assert!(!bds[..on].iter().any(|seen| matches!(seen, Seen::Hurt(_))));
    assert_eq!(
        bds[on..].iter().find(|seen| matches!(seen, Seen::Hurt(_))),
        Some(&Seen::Hurt(9))
    );

    let mut h = harness(Difficulty::Easy);
    h.ticks(5);
    h.drain();
    console(&mut h, "gamerule drowningdamage false");
    let feet = HOME.floor().as_ivec3();
    for pos in [feet, feet + IVec3::Y] {
        h.set_block(pos, *blocks::WATER);
    }
    h.ticks(380);
    let off = seen(&h.drain(), SESSION);
    assert!(resets(&off) >= 3);
    assert!(!off.iter().any(|seen| matches!(seen, Seen::Hurt(_))));
    assert_eq!(h.health(), 20.0);
    console(&mut h, "gamerule drowningdamage true");
    h.ticks(20);
    assert!(seen(&h.drain(), SESSION).contains(&Seen::Hurt(9)));
    assert_eq!(h.health(), 18.0);
}

/// `firedamage false`: a fire block sets the player alight (`ONFIRE`) but
/// hurts nothing; `true` while it still burns: the burning hurts again.
#[test]
fn fire_damage_off_burns_without_damage() {
    let bds = seen(&bds("fire_damage_off"), 1);
    let on = rule_at(&bds, "fireDamage", true);
    assert!(bds[..on].contains(&Seen::OnFire(true)));
    assert!(!bds[..on].iter().any(|seen| matches!(seen, Seen::Hurt(_))));
    assert!(bds[on..].contains(&Seen::Hurt(7)));

    let mut h = harness(Difficulty::Easy);
    h.ticks(5);
    h.drain();
    console(&mut h, "gamerule firedamage false");
    let feet = HOME.floor().as_ivec3();
    let fire = crate::world::chunk::blocks::get_block_id("minecraft:fire");
    h.set_block(feet, fire);
    h.ticks(40);
    h.set_block(feet, *blocks::AIR);
    h.ticks(5);
    let off = seen(&h.drain(), SESSION);
    assert!(off.contains(&Seen::OnFire(true)));
    assert!(!off.iter().any(|seen| matches!(seen, Seen::Hurt(_))));
    assert_eq!(h.health(), 20.0);
    console(&mut h, "gamerule firedamage true");
    h.ticks(40);
    assert!(
        seen(&h.drain(), SESSION)
            .iter()
            .any(|seen| matches!(seen, Seen::Hurt(_)))
    );
    assert!(h.health() < 20.0);
}

/// Each damage rule refuses its cause from any source while it is off
/// (BDS: `/damage` of fall, drowning, fire and freezing damage fails,
/// then succeeds once the rule is back on).
#[test]
fn damage_rules_refuse_their_causes() {
    let console_lines: Vec<String> = fixture()["phases"]["damage_command_rules"]["console"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap().to_owned())
        .collect();
    let mut h = harness(Difficulty::Easy);
    let player = h.player;
    for (rule, cause) in [
        ("falldamage", ActorDamageCause::Fall),
        ("drowningdamage", ActorDamageCause::Drowning),
        ("firedamage", ActorDamageCause::Fire),
        ("freezedamage", ActorDamageCause::Freezing),
    ] {
        let at = console_lines
            .iter()
            .position(|line| line == &format!("Game rule {rule} has been updated to false"))
            .unwrap();
        assert_eq!(console_lines[at + 1], "Could not apply damage to Subject");
        assert_eq!(console_lines[at + 3], "Applied damage to Subject");
        let hit = Hit {
            damage: 1.0,
            cause,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: KnockbackPower::DEFAULT,
        };
        console(&mut h, &format!("gamerule {rule} false"));
        assert_eq!(
            super::super::hurt_player(h.world(), player, &hit),
            0.0,
            "{rule}"
        );
        console(&mut h, &format!("gamerule {rule} true"));
        assert_eq!(
            super::super::hurt_player(h.world(), player, &hit),
            1.0,
            "{rule}"
        );
        h.ticks(20);
    }
}

/// `keepinventory`, `showdeathmessages` and `doimmediaterespawn` changed by
/// `/gamerule` apply to the next death.
#[test]
fn death_rules_apply_from_the_next_death() {
    let mut h = harness(Difficulty::Easy);
    let player = h.player;
    h.world()
        .get_mut::<crate::entity::components::MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, crate::item::test_support::stack("minecraft:diamond", 3))
        .unwrap();
    console(&mut h, "gamerule keepinventory true");
    console(&mut h, "gamerule showdeathmessages false");
    console(&mut h, "gamerule doimmediaterespawn true");
    h.drain();
    // Respawned at once, so `kill_player` finds it alive again.
    assert!(!super::super::kill_player(h.world(), player));
    let packets = h.drain();
    assert!(h.drops().is_empty(), "keepinventory keeps the diamonds");
    assert!(translations(&packets).is_empty(), "no death message");
    assert_eq!(death_infos(&packets).len(), 1);
    assert!(!h.dead(), "respawned at once");
    assert_eq!(
        h.world()
            .get::<Hunger>(player)
            .map(|hunger| hunger.food_level),
        Some(20)
    );
}
