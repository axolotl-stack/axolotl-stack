//! Random enchanting as BDS 1.26.51 performs it for loot and mob gear
//! (`enchant_random_gear`): the enchantment table, the selection of
//! enchantments for an item and an enchanting level, and the rule that adds
//! one enchantment to an item's existing set.
//!
//! The caller classifies items ([`ItemEnchanting`]: which enchantment slots
//! the item belongs to and its enchantability); this module has no item
//! registry. Enchantments are the generated [`Enchantment`]; the table rows
//! (weights, slots, costs, groups, lists) are generated from
//! `unastar-data/data/measured/enchantments.json`.

use rand::Rng;

use crate::ids::Enchantment;

/// Enchantment slot bits an item belongs to and enchantments apply to.
pub mod slot {
    pub const ARMOR_HEAD: u32 = 0x1;
    pub const ARMOR_TORSO: u32 = 0x2;
    pub const ARMOR_FEET: u32 = 0x4;
    pub const ARMOR_LEGS: u32 = 0x8;
    pub const SWORD: u32 = 0x10;
    pub const BOW: u32 = 0x20;
    pub const HOE: u32 = 0x40;
    pub const SHEARS: u32 = 0x80;
    pub const AXE: u32 = 0x200;
    pub const PICKAXE: u32 = 0x400;
    pub const SHOVEL: u32 = 0x800;
    pub const FISHING_ROD: u32 = 0x1000;
    pub const ELYTRA: u32 = 0x4000;
    pub const TRIDENT: u32 = 0x8000;
    pub const CROSSBOW: u32 = 0x10000;
    pub const MACE: u32 = 0x400000;
    pub const MELEE_SPEAR: u32 = 0x800000;
    /// Enchanted books: every enchantment.
    pub const ALL: u32 = u32::MAX;
}

/// How an item takes enchantments: its slot bits and enchantability. Slots
/// `0` (or enchantability `0`) means random enchanting leaves it unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemEnchanting {
    pub slots: u32,
    pub value: i32,
}

/// One enchantment on an item: an entry of its ordered enchantment list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnchantmentInstance {
    pub enchantment: Enchantment,
    pub level: u8,
}

impl From<(Enchantment, u8)> for EnchantmentInstance {
    fn from((enchantment, level): (Enchantment, u8)) -> Self {
        Self { enchantment, level }
    }
}

/// One row of the BDS enchantment table, generated for each
/// [`Enchantment`] from `unastar-data/data/measured/enchantments.json`.
#[derive(Debug)]
pub(crate) struct Definition {
    /// Resource-pack name key.
    pub(crate) lang_key: &'static str,
    /// Selection weight (BDS frequency 30/10/3/1 weighs 10/5/2/1).
    pub(crate) weight: u32,
    pub(crate) primary_slots: u32,
    pub(crate) secondary_slots: u32,
    /// Minimum enchanting cost of level `l`: `cost_base + cost_per_level * (l - 1)`.
    pub(crate) cost_base: i32,
    pub(crate) cost_per_level: i32,
    pub(crate) max_level: u8,
    pub(crate) treasure_only: bool,
    /// False: random enchanting never offers it, even with treasure allowed.
    pub(crate) discoverable: bool,
    /// Enchantments sharing a non-zero group exclude each other.
    pub(crate) group: u8,
    /// The enchantment it additionally excludes (trident, crossbow pairs).
    pub(crate) excludes: Option<Enchantment>,
    /// Item enchantments are kept in three lists; compatibility is only
    /// checked against the list the new enchantment goes to.
    pub(crate) list: u8,
}

/// Spread of the enchanting level: `level × (1 + uniform(-0.15, 0.15))`.
const LEVEL_SPREAD: f32 = 0.3;
const LEVEL_SPREAD_OFFSET: f32 = -0.15;
/// Each extra enchantment is kept while a uniform roll in `0..50` is at most
/// `cost`.
const EXTRA_ENCHANT_ROLL: i32 = 50;

impl Definition {
    fn min_cost(&self, level: i32) -> i32 {
        self.cost_base + self.cost_per_level * (level - 1)
    }

    fn compatible_with(&self, other: Enchantment) -> bool {
        let group = other.definition().group;
        self.excludes != Some(other) && (self.group == 0 || self.group != group)
    }

    fn fits(&self, slots: u32) -> bool {
        (self.primary_slots | self.secondary_slots) & slots != 0
    }
}

