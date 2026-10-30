//! Enchantments and durability stored in an item's user-data NBT.
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
    LuckOfTheSea = 23,
    Lure = 24,
    Impaling = 29,
    Riptide = 30,
    Loyalty = 31,
    Channeling = 32,
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
            "luck_of_the_sea" => Self::LuckOfTheSea,
            "lure" => Self::Lure,
            "impaling" => Self::Impaling,
            "riptide" => Self::Riptide,
            "loyalty" => Self::Loyalty,
            "channeling" => Self::Channeling,
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

/// Whether two stacks carry the same user data, comparing the NBT compound
/// by value: compound key order in the encoded bytes is not significant
/// (BDS stacked anvil-renamed sticks into one slot, client-diff anvil-wear).
pub fn same_user_data(a: &ItemStack, b: &ItemStack) -> bool {
    let (a, b) = (a.user_data(), b.user_data());
    match (split_user_data(&a), split_user_data(&b)) {
        (Some(a), Some(b)) => a == b,
        _ => a == b,
    }
}

/// `stack` with its NBT compound replaced by `compound`, keeping the rest of
/// the user data. An empty compound leaves the item without NBT.
pub fn with_item_tag(stack: &ItemStack, compound: tag::Compound) -> ItemStack {
    let default = super::user_data::network_user_data(stack.item_id(), None);
    let buffer = stack.nbt.as_deref().unwrap_or(&default);
    let tail = split_user_data(buffer).map_or(&default[2..], |(_, tail)| tail);
    let mut out = Vec::with_capacity(buffer.len() + 16);
    if compound.0.is_empty() {
        out.extend_from_slice(&0i16.to_le_bytes());
    } else {
        out.extend_from_slice(&NBT_PRESENT.to_le_bytes());
        out.push(NBT_VERSION);
        NBTTag::Compound(compound)
            .write(&mut out, LittleEndian)
            .expect("writing NBT into a Vec cannot fail");
    }
    out.extend_from_slice(tail);
    stack.clone().with_nbt(out)
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
    let list: Vec<(i16, i16)> = levels
        .iter()
        .map(|(enchantment, level)| (*enchantment as i16, i16::from(*level)))
        .collect();
    with_enchantment_list(stack, &list)
}

/// Every `(id, level)` in the item's `ench` list, in list order.
pub fn enchantments(stack: &ItemStack) -> Vec<(i16, i16)> {
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
                i16::try_from(entry.0.get("lvl").and_then(as_i32)?).ok()?,
            )),
            _ => None,
        })
        .collect()
}

/// `stack` with its `ench` list set to `levels`; an empty list removes it.
pub fn with_enchantment_list(stack: &ItemStack, levels: &[(i16, i16)]) -> ItemStack {
    let mut compound = item_tag(stack).unwrap_or_default();
    if levels.is_empty() {
        compound.0.remove("ench");
        return with_item_tag(stack, compound);
    }
    let list = levels
        .iter()
        .map(|(id, level)| {
            NBTTag::Compound(tag::Compound(
                [
                    ("id".to_string(), NBTTag::Short(tag::Short(*id))),
                    ("lvl".to_string(), NBTTag::Short(tag::Short(*level))),
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

/// The anvil prior-work penalty (`RepairCost`), 0 when absent.
pub fn repair_cost(stack: &ItemStack) -> i32 {
    item_tag(stack)
        .and_then(|compound| compound.0.get("RepairCost").and_then(as_i32))
        .unwrap_or(0)
}

/// Whether the item carries a `RepairCost` tag at all.
pub fn has_repair_cost(stack: &ItemStack) -> bool {
    item_tag(stack).is_some_and(|compound| compound.0.contains_key("RepairCost"))
}

/// `stack` with `RepairCost` set to `cost`, or removed for `None`.
pub fn with_repair_cost(stack: &ItemStack, cost: Option<i32>) -> ItemStack {
    let mut compound = item_tag(stack).unwrap_or_default();
    match cost {
        Some(cost) => {
            compound
                .0
                .insert("RepairCost".to_string(), NBTTag::Int(tag::Int(cost)));
        }
        None => {
            compound.0.remove("RepairCost");
        }
    }
    with_item_tag(stack, compound)
}

/// The item's custom name (`display.Name`), if it has one.
pub fn custom_name(stack: &ItemStack) -> Option<String> {
    let compound = item_tag(stack)?;
    let Some(NBTTag::Compound(display)) = compound.0.get("display") else {
        return None;
    };
    match display.0.get("Name") {
        Some(NBTTag::String(name)) => Some(name.0.clone()),
        _ => None,
    }
}

/// `stack` named `name`, or with its custom name removed for `None` (an
/// emptied `display` compound is removed too).
pub fn with_custom_name(stack: &ItemStack, name: Option<&str>) -> ItemStack {
    let mut compound = item_tag(stack).unwrap_or_default();
    let mut display = match compound.0.remove("display") {
        Some(NBTTag::Compound(display)) => display,
        _ => tag::Compound::default(),
    };
    match name {
        Some(name) => {
            display.0.insert(
                "Name".to_string(),
                NBTTag::String(tag::String(name.to_string())),
            );
        }
        None => {
            display.0.remove("Name");
        }
    }
    if !display.0.is_empty() {
        compound
            .0
            .insert("display".to_string(), NBTTag::Compound(display));
    }
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
