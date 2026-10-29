//! Hashed block network ids against an independent derivation.
//!
//! `BlockData::network_hashes` are BDS's measured `hashedNetworkId`s. A client in
//! hashed mode derives the same value itself: 32-bit FNV-1a over the little-endian
//! NBT compound `{name, states}` with the state properties in key order (the
//! algorithm gophertunnel implements). Every canonical state must agree, or a
//! hashed-mode client resolves the wrong block or none.

use unastar_data::blocks::{ALL_BLOCKS, StateValue};

fn put_string(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn fnv1a_32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |hash, &byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

#[test]
fn every_state_hash_is_fnv1a_of_its_little_endian_state_nbt() {
    let mut checked = 0;
    for block in ALL_BLOCKS.iter() {
        assert_eq!(block.network_hashes.len(), block.state_id_count as usize);
        for (offset, &expected) in block.network_hashes.iter().enumerate() {
            let runtime_id = block.min_state_id + offset as u32;
            let mut nbt = vec![10];
            put_string(&mut nbt, "");
            nbt.push(8);
            put_string(&mut nbt, "name");
            put_string(&mut nbt, block.identifier);
            nbt.push(10);
            put_string(&mut nbt, "states");
            // `properties` is sorted by name.
            for property in block.properties {
                match block.state_value(runtime_id, property.name).unwrap() {
                    StateValue::Byte(value) => {
                        nbt.push(1);
                        put_string(&mut nbt, property.name);
                        nbt.push(value);
                    }
                    StateValue::Int(value) => {
                        nbt.push(3);
                        put_string(&mut nbt, property.name);
                        nbt.extend_from_slice(&value.to_le_bytes());
                    }
                    StateValue::Str(value) => {
                        nbt.push(8);
                        put_string(&mut nbt, property.name);
                        put_string(&mut nbt, value);
                    }
                }
            }
            nbt.extend_from_slice(&[0, 0]);
            assert_eq!(
                fnv1a_32(&nbt),
                expected,
                "{} state {runtime_id}",
                block.identifier
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 22_091);
}
