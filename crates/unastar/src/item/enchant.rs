//! Enchantments, durability and the other item instance data stored in an
//! item's user data (NBT compound, can-place-on / can-destroy lists).
//!
//! Bedrock keeps both in the item's NBT compound (see
//! [`crate::item::user_data`] for the buffer layout): enchantments as the
//! `ench` list of `{id: short, lvl: short}` compounds, tool wear as the
//! `Damage` int (dragonfly `item/stack.go`, gophertunnel item NBT).
//!
//! This is the NBT edge of [`Enchantment`] (decision 0052): a stack decodes
//! its levels once when its user data is set ([`decode_levels`]), and
//! [`with_enchantments`] writes them back. `ench` entries whose id is not a
//! Bedrock enchantment stay in the buffer untouched; they never reach the
//! level table.

use std::io::Cursor;

use unastar_rules::enchanting::EnchantmentInstance;
use unastar_rules::ids::{Enchantment, Enchantments};
use zuri_nbt::encoding::LittleEndian;
use zuri_nbt::{NBTTag, tag};

use super::ItemStack;

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
    split_user_data(stack.nbt()?)?.0
}

/// `stack` with its NBT compound replaced by `compound`, keeping the rest of
/// the user data.
pub fn with_item_tag(stack: &ItemStack, compound: tag::Compound) -> ItemStack {
    let default = super::user_data::network_user_data(stack.item_id(), None);
    let buffer = stack.nbt().map_or(&default[..], |b| &b[..]);
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
    let buffer = stack.nbt().map_or(&default[..], |b| &b[..]);
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
    let lists = stack.nbt().and_then(|buffer| {
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

/// The `ench` entries of an NBT compound.
fn ench_entries(compound: &tag::Compound) -> &[NBTTag] {
    match compound.0.get("ench") {
        Some(NBTTag::List(list)) => &list.0,
        _ => &[],
    }
}

/// The enchantment an `ench` entry names; `None` for anything that is not
/// a compound with the id of a Bedrock enchantment.
fn entry_enchantment(entry: &NBTTag) -> Option<(Enchantment, &tag::Compound)> {
    let NBTTag::Compound(entry) = entry else {
        return None;
    };
    let id = i16::try_from(entry.0.get("id").and_then(as_i32)?).ok()?;
    Some((Enchantment::try_from(id).ok()?, entry))
}

fn entry_level(entry: &tag::Compound) -> Option<u8> {
    let level = entry.0.get("lvl").and_then(as_i32)?;
    Some(level.clamp(0, i32::from(u8::MAX)) as u8)
}

/// Decode the enchantment levels of a user-data buffer. The first `ench`
/// entry of an enchantment gives its level (0 without `lvl`); entries with
/// unknown ids are not levels and are skipped.
pub fn decode_levels(buffer: &[u8]) -> Enchantments {
    let Some((Some(compound), _)) = split_user_data(buffer) else {
        return Enchantments::NONE;
    };
    ench_entries(&compound)
        .iter()
        .filter_map(entry_enchantment)
        .map(|(enchantment, entry)| (enchantment, entry_level(entry).unwrap_or(0)))
        .collect()
}

/// The enchantments on `stack` in stored order, as the enchant rules add to
/// them. Entries with unknown ids or without a level are left out.
pub fn enchantment_list(stack: &ItemStack) -> Vec<EnchantmentInstance> {
    let Some(compound) = item_tag(stack) else {
        return Vec::new();
    };
    ench_entries(&compound)
        .iter()
        .filter_map(|entry| {
            let (enchantment, entry) = entry_enchantment(entry)?;
            Some(EnchantmentInstance {
                enchantment,
                level: entry_level(entry)?,
            })
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

/// `stack` carrying `enchantments` as its `ench` list, in order, as
/// `{id: short, lvl: short}` entries. Known enchantments it had are replaced;
/// entries whose id is not a Bedrock enchantment are kept after them.
pub fn with_enchantments<E: Into<EnchantmentInstance>>(
    stack: &ItemStack,
    enchantments: impl IntoIterator<Item = E>,
) -> ItemStack {
    let mut compound = item_tag(stack).unwrap_or_default();
    let unknown = ench_entries(&compound)
        .iter()
        .filter(|entry| entry_enchantment(entry).is_none())
        .cloned();
    let list = enchantments
        .into_iter()
        .map(|enchantment| {
            let EnchantmentInstance { enchantment, level } = enchantment.into();
            NBTTag::Compound(tag::Compound(
                [
                    (
                        "id".to_string(),
                        NBTTag::Short(tag::Short(enchantment.id())),
                    ),
                    (
                        "lvl".to_string(),
                        NBTTag::Short(tag::Short(i16::from(level))),
                    ),
                ]
                .into_iter()
                .collect(),
            ))
        })
        .chain(unknown)
        .collect();
    compound
        .0
        .insert("ench".to_string(), NBTTag::List(tag::List(list)));
    with_item_tag(stack, compound)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Provenance rule: comments name no scope-qualified engine type (an
    /// Outer::Inner pair of type names in backticks), even one that appears
    /// in a public changelog.
    #[test]
    fn comments_name_no_scope_qualified_engine_types() {
        let source = include_str!("enchant.rs");
        let upper = |s: &str| s.chars().next().is_some_and(|c| c.is_ascii_uppercase());
        for (number, line) in source.lines().enumerate() {
            let Some((_, comment)) = line.split_once("//") else {
                continue;
            };
            for span in comment.split('`').skip(1).step_by(2) {
                if let Some((outer, inner)) = span.split_once("::") {
                    assert!(
                        outer == "Self" || !(upper(outer) && upper(inner)),
                        "enchant.rs:{}: `{span}`",
                        number + 1
                    );
                }
            }
        }
    }

    #[test]
    fn plain_items_have_no_enchantments_or_wear() {
        let pick = crate::item::test_support::stack("minecraft:iron_pickaxe", 1);
        assert_eq!(pick.enchantment_level(Enchantment::Efficiency), 0);
        assert!(pick.enchantments().is_empty());
        assert_eq!(durability_damage(&pick), 0);
    }

    #[test]
    fn enchantments_and_damage_round_trip_through_user_data() {
        let pick = crate::item::test_support::stack("minecraft:iron_pickaxe", 1);
        let pick = with_enchantments(
            &pick,
            [(Enchantment::Efficiency, 5), (Enchantment::SilkTouch, 1)],
        );
        let pick = with_durability_damage(&pick, 17);
        assert_eq!(pick.enchantment_level(Enchantment::Efficiency), 5);
        assert_eq!(pick.enchantment_level(Enchantment::SilkTouch), 1);
        assert_eq!(pick.enchantment_level(Enchantment::Fortune), 0);
        assert_eq!(durability_damage(&pick), 17);
        let buffer = pick.nbt().unwrap();
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
        let buffer = shield.nbt().unwrap();
        assert_eq!(&buffer[buffer.len() - 16..], &[0; 16]);
        assert_eq!(durability_damage(&shield), 3);
    }

    /// The `ench` list as BDS writes it: `{id: short, lvl: short}` entries.
    fn ench_tag(entries: &[(i16, i16)]) -> tag::Compound {
        let list = entries
            .iter()
            .map(|(id, lvl)| {
                NBTTag::Compound(tag::Compound(
                    [
                        ("id".to_string(), NBTTag::Short(tag::Short(*id))),
                        ("lvl".to_string(), NBTTag::Short(tag::Short(*lvl))),
                    ]
                    .into_iter()
                    .collect(),
                ))
            })
            .collect();
        tag::Compound(
            [
                ("ench".to_string(), NBTTag::List(tag::List(list))),
                ("Damage".to_string(), NBTTag::Int(tag::Int(4))),
                (
                    "display".to_string(),
                    NBTTag::String(tag::String("kept".into())),
                ),
            ]
            .into_iter()
            .collect(),
        )
    }

    /// The level table decodes from NBT once without touching the stored
    /// bytes, and writing the same enchantments back keeps other keys and an
    /// entry with an unknown id.
    #[test]
    fn level_table_round_trips_through_nbt() {
        let sword = crate::item::test_support::stack("minecraft:diamond_sword", 1);
        let stored = with_item_tag(&sword, ench_tag(&[(9, 3), (17, 2), (99, 1)]));
        assert_eq!(stored.enchantment_level(Enchantment::Sharpness), 3);
        assert_eq!(stored.enchantment_level(Enchantment::Unbreaking), 2);
        assert_eq!(
            stored.enchantments().iter().collect::<Vec<_>>(),
            [(Enchantment::Sharpness, 3), (Enchantment::Unbreaking, 2)],
            "the unknown id 99 is not a level"
        );
        let list = enchantment_list(&stored);
        assert_eq!(
            list,
            [
                (Enchantment::Sharpness, 3).into(),
                (Enchantment::Unbreaking, 2).into()
            ]
        );
        // Writing the same enchantments back keeps every key and the
        // unknown entry. (Compound key order is a hash map's, so a rewrite is
        // compared as NBT, as before the level table.)
        let rewritten = with_enchantments(&stored, list);
        assert_eq!(item_tag(&rewritten), item_tag(&stored));
        assert_eq!(rewritten.enchantments(), stored.enchantments());

        // Decoding never rewrites: the stack keeps the bytes it was given.
        let bytes = stored.nbt().cloned();
        let raw = ItemStack::clone(&sword).with_raw_nbt(bytes.clone());
        assert_eq!(raw.nbt(), bytes.as_ref(), "byte-identical user data");
        assert_eq!(raw.enchantments(), stored.enchantments());
        let normalised = ItemStack::clone(&sword).with_nbt(bytes.clone().unwrap());
        assert_eq!(normalised.nbt(), bytes.as_ref());
        assert_eq!(normalised, stored);
    }

    /// Lookups keep the first entry of an enchantment, as before the level
    /// table; ids are compared as numbers whatever the tag type.
    #[test]
    fn first_entry_wins_and_other_tags_are_ignored() {
        let sword = crate::item::test_support::stack("minecraft:diamond_sword", 1);
        let stored = with_item_tag(&sword, ench_tag(&[(9, 2), (9, 5), (-1, 1), (300, 4)]));
        assert_eq!(stored.enchantment_level(Enchantment::Sharpness), 2);
        assert_eq!(
            stored.enchantments().iter().count(),
            1,
            "negative and out-of-range ids are not enchantments"
        );
    }
}
