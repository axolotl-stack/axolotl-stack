//! The packet-order model against the BDS 1.26.51 PvP race captures.
//!
//! `testdata/pvp-race-*.jsonl` are cut (`tools/bds-sweep/pvp_race_fixture.py`) from
//! `order_probe` captures (`tools/bds-sweep/pvp_race.py`, 2026-11-11): three scripted
//! players in one process against plain BDS, so the order of packets within a client
//! tick is the script's. The first line of each file holds every variant's outcome over
//! all its trials; the rest are the packets of one trial per variant.
//!
//! The tests replay the client packets through [`PlayerQueue`], one network pass per
//! client tick (a client tick's packets left within a millisecond of each other), and
//! compare what the model predicts with what BDS sent: when a hit resolved, and which
//! sprint, fall and facing state it used.

use std::collections::{BTreeMap, HashMap};

use jolyne::valentine::types::{
    EnumsItemUseOnActorInventoryTransactionActionType, InventoryTransactionPacketTransaction,
};
use jolyne::valentine::{EnumsPlayerAuthInputPacketPayloadInputData as Flag, McpePacketData};

use super::*;
use crate::server::game::inventory::capture::decode;

const QUEUE: &str = include_str!("testdata/pvp-race-queue.jsonl");
const SPRINT: &str = include_str!("testdata/pvp-race-sprint.jsonl");
const CRIT: &str = include_str!("testdata/pvp-race-crit.jsonl");
const MODEL_STRICT: &str = include_str!("testdata/pvp-race-model-strict.jsonl");

/// A client packet of a fixture trial.
pub(super) struct Sent {
    pub(super) rel: i64,
    pub(super) player: String,
    pub(super) packet: McpePacketData,
}

pub(super) struct Fixture {
    pub(super) header: serde_json::Value,
    /// Client packets by variant, in the order they were sent.
    pub(super) sent: BTreeMap<String, Vec<Sent>>,
}

pub(super) fn fixture(text: &str) -> Fixture {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    let mut sent: BTreeMap<String, Vec<Sent>> = BTreeMap::new();
    for line in lines {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        if row["dir"] != "c2s" {
            continue;
        }
        sent.entry(row["variant"].as_str().unwrap().to_owned())
            .or_default()
            .push(Sent {
                rel: row["rel"].as_i64().unwrap(),
                player: row["player"].as_str().unwrap().to_owned(),
                packet: decode(row["hex"].as_str().unwrap()),
            });
    }
    Fixture { header, sent }
}

impl Fixture {
    /// The events of a variant's most frequent outcome over all trials.
    pub(super) fn majority(&self, variant: &str) -> Vec<String> {
        self.header["outcomes"][variant][0]["events"]
            .as_array()
            .unwrap_or_else(|| panic!("no outcome for {variant}"))
            .iter()
            .map(|event| event.as_str().unwrap().to_owned())
            .collect()
    }

    /// The client tick, relative to the race, at which the majority outcome's first
    /// hurt event reached the bystander; `None` when no hit landed.
    fn hurt_tick(&self, variant: &str) -> Option<i64> {
        self.majority(variant)
            .iter()
            .find(|event| event.contains(" hurt B "))
            .map(|event| event.split_whitespace().next().unwrap().parse().unwrap())
    }

    /// The majority outcome's knockback on B, if any.
    fn knockback(&self, variant: &str) -> Option<[f64; 3]> {
        let events = self.majority(variant);
        let event = events.iter().find(|event| event.contains(" motion B "))?;
        let inside = &event[event.find('[')? + 1..event.find(']')?];
        let values: Vec<f64> = inside
            .split(',')
            .map(|v| v.trim().parse().unwrap())
            .collect();
        Some([values[0], values[1], values[2]])
    }
}

fn is_attack(packet: &McpePacketData) -> bool {
    matches!(
        packet,
        McpePacketData::InventoryTransactionPacket(transaction)
            if matches!(
                &transaction.transaction,
                InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(on_actor)
                    if on_actor.action_type
                        == EnumsItemUseOnActorInventoryTransactionActionType::Attack
            )
    )
}

fn timing_of(packet: &McpePacketData) -> Timing {
    timing(packet.packet_id(), is_attack(packet))
}

/// One resolved attack: the network pass that resolved it and the inputs the attacker
/// had applied before it, oldest first.
struct Resolved {
    pass: i64,
    before: Vec<McpePacketData>,
}

/// Replays a variant's client packets: every attack `attacker` sent, as the model
/// resolves it.
fn resolve(sent: &[Sent], attacker: &str) -> Vec<Resolved> {
    let mut queue = PlayerQueue::default();
    let mut applied: Vec<McpePacketData> = Vec::new();
    let mut resolved = Vec::new();
    let mut passes: BTreeMap<i64, Vec<&Sent>> = BTreeMap::new();
    for packet in sent.iter().filter(|packet| packet.player == attacker) {
        passes.entry(packet.rel).or_default().push(packet);
    }
    for (pass, packets) in passes {
        for packet in packets {
            let _ = queue.arrive(timing_of(&packet.packet), &packet.packet);
        }
        let consumed: Vec<_> = queue.drain_inputs().collect();
        for input in consumed {
            for action in &input.held {
                if is_attack(action) {
                    resolved.push(Resolved {
                        pass,
                        before: applied.clone(),
                    });
                }
            }
            applied.push(input.input.clone().clone());
        }
    }
    resolved
}