/// Add `enchantment` to `existing` (an item of `slots`) the way BDS item
/// enchant lists do: the enchantment must fit the item's slots and be
/// compatible with the enchantments in its list; an enchantment the item
/// already has below its maximum level is raised (one level when equal,
/// else to the higher level). Returns whether the item changed.
pub fn add_enchantment(
    existing: &mut Vec<EnchantmentInstance>,
    slots: u32,
    enchantment: EnchantmentInstance,
) -> bool {
    let new = enchantment.enchantment.definition();
    if slots == 0 || !new.fits(slots) {
        return false;
    }
    for current in existing.iter_mut() {
        if current.enchantment.definition().list != new.list {
            continue;
        }
        if current.enchantment == enchantment.enchantment {
            if current.level >= new.max_level {
                return false;
            }
            current.level = if current.level == enchantment.level {
                current.level + 1
            } else {
                current.level.max(enchantment.level)
            };
            return true;
        }
        if !new.compatible_with(current.enchantment) {
            return false;
        }
    }
    existing.push(enchantment);
    true
}

/// Choose enchantments for an item at enchanting `level`, as BDS does for
/// random gear and loot (no enchanting-table bonus): the level is spread by
/// the item's enchantability, every applicable enchantment gets the highest
/// level whose minimum cost it reaches, then one weighted pick plus further
/// picks while a uniform roll in `0..50` is at most `cost` (cost decays to
/// `cost * 4 / 5 + 1` each time). Picks incompatible with earlier ones are
/// dropped.
pub fn select_enchantments(
    item: ItemEnchanting,
    level: i32,
    allow_treasure: bool,
    rng: &mut impl Rng,
) -> Vec<EnchantmentInstance> {
    let mut chosen = Vec::new();
    if item.slots == 0 || item.value <= 0 {
        return chosen;
    }
    let quarter = item.value / 4 + 1;
    let spread = rng.gen_range(0..quarter) + rng.gen_range(0..quarter);
    let factor = rng.r#gen::<f32>() * LEVEL_SPREAD + LEVEL_SPREAD_OFFSET + 1.0;
    let mut cost = (((spread + 1 + level) as f32) * factor + 0.5) as i32;
    cost = cost.max(1);

    let mut candidates: Vec<(EnchantmentInstance, u32)> = Enchantment::ALL
        .into_iter()
        .filter(|enchantment| {
            let d = enchantment.definition();
            (item.slots == slot::ALL || d.fits(item.slots))
                && (!d.treasure_only || allow_treasure)
                && d.discoverable
        })
        .filter_map(|enchantment| {
            let d = enchantment.definition();
            (1..=d.max_level)
                .rev()
                .find(|level| d.min_cost(i32::from(*level)) <= cost)
                .map(|level| (EnchantmentInstance { enchantment, level }, d.weight))
        })
        .collect();
    let count = candidates.len();
    if count == 0 {
        return chosen;
    }
    let mut total: u32 = candidates.iter().map(|(_, w)| w).sum();
    let first = pick_weighted(&mut candidates, &mut total, rng);
    add_enchantment(&mut chosen, item.slots, first);
    let mut picked = 1;
    while rng.gen_range(0..EXTRA_ENCHANT_ROLL) <= cost && picked < count {
        let next = pick_weighted(&mut candidates, &mut total, rng);
        add_enchantment(&mut chosen, item.slots, next);
        cost = cost * 4 / 5 + 1;
        picked += 1;
    }
    chosen.sort_by_key(|e| e.enchantment.definition().list);
    chosen
}

