//! Spear-holding zombies replayed against the BDS 1.26.51 recordings of the
//! client-diff scenarios `zombie-spear-field` and `zombie-door-reach-spear`
//! (`crates/client-diff/tests/fixtures/bds-zombie/`).
//!
//! Each recording is decoded here. From the zombie raising its spear (its
//! `USINGITEM` flag) on, the flag changes, the actor events of the fight,
//! the player's health after each hit and the knockback it got are compared
//! with a zombie in the same layout, on the same ticks. The recorded client
//! walked back after each hit's knockback; this harness has no client, so
//! the player is moved where the recorded client reported itself after the
//! matching hit.

use bevy_ecs::prelude::*;
use bytes::Bytes;
use glam::{DVec3, IVec3};
use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::{DataItemEntryPayload, EnumsActorEvent};
use jolyne::valentine::{McpePacketArgs, McpePacketData, SetActorDataPacket};
use unastar_data::actor_components::body::ActorDamageCause;

use super::doors::{Harness, air, built, door_facing};
use crate::entity::components::{Health, Position, RuntimeId};
use crate::entity::mob::combat;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::gear::replicate::changed_packets;
use crate::entity::mob::gear::{GearSlot, MobEquipment};
use crate::registry::BlockRegistry;
use crate::server::game::types::{ItemRegistryResource, block_network_ids};
use crate::server::game::world_stack;
use crate::world::rules::Difficulty;

const FIELD: &str =
    include_str!("../../../../../client-diff/tests/fixtures/bds-zombie/zombie-spear-field.jsonl");
const DOOR_REACH: &str = include_str!(
    "../../../../../client-diff/tests/fixtures/bds-zombie/zombie-door-reach-spear.jsonl"
);

/// The recorded worlds' floor is at y = -61; ours is at 63.
const RECORDED_Y_OFFSET: f64 = 124.0;
/// Milliseconds per server tick.
const TICK_MS: f64 = 50.0;
/// Runtime ID of the recorded player and of the harness player.
const PLAYER_RUNTIME_ID: u64 = 1;

/// One decoded packet of a recording.
struct Recorded {
    t_ms: f64,
    s2c: bool,
    /// The packet as recorded: its header and body.
    bytes: Vec<u8>,
    packet: McpePacketData,
}

/// The packets of `text` (a client-diff JSONL recording) these tests read,
/// decoded.
fn recording(text: &str) -> Vec<Recorded> {
    const READ: [&str; 7] = [
        "AddActor",
        "ActorEvent",
        "MobEquipment",
        "PlayerAuthInput",
        "SetActorData",
        "SetActorMotion",
        "UpdateAttributes",
    ];
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let record: serde_json::Value = serde_json::from_str(line).expect("JSONL record");
            let name = record["name"].as_str().expect("packet name");
            if !READ.contains(&name) {
                return None;
            }
            let hex = record["hex"].as_str().expect("packet hex");
            let bytes: Vec<u8> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex byte"))
                .collect();
            let (_, packet) =
                McpePacketData::decode_inner_bytes(&mut frame(&bytes), McpePacketArgs)
                    .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            Some(Recorded {
                t_ms: record["t_ms"].as_f64().expect("t_ms"),
                s2c: record["dir"] == "s2c",
                bytes,
                packet,
            })
        })
        .collect()
}

/// `bytes` behind their length, as a batch carries a packet.
fn frame(bytes: &[u8]) -> Bytes {
    let mut framed = Vec::with_capacity(bytes.len() + 5);
    let mut len = bytes.len() as u32;
    while len >= 0x80 {
        framed.push(len as u8 | 0x80);
        len >>= 7;
    }
    framed.push(len as u8);
    framed.extend_from_slice(bytes);
    framed.into()
}

/// The first actor flag word a `SetActorData` carries.
fn flag_word(packet: &SetActorDataPacket) -> Option<i64> {
    packet
        .actor_data
        .data
        .iter()
        .find(|entry| entry.id == ids::RESERVED_0)
        .and_then(|entry| match &entry.payload {
            DataItemEntryPayload::DataItemInt64Payload(long) => Some(long.value),
            _ => None,
        })
}