fn flags(input: &McpePacketData) -> &[Flag] {
    match input {
        McpePacketData::PlayerAuthInputPacket(input) => &input.input_data,
        _ => &[],
    }
}

fn yaw(input: &McpePacketData) -> f32 {
    match input {
        McpePacketData::PlayerAuthInputPacket(input) => input.player_rotation.y,
        _ => panic!("not an input"),
    }
}

/// Whether the inputs leave the player sprinting: the last start or stop wins.
fn sprinting(inputs: &[McpePacketData]) -> bool {
    inputs
        .iter()
        .rev()
        .find_map(|input| {
            let flags = flags(input);
            if flags.contains(&Flag::Stopsprinting) {
                Some(false)
            } else if flags.contains(&Flag::Startsprinting) {
                Some(true)
            } else {
                None
            }
        })
        .unwrap_or(false)
}

#[test]
fn an_attack_resolves_with_its_players_next_input_like_bds() {
    let queue = fixture(QUEUE);
    let mut failures = Vec::new();
    for (variant, sent) in &queue.sent {
        let resolved = resolve(sent, "A");
        let Some(first) = resolved.first() else {
            failures.push(format!("{variant}: the model resolved no attack"));
            continue;
        };
        // A pass's flush reaches the client during the next client tick.
        let predicted = first.pass + 1;
        let bds = queue.hurt_tick(variant);
        if bds != Some(predicted) {
            failures.push(format!(
                "{variant}: BDS hurt at {bds:?}, model resolves at pass {} (+1 = {predicted})",
                first.pass
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_held_attack_uses_the_sprint_state_from_before_its_input_like_bds() {
    let sprint = fixture(SPRINT);
    let mut failures = Vec::new();
    let mut checked = 0;
    for (variant, sent) in &sprint.sent {
        if variant.contains("sneak") {
            continue;
        }
        let Some(first) = resolve(sent, "A").into_iter().next() else {
            failures.push(format!("{variant}: the model resolved no attack"));
            continue;
        };
        let Some([x, _, _]) = sprint.knockback(variant) else {
            failures.push(format!("{variant}: BDS sent no knockback"));
            continue;
        };
        // A sprint hit pushes 0.6 horizontally, a plain one 0.4 (BDS capture).
        let bds_sprint = x > 0.5;
        if sprinting(&first.before) != bds_sprint {
            failures.push(format!(
                "{variant}: BDS sprint knockback {bds_sprint}, model state {}",
                sprinting(&first.before)
            ));
        }
        checked += 1;
    }
    assert!(checked >= 10, "only {checked} sprint variants");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn crits_depend_on_the_inputs_applied_before_the_attack_in_both_orders_like_bds() {
    let crit = fixture(CRIT);
    // Inputs A applied after its teleport and before the attack, per order, for the
    // steps at which BDS answered with a critical hit.
    let mut crit_counts: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut plain_counts: HashMap<&str, Vec<usize>> = HashMap::new();
    for (variant, sent) in &crit.sent {
        let order = if variant.starts_with("input-then-hit") {
            "input-then-hit"
        } else {
            "hit-then-input"
        };
        let first = resolve(sent, "A")
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{variant}: no attack resolved"));
        let teleport = sent
            .iter()
            .position(|packet| {
                packet.player == "A"
                    && matches!(packet.packet, McpePacketData::CommandRequestPacket(_))
            })
            .unwrap_or_else(|| panic!("{variant}: no teleport"));
        let inputs_before_teleport = sent[..teleport]
            .iter()
            .filter(|packet| {
                packet.player == "A"
                    && matches!(packet.packet, McpePacketData::PlayerAuthInputPacket(_))
            })
            .count();
        let since = first.before.len() - inputs_before_teleport;
        let critical = crit
            .majority(variant)
            .iter()
            .any(|event| event.contains(" crit "));
        let counts = if critical {
            &mut crit_counts
        } else {
            &mut plain_counts
        };
        counts.entry(order).or_default().push(since);
    }
    for counts in [&mut crit_counts, &mut plain_counts] {
        for list in counts.values_mut() {
            list.sort_unstable();
        }
    }
    assert!(!crit_counts.is_empty(), "no critical hits in the capture");
    assert_eq!(
        crit_counts.get("input-then-hit"),
        crit_counts.get("hit-then-input"),
        "the same number of applied inputs should give a crit in both orders"
    );
}

#[test]
fn under_strict_interactions_a_held_attack_uses_the_facing_from_before_its_input_like_bds() {
    let strict = fixture(MODEL_STRICT);
    let mut checked = 0;
    for (variant, sent) in &strict.sent {
        if !variant.contains("face") {
            continue;
        }
        let first = resolve(sent, "A")
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{variant}: no attack resolved"));
        // Facing B is yaw -90 (east); facing away is 90.
        let facing_b = first.before.last().is_none_or(|input| yaw(input) < 0.0);
        let landed = strict.hurt_tick(variant).is_some();
        assert_eq!(
            landed, facing_b,
            "{variant}: BDS landed {landed}, model facing B {facing_b}"
        );
        checked += 1;
    }
    assert_eq!(checked, 4);
}

#[test]
fn held_actions_wait_for_the_next_input_and_inputs_queue_in_order() {
    let mut queue = PlayerQueue::default();
    assert_eq!(
        queue.arrive(Timing::OnArrival, "chat"),
        Arrived::Apply("chat")
    );
    assert_eq!(
        queue.arrive(Timing::WithNextInput, "attack"),
        Arrived::Queued
    );
    assert_eq!(queue.held(), ["attack"]);
    assert_eq!(queue.arrive(Timing::Input, "input 1"), Arrived::Queued);
    assert_eq!(queue.arrive(Timing::Input, "input 2"), Arrived::Queued);
    assert_eq!(
        queue.arrive(Timing::WithNextInput, "late attack"),
        Arrived::Queued
    );
    assert_eq!(queue.queued_inputs(), 2);
    let drained: Vec<_> = queue.drain_inputs().collect();
    assert_eq!(
        drained,
        [
            QueuedInput {
                held: vec!["attack"],
                input: "input 1"
            },
            QueuedInput {
                held: vec![],
                input: "input 2"
            }
        ]
    );
    // The attack after the last input stays held for the next one.
    assert_eq!(queue.held(), ["late attack"]);
}

const PAIRS: [&str; 4] = [
    include_str!("testdata/pvp-race-pairs-abc.jsonl"),
    include_str!("testdata/pvp-race-pairs-cba.jsonl"),
    include_str!("testdata/pvp-race-pairs-bca.jsonl"),
    include_str!("testdata/pvp-race-pairs-acb.jsonl"),
];

/// Every pair of three players at one health hit each other in one tick, in each
/// arrival order, separate and batched; the one whose attack resolves first survives.
/// Each pair is judged over all its variants (16 races per pair and join order), since
/// about one race in ten went the other way on BDS; the pass order must match at least
/// two of every three races.
#[test]
fn players_resolve_in_reverse_join_order_like_bds() {
    let mut checked = 0;
    for text in PAIRS {
        let pairs = fixture(text);
        let join = pairs.header["join_order"].as_str().unwrap().to_owned();
        let mut order = PlayerOrder::default();
        for player in join.chars() {
            order.join(player);
        }
        let pass: Vec<char> = order.iter().collect();
        // Who died first within the race, per pair, over every trial of its variants.
        let mut deaths: BTreeMap<(char, char), BTreeMap<char, u64>> = BTreeMap::new();
        for (variant, outcomes) in pairs.header["outcomes"].as_object().unwrap() {
            let mut pair: Vec<char> = variant.chars().take(2).collect();
            pair.sort_unstable();
            for outcome in outcomes.as_array().unwrap() {
                let count = outcome["count"].as_u64().unwrap();
                let first_death = outcome["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find_map(|event| {
                        let event = event.as_str()?;
                        let mut words = event.split_whitespace();
                        let tick: i64 = words.next()?.parse().ok()?;
                        (words.next()? == "death" && event.contains(" Death ") && tick <= 4)
                            .then(|| words.next()?.chars().next())
                            .flatten()
                    });
                if let Some(player) = first_death {
                    *deaths
                        .entry((pair[0], pair[1]))
                        .or_default()
                        .entry(player)
                        .or_default() += count;
                }
            }
        }
        for ((one, other), counts) in deaths {
            let first =
                if pass.iter().position(|p| *p == one) < pass.iter().position(|p| *p == other) {
                    one
                } else {
                    other
                };
            let second = if first == one { other } else { one };
            let lost = counts.get(&second).copied().unwrap_or(0);
            let won = counts.get(&first).copied().unwrap_or(0);
            assert!(
                lost >= 2 * won,
                "join {join}, pair {one}{other}: pass order {pass:?} resolves {first} first, but BDS killed {second} {lost} times and {first} {won} times"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 4 * 3);
}

#[test]
fn the_latest_joiner_goes_first_and_a_rejoin_counts_as_latest() {
    let mut order = PlayerOrder::default();
    for player in ["a", "b", "c"] {
        order.join(player);
    }
    assert_eq!(order.iter().collect::<Vec<_>>(), ["c", "b", "a"]);
    order.join("a");
    assert_eq!(order.iter().collect::<Vec<_>>(), ["a", "c", "b"]);
    order.leave("c");
    assert_eq!(order.iter().collect::<Vec<_>>(), ["a", "b"]);
}
