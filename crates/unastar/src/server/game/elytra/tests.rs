//! Elytra wear against BDS 1.26.51.
//!
//! `testdata/bds-elytra.jsonl` is cut (`tools/bds-sweep/combat_fixture.py
//! --no-inputs --also InventoryContent`) from a `bds-sweep-client` capture of
//! `tools/bds-sweep/scripts/elytra-3.txt` played against plain BDS 1.26.51
//! (2026-11-10). Each of its seven glides starts at the input tick BDS stamps
//! on the player's gliding flag and ends at the script's teleport to the
//! ground (or, for the elytra with 12 durability left, when BDS stops it);
//! after each landing BDS's whole-inventory resync shows the elytra's damage.
//!
//! The 20-tick wear interval's phase is not pinned down to the tick: the
//! glides' measured lengths disagree with any single phase by up to two
//! ticks. So each glide is replayed through [`tick_gliding`] for every length
//! within two ticks of the measured one, and one of them must end with BDS's
//! damage; the glide that BDS stopped must stop within two ticks of BDS's
//! stamp, at the same damage.

use bevy_ecs::prelude::*;
use jolyne::valentine::McpePacketData;
use jolyne::valentine::types::DataItemEntryPayload;

use super::*;
use crate::item::enchant::{durability_damage, with_durability_damage};
use crate::server::game::capture_replay::{BDS_PLAYER, records};

const BDS: &str = include_str!("testdata/bds-elytra.jsonl");
/// The script's teleports to the ground, by glide.
const LANDINGS: [u64; 7] = [66, 131, 198, 283, 370, 516, 956];
/// Damage each glide's elytra started with.
const STARTING_DAMAGE: [i32; 7] = [0, 0, 0, 0, 0, 0, 420];

/// The glides' start stamps, BDS's own stop stamp (if it stopped one) and
/// the damage read back after each landing.
fn measured() -> Vec<(u64, Option<u64>, i32)> {
    let records = records(BDS);
    let mut starts = Vec::new();
    let mut stops = Vec::new();
    let mut damages = Vec::new();
    let mut gliding = false;
    for record in records.iter().filter(|record| !record.from_client) {
        match &record.packet {
            McpePacketData::SetActorDataPacket(data)
                if data.target_runtime_id.actor_runtime_id == BDS_PLAYER =>
            {
                let Some(flags) =
                    data.actor_data
                        .data
                        .iter()
                        .find_map(|entry| match &entry.payload {
                            DataItemEntryPayload::DataItemInt64Payload(flags) if entry.id == 0 => {
                                Some(flags.value)
                            }
                            _ => None,
                        })
                else {
                    continue;
                };
                let now = flags & (1 << jolyne::valentine::actor_flags::GLIDING) != 0;
                if now && !gliding {
                    starts.push(data.tick.inputtick);
                } else if !now && gliding {
                    stops.push(data.tick.inputtick);
                }
                gliding = now;
            }
            McpePacketData::InventoryContentPacket(content) if content.container_id == 120 => {
                if let Some(elytra) = content.slots.get(1).filter(|item| item.id != 0) {
                    damages.push(wire_damage(&elytra.user_data_buffer));
                }
            }
            _ => {}
        }
    }
    assert_eq!(starts.len(), 7, "seven glides");
    assert_eq!(damages.len(), 7, "seven read-backs");
    starts
        .into_iter()
        .zip(stops)
        .zip(damages)
        .enumerate()
        .map(|(index, ((start, stop), damage))| {
            // BDS stopped the last glide itself, before the teleport.
            let own_stop = (stop + 3 < LANDINGS[index]).then_some(stop);
            (start, own_stop, damage)
        })
        .collect()
}

/// The `Damage` an item's wire user data names: the little-endian int after
/// the tag name in its NBT (0 when it has none).
fn wire_damage(user_data: &[u8]) -> i32 {
    let name = b"Damage";
    user_data
        .windows(name.len())
        .position(|window| window == name)
        .and_then(|at| user_data.get(at + name.len()..at + name.len() + 4))
        .map_or(0, |bytes| i32::from_le_bytes(bytes.try_into().unwrap()))
}

/// Glide `ticks` ticks with an elytra of `damage`; the damage after and the
/// tick the glide stopped by itself, if it did.
fn glide(damage: i32, ticks: u64) -> (i32, Option<u64>) {
    let mut world = World::new();
    world.insert_resource(crate::entity::EntityRandom::seeded(3));
    let mut armour = ArmourInventory::default();
    armour
        .set_chestplate(with_durability_damage(
            &crate::item::test_support::stack(ELYTRA, 1),
            damage,
        ))
        .unwrap();
    let player = world
        .spawn((
            armour,
            GameMode::Survival,
            PlayerState {
                gliding: true,
                ..Default::default()
            },
        ))
        .id();
    let mut players = world.query::<(Entity, &PlayerState)>();
    let mut stopped = None;
    for tick in 1..=ticks {
        tick_gliding(&mut world, &mut players);
        if stopped.is_none() && !world.get::<PlayerState>(player).unwrap().gliding {
            stopped = Some(tick);
        }
    }
    let chest = world
        .get::<ArmourInventory>(player)
        .unwrap()
        .chestplate()
        .clone();
    (durability_damage(&chest), stopped)
}

#[test]
fn gliding_wears_the_elytra_like_bds() {
    for (index, (start, own_stop, bds_damage)) in measured().into_iter().enumerate() {
        let damage = STARTING_DAMAGE[index];
        match own_stop {
            None => {
                let length = LANDINGS[index] - start;
                let fits = (length.saturating_sub(2)..=length + 2)
                    .any(|ticks| glide(damage, ticks) == (bds_damage, None));
                assert!(
                    fits,
                    "glide {index} of {length} ticks: BDS left damage {bds_damage}, ours {:?}",
                    glide(damage, length)
                );
            }
            Some(stop) => {
                let (ours, stopped) = glide(damage, LANDINGS[index] - start);
                assert_eq!(ours, bds_damage, "the stopped glide's damage");
                let stopped = stopped.expect("the glide stops by itself") as i64;
                assert!(
                    (stopped - (stop - start) as i64).abs() <= 2,
                    "stopped after {stopped} ticks, BDS after {}",
                    stop - start
                );
            }
        }
    }
}

#[test]
fn no_glide_without_an_elytra_with_two_durability_left() {
    for (chest, glides) in [
        (None, false),
        (Some(("minecraft:iron_chestplate", 0)), false),
        (Some((ELYTRA, 431)), false),
        (Some((ELYTRA, 430)), true),
    ] {
        let mut world = World::new();
        let mut armour = ArmourInventory::default();
        if let Some((item, damage)) = chest {
            armour
                .set_chestplate(with_durability_damage(
                    &crate::item::test_support::stack(item, 1),
                    damage,
                ))
                .unwrap();
        }
        let player = world
            .spawn((
                armour,
                GameMode::Survival,
                PlayerState {
                    gliding: true,
                    ..Default::default()
                },
            ))
            .id();
        let mut players = world.query::<(Entity, &PlayerState)>();
        tick_gliding(&mut world, &mut players);
        assert_eq!(
            world.get::<PlayerState>(player).unwrap().gliding,
            glides,
            "{chest:?}"
        );
    }
}