fn using_item(word: i64) -> bool {
    word >> actor_flags::USINGITEM & 1 == 1
}

/// What a fight shows on the wire.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Seen {
    /// The zombie's `USINGITEM` flag (the raised spear) turned on or off.
    Spear(bool),
    /// An actor event on the player (hurt or death) and its data.
    Player(EnumsActorEvent, i32),
    /// An actor event on the zombie and its data.
    Zombie(EnumsActorEvent, i32),
}

/// A fight from the zombie raising its spear to it lowering it again, on
/// ticks counted from the raise.
#[derive(Debug, Default)]
struct Fight {
    seen: Vec<(u64, Seen)>,
    /// The player's health after each change.
    health: Vec<f32>,
    /// The knockback velocity each hit sent the player.
    knockback: Vec<DVec3>,
    /// Where the recorded client put the player's eye after each hit:
    /// `(hit, ticks after the hit, eye)` in our coordinates.
    moves: Vec<(usize, u64, DVec3)>,
}

impl Fight {
    fn spear_lowered(&self) -> bool {
        self.seen
            .iter()
            .any(|(tick, seen)| *tick > 0 && *seen == Seen::Spear(false))
    }

    fn hits(&self) -> Vec<u64> {
        self.seen
            .iter()
            .filter(|(_, seen)| {
                matches!(seen, Seen::Zombie(EnumsActorEvent::KineticDamageDealt, _))
            })
            .map(|(tick, _)| *tick)
            .collect()
    }
}

/// The recorded fight of the first zombie in `records`, from its first
/// spear raise until the next lowering (or the end of the recording when
/// `whole`).
fn recorded_fight(records: &[Recorded], whole: bool) -> Fight {
    let zombie = records
        .iter()
        .find_map(|r| match &r.packet {
            McpePacketData::AddActorPacket(add) if &*add.actor_type == "minecraft:zombie" => {
                Some(add.target_runtime_id.actor_runtime_id)
            }
            _ => None,
        })
        .expect("the recording spawns a zombie");
    let mut fight = Fight::default();
    let mut raised_at = None;
    let mut word = None;
    let mut hits_at: Vec<f64> = Vec::new();
    let mut last_eye = None;
    for record in records {
        let tick = |raised: f64| ((record.t_ms - raised) / TICK_MS).round() as u64;
        if !record.s2c {
            if let (McpePacketData::PlayerAuthInputPacket(input), Some(&hit_t)) =
                (&record.packet, hits_at.last())
            {
                let p = &input.position;
                let eye = DVec3::new(
                    f64::from(p.x),
                    f64::from(p.y) + RECORDED_Y_OFFSET,
                    f64::from(p.z),
                );
                if last_eye != Some(eye) {
                    last_eye = Some(eye);
                    let after = ((record.t_ms - hit_t) / TICK_MS).round().max(1.0) as u64;
                    fight.moves.push((hits_at.len() - 1, after, eye));
                }
            }
            continue;
        }
        match &record.packet {
            McpePacketData::SetActorDataPacket(data)
                if data.target_runtime_id.actor_runtime_id == zombie =>
            {
                let Some(now) = flag_word(data) else {
                    continue;
                };
                let changed = word.is_some_and(|old| using_item(old) != using_item(now));
                word = Some(now);
                if !changed {
                    continue;
                }
                if using_item(now) && raised_at.is_none() {
                    raised_at = Some(record.t_ms);
                }
                if let Some(raised) = raised_at {
                    fight
                        .seen
                        .push((tick(raised), Seen::Spear(using_item(now))));
                    if !using_item(now) && !whole {
                        break;
                    }
                }
            }
            McpePacketData::ActorEventPacket(event) => {
                let Some(raised) = raised_at else {
                    continue;
                };
                let target = event.target_runtime_id.actor_runtime_id;
                let seen = if target == zombie {
                    Seen::Zombie(event.event_id, event.data)
                } else if target == PLAYER_RUNTIME_ID
                    && matches!(
                        event.event_id,
                        EnumsActorEvent::Hurt | EnumsActorEvent::Death
                    )
                {
                    Seen::Player(event.event_id, event.data)
                } else {
                    continue;
                };
                if matches!(seen, Seen::Zombie(EnumsActorEvent::KineticDamageDealt, _)) {
                    hits_at.push(record.t_ms);
                }
                fight.seen.push((tick(raised), seen));
            }
            McpePacketData::SetActorMotionPacket(motion)
                if raised_at.is_some()
                    && motion.target_runtime_id.actor_runtime_id == PLAYER_RUNTIME_ID =>
            {
                let m = &motion.motion;
                fight
                    .knockback
                    .push(DVec3::new(f64::from(m.x), f64::from(m.y), f64::from(m.z)));
            }
            McpePacketData::UpdateAttributesPacket(update)
                if raised_at.is_some()
                    && update.target_runtime_id.actor_runtime_id == PLAYER_RUNTIME_ID =>
            {
                if let Some(health) = update
                    .attribute_list
                    .iter()
                    .find(|a| &*a.name == "minecraft:health")
                    && fight.health.last() != Some(&health.current_value)
                {
                    fight.health.push(health.current_value);
                }
            }
            _ => {}
        }
    }
    assert!(raised_at.is_some(), "the recorded zombie raises its spear");
    fight
}

