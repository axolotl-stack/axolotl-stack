//! Random enchanting: the enchanting table's three offers and the loot
//! function `enchant_with_levels`.
//!
//! Measured on BDS 1.26.51 (client-diff items3-20261030 `enchanting`, two
//! player seeds): three offers for every enchantable item without
//! enchantments, also when an offer carries no enchantment and when an
//! offer costs less than its slot number; books may carry several
//! enchantments; the applied `ench` list is the offer in wire-list order
//! (equipped, held, self); the same seed, item and bookshelf count give
//! the same offers until an enchant replaces the seed.
//!
//! Public reference (minecraft.wiki "Enchanting table mechanics", within
//! the measured cost bounds; the tables live in `unastar-data`'s
//! `enchantments.json`):
//! - experience cost of each offer: `base = 1 + randInt(7) + b / 2 +
//!   randInt(b)` for `b` bookshelves (at most 15), top `max(base / 3, 1)`,
//!   middle `base * 2 / 3 + 1`, bottom `max(base, 2b)`;
//! - enchantment cost: `round((xp + 1 + randInt(e / 4) + randInt(e / 4)) ×
//!   (1 + randTri(0.15)))` for enchantability `e`;
//! - candidates: every enchantment the item takes (a book takes every one)
//!   whose cost range contains the enchantment cost, at the highest such
//!   level; treasure only where allowed (never at the table);
//! - selection: weighted pick, drop it and what conflicts with it, continue
//!   with chance `(cost + 1) / 50`, halving the cost each time.
//!
//! The random generator is ours (seeded per offer from the player's seed),
//! so the same seed gives the same offers here but not the ones BDS makes.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use unastar_data::enchantments::{self, Category};

use super::ItemStack;
use super::enchant::{
    durability_damage, enchantments as enchantment_list, item_tag, with_durability_damage,
    with_enchantment_list, with_item_tag,
};
use crate::registry::ItemRegistry;
use crate::registry::item::ItemEntry;

pub const ENCHANTED_BOOK: &str = "minecraft:enchanted_book";
pub const BOOK: &str = "minecraft:book";

/// Bookshelves beyond this many add nothing.
pub const MAX_BOOKSHELVES: u8 = 15;

/// Words of an offer's display name, which the client renders in the
/// Standard Galactic Alphabet (public reference: minecraft.wiki
/// "Enchanting Table"; gophertunnel documents BDS names such as
/// `"bless inside creature shrink "`).
const NAME_WORDS: &[&str] = &[
    "the",
    "elder",
    "scrolls",
    "klaatu",
    "berata",
    "niktu",
    "xyzzy",
    "bless",
    "curse",
    "light",
    "darkness",
    "fire",
    "air",
    "earth",
    "water",
    "hot",
    "dry",
    "cold",
    "wet",
    "ignite",
    "snuff",
    "embiggen",
    "twist",
    "shorten",
    "stretch",
    "fiddle",
    "destroy",
    "imbue",
    "galvanize",
    "enchant",
    "free",
    "limited",
    "range",
    "of",
    "towards",
    "inside",
    "sphere",
    "cube",
    "self",
    "other",
    "ball",
    "mental",
    "physical",
    "grow",
    "shrink",
    "demon",
    "elemental",
    "spirit",
    "animal",
    "creature",
    "beast",
    "humanoid",
    "undead",
    "fresh",
    "stale",
    "phnglui",
    "mglwnafh",
    "cthulhu",
    "rlyeh",
    "wgahnagl",
    "fhtagn",
    "baguette",
];

fn in_category(stack: &ItemStack, item: &ItemEntry, category: &Category) -> bool {
    if category
        .exclude_items
        .iter()
        .any(|id| id == item.identifier())
    {
        return false;
    }
    category
        .armor_slot
        .is_some_and(|slot| super::equipment::accepts_armour(stack, usize::from(slot)))
        || category.tags.iter().any(|tag| item.has_tag(tag))
        || category.items.iter().any(|id| id == item.identifier())
        || (category.durable && crate::server::game::durability::max_durability(item) > 0)
}

/// Whether `stack` can carry enchantment `id` (books take every one).
pub fn accepts(stack: &ItemStack, id: i16) -> bool {
    let (Some(item), Some(enchantment)) = (stack.item(), enchantments::get(id)) else {
        return false;
    };
    item.identifier() == ENCHANTED_BOOK
        || item.identifier() == BOOK
        || enchantment.targets.iter().any(|target| {
            enchantments::category(target).is_some_and(|c| in_category(stack, item, c))
        })
}

/// Enchanting-table enchantability of `stack` (0: not enchantable).
pub fn enchantability(stack: &ItemStack) -> i32 {
    stack
        .item()
        .map_or(0, |item| enchantments::enchantability(item.identifier()))
}