/// Draw one candidate by weight and remove it from the pool.
fn pick_weighted(
    candidates: &mut Vec<(EnchantmentInstance, u32)>,
    total: &mut u32,
    rng: &mut impl Rng,
) -> EnchantmentInstance {
    let mut roll = i64::from(rng.gen_range(0..*total));
    let index = candidates
        .iter()
        .position(|(_, weight)| {
            roll -= i64::from(*weight);
            roll < 0
        })
        .expect("roll below the total weight");
    let (enchantment, weight) = candidates.remove(index);
    *total -= weight;
    enchantment
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    const IRON_SWORD: ItemEnchanting = ItemEnchanting {
        slots: slot::SWORD,
        value: 14,
    };
    const DIAMOND_HELMET: ItemEnchanting = ItemEnchanting {
        slots: slot::ARMOR_HEAD,
        value: 10,
    };

    #[test]
    fn selected_enchantments_fit_the_item_and_never_conflict() {
        for seed in 0..500 {
            let mut rng = StdRng::seed_from_u64(seed);
            for (item, level) in [(IRON_SWORD, 22), (DIAMOND_HELMET, 15), (IRON_SWORD, 5)] {
                let chosen = select_enchantments(item, level, false, &mut rng);
                assert!(!chosen.is_empty(), "a positive level always enchants");
                for (i, e) in chosen.iter().enumerate() {
                    let d = e.enchantment.definition();
                    assert!(d.fits(item.slots));
                    assert!(!d.treasure_only, "no treasure enchantments: {e:?}");
                    assert!((1..=d.max_level).contains(&e.level));
                    for other in &chosen[i + 1..] {
                        assert_ne!(e.enchantment, other.enchantment);
                        assert!(d.compatible_with(other.enchantment), "{e:?} with {other:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn levels_follow_the_minimum_costs() {
        // Sharpness V needs cost 45; a level-5 roll on enchantability 14
        // reaches at most (3 + 3 + 1 + 5) * 1.15 + 0.5 = 14.3.
        for seed in 0..300 {
            let chosen =
                select_enchantments(IRON_SWORD, 5, false, &mut StdRng::seed_from_u64(seed));
            for e in chosen {
                let d = e.enchantment.definition();
                assert!(d.min_cost(i32::from(e.level)) <= 14, "{e:?}");
            }
        }
    }

    #[test]
    fn unenchantable_items_stay_plain() {
        let mut rng = StdRng::seed_from_u64(1);
        let elytra = ItemEnchanting {
            slots: slot::ELYTRA,
            value: 0,
        };
        assert!(select_enchantments(elytra, 30, false, &mut rng).is_empty());
        let plain = ItemEnchanting { slots: 0, value: 9 };
        assert!(select_enchantments(plain, 30, false, &mut rng).is_empty());
    }

    #[test]
    fn adding_respects_slots_lists_and_exclusive_groups() {
        let sharpness = EnchantmentInstance::from((Enchantment::Sharpness, 2));
        let smite = EnchantmentInstance::from((Enchantment::Smite, 1));
        let knockback = EnchantmentInstance::from((Enchantment::Knockback, 1));
        let protection = EnchantmentInstance::from((Enchantment::Protection, 1));
        let mut sword = Vec::new();
        assert!(add_enchantment(&mut sword, slot::SWORD, sharpness));
        assert!(
            !add_enchantment(&mut sword, slot::SWORD, smite),
            "damage group"
        );
        assert!(add_enchantment(&mut sword, slot::SWORD, knockback));
        assert!(
            !add_enchantment(&mut sword, slot::SWORD, protection),
            "armour only"
        );
        assert!(
            add_enchantment(&mut sword, slot::SWORD, sharpness),
            "equal level raises"
        );
        assert_eq!(sword[0], (Enchantment::Sharpness, 3).into());
        let mut trident = Vec::new();
        assert!(add_enchantment(
            &mut trident,
            slot::TRIDENT,
            (Enchantment::Riptide, 1).into()
        ));
        assert!(!add_enchantment(
            &mut trident,
            slot::TRIDENT,
            (Enchantment::Channeling, 1).into()
        ));
        assert!(!add_enchantment(
            &mut trident,
            slot::TRIDENT,
            (Enchantment::Loyalty, 1).into()
        ));
    }

    /// The generated rows are the table this module carried before decision
    /// 0052, spot-checked where the shorthand used named constants.
    #[test]
    fn generated_rows_keep_the_measured_table() {
        let unbreaking = Enchantment::Unbreaking.definition();
        assert_eq!(
            (unbreaking.primary_slots, unbreaking.secondary_slots),
            (0x0081_1e7f, 0x0072_e1c0)
        );
        assert_eq!(Enchantment::Mending.definition().group, 5);
        assert_eq!(Enchantment::Vanishing.definition().primary_slots, slot::ALL);
        let soul_speed = Enchantment::SoulSpeed.definition();
        assert!(soul_speed.treasure_only && !soul_speed.discoverable);
        assert_eq!(
            Enchantment::Riptide.definition().excludes,
            Some(Enchantment::Channeling)
        );
        let sharpness = Enchantment::Sharpness.definition();
        assert_eq!(
            (
                sharpness.weight,
                sharpness.primary_slots,
                sharpness.secondary_slots,
                sharpness.cost_base,
                sharpness.cost_per_level,
                sharpness.group,
                sharpness.list
            ),
            (10, 0x80_0010, slot::AXE, 1, 11, 1, 1)
        );
    }
}