/// The harness player's eye at `feet`.
fn place_player(h: &mut Harness, feet: DVec3) {
    let player = h.player;
    h.world().get_mut::<Position>(player).unwrap().0 =
        feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
}

/// A plain adult zombie at `feet` holding only an iron spear (the
/// scenarios' `/replaceitem ... slot.weapon.mainhand 0 iron_spear`), one
/// tick old so the player already sees it (the scenarios summon it before
/// they provoke it), and the packets of that tick dropped.
fn spear_zombie(h: &mut Harness, feet: DVec3) -> Entity {
    let zombie = h.zombie_at(feet, false);
    let world = h.world();
    let spear = world_stack(world, "minecraft:iron_spear", 1).expect("vanilla spear");
    let mut equipment = MobEquipment::default();
    equipment.set(GearSlot::Mainhand, spear, false);
    world.entity_mut(zombie).insert(equipment);
    h.server.tick();
    while h.rx.try_recv().is_ok() {}
    zombie
}

/// The player hurts `zombie` from where it stands (the scenarios'
/// `/damage @e[type=zombie] 1 entity_attack entity @s`). The command's
/// knockback is left out: it pushed the recorded field zombie about 1.3
/// blocks, ours flies about 3, which in the open field carries the zombie
/// out of its 16-block follow range. Each fight is compared from the raise
/// on, so where the zombie starts its walk does not matter.
fn provoke(h: &mut Harness, zombie: Entity) {
    let player = h.player;
    combat::hurt_mob(
        h.world(),
        zombie,
        combat::Hit {
            damage: 1.0,
            cause: ActorDamageCause::EntityAttack,
            attacker: Some(player),
            by_player: true,
            knockback_from: None,
            knockback_power: KnockbackPower::DEFAULT,
        },
    );
}