/// Whether the enchanting table makes offers for `stack`: an enchantable
/// item without enchantments.
pub fn can_enchant(stack: &ItemStack) -> bool {
    !stack.is_empty() && enchantability(stack) > 0 && enchantment_list(stack).is_empty()
}

/// The experience cost of the three offers for `bookshelves`.
pub fn offer_costs(bookshelves: u8, rng: &mut impl Rng) -> [i32; 3] {
    let shelves = i32::from(bookshelves.min(MAX_BOOKSHELVES));
    std::array::from_fn(|slot| {
        let base = 1 + rng.gen_range(0..=7) + shelves / 2 + rng.gen_range(0..=shelves);
        match slot {
            0 => (base / 3).max(1),
            1 => base * 2 / 3 + 1,
            _ => base.max(shelves * 2),
        }
    })
}

/// The enchantment cost an experience cost of `level` gives an item of
/// `enchantability`.
fn enchantment_cost(level: i32, enchantability: i32, rng: &mut impl Rng) -> i32 {
    let quarter = enchantability / 4;
    let cost = level + 1 + rng.gen_range(0..=quarter) + rng.gen_range(0..=quarter);
    let bonus = (rng.r#gen::<f32>() + rng.r#gen::<f32>() - 1.0) * 0.15;
    ((cost as f32) * (1.0 + bonus)).round().max(1.0) as i32
}

/// Random enchantments for `stack` at experience cost `level`, in pick
/// order; empty when nothing qualifies.
fn select(stack: &ItemStack, level: i32, treasure: bool, rng: &mut impl Rng) -> Vec<(i16, i16)> {
    let enchantability = enchantability(stack);
    if enchantability <= 0 {
        return Vec::new();
    }
    let mut cost = enchantment_cost(level, enchantability, rng);
    let mut candidates: Vec<(i16, i16, u32)> = enchantments::all()
        .iter()
        .filter(|enchantment| treasure || !enchantment.treasure)
        .filter(|enchantment| accepts(stack, enchantment.id))
        .filter_map(|enchantment| {
            let level = enchantment.level_for_cost(cost)?;
            Some((enchantment.id, level, enchantment.weight))
        })
        .collect();
    let mut picked = Vec::new();
    while !candidates.is_empty() {
        let total: u32 = candidates.iter().map(|(_, _, weight)| weight).sum();
        let mut roll = rng.gen_range(0..total);
        let index = candidates
            .iter()
            .position(|(_, _, weight)| {
                if roll < *weight {
                    true
                } else {
                    roll -= weight;
                    false
                }
            })
            .expect("roll is below the total weight");
        let (id, level, _) = candidates.swap_remove(index);
        picked.push((id, level));
        candidates.retain(|(other, _, _)| !enchantments::incompatible(id, *other));
        if candidates.is_empty() || rng.gen_range(0..50) > cost {
            break;
        }
        cost /= 2;
    }
    picked
}

/// One enchanting-table offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// Experience levels the player needs.
    pub cost: i32,
    /// `(id, level)` the enchant applies, in wire-list order (equipped,
    /// held, self; pick order within a list). May be empty.
    pub enchantments: Vec<(i16, i16)>,
}

/// The generator of offer `slot` for `seed` (slot 3: the costs).
fn offer_rng(seed: u32, slot: u64) -> StdRng {
    StdRng::seed_from_u64(u64::from(seed) << 2 | slot)
}

/// The enchanting table's three offers for `stack` with `bookshelves`
/// around it and the player's enchanting `seed`; `None` for an item the
/// table cannot enchant.
pub fn offers(stack: &ItemStack, bookshelves: u8, seed: u32) -> Option<[Offer; 3]> {
    if !can_enchant(stack) {
        return None;
    }
    let costs = offer_costs(bookshelves, &mut offer_rng(seed, 3));
    Some(std::array::from_fn(|slot| {
        let cost = costs[slot];
        let mut enchantments = select(stack, cost, false, &mut offer_rng(seed, slot as u64));
        enchantments.sort_by_key(|(id, _)| {
            enchantments::get(*id).map_or(0, |enchantment| enchantment.activation.index())
        });
        Offer { cost, enchantments }
    }))
}

/// The enchant-slot bits `PlayerEnchantOptions` sends for `stack`: -1 for
/// a book, else the first slot-mask category the item is in (0: none).
pub fn enchant_slot(stack: &ItemStack) -> i32 {
    let Some(item) = stack.item() else {
        return 0;
    };
    if item.identifier() == BOOK {
        return -1;
    }
    enchantments::slot_masks()
        .iter()
        .find(|slot| {
            enchantments::category(&slot.category)
                .is_some_and(|category| in_category(stack, item, category))
        })
        .map_or(0, |slot| slot.mask)
}

