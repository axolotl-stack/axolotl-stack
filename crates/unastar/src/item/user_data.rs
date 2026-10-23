//! The `user_data_buffer` of network item descriptors.
//!
//! Every non-air item instance carries this buffer (little-endian, as BDS and
//! gophertunnel write it):
//!
//! ```text
//! i16 NBT marker   0 = no NBT; -1 = u8 version (1) + one NBT compound follows
//! i32 count        can-place-on block names (u16-prefixed strings)
//! i32 count        can-destroy block names
//! i64              blocking tick, only for minecraft:shield
//! ```
//!
//! The 26.5x client cannot read an item whose buffer is empty: a creative
//! inventory of such items made it drop the connection right after spawning
//! with `BadPacket` (`InitialConnection-90`). Air keeps an empty buffer.
//!
//! [`ItemStack::nbt`](super::ItemStack::nbt) keeps a buffer received from the
//! client verbatim; `None` stands for the default (empty) user data.

use bytes::Bytes;

/// The only item with a blocking tick (gophertunnel `shieldID`).
const SHIELD: &str = "minecraft:shield";

/// No NBT, no can-place-on or can-destroy lists.
const DEFAULT: [u8; 10] = [0; 10];
/// [`DEFAULT`] plus a zero blocking tick.
const DEFAULT_SHIELD: [u8; 18] = [0; 18];

fn default_for(item_id: &str) -> &'static [u8] {
    if item_id == SHIELD {
        &DEFAULT_SHIELD
    } else {
        &DEFAULT
    }
}

/// The buffer to send for a non-air `item_id` whose stored user data is
/// `stored` (`None`: the default).
pub fn network_user_data(item_id: &str, stored: Option<&[u8]>) -> Bytes {
    match stored {
        Some(buffer) => Bytes::copy_from_slice(buffer),
        None => Bytes::from_static(default_for(item_id)),
    }
}

/// What to store for a buffer received for `item_id`: `None` when it is the
/// default (or omitted), so it compares equal to server-created stacks.
pub fn stored_user_data(item_id: &str, buffer: &[u8]) -> Option<Vec<u8>> {
    (!buffer.is_empty() && buffer != default_for(item_id)).then(|| buffer.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BDS 1.26.51 `CreativeContent` (tools/client-diff capture): plain items
    /// carry ten zero bytes, the shield (network ID 358) eighteen.
    #[test]
    fn default_user_data_matches_bds() {
        assert_eq!(network_user_data("minecraft:oak_planks", None)[..], [0; 10]);
        assert_eq!(network_user_data(SHIELD, None)[..], [0; 18]);
    }

    #[test]
    fn received_user_data_round_trips() {
        let enchanted = b"\xff\xff\x01\x0a\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let stored = stored_user_data("minecraft:book", enchanted);
        assert_eq!(stored.as_deref(), Some(&enchanted[..]));
        assert_eq!(
            network_user_data("minecraft:book", stored.as_deref())[..],
            enchanted[..]
        );
        for item in ["minecraft:stone", SHIELD] {
            let default = network_user_data(item, None);
            assert_eq!(stored_user_data(item, &default), None);
            assert_eq!(stored_user_data(item, &[]), None);
        }
        // A shield's longer default is real data for any other item.
        assert!(stored_user_data("minecraft:stone", &[0; 18]).is_some());
    }
}