/// Run up to `max_ticks` ticks and record the fight as [`recorded_fight`]
/// reads it, moving the player after each hit as `moves` (a recorded
/// fight's) say. Stops once the spear is lowered unless `whole`.
fn our_fight(
    h: &mut Harness,
    zombie: Entity,
    moves: &[(usize, u64, DVec3)],
    max_ticks: u64,
    whole: bool,
) -> Fight {
    let zombie_id = h.world().get::<RuntimeId>(zombie).unwrap().0 as u64;
    let player = h.player;
    let mut fight = Fight::default();
    let mut raised_at = None;
    let mut word: Option<i64> = None;
    let mut hits_at: Vec<u64> = Vec::new();
    let mut health = h.world().get::<Health>(player).unwrap().current;
    for tick in 0..max_ticks {
        for &(hit, after, eye) in moves {
            if hits_at.get(hit).is_some_and(|at| at + after == tick) {
                h.world().get_mut::<Position>(player).unwrap().0 = eye;
            }
        }
        h.server.tick();
        let packets: Vec<_> = std::iter::from_fn(|| h.rx.try_recv().ok().map(|p| p.data)).collect();
        for packet in packets {
            match packet {
                McpePacketData::SetActorDataPacket(data)
                    if data.target_runtime_id.actor_runtime_id == zombie_id =>
                {
                    let Some(now) = flag_word(&data) else {
                        continue;
                    };
                    let changed = word.is_some_and(|old| using_item(old) != using_item(now))
                        || (word.is_none() && using_item(now));
                    word = Some(now);
                    if !changed {
                        continue;
                    }
                    if using_item(now) && raised_at.is_none() {
                        raised_at = Some(tick);
                    }
                    if let Some(raised) = raised_at {
                        fight
                            .seen
                            .push((tick - raised, Seen::Spear(using_item(now))));
                    }
                }
                McpePacketData::ActorEventPacket(event) => {
                    let Some(raised) = raised_at else {
                        continue;
                    };
                    let target = event.target_runtime_id.actor_runtime_id;
                    let seen = if target == zombie_id {
                        Seen::Zombie(event.event_id, event.data)
                    } else if target == PLAYER_RUNTIME_ID
                        && matches!(
                            event.event_id,
                            EnumsActorEvent::Hurt | EnumsActorEvent::Death
                        )
                    {
                        Seen::Player(event.event_id, event.data)
                    } else {
                        continue;
                    };
                    if matches!(seen, Seen::Zombie(EnumsActorEvent::KineticDamageDealt, _)) {
                        hits_at.push(tick);
                    }
                    fight.seen.push((tick - raised, seen));
                }
                McpePacketData::SetActorMotionPacket(motion)
                    if raised_at.is_some()
                        && motion.target_runtime_id.actor_runtime_id == PLAYER_RUNTIME_ID =>
                {
                    let m = motion.motion;
                    fight.knockback.push(DVec3::new(
                        f64::from(m.x),
                        f64::from(m.y),
                        f64::from(m.z),
                    ));
                }
                _ => {}
            }
        }
        let now = h.world().get::<Health>(player).unwrap().current;
        if raised_at.is_some() && now != health {
            fight.health.push(now);
        }
        health = now;
        if fight.spear_lowered() && !whole {
            break;
        }
    }
    fight
}

fn kinds(seen: &[(u64, Seen)]) -> Vec<Seen> {
    seen.iter().map(|(_, seen)| *seen).collect()
}

/// `zombie-spear-field` (Hard): a spear zombie walks at the player across
/// open ground. It raises the spear once the player is within 10 blocks
/// and keeps walking; each hit deals the kinetic damage (2 at walking
/// speed plus the zombie's attack of 3, × 1.5 on Hard: 7.5), knocks the
/// player back and shows `KineticDamageDealt` on the zombie. The third hit
/// kills the player and the zombie lowers the spear the next tick.
#[test]
fn spear_zombie_fight_in_the_open_matches_the_recording() {
    let recorded = recorded_fight(&recording(FIELD), false);
    assert_eq!(recorded.hits().len(), 3, "recorded: {:?}", recorded.seen);

    let mut h = built(21, Difficulty::Hard, |_| None);
    place_player(&mut h, DVec3::new(0.5, 64.0, 0.5));
    let zombie = spear_zombie(&mut h, DVec3::new(14.5, 64.0, 0.5));
    provoke(&mut h, zombie);
    let ours = our_fight(&mut h, zombie, &recorded.moves, 1_000, false);

    assert_eq!(
        kinds(&ours.seen),
        kinds(&recorded.seen),
        "ours: {:?}\nrecorded: {:?}",
        ours.seen,
        recorded.seen
    );
    assert_eq!(
        ours.seen, recorded.seen,
        "ticks after the spear went up differ"
    );
    assert_eq!(ours.health, recorded.health, "health after each hit");
    assert_eq!(ours.knockback.len(), recorded.knockback.len());
    for (ours, recorded) in ours.knockback.iter().zip(&recorded.knockback) {
        assert!(
            (*ours - *recorded).abs().max_element() < 1.0e-3,
            "knockback {ours} vs recorded {recorded}"
        );
    }
}