/// A display name for an offer: three or four words, each followed by a
/// space.
pub fn offer_name(rng: &mut impl Rng) -> String {
    let words = rng.gen_range(3..=4);
    (0..words)
        .map(|_| NAME_WORDS[rng.gen_range(0..NAME_WORDS.len())])
        .fold(String::new(), |mut name, word| {
            name.push_str(word);
            name.push(' ');
            name
        })
}

/// `stack` carrying `levels` after its own enchantments; a book becomes an
/// enchanted book with the book's other user data, and an item with
/// durability writes its `Damage` (BDS's enchanted sword carried
/// `Damage: 0`). `None` when the registry has no enchanted book.
pub fn enchanted(
    stack: &ItemStack,
    levels: &[(i16, i16)],
    items: &ItemRegistry,
) -> Option<ItemStack> {
    let base = if stack.item_id() == BOOK {
        let book = items.stack(ENCHANTED_BOOK, stack.count)?;
        match item_tag(stack) {
            Some(compound) => with_item_tag(&book, compound),
            None => book,
        }
    } else if stack
        .item()
        .is_some_and(|item| crate::server::game::durability::max_durability(item) > 0)
    {
        with_durability_damage(stack, durability_damage(stack))
    } else {
        stack.clone()
    };
    let mut list = enchantment_list(&base);
    list.extend_from_slice(levels);
    Some(with_enchantment_list(&base, &list))
}

