//! Random enchanting as BDS 1.26.51 performs it for loot and mob gear
//! (`enchant_random_gear`): the enchantment table, the selection of
//! enchantments for an item and an enchanting level, and the rule that adds
//! one enchantment to an item's existing set.
//!
//! The caller classifies items ([`ItemEnchanting`]: which enchantment slots
//! the item belongs to and its enchantability); this module has no item
//! registry. Enchantment ids are the Bedrock `Enchant::Type` numbers that item
//! NBT stores in `ench[].id`.

use rand::Rng;

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

/// One enchantment on an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnchantmentInstance {
    pub id: u8,
    pub level: u8,
}

/// One row of the BDS enchantment table.
struct Definition {
    /// Selection weight (BDS frequency 30/10/3/1 weighs 10/5/2/1).
    weight: u32,
    primary_slots: u32,
    secondary_slots: u32,
    /// Minimum enchanting cost of level `l`: `cost_base + cost_per_level * (l - 1)`.
    cost_base: i32,
    cost_per_level: i32,
    max_level: u8,
    treasure_only: bool,
    discoverable: bool,
    /// Enchantments sharing a non-zero group exclude each other.
    group: u8,
    /// The enchantment it additionally excludes (trident, crossbow pairs).
    excludes: Option<u8>,
    /// Item enchantments are kept in three lists; compatibility is only
    /// checked against the list the new enchantment goes to.
    list: u8,
}

const fn def(
    weight: u32,
    primary_slots: u32,
    secondary_slots: u32,
    cost: (i32, i32),
    max_level: u8,
    group: u8,
    list: u8,
) -> Definition {
    Definition {
        weight,
        primary_slots,
        secondary_slots,
        cost_base: cost.0,
        cost_per_level: cost.1,
        max_level,
        treasure_only: false,
        discoverable: true,
        group,
        excludes: None,
        list,
    }
}

const fn treasure(mut d: Definition) -> Definition {
    d.treasure_only = true;
    d
}

/// Treasure that random enchanting never offers, even with treasure allowed.
const fn undiscoverable(mut d: Definition) -> Definition {
    d.treasure_only = true;
    d.discoverable = false;
    d
}

const fn excludes(mut d: Definition, other: u8) -> Definition {
    d.excludes = Some(other);
    d
}

const ARMOR: u32 = slot::ARMOR_HEAD | slot::ARMOR_TORSO | slot::ARMOR_FEET | slot::ARMOR_LEGS;
const DIGGER: u32 = slot::HOE | slot::AXE | slot::PICKAXE | slot::SHOVEL;
/// Unbreaking and mending: primary and secondary slot sets.
const DURABLE: (u32, u32) = (0x0081_1e7f, 0x0072_e1c0);

/// The BDS enchantment table indexed by `Enchant::Type`.
const ENCHANTMENTS: [Definition; 42] = [
    def(10, ARMOR, 0, (1, 11), 4, 3, 0),               // protection
    def(5, ARMOR, 0, (10, 8), 4, 3, 0),                // fire_protection
    def(5, slot::ARMOR_FEET, 0, (5, 6), 4, 0, 0),      // feather_falling
    def(2, ARMOR, 0, (5, 8), 4, 3, 0),                 // blast_protection
    def(5, ARMOR, 0, (3, 6), 4, 3, 0),                 // projectile_protection
    def(1, slot::ARMOR_TORSO, 0xd, (10, 20), 3, 0, 0), // thorns
    def(2, slot::ARMOR_HEAD, 0, (10, 10), 3, 0, 0),    // respiration
    def(2, slot::ARMOR_FEET, 0, (10, 10), 3, 4, 0),    // depth_strider
    def(2, slot::ARMOR_HEAD, 0, (1, 0), 1, 0, 0),      // aqua_affinity
    def(10, 0x80_0010, slot::AXE, (1, 11), 5, 1, 1),   // sharpness
    def(5, 0xc0_0010, slot::AXE, (5, 8), 5, 1, 1),     // smite
    def(5, 0xc0_0010, slot::AXE, (5, 8), 5, 1, 1),     // bane_of_arthropods
    def(5, 0x80_0010, 0, (5, 20), 2, 0, 2),            // knockback
    def(2, 0xc0_0010, 0, (10, 20), 2, 0, 1),           // fire_aspect
    def(2, 0x80_0010, 0, (15, 9), 3, 0, 1),            // looting
    def(10, DIGGER, slot::SHEARS, (1, 10), 5, 0, 2),   // efficiency
    def(1, DIGGER, slot::SHEARS, (15, 0), 1, 2, 1),    // silk_touch
    def(5, DURABLE.0, DURABLE.1, (5, 8), 3, 0, 1),     // unbreaking
    def(2, DIGGER, 0, (15, 9), 3, 2, 1),               // fortune
    def(10, slot::BOW, 0, (1, 10), 5, 0, 2),           // power
    def(2, slot::BOW, 0, (12, 20), 2, 0, 2),           // punch
    def(2, slot::BOW, 0, (20, 0), 1, 0, 1),            // flame
    def(1, slot::BOW, 0, (20, 0), 1, 5, 2),            // infinity
    def(2, slot::FISHING_ROD, 0, (15, 9), 3, 0, 1),    // luck_of_the_sea
    def(2, slot::FISHING_ROD, 0, (15, 9), 3, 0, 2),    // lure
    treasure(def(2, slot::ARMOR_FEET, 0, (10, 10), 2, 4, 0)), // frost_walker
    treasure(def(2, DURABLE.0, DURABLE.1, (25, 25), 1, 5, 2)), // mending
    treasure(def(1, 0x4_400f, 0, (25, 0), 1, 0, 2)),   // binding
    treasure(def(1, slot::ALL, 0, (25, 0), 1, 0, 2)),  // vanishing
    def(5, slot::TRIDENT, 0, (1, 8), 5, 0, 1),         // impaling
    excludes(def(2, slot::TRIDENT, 0, (17, 7), 3, 6, 2), 32), // riptide
    def(10, slot::TRIDENT, 0, (12, 7), 3, 6, 2),       // loyalty
    excludes(def(1, slot::TRIDENT, 0, (25, 0), 1, 0, 2), 30), // channeling
    excludes(def(2, slot::CROSSBOW, 0, (20, 0), 1, 0, 2), 34), // multishot
    excludes(def(10, slot::CROSSBOW, 0, (1, 10), 4, 0, 2), 33), // piercing
    def(5, slot::CROSSBOW, 0, (12, 20), 3, 0, 2),      // quick_charge
    undiscoverable(def(1, slot::ARMOR_FEET, 0, (10, 10), 3, 0, 0)), // soul_speed
    undiscoverable(def(1, slot::ARMOR_LEGS, 0, (10, 10), 3, 0, 0)), // swift_sneak
    undiscoverable(def(2, slot::MACE, 0, (15, 9), 3, 0, 1)), // wind_burst
    def(5, slot::MACE, 0, (5, 8), 5, 1, 1),            // density
    def(2, slot::MACE, 0, (15, 9), 4, 1, 1),           // breach
    def(5, slot::MELEE_SPEAR, 0, (5, 8), 3, 0, 1),     // lunge
];