/// `zombie-door-reach-spear` (Normal): the player stands in the doorway of
/// an oak-plank room behind a closed door; the spear zombie outside raises
/// its spear at once (the player is within 10 blocks) and walks up to the
/// door, but never hits through it. The spear's kinetic effects run out
/// 225 + 12 ticks after the raise; the zombie lowers the spear, backs off
/// for its cooldown and raises it again.
#[test]
fn spear_zombie_at_a_closed_door_matches_the_recording() {
    let recorded = recorded_fight(&recording(DOOR_REACH), true);
    assert!(recorded.hits().is_empty(), "recorded: {:?}", recorded.seen);

    let planks = BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:oak_planks")
        .unwrap();
    let lower = door_facing("minecraft:wooden_door", "north", false, false);
    let upper = door_facing("minecraft:wooden_door", "north", false, true);
    let mut h = built(22, Difficulty::Normal, move |pos| {
        let room =
            (-2..=1).contains(&pos.x) && (-2..=2).contains(&pos.z) && (63..=66).contains(&pos.y);
        if !room {
            return None;
        }
        if pos == IVec3::new(1, 64, 0) {
            return Some(lower);
        }
        if pos == IVec3::new(1, 65, 0) {
            return Some(upper);
        }
        let shell = pos.x == -2 || pos.x == 1 || pos.z.abs() == 2 || pos.y == 63 || pos.y == 66;
        Some(if shell { planks } else { air() })
    });
    place_player(&mut h, DVec3::new(1.5, 64.0, 0.5));
    let zombie = spear_zombie(&mut h, DVec3::new(5.5, 64.0, 0.5));
    provoke(&mut h, zombie);
    let span = recorded.seen.last().map_or(0, |(tick, _)| *tick);
    let ours = our_fight(&mut h, zombie, &[], span + 200, true);

    let spear = |fight: &Fight| -> Vec<(u64, Seen)> {
        fight
            .seen
            .iter()
            .copied()
            .filter(|(_, seen)| matches!(seen, Seen::Spear(_)))
            .collect()
    };
    let (ours_spear, recorded_spear) = (spear(&ours), spear(&recorded));
    assert!(
        ours.hits().is_empty(),
        "hit through the door: {:?}",
        ours.seen
    );
    assert!(
        ours_spear.len() >= recorded_spear.len(),
        "ours: {:?}\nrecorded: {:?}",
        ours.seen,
        recorded.seen
    );
    assert_eq!(
        kinds(&ours_spear[..recorded_spear.len()]),
        kinds(&recorded_spear),
        "ours: {ours_spear:?}\nrecorded: {recorded_spear:?}"
    );
    // Raised, then lowered when the kinetic effects ran out.
    assert_eq!(
        ours_spear[1].0, recorded_spear[1].0,
        "ours: {ours_spear:?}\nrecorded: {recorded_spear:?}"
    );
}

/// The `MobEquipment` our zombie's viewers get when its main hand becomes
/// an iron spear, byte for byte as BDS sent it in `zombie-spear-field`
/// after `/replaceitem`: network id 857, count 1, aux 0, no stack network
/// id, block runtime id 0, user data `{Damage: 0}`, slot 0, selected 0,
/// container 0.
#[test]
fn spear_mob_equipment_matches_the_recorded_bytes() {
    let records = recording(FIELD);
    let recorded = records
        .iter()
        .find(|r| r.s2c && matches!(r.packet, McpePacketData::MobEquipmentPacket(_)))
        .expect("the recording equips the zombie");
    let McpePacketData::MobEquipmentPacket(bds) = &recorded.packet else {
        unreachable!()
    };
    let mut h = built(23, Difficulty::Hard, |_| None);
    let world = h.world();
    let spear = world_stack(world, "minecraft:iron_spear", 1).expect("vanilla spear");
    let mut equipment = MobEquipment::default();
    equipment.set(GearSlot::Mainhand, spear, false);
    let items = &world.resource::<ItemRegistryResource>().0;
    let mut out = Vec::new();
    changed_packets(
        bds.target_runtime_id.actor_runtime_id as i64,
        &mut equipment,
        items,
        block_network_ids(world),
        &mut out,
    );
    assert_eq!(out.len(), 1);
    let mut ours = Vec::new();
    out[0]
        .data
        .encode_inner(&mut ours, 0, 0)
        .expect("encode MobEquipment");
    assert_eq!(
        ours,
        frame(&recorded.bytes).to_vec(),
        "ours: {:?}\nBDS: {bds:?}",
        out[0].data
    );
}
