//! Enchantments, durability and the other item instance data stored in an
//! item's user data (NBT compound, can-place-on / can-destroy lists).
//!
//! Bedrock keeps both in the item's NBT compound (see
//! [`crate::item::user_data`] for the buffer layout): enchantments as the
//! `ench` list of `{id: short, lvl: short}` compounds, tool wear as the
//! `Damage` int (dragonfly `item/stack.go`, gophertunnel item NBT).

use std::io::Cursor;

use zuri_nbt::encoding::LittleEndian;
use zuri_nbt::{NBTTag, tag};

use super::ItemStack;

/// Bedrock enchantment IDs (the `Enchant::Type` enum of Mojang's public
/// bedrock-protocol-docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i16)]
pub enum Enchantment {
    AquaAffinity = 8,
    Looting = 14,
    Efficiency = 15,
    SilkTouch = 16,
    Unbreaking = 17,
    Fortune = 18,
    Infinity = 22,
}

impl Enchantment {
    /// Resolve a loot-table enchantment name (`silk_touch`, `fortune`).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.strip_prefix("minecraft:").unwrap_or(name) {
            "aqua_affinity" => Self::AquaAffinity,
            "looting" => Self::Looting,
            "efficiency" => Self::Efficiency,
            "silk_touch" => Self::SilkTouch,
            "unbreaking" => Self::Unbreaking,
            "fortune" => Self::Fortune,
            "infinity" => Self::Infinity,
            _ => return None,
        })
    }
}

/// The NBT marker of a user-data buffer that carries a compound.
const NBT_PRESENT: i16 = -1;
const NBT_VERSION: u8 = 1;

/// Split a user-data buffer into its NBT compound and the remaining bytes
/// (can-place-on / can-destroy lists, shield tick).
fn split_user_data(buffer: &[u8]) -> Option<(Option<tag::Compound>, &[u8])> {
    let marker = i16::from_le_bytes(buffer.get(..2)?.try_into().ok()?);
    if marker != NBT_PRESENT {
        return Some((None, buffer.get(2..)?));
    }
    if *buffer.get(2)? != NBT_VERSION {
        return None;
    }
    let mut cursor = Cursor::new(buffer.get(3..)?);
    let NBTTag::Compound(compound) = NBTTag::read(&mut cursor, LittleEndian).ok()? else {
        return None;
    };
    let consumed = 3 + cursor.position() as usize;
    Some((Some(compound), buffer.get(consumed..)?))
}

/// The item's NBT compound, if its user data carries one.
pub fn item_tag(stack: &ItemStack) -> Option<tag::Compound> {
    split_user_data(stack.nbt.as_deref()?)?.0
}

/// `stack` with its NBT compound replaced by `compound`, keeping the rest of
/// the user data.
pub fn with_item_tag(stack: &ItemStack, compound: tag::Compound) -> ItemStack {
    let default = super::user_data::network_user_data(stack.item_id(), None);
    let buffer = stack.nbt.as_deref().unwrap_or(&default);
    let tail = split_user_data(buffer).map_or(&default[2..], |(_, tail)| tail);
    let mut out = Vec::with_capacity(buffer.len() + 16);
    out.extend_from_slice(&NBT_PRESENT.to_le_bytes());
    out.push(NBT_VERSION);
    NBTTag::Compound(compound)
        .write(&mut out, LittleEndian)
        .expect("writing NBT into a Vec cannot fail");
    out.extend_from_slice(tail);
    stack.clone().with_nbt(out)
}

/// Read one block-name list (`i32` count of u16-prefixed strings) from the
/// start of `bytes`; returns the names and the bytes after the list.
fn read_block_list(bytes: &[u8]) -> Option<(Vec<String>, &[u8])> {
    let count = i32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
    let mut rest = bytes.get(4..)?;
    let mut names = Vec::new();
    for _ in 0..count.max(0) {
        let len = usize::from(u16::from_le_bytes(rest.get(..2)?.try_into().ok()?));
        names.push(String::from_utf8_lossy(rest.get(2..2 + len)?).into_owned());
        rest = rest.get(2 + len..)?;
    }
    Some((names, rest))
}

fn write_block_list(out: &mut Vec<u8>, blocks: &[String]) {
    out.extend_from_slice(&(blocks.len() as i32).to_le_bytes());
    for block in blocks {
        let bytes = &block.as_bytes()[..block.len().min(usize::from(u16::MAX))];
        out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(bytes);
    }
}

/// `stack` with its can-place-on and can-destroy block lists replaced,
/// keeping its NBT compound and shield blocking tick.
pub fn with_block_lists(
    stack: &ItemStack,
    can_place_on: &[String],
    can_destroy: &[String],
) -> ItemStack {
    let default = super::user_data::network_user_data(stack.item_id(), None);
    let buffer = stack.nbt.as_deref().unwrap_or(&default);
    let (compound, tail) = split_user_data(buffer).unwrap_or((None, &default[2..]));
    let after_lists = read_block_list(tail)
        .and_then(|(_, rest)| read_block_list(rest))
        .map_or(&default[10..], |(_, rest)| rest);
    let mut out = Vec::with_capacity(buffer.len() + 32);
    match compound {
        Some(compound) => {
            out.extend_from_slice(&NBT_PRESENT.to_le_bytes());
            out.push(NBT_VERSION);
            NBTTag::Compound(compound)
                .write(&mut out, LittleEndian)
                .expect("writing NBT into a Vec cannot fail");
        }
        None => out.extend_from_slice(&0i16.to_le_bytes()),
    }
    write_block_list(&mut out, can_place_on);
    write_block_list(&mut out, can_destroy);
    out.extend_from_slice(after_lists);
    stack.clone().with_nbt(out)
}