/// Loot function `enchant_with_levels`: `stack` with random enchantments
/// for experience cost `levels` (treasure enchantments only when
/// `treasure`). Items the table cannot enchant come back unchanged.
pub fn enchant_with_levels(
    stack: &ItemStack,
    levels: i32,
    treasure: bool,
    items: &ItemRegistry,
    rng: &mut impl Rng,
) -> ItemStack {
    let picked = select(stack, levels, treasure, rng);
    if picked.is_empty() {
        return stack.clone();
    }
    enchanted(stack, &picked, items).unwrap_or_else(|| stack.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::test_support::{VANILLA, stack};

    #[test]
    fn offer_costs_follow_the_bookshelf_ranges() {
        // minecraft.wiki ranges: 0 shelves top 1-2, middle 1-6, bottom 1-8
        // (BDS offered 1/2/2 and 2/5/7); 15 shelves top 2-10, middle 6-21,
        // bottom exactly 30; 7 shelves bottom 14 and 6 shelves bottom 12
        // (measured maxima).
        for seed in 0..2000u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let [top, middle, bottom] = offer_costs(0, &mut rng);
            assert!((1..=2).contains(&top), "{top}");
            assert!((1..=6).contains(&middle), "{middle}");
            assert!((1..=8).contains(&bottom), "{bottom}");
            let [top, middle, bottom] = offer_costs(15, &mut rng);
            assert!((2..=10).contains(&top), "{top}");
            assert!((6..=21).contains(&middle), "{middle}");
            assert_eq!(bottom, 30);
            assert_eq!(offer_costs(40, &mut rng)[2], 30);
            assert!((14..=18).contains(&offer_costs(7, &mut rng)[2]));
            assert!((12..=17).contains(&offer_costs(6, &mut rng)[2]));
        }
    }

    #[test]
    fn offers_are_fixed_by_seed_item_kind_and_shelves() {
        let sword = stack("minecraft:iron_sword", 1);
        let first = offers(&sword, 15, 1234).unwrap();
        assert_eq!(Some(&first), offers(&sword, 15, 1234).as_ref());
        assert_eq!(
            Some(&first),
            offers(&stack("minecraft:iron_sword", 1), 15, 1234).as_ref()
        );
        assert_eq!(first[2].cost, 30);
        // Another seed changes them.
        assert!((0..20).any(|seed| offers(&sword, 15, seed).as_ref() != Some(&first)));
        for seed in 0..200 {
            for offer in offers(&sword, 15, seed).unwrap() {
                let mut list = 0;
                for &(id, level) in &offer.enchantments {
                    assert!(accepts(&sword, id), "{id}");
                    let enchantment = enchantments::get(id).unwrap();
                    assert!(!enchantment.treasure);
                    assert!((1..=enchantment.max_level).contains(&level));
                    // Wire-list order, as BDS writes the `ench` list.
                    assert!(enchantment.activation.index() >= list);
                    list = enchantment.activation.index();
                }
                for (i, (a, _)) in offer.enchantments.iter().enumerate() {
                    for (b, _) in &offer.enchantments[i + 1..] {
                        assert!(!enchantments::incompatible(*a, *b), "{a} {b}");
                        assert_ne!(a, b);
                    }
                }
            }
        }
    }

    #[test]
    fn unenchantable_and_enchanted_items_get_no_offers() {
        assert_eq!(offers(&stack("minecraft:stick", 1), 15, 7), None);
        assert_eq!(offers(&stack("minecraft:shears", 1), 15, 7), None);
        let enchanted = with_enchantment_list(&stack("minecraft:iron_sword", 1), &[(9, 1)]);
        assert_eq!(offers(&enchanted, 15, 7), None);
        assert_eq!(offers(&ItemStack::empty(), 15, 7), None);
    }

    #[test]
    fn books_may_carry_several_enchantments_and_the_mace_no_sword_ones() {
        let book = stack("minecraft:book", 1);
        let mace = stack("minecraft:mace", 1);
        let mut several = false;
        for seed in 0..300 {
            several |= offers(&book, 15, seed)
                .unwrap()
                .iter()
                .any(|offer| offer.enchantments.len() > 1);
            for offer in offers(&mace, 15, seed).unwrap() {
                for (id, _) in &offer.enchantments {
                    // Sharpness, knockback, looting are sword-only.
                    assert!(![9, 12, 14].contains(id), "{id}");
                }
            }
        }
        assert!(several);
    }

    #[test]
    fn enchant_slots_match_bds() {
        // Measured: sword 16, pickaxe 1024, book -1, bow 32, mace 1 << 22,
        // iron spear 1 << 23.
        for (item, slot) in [
            ("minecraft:iron_sword", 16),
            ("minecraft:diamond_pickaxe", 1024),
            ("minecraft:book", -1),
            ("minecraft:bow", 32),
            ("minecraft:mace", 4_194_304),
            ("minecraft:iron_spear", 8_388_608),
            ("minecraft:iron_helmet", 1),
            ("minecraft:turtle_helmet", 1),
            ("minecraft:iron_boots", 4),
            ("minecraft:elytra", 16_384),
            ("minecraft:stick", 0),
        ] {
            assert_eq!(enchant_slot(&stack(item, 1)), slot, "{item}");
        }
    }

    #[test]
    fn enchanting_a_book_makes_an_enchanted_book() {
        let book = stack("minecraft:book", 1);
        let result = enchanted(&book, &[(29, 1), (17, 1)], &VANILLA).unwrap();
        assert_eq!(result.item_id(), ENCHANTED_BOOK);
        assert_eq!(enchantment_list(&result), vec![(29, 1), (17, 1)]);
        // Measured: no other keys on the book.
        assert_eq!(item_tag(&result).unwrap().0.len(), 1);
        let sword = enchanted(&stack("minecraft:iron_sword", 1), &[(9, 2)], &VANILLA).unwrap();
        assert_eq!(sword.item_id(), "minecraft:iron_sword");
        assert_eq!(enchantment_list(&sword), vec![(9, 2)]);
        // Measured: `{Damage: 0, ench: [...]}`, no RepairCost.
        let mut keys: Vec<_> = item_tag(&sword).unwrap().0.into_keys().collect();
        keys.sort();
        assert_eq!(keys, ["Damage", "ench"]);
    }

    #[test]
    fn enchant_with_levels_allows_treasure_only_when_asked() {
        let rod = stack("minecraft:fishing_rod", 1);
        let book = stack("minecraft:book", 1);
        let mut treasure_seen = false;
        for seed in 0..500 {
            let mut rng = StdRng::seed_from_u64(seed);
            let plain = enchant_with_levels(&book, 30, false, &VANILLA, &mut rng);
            assert_eq!(plain.item_id(), ENCHANTED_BOOK);
            for (id, _) in enchantment_list(&plain) {
                assert!(!enchantments::get(id).unwrap().treasure);
            }
            let loot = enchant_with_levels(&book, 30, true, &VANILLA, &mut rng);
            treasure_seen |= enchantment_list(&loot)
                .iter()
                .any(|(id, _)| enchantments::get(*id).unwrap().treasure);
            let rod = enchant_with_levels(&rod, 30, true, &VANILLA, &mut rng);
            assert!(!enchantment_list(&rod).is_empty());
            for (id, _) in enchantment_list(&rod) {
                assert!(accepts(&rod, id), "{id}");
            }
        }
        assert!(treasure_seen);
        let stick = stack("minecraft:stick", 1);
        assert_eq!(
            enchant_with_levels(&stick, 30, true, &VANILLA, &mut StdRng::seed_from_u64(1)),
            stick
        );
    }

    #[test]
    fn offer_names_are_words_with_trailing_spaces() {
        let mut rng = StdRng::seed_from_u64(3);
        for _ in 0..50 {
            let name = offer_name(&mut rng);
            assert!(name.ends_with(' '));
            let words: Vec<_> = name.split_whitespace().collect();
            assert!((3..=4).contains(&words.len()));
            assert!(words.iter().all(|word| NAME_WORDS.contains(word)));
        }
    }
}
