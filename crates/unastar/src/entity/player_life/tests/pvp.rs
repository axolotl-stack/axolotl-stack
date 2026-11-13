//! Player-versus-player hits replayed against BDS 1.26.51
//! (`testdata/bds-1.26.51-pvp.json`, distilled by
//! `tools/bds-sweep/pvp_fixture.py` from three lockstep clients).
//!
//! Each trial's attacks are applied in the tick and order BDS applied them
//! (`attempts`), each with the attacker's and the victim's state at that
//! moment, through [`pvp::player_hits_player`]; the victim's pushes, hurt
//! and death events and health are compared with what BDS sent, tick by
//! tick. A few packet-level tests route the attack through the server's
//! queues instead, as the current schedule handles it; the cases that
//! schedule cannot reproduce are ignored tests that point at the ordering
//! work (decision 0045).

use super::*;
use crate::entity::components::{
    ArmourInventory, EffectType, Effects, HeldSlot, MainInventory, PlayerInput,
};
use crate::entity::player_life::pvp::{self, Swing};
use crate::item::ItemStack;
use crate::item::enchant::with_enchantments;
use crate::item::test_support::stack;
use jolyne::valentine::types::ItemUseOnActorInventoryTransaction;
use serde_json::Value;
use unastar_rules::ids::Enchantment;

fn fixture() -> &'static Value {
    static FIXTURE: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-pvp.json")).unwrap()
    })
}

/// Every trial whose key starts with one of `prefixes`.
fn trials(prefixes: &[&str]) -> Vec<&'static Value> {
    let trials: Vec<_> = fixture()["trials"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|trial| {
            let key = trial["key"].as_str().unwrap();
            prefixes.iter().any(|prefix| key.starts_with(prefix))
        })
        .collect();
    assert!(!trials.is_empty(), "no trials for {prefixes:?}");
    trials
}

/// Where BDS stood A at the start of every trial; the replay puts that
/// spot at [`HOME`].
const BDS_ORIGIN: DVec3 = DVec3::new(0.5, -60.0, 0.5);
const ROLES: [&str; 3] = ["A", "B", "C"];

fn vec3(value: &Value) -> DVec3 {
    let v: Vec<f64> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    DVec3::new(v[0], v[1], v[2])
}

/// BDS feet to the replay's eyes, as f32 like every position a client
/// reports.
fn eyes_of(bds_feet: DVec3) -> DVec3 {
    eyes(HOME + bds_feet - BDS_ORIGIN).as_vec3().as_dvec3()
}