/// The can-place-on and can-destroy block lists of `stack`'s user data.
pub fn block_lists(stack: &ItemStack) -> (Vec<String>, Vec<String>) {
    let lists = stack.nbt.as_deref().and_then(|buffer| {
        let (_, tail) = split_user_data(buffer)?;
        let (place_on, rest) = read_block_list(tail)?;
        let (destroy, _) = read_block_list(rest)?;
        Some((place_on, destroy))
    });
    lists.unwrap_or_default()
}

fn as_i32(tag: &NBTTag) -> Option<i32> {
    match tag {
        NBTTag::Byte(v) => Some(i32::from(v.0)),
        NBTTag::Short(v) => Some(i32::from(v.0)),
        NBTTag::Int(v) => Some(v.0),
        _ => None,
    }
}

/// Level of `enchantment` on `stack` (0 when absent).
pub fn enchantment_level(stack: &ItemStack, enchantment: Enchantment) -> u8 {
    let Some(compound) = item_tag(stack) else {
        return 0;
    };
    let Some(NBTTag::List(list)) = compound.0.get("ench") else {
        return 0;
    };
    list.0
        .iter()
        .filter_map(|entry| match entry {
            NBTTag::Compound(entry) => Some(entry),
            _ => None,
        })
        .find(|entry| entry.0.get("id").and_then(as_i32) == Some(i32::from(enchantment as i16)))
        .and_then(|entry| entry.0.get("lvl").and_then(as_i32))
        .map_or(0, |level| level.clamp(0, i32::from(u8::MAX)) as u8)
}

/// Every enchantment on `stack` as `(Bedrock enchantment id, level)`, in
/// stored order.
pub fn enchantment_ids(stack: &ItemStack) -> Vec<(i16, u8)> {
    let Some(compound) = item_tag(stack) else {
        return Vec::new();
    };
    let Some(NBTTag::List(list)) = compound.0.get("ench") else {
        return Vec::new();
    };
    list.0
        .iter()
        .filter_map(|entry| match entry {
            NBTTag::Compound(entry) => Some((
                i16::try_from(entry.0.get("id").and_then(as_i32)?).ok()?,
                entry
                    .0
                    .get("lvl")
                    .and_then(as_i32)?
                    .clamp(0, i32::from(u8::MAX)) as u8,
            )),
            _ => None,
        })
        .collect()
}

/// Tool wear (`Damage`) of `stack`.
pub fn durability_damage(stack: &ItemStack) -> i32 {
    item_tag(stack)
        .and_then(|compound| compound.0.get("Damage").and_then(as_i32))
        .unwrap_or(0)
}

/// `stack` with its `Damage` set to `damage`.
pub fn with_durability_damage(stack: &ItemStack, damage: i32) -> ItemStack {
    let mut compound = item_tag(stack).unwrap_or_default();
    compound
        .0
        .insert("Damage".to_string(), NBTTag::Int(tag::Int(damage)));
    with_item_tag(stack, compound)
}

/// `stack` carrying `levels` (test and plugin helper).
pub fn with_enchantments(stack: &ItemStack, levels: &[(Enchantment, u8)]) -> ItemStack {
    with_enchantment_ids(
        stack,
        levels
            .iter()
            .map(|(enchantment, level)| (*enchantment as i16, *level)),
    )
}

/// `stack` carrying the enchantments `(Bedrock enchantment id, level)`, in
/// order, replacing any it had.
pub fn with_enchantment_ids(
    stack: &ItemStack,
    levels: impl IntoIterator<Item = (i16, u8)>,
) -> ItemStack {
    let mut compound = item_tag(stack).unwrap_or_default();
    let list = levels
        .into_iter()
        .map(|(id, level)| {
            NBTTag::Compound(tag::Compound(
                [
                    ("id".to_string(), NBTTag::Short(tag::Short(id))),
                    (
                        "lvl".to_string(),
                        NBTTag::Short(tag::Short(i16::from(level))),
                    ),
                ]
                .into_iter()
                .collect(),
            ))
        })
        .collect();
    compound
        .0
        .insert("ench".to_string(), NBTTag::List(tag::List(list)));
    with_item_tag(stack, compound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_items_have_no_enchantments_or_wear() {
        let pick = crate::item::test_support::stack("minecraft:iron_pickaxe", 1);
        assert_eq!(enchantment_level(&pick, Enchantment::Efficiency), 0);
        assert_eq!(durability_damage(&pick), 0);
    }

    #[test]
    fn enchantments_and_damage_round_trip_through_user_data() {
        let pick = crate::item::test_support::stack("minecraft:iron_pickaxe", 1);
        let pick = with_enchantments(
            &pick,
            &[(Enchantment::Efficiency, 5), (Enchantment::SilkTouch, 1)],
        );
        let pick = with_durability_damage(&pick, 17);
        assert_eq!(enchantment_level(&pick, Enchantment::Efficiency), 5);
        assert_eq!(enchantment_level(&pick, Enchantment::SilkTouch), 1);
        assert_eq!(enchantment_level(&pick, Enchantment::Fortune), 0);
        assert_eq!(durability_damage(&pick), 17);
        let buffer = pick.nbt.as_deref().unwrap();
        assert_eq!(&buffer[..3], &[0xff, 0xff, 1]);
        assert_eq!(
            &buffer[buffer.len() - 8..],
            &[0; 8],
            "empty can-place/destroy lists kept"
        );
    }

    #[test]
    fn shield_tick_survives_tag_rewrites() {
        let shield = crate::item::test_support::stack("minecraft:shield", 1);
        let shield = with_durability_damage(&shield, 3);
        let buffer = shield.nbt.as_deref().unwrap();
        assert_eq!(&buffer[buffer.len() - 16..], &[0; 16]);
        assert_eq!(durability_damage(&shield), 3);
    }
}