/// Spread of the enchanting level: `level × (1 + uniform(-0.15, 0.15))`.
const LEVEL_SPREAD: f32 = 0.3;
const LEVEL_SPREAD_OFFSET: f32 = -0.15;
/// Each extra enchantment is kept while `nextInt(50) <= cost`.
const EXTRA_ENCHANT_ROLL: i32 = 50;

impl Definition {
    fn min_cost(&self, level: i32) -> i32 {
        self.cost_base + self.cost_per_level * (level - 1)
    }

    fn compatible_with(&self, other: u8) -> bool {
        let group = ENCHANTMENTS[usize::from(other)].group;
        self.excludes != Some(other) && (self.group == 0 || self.group != group)
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
    let Some(new) = ENCHANTMENTS.get(usize::from(enchantment.id)) else {
        return false;
    };
    if slots == 0 || (new.primary_slots | new.secondary_slots) & slots == 0 {
        return false;
    }
    for current in existing.iter_mut() {
        if ENCHANTMENTS[usize::from(current.id)].list != new.list {
            continue;
        }
        if current.id == enchantment.id {
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
        if !new.compatible_with(current.id) {
            return false;
        }
    }
    existing.push(enchantment);
    true
}

/// Choose enchantments for an item at enchanting `level` (BDS
/// `selectEnchantments` with no bonus): the level is spread by the item's
/// enchantability, every applicable enchantment gets the highest level whose
/// minimum cost it reaches, then one weighted pick plus further picks while
/// `nextInt(50) <= cost` (cost decays to `cost * 4 / 5 + 1` each time). Picks
/// incompatible with earlier ones are dropped.
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

    let mut candidates: Vec<(EnchantmentInstance, u32)> = ENCHANTMENTS
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            (item.slots == slot::ALL || (d.primary_slots | d.secondary_slots) & item.slots != 0)
                && (!d.treasure_only || allow_treasure)
                && d.discoverable
        })
        .filter_map(|(id, d)| {
            (1..=d.max_level)
                .rev()
                .find(|level| d.min_cost(i32::from(*level)) <= cost)
                .map(|level| {
                    let id = u8::try_from(id).expect("42 enchantments");
                    (EnchantmentInstance { id, level }, d.weight)
                })
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
    chosen.sort_by_key(|e| ENCHANTMENTS[usize::from(e.id)].list);
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
                    let d = &ENCHANTMENTS[usize::from(e.id)];
                    assert!((d.primary_slots | d.secondary_slots) & item.slots != 0);
                    assert!(!d.treasure_only, "no treasure enchantments: {e:?}");
                    assert!((1..=d.max_level).contains(&e.level));
                    for other in &chosen[i + 1..] {
                        assert_ne!(e.id, other.id);
                        assert!(d.compatible_with(other.id), "{e:?} with {other:?}");
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
                let d = &ENCHANTMENTS[usize::from(e.id)];
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
        let sharpness = EnchantmentInstance { id: 9, level: 2 };
        let smite = EnchantmentInstance { id: 10, level: 1 };
        let knockback = EnchantmentInstance { id: 12, level: 1 };
        let protection = EnchantmentInstance { id: 0, level: 1 };
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
        assert_eq!(sword[0], EnchantmentInstance { id: 9, level: 3 });
        let mut trident = Vec::new();
        assert!(add_enchantment(
            &mut trident,
            slot::TRIDENT,
            EnchantmentInstance { id: 30, level: 1 }
        ));
        assert!(!add_enchantment(
            &mut trident,
            slot::TRIDENT,
            EnchantmentInstance { id: 32, level: 1 }
        ));
        assert!(!add_enchantment(
            &mut trident,
            slot::TRIDENT,
            EnchantmentInstance { id: 31, level: 1 }
        ));
    }
}
