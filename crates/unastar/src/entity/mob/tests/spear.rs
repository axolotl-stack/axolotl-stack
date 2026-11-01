//! Spear-holding zombies replayed against the BDS 1.26.51 recordings of the
//! client-diff scenario `zombie-spear-field`
//! (`crates/client-diff/tests/fixtures/bds-zombie/`).

use bytes::Bytes;
use jolyne::valentine::{McpePacketArgs, McpePacketData};

use super::doors::built;
use crate::entity::mob::gear::replicate::changed_packets;
use crate::entity::mob::gear::{GearSlot, MobEquipment};
use crate::server::game::types::{ItemRegistryResource, block_network_ids};
use crate::server::game::world_stack;
use crate::world::rules::Difficulty;

const FIELD: &str =
    include_str!("../../../../../client-diff/tests/fixtures/bds-zombie/zombie-spear-field.jsonl");

/// One decoded packet of a recording.
struct Recorded {
    s2c: bool,
    /// The packet as recorded: its header and body.
    bytes: Vec<u8>,
    packet: McpePacketData,
}

/// The packets of `text` (a client-diff JSONL recording) these tests read,
/// decoded.
fn recording(text: &str) -> Vec<Recorded> {
    const READ: [&str; 1] = ["MobEquipment"];
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