/// An item spec of the sweep (`{item, enchants?}`), or nothing.
fn stack_of(spec: &Value) -> ItemStack {
    if spec.is_null() {
        return ItemStack::empty();
    }
    let item = stack(spec["item"].as_str().unwrap(), 1);
    let levels: Vec<(Enchantment, u8)> = spec["enchants"]
        .as_object()
        .map(|enchants| {
            enchants
                .iter()
                .map(|(name, level)| {
                    (
                        name.parse::<Enchantment>()
                            .unwrap_or_else(|error| panic!("enchantment {name}: {error}")),
                        level.as_u64().unwrap() as u8,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if levels.is_empty() {
        item
    } else {
        with_enchantments(&item, levels)
    }
}

struct Replay {
    h: Harness,
    players: [Entity; 3],
    viewers: [Option<Joined>; 2],
}

impl Replay {
    fn new(trial: &Value) -> Self {
        let mut h = harness(Difficulty::Normal);
        h.rules().natural_regeneration = false;
        let setup = &trial["setup"];
        let at = setup["at"].as_array().unwrap();
        let spot = |role: usize| {
            let v = at[role].as_array().unwrap();
            let feet = DVec3::new(
                v[0].as_f64().unwrap(),
                v[1].as_f64().unwrap(),
                v[2].as_f64().unwrap(),
            );
            HOME + feet - BDS_ORIGIN
        };
        let b = join(&mut h.server, VIEWER, "Alex", spot(1), None);
        let c = join(&mut h.server, VIEWER + 1, "Sam", spot(2), None);
        let players = [h.player, b.entity, c.entity];
        let mut replay = Self {
            h,
            players,
            viewers: [Some(b), Some(c)],
        };
        let world = replay.h.world();
        for (role, player) in players.into_iter().enumerate() {
            world.get_mut::<Position>(player).unwrap().0 = eyes(spot(role));
            if let Some(mode) = setup["modes"].as_array().map(|modes| &modes[role]) {
                *world.get_mut::<GameMode>(player).unwrap() = match mode.as_str().unwrap() {
                    "survival" => GameMode::Survival,
                    "creative" => GameMode::Creative,
                    "adventure" => GameMode::Adventure,
                    "spectator" => GameMode::Spectator,
                    other => panic!("mode {other}"),
                };
            }
            let mut inventory = world.get_mut::<MainInventory>(player).unwrap();
            if let Some(hotbar) = setup["hotbar"].as_array().and_then(|h| h[role].as_object()) {
                for (slot, spec) in hotbar {
                    inventory
                        .0
                        .set_item(slot.parse().unwrap(), stack_of(spec))
                        .unwrap();
                }
            }
            if let Some(held) = setup["held"].as_array() {
                inventory.0.set_item(0, stack_of(&held[role])).unwrap();
            }
            if let Some(worn) = setup["armour"].as_array().and_then(|a| a[role].as_object()) {
                let mut armour = world.get_mut::<ArmourInventory>(player).unwrap();
                for (slot, spec) in worn {
                    let stack = stack_of(spec);
                    match slot.as_str() {
                        "helmet" => armour.set_helmet(stack),
                        "chestplate" => armour.set_chestplate(stack),
                        "leggings" => armour.set_leggings(stack),
                        "boots" => armour.set_boots(stack),
                        other => panic!("armour slot {other}"),
                    }
                    .unwrap();
                }
            }
            // The health BDS had when the trial started (its setup `/damage`
            // failed once, right after a respawn).
            if let Some(health) = trial["start_health"][ROLES[role]].as_f64() {
                world.get_mut::<Health>(player).unwrap().current = health as f32;
            }
        }
        for line in setup["effects"].as_array().into_iter().flatten() {
            // `effect "{A}" strength 100 0 true`
            let words: Vec<&str> = line.as_str().unwrap().split_whitespace().collect();
            let role = ROLES
                .iter()
                .position(|role| words[1] == format!("\"{{{role}}}\""))
                .unwrap();
            let effect = match words[2] {
                "strength" => EffectType::STRENGTH,
                "weakness" => EffectType::WEAKNESS,
                "resistance" => EffectType::RESISTANCE,
                "blindness" => EffectType::BLINDNESS,
                "slow_falling" => EffectType::SLOW_FALLING,
                other => panic!("effect {other}"),
            };
            world
                .entity_mut(players[role])
                .entry::<Effects>()
                .or_default()
                .get_mut()
                .add(effect, words[4].parse().unwrap(), 100_000);
        }
        for line in setup["setup"].as_array().into_iter().flatten() {
            if line.as_str() == Some("gamerule pvp false") {
                world.resource_mut::<GameRules>().pvp = false;
            }
        }
        replay.tick();
        replay
    }

    fn tick(&mut self) {
        self.h.tick();
    }

    /// The packets `role` got since the last call.
    fn drain(&mut self, role: usize) -> Vec<McpePacketData> {
        match role {
            0 => self.h.drain(),
            _ => drain(self.viewers[role - 1].as_mut().unwrap()),
        }
    }

    /// Applies `attempt`; `velocity` is the victim's velocity before it,
    /// when known.
    fn attack(&mut self, attempt: &Value, velocity: Option<DVec3>) -> Result<f32, pvp::Refusal> {
        let role = |name: &Value| {
            ROLES
                .iter()
                .position(|r| *r == name.as_str().unwrap())
                .unwrap()
        };
        let (attacker, victim) = (role(&attempt["attacker"]), role(&attempt["victim"]));
        let (attacker, victim) = (self.players[attacker], self.players[victim]);
        let world = self.h.world();
        let state = &attempt["attacker_state"];
        let falling = !state["ground"].as_bool().unwrap() && vec3(&state["vel"]).y < -1.0e-9;
        let swing = Swing {
            eye: eyes_of(vec3(&state["pos"])),
            on_ground: state["ground"].as_bool().unwrap(),
            falling,
            // The sprint of the attacker's input before the one the attack
            // joins (its client's capture).
            sprinting: attempt["attacker_sprinting"]
                .as_bool()
                .or_else(|| state["sprinting"].as_bool())
                .unwrap_or(false),
        };
        world.get_mut::<Position>(attacker).unwrap().0 = swing.eye;
        world.get_mut::<HeldSlot>(attacker).unwrap().0 = attempt["slot"].as_u64().unwrap() as u8;
        let victim_state = &attempt["victim_state"];
        if !victim_state.is_null() {
            world.get_mut::<Position>(victim).unwrap().0 = eyes_of(vec3(&victim_state["pos"]));
        }
        if let Some(velocity) = velocity {
            world.get_mut::<PlayerInput>(victim).unwrap().velocity = velocity;
        }
        let offset = vec3(&attempt["claim_offset"]);
        let claimed = swing.eye + offset;
        // The slot the attack names (`claim-slot`, else the held one) and
        // the item in it.
        let slot = attempt["claim_slot"]
            .as_u64()
            .unwrap_or(attempt["slot"].as_u64().unwrap()) as u8;
        let item_id = world
            .get::<MainInventory>(attacker)
            .unwrap()
            .hotbar(slot)
            .and_then(|stack| stack.item())
            .map_or(0, |entry| entry.network_id() as i16);
        let transaction = ItemUseOnActorInventoryTransaction {
            slot: i32::from(slot),
            item: jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData {
                id: item_id,
                ..Default::default()
            },
            from_position: Vec3 {
                x: claimed.x as f32,
                y: claimed.y as f32,
                z: claimed.z as f32,
            },
            ..Default::default()
        };
        // The window counts from the tick the packet arrived in.
        let now = world
            .resource::<crate::ecs::resources::TickCounter>()
            .current;
        let late = attempt["t"].as_i64().unwrap()
            - attempt["arrived"]
                .as_i64()
                .unwrap_or(attempt["t"].as_i64().unwrap());
        let arrived = now - late.max(0) as u64;
        pvp::player_hits_player(world, attacker, victim, swing, &transaction, arrived)
    }
}

/// What a player got about itself in one tick, and the hit feedback it
/// got about anyone: particles (`Animate` action and data), last-hurt-by
/// notes and attack sounds (name and the x, y, z of where).
#[derive(Debug, Default, PartialEq)]
struct Seen {
    motions: Vec<[f32; 3]>,
    events: Vec<(i32, i32)>,
    feedback: Vec<String>,
}

fn seen(packets: &[McpePacketData], runtime_id: u64) -> Seen {
    let mut out = Seen::default();
    for packet in packets {
        match packet {
            McpePacketData::SetActorMotionPacket(motion)
                if motion.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                out.motions
                    .push([motion.motion.x, motion.motion.y, motion.motion.z]);
            }
            McpePacketData::ActorEventPacket(event)
                if event.target_runtime_id.actor_runtime_id == runtime_id
                    && matches!(
                        event.event_id,
                        EnumsActorEvent::Hurt | EnumsActorEvent::Death
                    ) =>
            {
                out.events.push((
                    if event.event_id == EnumsActorEvent::Hurt {
                        2
                    } else {
                        3
                    },
                    event.data,
                ));
            }
            McpePacketData::AnimatePacket(animate) => out.feedback.push(format!(
                "animate {} {}",
                match animate.action {
                    jolyne::valentine::types::EnumsAnimatePacketPayloadAction::Criticalhit => 4,
                    jolyne::valentine::types::EnumsAnimatePacketPayloadAction::Magiccriticalhit =>
                        5,
                    _ => -1,
                },
                animate.data
            )),
            McpePacketData::SetLastHurtByPacket(_) => out.feedback.push("last hurt by".into()),
            McpePacketData::LevelSoundEventPacket(sound) => {
                let name = format!("{:?}", sound.sound_event);
                if name.contains("attack.") {
                    out.feedback.push(format!(
                        "sound {} {:.2} {:.2} {:.2}",
                        name.trim_matches('"'),
                        sound.position.x,
                        sound.position.y,
                        sound.position.z
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

/// What BDS sent `role` about itself in trial tick `t`.
fn bds_seen(trial: &Value, role: &str, t: i64) -> Seen {
    let mut out = Seen::default();
    for sent in trial["sent"].as_array().unwrap() {
        if sent["t"].as_i64() != Some(t) || sent["to"] != role || sent["about"] != role {
            continue;
        }
        match sent["packet"].as_str().unwrap() {
            "SetActorMotion" => {
                let m = vec3(&sent["motion"]);
                out.motions.push([m.x as f32, m.y as f32, m.z as f32]);
            }
            "ActorEvent" if matches!(sent["event"].as_i64(), Some(2 | 3)) => out.events.push((
                sent["event"].as_i64().unwrap() as i32,
                sent["data"].as_i64().unwrap() as i32,
            )),
            _ => {}
        }
    }
    // Feedback goes to anyone, about anyone.
    for sent in trial["sent"].as_array().unwrap() {
        if sent["t"].as_i64() != Some(t) || sent["to"] != role {
            continue;
        }
        match sent["packet"].as_str().unwrap() {
            "Animate" => out.feedback.push(format!(
                "animate {} {}",
                sent["action"].as_i64().unwrap(),
                sent["data"].as_f64().unwrap()
            )),
            "SetLastHurtBy" => out.feedback.push("last hurt by".into()),
            "LevelSoundEvent" if sent["sound"].as_str().unwrap().starts_with("attack.") => {
                let at = vec3(&sent["position"]) - BDS_ORIGIN + HOME - DVec3::new(0.0, 0.0, 0.0);
                out.feedback.push(format!(
                    "sound {} {:.2} {:.2} {:.2}",
                    sent["sound"].as_str().unwrap(),
                    at.x,
                    at.y,
                    at.z
                ));
            }
            _ => {}
        }
    }
    out
}

/// The victim's velocity before `attempt` (a hit in tick `t`): BDS's own,
/// when every hit on it that tick pushed once (the push BDS sent for this
/// hit less the change its knockback event reports; BDS may move the victim
/// between two hits of a tick, when it consumes the victim's input between
/// them), else, for the tick's first hit, the velocity its client reported
/// with its last input. (The sweep client applies a push when it arrives, a
/// tick later than BDS's simulation of it, so its reports lag while it
/// flies; a vanilla client's do not.)
fn velocity_before(trial: &Value, attempt: &Value, t: i64, first_on_victim: bool) -> Option<DVec3> {
    let victim = attempt["victim"].as_str().unwrap();
    let landed: Vec<&Value> = trial["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["t"].as_i64() == Some(t) && a["victim"] == victim && a["landed"].is_u64())
        .collect();
    let bds = bds_seen(trial, victim, t);
    let index = landed.iter().position(|a| *a == attempt);
    let change = index.and_then(|_| {
        trial["hits"][attempt["landed"].as_u64().unwrap() as usize]["knockback"].as_array()
    });
    if let (Some(index), Some(change)) = (index, change)
        && bds.motions.len() == landed.len()
    {
        let pushed = bds.motions[index];
        let change = vec3(&Value::from(change.clone()));
        return Some(DVec3::new(
            f64::from(pushed[0]) - change.x,
            f64::from(pushed[1]) - change.y,
            f64::from(pushed[2]) - change.z,
        ));
    }
    if !first_on_victim {
        return None;
    }
    attempt["victim_reported_velocity"]
        .as_array()
        .map(|v| vec3(&Value::from(v.clone())))
}

/// Replays every trial with one of `prefixes` and compares each tick.
fn replay_against_bds(prefixes: &[&str]) {
    let runtime_ids = [SESSION, VIEWER, VIEWER + 1];
    for trial in trials(prefixes) {
        let key = format!(
            "{}#{}",
            trial["key"].as_str().unwrap(),
            trial["repeat"].as_u64().unwrap()
        );
        let attempts: Vec<&Value> = trial["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|attempt| attempt["t"].is_i64())
            .collect();
        if attempts.is_empty() {
            continue;
        }
        let mut replay = Replay::new(trial);
        for role in 0..3 {
            replay.drain(role);
        }
        let first = attempts[0]["t"].as_i64().unwrap();
        // One tick past the last hit, for its sound.
        let last = attempts.last().unwrap()["t"].as_i64().unwrap() + 1;
        for t in first..=last {
            // The tick runs first and the hits come after it, as the server
            // applies them inside its tick (a hit's sound waits for the next
            // tick).
            replay.tick();
            let mut pushed = [false; 3];
            for attempt in attempts.iter().filter(|a| a["t"].as_i64() == Some(t)) {
                let victim = ROLES.iter().position(|r| attempt["victim"] == *r).unwrap();
                let first_on_victim = !std::mem::replace(&mut pushed[victim], true);
                let velocity = velocity_before(trial, attempt, t, first_on_victim);
                let _ = replay.attack(attempt, velocity);
            }
            for (role, name) in ROLES.iter().enumerate() {
                let ours = seen(&replay.drain(role), runtime_ids[role]);
                let bds = bds_seen(trial, name, t);
                assert_eq!(
                    ours.events, bds.events,
                    "{key} t{t}: hurt and death events of {name}"
                );
                if trial["feedback"].as_bool() == Some(true) {
                    let (mut ours, mut bds) = (ours.feedback.clone(), bds.feedback.clone());
                    ours.sort();
                    bds.sort();
                    assert_eq!(ours, bds, "{key} t{t}: hit feedback {name} got");
                }
                assert_eq!(
                    ours.motions.len(),
                    bds.motions.len(),
                    "{key} t{t}: pushes of {name}: {:?} vs BDS {:?}",
                    ours.motions,
                    bds.motions
                );
                for (ours, bds) in ours.motions.iter().zip(&bds.motions) {
                    for axis in 0..3 {
                        assert!(
                            (ours[axis] - bds[axis]).abs() < 1.0e-5,
                            "{key} t{t}: push of {name} {ours:?} vs BDS {bds:?}"
                        );
                    }
                }
                let health = trial["sent"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rfind(|s| {
                        s["t"].as_i64() == Some(t)
                            && s["packet"] == "Health"
                            && s["to"] == *name
                            && s["about"] == *name
                    })
                    .map(|s| s["value"].as_f64().unwrap() as f32);
                if let Some(bds) = health {
                    let player = replay.players[role];
                    let ours = replay.h.world().get::<Health>(player).unwrap().current;
                    assert!(
                        (ours - bds).abs() < 1.0e-4,
                        "{key} t{t}: health of {name} {ours} vs BDS {bds}"
                    );
                }
            }
        }
    }
}

#[test]
fn weapons_enchantments_effects_and_armour_deal_the_bds_damage() {
    replay_against_bds(&[
        "weapon|",
        "sharpness|",
        "effect|",
        "resistance|",
        "armour|",
        "difficulty|",
    ]);
}

#[test]
fn game_modes_and_the_pvp_rule_decide_who_can_be_hit() {
    replay_against_bds(&["mode|", "rule|"]);
}

#[test]
fn critical_hits_need_a_fall_and_no_blindness_or_slow_falling() {
    replay_against_bds(&["crit|"]);
}

#[test]
fn knockback_pushes_like_bds_with_sprint_enchantment_and_resistance() {
    replay_against_bds(&["knockback|"]);
}

#[test]
fn reach_and_the_claimed_position_limit_hits_like_bds() {
    replay_against_bds(&["reach|", "claim|", "facing|", "wall|"]);
}

#[test]
fn the_hurt_window_ignores_hits_and_deals_only_the_excess_like_bds() {
    replay_against_bds(&["window|", "h9|", "h11|", "burst|"]);
}

#[test]
fn attacks_in_one_tick_apply_in_bds_order() {
    replay_against_bds(&["two|", "kill|", "mutual|", "victim|", "sprint|", "swing|"]);
}

// --- The attack through the server's packet queues --------------------------

/// Route `packet` from the player with `session`.
fn route_from(h: &mut Harness, session: u64, packet: impl Into<McpePacket>) {
    let packet: McpePacket = packet.into();
    let mut buf = BytesMut::new();
    packet.data.encode_inner(&mut buf, 0, 0).unwrap();
    let borrowed = BorrowedMcpePacket::decode_inner(&mut buf.freeze()).unwrap();
    h.server.route_packet(session, McpePacketArgs, borrowed);
}

/// One `PlayerAuthInput` standing (or walking forward) at `feet`, at rest
/// on the ground (a vanilla client reports 0.0784 down).
fn input(
    feet: DVec3,
    forward: bool,
    flags: &[EnumsPlayerAuthInputPacketPayloadInputData],
) -> PlayerAuthInputPacket {
    let eye = eyes(feet);
    PlayerAuthInputPacket {
        position: Vec3 {
            x: eye.x as f32,
            y: eye.y as f32,
            z: eye.z as f32,
        },
        move_vector: jolyne::valentine::types::Vec2 {
            x: 0.0,
            y: if forward { 1.0 } else { 0.0 },
        },
        pos_delta: Vec3 {
            x: 0.0,
            y: -0.0784,
            z: 0.0,
        },
        input_data: [EnumsPlayerAuthInputPacketPayloadInputData::Verticalcollision]
            .into_iter()
            .chain(flags.iter().copied())
            .collect(),
        ..Default::default()
    }
}

/// `attacker`'s attack on the player with runtime id `target`, naming its
/// held slot and item and its own position.
fn attack_packet(
    h: &mut Harness,
    attacker: Entity,
    target: u64,
) -> jolyne::valentine::InventoryTransactionPacket {
    use jolyne::valentine::types::{
        EnumsItemUseOnActorInventoryTransactionActionType as Action,
        InventoryTransactionPacketTransaction as Transaction,
    };
    let world = h.world();
    let eye = world.get::<Position>(attacker).unwrap().0;
    let slot = world.get::<HeldSlot>(attacker).unwrap().0;
    let id = world
        .get::<MainInventory>(attacker)
        .unwrap()
        .hotbar(slot)
        .and_then(|stack| stack.item())
        .map_or(0, |entry| entry.network_id() as i16);
    jolyne::valentine::InventoryTransactionPacket {
        transaction: Transaction::ItemUseOnActorInventoryTransaction(
            ItemUseOnActorInventoryTransaction {
                runtime_id: ActorRuntimeId {
                    actor_runtime_id: target,
                },
                action_type: Action::Attack,
                slot: i32::from(slot),
                item: jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData {
                    id,
                    ..Default::default()
                },
                from_position: Vec3 {
                    x: eye.x as f32,
                    y: eye.y as f32,
                    z: eye.z as f32,
                },
                ..Default::default()
            },
        ),
        ..Default::default()
    }
}

const B_FEET: DVec3 = DVec3::new(HOME.x + 2.0, HOME.y, HOME.z);
const C_FEET: DVec3 = DVec3::new(HOME.x + 2.0, HOME.y, HOME.z + 2.0);

/// The attacker at [`HOME`] holding `weapon`, the victim 2 blocks east and
/// a third player 2 blocks south of the victim, each standing and having
/// sent an input.
fn duel(weapon: Option<&str>) -> (Harness, Joined, Joined) {
    let mut h = harness(Difficulty::Normal);
    h.rules().natural_regeneration = false;
    let mut b = join(&mut h.server, VIEWER, "Alex", B_FEET, None);
    let mut c = join(&mut h.server, VIEWER + 1, "Sam", C_FEET, None);
    let player = h.player;
    if let Some(weapon) = weapon {
        h.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, stack(weapon, 1))
            .unwrap();
    }
    for (session, feet) in [(SESSION, HOME), (VIEWER, B_FEET), (VIEWER + 1, C_FEET)] {
        route_from(&mut h, session, input(feet, false, &[]));
    }
    h.ticks(2);
    h.drain();
    drain(&mut b);
    drain(&mut c);
    (h, b, c)
}

/// BDS 1.26.51 `weapon|diamond_sword`: the diamond sword takes 8 from the
/// victim, pushes it (0.4, 0.4, 0) and shows the hurt; a vanilla client
/// sends the attack before its tick's input.
#[test]
fn a_routed_attack_hurts_and_pushes_the_other_player_like_bds() {
    let (mut h, mut b, _c) = duel(Some("minecraft:diamond_sword"));
    let player = h.player;
    let attack = attack_packet(&mut h, player, VIEWER);
    route_from(&mut h, SESSION, attack);
    route_from(&mut h, SESSION, input(HOME, false, &[]));
    route_from(&mut h, VIEWER, input(B_FEET, false, &[]));
    h.tick();
    let got = seen(&drain(&mut b), VIEWER);
    assert_eq!(got.events, [(2, 2)]);
    assert_eq!(got.motions.len(), 1);
    for (ours, bds) in got.motions[0].iter().zip([0.4f32, 0.4, 0.0]) {
        assert!((ours - bds).abs() < 1.0e-6, "{:?}", got.motions);
    }
    assert_eq!(h.world().get::<Health>(b.entity).unwrap().current, 12.0);
}

/// A sprinting attacker whose tick sends `steps` (the attack and the input
/// that ends its sprint, in that order or the other); returns the victim's
/// pushes.
fn sprint_then(steps: &[&str]) -> Vec<[f32; 3]> {
    use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
    let (mut h, mut b, _c) = duel(None);
    let player = h.player;
    route_from(
        &mut h,
        SESSION,
        input(
            HOME,
            true,
            &[Flag::Up, Flag::Startsprinting, Flag::Sprinting],
        ),
    );
    h.tick();
    route_from(
        &mut h,
        SESSION,
        input(HOME, true, &[Flag::Up, Flag::Sprinting]),
    );
    h.tick();
    assert!(
        h.world()
            .get::<crate::entity::components::PlayerState>(player)
            .unwrap()
            .sprinting
    );
    drain(&mut b);
    for step in steps {
        match *step {
            "attack" => {
                let attack = attack_packet(&mut h, player, VIEWER);
                route_from(&mut h, SESSION, attack);
            }
            "stop" => route_from(&mut h, SESSION, input(HOME, false, &[Flag::Stopsprinting])),
            other => panic!("{other}"),
        }
    }
    h.tick();
    seen(&drain(&mut b), VIEWER).motions
}

/// BDS 1.26.51 `sprint|hit_then_stop`: the attack, then the input that ends
/// the sprint, in one tick: the sprint's extra push lands (two
/// `SetActorMotion`, both (0.6, 0.32, 0) from rest). The server's copy of
/// the sprinting attacker moves on from where its inputs say it stands, so
/// the push's direction is checked by its length, not its axis.
#[test]
fn an_attack_before_the_input_that_ends_the_sprint_is_a_sprint_hit() {
    let pushes = sprint_then(&["attack", "stop"]);
    assert_eq!(pushes.len(), 2, "{pushes:?}");
    for push in pushes {
        let horizontal = push[0].hypot(push[2]);
        assert!((horizontal - 0.6).abs() < 1.0e-5, "{push:?}");
        assert!((push[1] - 0.32).abs() < 1.0e-5, "{push:?}");
    }
}

/// BDS 1.26.51 `sprint|stop_then_hit`: the input that ends the sprint, then
/// the attack: BDS applies the attack with the attacker's next input, after
/// the sprint ended, so there is no extra push. The current schedule applies
/// every input of the tick before every attack, with the state from before
/// the tick's first input.
#[test]
#[ignore = "needs packets applied in arrival order (decision 0045): an attack sent after its player's input of the tick joins the next input"]
fn an_attack_after_the_input_that_ends_the_sprint_is_a_plain_hit() {
    let pushes = sprint_then(&["stop", "attack"]);
    assert_eq!(pushes.len(), 1, "{pushes:?}");
    for (ours, bds) in pushes[0].iter().zip([0.4f32, 0.4, 0.0]) {
        assert!((ours - bds).abs() < 1.0e-5, "{pushes:?}");
    }
}

/// BDS 1.26.51 `two|hand_then_sword`: the attacker's fist and the third
/// player's diamond sword reach the victim in one tick, the fist's packets
/// first; BDS applied the sword first (8, one push) and ignored the fist
/// under the hurt window. The current schedule applies the attacks in
/// arrival order: the fist's 1, then the sword's excess 7 with a second push
/// and hurt event.
#[test]
#[ignore = "needs BDS's application order of different players' packets within a tick (decision 0045)"]
fn two_attackers_in_one_tick_apply_in_bds_order() {
    let (mut h, mut b, c) = duel(None);
    h.world()
        .get_mut::<MainInventory>(c.entity)
        .unwrap()
        .0
        .set_item(0, stack("minecraft:diamond_sword", 1))
        .unwrap();
    let player = h.player;
    let fist = attack_packet(&mut h, player, VIEWER);
    route_from(&mut h, SESSION, fist);
    route_from(&mut h, SESSION, input(HOME, false, &[]));
    let sword = attack_packet(&mut h, c.entity, VIEWER);
    route_from(&mut h, VIEWER + 1, sword);
    route_from(&mut h, VIEWER + 1, input(C_FEET, false, &[]));
    h.tick();
    let got = seen(&drain(&mut b), VIEWER);
    assert_eq!(got.events, [(2, 2)]);
    assert_eq!(got.motions.len(), 1);
    assert_eq!(h.world().get::<Health>(b.entity).unwrap().current, 12.0);
}

/// A spectator's attack changes nothing (not measured: a vanilla spectator
/// never attacks; refused as the mob path refuses it).
#[test]
fn a_spectator_cannot_hit() {
    let (mut h, mut b, _c) = duel(Some("minecraft:diamond_sword"));
    let player = h.player;
    h.set_mode(GameMode::Spectator);
    let attack = attack_packet(&mut h, player, VIEWER);
    route_from(&mut h, SESSION, attack);
    route_from(&mut h, SESSION, input(HOME, false, &[]));
    h.tick();
    assert_eq!(seen(&drain(&mut b), VIEWER), Seen::default());
    assert_eq!(h.world().get::<Health>(b.entity).unwrap().current, 20.0);
}
