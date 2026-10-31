//! Random enchanting for items: the enchantment slots and enchantability
//! of each item, the enchanting table's three offers, and stacks carrying
//! the enchantments random enchanting picks. The selection itself (the
//! enchantment table, levels for an enchanting level, compatibility) is
//! `unastar_rules::enchanting`, shared with the loot functions.
//!
//! Tools use their tier's enchantability (wood 15, stone 5, copper 13,
//! iron 14, diamond 10, gold 22, netherite 15), humanoid armour its
//! material's (leather 15, chainmail 12, copper 8, iron 9, gold 25,
//! diamond 10, netherite 15, turtle 9, elytra 0); bows, crossbows and
//! tridents have 1. Data-driven items carry `minecraft:enchantable`.
//! Books (every enchantment), fishing rods and carrots on a stick (1) are
//! public reference (minecraft.wiki "Enchanting mechanics"). The mace's
//! offers in the captures fix `enchantability / 4 = 2` (8 to 11 all fit,
//! 10 is used; the wiki's 15 does not fit).
//!
//! The enchanting table reproduces the BDS 1.26.51 captures exactly
//! (client-diff items3-20261030 `enchanting`, `enchanting-seed2`,
//! `enchanting-slots` and `dry`: four players, 219 offers with their
//! costs, enchantments and names):
//! - the player's enchanting seed seeds a 32-bit Mersenne Twister; every
//!   bounded draw `next(n)` is one output modulo `n`;
//! - one base cost for all three offers, `base = 1 + next(8) + b / 2 +
//!   next(b + 1)` for `b` bookshelves (at most 15): top `max(base / 3, 1)`,
//!   middle `base * 2 / 3 + 1`, bottom `max(base, 2b)`;
//! - then, offer by offer from the same stream, its enchantments
//!   ([`select_enchantments`] at the offer's cost, no treasure) and its
//!   name: `3 + next(2)` words `NAME_WORDS[next(55)]`, each followed by a
//!   space;
//! - so the same seed, item and bookshelf count give the same offers, also
//!   when an offer carries no enchantment or costs less than its slot
//!   number, until a successful enchant replaces the seed with a new
//!   non-negative 31-bit value (every seed in the captures is below 2^31;
//!   consecutive seeds are unrelated).
//!
//! Books may carry several enchantments; the applied `ench` list is the
//! offer in wire-list order (equipped, held, self).

use rand::RngCore;
use unastar_data::armour::{ArmourSlot, armour_slot};
use unastar_data::enchantments::{self, Category};
use unastar_rules::enchanting::{ItemEnchanting, select_enchantments, slot};
use unastar_rules::random::BedrockRandom;

use super::ItemStack;
use super::enchant::{
    durability_damage, enchantments as enchantment_list, item_tag, with_durability_damage,
    with_enchantment_list, with_item_tag,
};
use crate::registry::ItemRegistry;
use crate::registry::item::{ItemEntry, ToolKind, ToolTier};

pub const ENCHANTED_BOOK: &str = "minecraft:enchanted_book";
pub const BOOK: &str = "minecraft:book";

/// Bookshelves beyond this many add nothing.
pub const MAX_BOOKSHELVES: u8 = 15;

/// Armour material enchantability, by the item's tier tag.
const ARMOUR_MATERIALS: [(&str, i32); 7] = [
    ("minecraft:leather_tier", 15),
    ("minecraft:chainmail_tier", 12),
    ("minecraft:copper_tier", 8),
    ("minecraft:iron_tier", 9),
    ("minecraft:golden_tier", 25),
    ("minecraft:diamond_tier", 10),
    ("minecraft:netherite_tier", 15),
];
const TURTLE_HELMET: &str = "minecraft:turtle_helmet";
const TURTLE_ENCHANTABILITY: i32 = 9;
/// Items classified by identifier: their slots and enchantability.
const BY_IDENTIFIER: [(&str, u32, i32); 8] = [
    ("minecraft:bow", slot::BOW, 1),
    ("minecraft:crossbow", slot::CROSSBOW, 1),
    ("minecraft:trident", slot::TRIDENT, 1),
    ("minecraft:fishing_rod", slot::FISHING_ROD, 1),
    ("minecraft:carrot_on_a_stick", slot::CARROT_ON_A_STICK, 1),
    ("minecraft:mace", slot::MACE, 10),
    ("minecraft:elytra", slot::ELYTRA, 0),
    (BOOK, slot::ALL, 1),
];

/// The words of offer names, which the client renders in the Standard
/// Galactic Alphabet, in draw order: fitted to the BDS 1.26.51 captures,
/// whose every offer name they reproduce.
const NAME_WORDS: [&str; 55] = [
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
];

fn tier_enchantability(tier: ToolTier) -> i32 {
    match tier {
        ToolTier::Wooden => 15,
        ToolTier::Golden => 22,
        ToolTier::Stone => 5,
        ToolTier::Copper => 13,
        ToolTier::Iron => 14,
        ToolTier::Diamond => 10,
        ToolTier::Netherite => 15,
    }
}

/// Slot bits of a `minecraft:enchantable` slot name used by vanilla items.
fn component_slot(name: &str) -> Option<u32> {
    Some(match name {
        "none" => 0,
        "melee_spear" => slot::MELEE_SPEAR,
        _ => return None,
    })
}

/// Enchantment slots and enchantability of `item`; `None` when the item
/// is not of a classified vanilla item class.
pub fn item_enchanting(item: &ItemEntry) -> Option<ItemEnchanting> {
    if let Some(enchantable) = &item.components().enchantable {
        return Some(ItemEnchanting {
            slots: component_slot(&enchantable.slot)?,
            value: enchantable.value,
        });
    }
    let item_id = item.identifier();
    if let Some((_, slots, value)) = BY_IDENTIFIER.iter().find(|(id, ..)| *id == item_id) {
        return Some(ItemEnchanting {
            slots: *slots,
            value: *value,
        });
    }
    if let Some(worn) = armour_slot(item_id) {
        let slots = match worn {
            ArmourSlot::Head => slot::ARMOR_HEAD,
            ArmourSlot::Chest => slot::ARMOR_TORSO,
            ArmourSlot::Legs => slot::ARMOR_LEGS,
            ArmourSlot::Feet => slot::ARMOR_FEET,
        };
        let value = if item_id == TURTLE_HELMET {
            TURTLE_ENCHANTABILITY
        } else {
            ARMOUR_MATERIALS
                .iter()
                .find(|(tag, _)| item.has_tag(tag))
                .map(|(_, value)| *value)?
        };
        return Some(ItemEnchanting { slots, value });
    }
    let tool = item.tool_info();
    let slots = match tool.kind? {
        ToolKind::Sword => slot::SWORD,
        ToolKind::Shovel => slot::SHOVEL,
        ToolKind::Pickaxe => slot::PICKAXE,
        ToolKind::Axe => slot::AXE,
        ToolKind::Hoe => slot::HOE,
        ToolKind::Shears => return None,
    };
    Some(ItemEnchanting {
        slots,
        value: tier_enchantability(tool.tier?),
    })
}

/// The random-enchanting facts of `stack`'s item, if it is classified.
fn stack_enchanting(stack: &ItemStack) -> Option<ItemEnchanting> {
    stack.item().and_then(item_enchanting)
}

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

/// Whether the enchanting table makes offers for `stack`: an enchantable
/// item without enchantments.
pub fn can_enchant(stack: &ItemStack) -> bool {
    !stack.is_empty()
        && stack_enchanting(stack).is_some_and(|facts| facts.slots != 0 && facts.value > 0)
        && enchantment_list(stack).is_empty()
}

/// One enchanting-table offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// Experience levels the player needs.
    pub cost: i32,
    /// `(id, level)` the enchant applies, in wire-list order (equipped,
    /// held, self; pick order within a list). May be empty.
    pub enchantments: Vec<(i16, i16)>,
    /// The display name: three or four words, each followed by a space.
    pub name: String,
}

/// The enchanting table's three offers for `stack` with `bookshelves`
/// around it and the player's enchanting `seed`; `None` for an item the
/// table cannot enchant.
pub fn offers(stack: &ItemStack, bookshelves: u8, seed: u32) -> Option<[Offer; 3]> {
    if !can_enchant(stack) {
        return None;
    }
    let facts = stack_enchanting(stack)?;
    let mut random = BedrockRandom::new(seed);
    let shelves = i32::from(bookshelves.min(MAX_BOOKSHELVES));
    let base = 1 + random.next_int(8) + shelves / 2 + random.next_int(shelves + 1);
    let costs = [(base / 3).max(1), base * 2 / 3 + 1, base.max(shelves * 2)];
    Some(costs.map(|cost| {
        let enchantments = select_enchantments(facts, cost, false, &mut random)
            .into_iter()
            .map(|e| (i16::from(e.id), i16::from(e.level)))
            .collect();
        let name = offer_name(&mut random);
        Offer {
            cost,
            enchantments,
            name,
        }
    }))
}

/// The enchant-slot bits `PlayerEnchantOptions` sends for `stack`: the
/// item's enchantment slots (-1 for a book: every slot; 0: none).
pub fn enchant_slot(stack: &ItemStack) -> i32 {
    stack_enchanting(stack).map_or(0, |facts| facts.slots as i32)
}

/// An offer's display name: three or four words, each followed by a space.
fn offer_name(random: &mut BedrockRandom) -> String {
    let words = 3 + random.next_int(2);
    (0..words)
        .map(|_| NAME_WORDS[random.next_int(NAME_WORDS.len() as i32) as usize])
        .fold(String::new(), |mut name, word| {
            name.push_str(word);
            name.push(' ');
            name
        })
}

/// A new enchanting seed after an enchant (or for a new player): a
/// non-negative 31-bit value.
pub fn new_seed(rng: &mut impl RngCore) -> u32 {
    rng.next_u32() >> 1
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::test_support::{VANILLA, stack};

    fn facts(item: &str) -> (u32, i32) {
        let facts = item_enchanting(entry(item)).unwrap_or_else(|| panic!("{item} unclassified"));
        (facts.slots, facts.value)
    }

    fn entry(item: &str) -> &'static ItemEntry {
        VANILLA
            .get_by_name(item)
            .unwrap_or_else(|| panic!("{item} is a vanilla item"))
    }

    #[test]
    fn zombie_gear_is_classified_like_bds() {
        assert_eq!(facts("minecraft:iron_sword"), (slot::SWORD, 14));
        assert_eq!(facts("minecraft:iron_shovel"), (slot::SHOVEL, 14));
        assert_eq!(facts("minecraft:iron_spear"), (slot::MELEE_SPEAR, 14));
        assert_eq!(facts("minecraft:golden_spear"), (slot::MELEE_SPEAR, 22));
        assert_eq!(facts("minecraft:leather_helmet"), (slot::ARMOR_HEAD, 15));
        assert_eq!(facts("minecraft:copper_chestplate"), (slot::ARMOR_TORSO, 8));
        assert_eq!(facts("minecraft:golden_leggings"), (slot::ARMOR_LEGS, 25));
        assert_eq!(facts("minecraft:chainmail_boots"), (slot::ARMOR_FEET, 12));
        assert_eq!(facts("minecraft:iron_helmet"), (slot::ARMOR_HEAD, 9));
        assert_eq!(facts("minecraft:diamond_boots"), (slot::ARMOR_FEET, 10));
        assert_eq!(facts("minecraft:turtle_helmet"), (slot::ARMOR_HEAD, 9));
        assert_eq!(facts("minecraft:bow"), (slot::BOW, 1));
        assert_eq!(facts("minecraft:golden_sword"), (slot::SWORD, 22));
        assert_eq!(item_enchanting(entry("minecraft:rotten_flesh")), None);
    }

    /// One captured table state: the player's enchanting seed, the
    /// bookshelves, the item, the enchant-slot bits BDS sent and its three
    /// offers (cost, enchantments in wire-list order, name).
    type Captured = (
        u32,
        u8,
        &'static str,
        i32,
        [(i32, &'static [(i16, i16)], &'static str); 3],
    );

    /// Every distinct offer state in the BDS 1.26.51 captures (client-diff
    /// items3-20261030 `enchanting`, `enchanting-seed2`, `enchanting-slots`,
    /// `dry`). A player's first seed is the enchantment seed of its
    /// StartGame; the seeds after enchants were recovered from the offers by
    /// a search over all 2^32 seeds (each the only seed giving that packet).
    #[rustfmt::skip]
    const BDS_OFFERS: &[Captured] = &[
        (600484243, 0, "iron_sword", 16, [(2, &[(9, 1)], "klaatu snuff the scrolls "), (5, &[(10, 1)], "towards humanoid earth "), (7, &[(9, 1)], "mental range other cold ")]),
        (600484243, 0, "diamond_pickaxe", 1024, [(2, &[(15, 1)], "klaatu snuff the scrolls "), (5, &[(17, 1)], "towards humanoid earth "), (7, &[(17, 1)], "mental range other cold ")]),
        (600484243, 0, "book", -1, [(2, &[(0, 1)], "klaatu snuff the scrolls "), (5, &[(4, 1)], "towards humanoid earth "), (7, &[(19, 1)], "mental range other cold ")]),
        (600484243, 0, "bow", 32, [(2, &[(19, 1)], "klaatu snuff the scrolls "), (5, &[(19, 1)], "towards humanoid earth "), (7, &[(19, 1)], "mental range other cold ")]),
        (600484243, 0, "mace", 4194304, [(2, &[(17, 1)], "klaatu snuff the scrolls "), (5, &[(10, 1)], "towards humanoid earth "), (7, &[(17, 1)], "mental range other cold ")]),
        (600484243, 0, "iron_spear", 8388608, [(2, &[(9, 1)], "klaatu snuff the scrolls "), (5, &[(11, 1)], "towards humanoid earth "), (7, &[(17, 1)], "mental range other cold ")]),
        (600484243, 15, "iron_sword", 16, [(9, &[(9, 2)], "klaatu snuff the scrolls "), (19, &[(9, 3)], "towards humanoid earth "), (30, &[(13, 2), (9, 4), (17, 3), (12, 2)], "destroy other inside ")]),
        (600484243, 15, "diamond_pickaxe", 1024, [(9, &[(15, 2)], "klaatu snuff the scrolls "), (19, &[(15, 2)], "towards humanoid earth "), (30, &[(17, 3), (18, 3), (15, 4)], "destroy other inside ")]),
        (600484243, 15, "book", -1, [(9, &[(0, 1)], "klaatu snuff the scrolls "), (19, &[(39, 2)], "towards humanoid earth "), (30, &[(9, 4), (34, 4), (19, 4), (12, 2)], "destroy other inside ")]),
        (600484243, 15, "bow", 32, [(9, &[(17, 1)], "klaatu snuff the scrolls "), (19, &[(17, 2)], "towards humanoid earth "), (30, &[(17, 3), (21, 1), (19, 4), (20, 2)], "destroy other inside ")]),
        (600484243, 15, "mace", 4194304, [(9, &[(10, 2)], "klaatu snuff the scrolls "), (19, &[(11, 2)], "towards humanoid earth "), (30, &[(39, 5), (17, 3)], "destroy other inside ")]),
        (600484243, 15, "iron_spear", 8388608, [(9, &[(9, 2)], "klaatu snuff the scrolls "), (19, &[(11, 3)], "towards humanoid earth "), (30, &[(9, 4), (41, 3), (12, 2)], "destroy other inside ")]),
        (1589289030, 15, "diamond_pickaxe", 1024, [(7, &[(17, 1), (15, 1)], "beast klaatu humanoid shrink "), (16, &[(18, 1)], "animal free shorten "), (30, &[(17, 3), (15, 3)], "bless cube fiddle ")]),
        (1589289030, 15, "book", -1, [(7, &[(29, 1), (17, 1)], "beast klaatu humanoid shrink "), (16, &[(8, 1)], "animal free shorten "), (30, &[(9, 3), (19, 3)], "bless cube fiddle ")]),
        (1578384666, 15, "diamond_pickaxe", 1024, [(6, &[(17, 1)], "snuff water niktu "), (13, &[(15, 2)], "air the dry "), (30, &[(18, 3), (17, 3), (15, 4)], "fire berata self scrolls ")]),
        (1578384666, 7, "diamond_pickaxe", 1024, [(4, &[(17, 1)], "snuff water niktu "), (10, &[(15, 2)], "air the dry "), (14, &[(18, 1), (17, 2), (15, 2)], "ignite wet fire berata ")]),
        (1578384666, 7, "bow", 32, [(4, &[(19, 1)], "snuff water niktu "), (10, &[(19, 1)], "air the dry "), (14, &[(17, 2), (19, 2)], "light enchant ignite ")]),
        (1578384666, 6, "diamond_pickaxe", 1024, [(3, &[(17, 1)], "snuff water niktu "), (8, &[(15, 1)], "air the dry "), (12, &[(18, 1), (15, 2)], "light enchant ignite ")]),
        (1578384666, 6, "bow", 32, [(3, &[(19, 1)], "snuff water niktu "), (8, &[(19, 1)], "air the dry "), (12, &[(17, 2), (19, 2)], "light enchant ignite ")]),
        (1135045863, 0, "iron_sword", 16, [(1, &[(9, 1), (17, 1), (12, 1)], "other imbue curse "), (2, &[(9, 1)], "enchant range animal the "), (2, &[(11, 1)], "grow earth creature beast ")]),
        (1135045863, 0, "diamond_pickaxe", 1024, [(1, &[(15, 1)], "physical water humanoid imbue "), (2, &[(15, 1)], "snuff curse free fresh "), (2, &[(17, 1)], "klaatu stale destroy ")]),
        (1135045863, 0, "book", -1, [(1, &[(29, 1)], "physical water humanoid imbue "), (2, &[(29, 1)], "snuff curse free fresh "), (2, &[(15, 1)], "klaatu stale destroy ")]),
        (1135045863, 0, "bow", 32, [(1, &[(19, 1)], "physical water humanoid imbue "), (2, &[(19, 1)], "snuff curse free fresh "), (2, &[(19, 1)], "klaatu stale destroy ")]),
        (1135045863, 0, "mace", 4194304, [(1, &[], "inside curse physical "), (2, &[(17, 1), (10, 1)], "the snuff curse "), (2, &[(11, 1)], "klaatu stale destroy ")]),
        (1135045863, 0, "iron_spear", 8388608, [(1, &[(10, 1), (17, 1), (41, 1), (12, 1)], "other imbue curse "), (2, &[(10, 1)], "enchant range animal the "), (2, &[(10, 1)], "grow earth creature beast ")]),
        (1135045863, 15, "iron_sword", 16, [(6, &[(11, 1), (17, 1)], "other imbue curse "), (14, &[(17, 2)], "enchant range animal the "), (30, &[(10, 4), (13, 2), (14, 2), (12, 2)], "fire destroy snuff humanoid ")]),
        (1135045863, 15, "diamond_pickaxe", 1024, [(6, &[(17, 1), (15, 1)], "humanoid imbue other enchant "), (14, &[(17, 2), (15, 2)], "earth enchant range "), (30, &[(16, 1), (17, 3), (15, 3)], "ball air demon ")]),
        (1135045863, 15, "book", -1, [(6, &[(0, 1), (39, 1), (34, 1)], "other imbue curse "), (14, &[(35, 1)], "enchant range animal the "), (30, &[(4, 4), (11, 3), (31, 3)], "ball air demon ")]),
        (1135045863, 15, "bow", 32, [(6, &[(17, 1), (19, 1)], "humanoid imbue other enchant "), (14, &[(17, 2), (19, 2)], "earth enchant range "), (30, &[(21, 1), (17, 3), (19, 3), (20, 1)], "ball air demon ")]),
        (1135045863, 15, "mace", 4194304, [(6, &[(11, 1), (17, 1)], "other imbue curse "), (14, &[(13, 1)], "enchant range animal the "), (30, &[(13, 1), (17, 3), (10, 3)], "fire destroy snuff humanoid ")]),
        (1135045863, 15, "iron_spear", 8388608, [(6, &[(9, 2), (17, 1), (12, 1)], "other imbue curse "), (14, &[(10, 2)], "enchant range animal the "), (30, &[(41, 3), (9, 3), (17, 3), (12, 2)], "fire destroy snuff humanoid ")]),
        (1106337166, 15, "diamond_pickaxe", 1024, [(5, &[(15, 1)], "shorten ball free stretch "), (12, &[(17, 2), (15, 2)], "animal imbue water air "), (30, &[(17, 3), (18, 3), (15, 4)], "niktu cold cube ")]),
        (1106337166, 15, "book", -1, [(5, &[(11, 1)], "shorten ball free stretch "), (12, &[(39, 1), (34, 2)], "animal imbue water air "), (30, &[(0, 4), (8, 1), (41, 3), (15, 4), (35, 2), (12, 2)], "cold cube physical inside ")]),
        (1541535686, 15, "diamond_pickaxe", 1024, [(4, &[(17, 1)], "grow bless light "), (9, &[(15, 2)], "berata other air "), (30, &[(17, 3), (15, 4)], "embiggen ignite light of ")]),
        (1541535686, 7, "diamond_pickaxe", 1024, [(2, &[(17, 1)], "grow bless light "), (6, &[(15, 1)], "berata other air "), (14, &[(17, 2), (15, 2)], "embiggen ignite light of ")]),
        (1541535686, 7, "bow", 32, [(2, &[(19, 1)], "grow bless light "), (6, &[(19, 1)], "berata other air "), (14, &[(17, 2), (19, 2)], "embiggen ignite light of ")]),
        (1541535686, 6, "diamond_pickaxe", 1024, [(2, &[(17, 1)], "grow bless light "), (6, &[(15, 1)], "berata other air "), (12, &[(17, 2), (15, 2)], "embiggen ignite light of ")]),
        (1541535686, 6, "bow", 32, [(2, &[(19, 1)], "grow bless light "), (6, &[(19, 1)], "berata other air "), (12, &[(17, 2), (19, 2)], "embiggen ignite light of ")]),
        (2114989965, 0, "iron_helmet", 1, [(1, &[(0, 1)], "limited earth limited beast "), (2, &[(8, 1)], "twist xyzzy ball of "), (2, &[(0, 1)], "destroy bless other ")]),
        (2114989965, 0, "iron_chestplate", 2, [(1, &[(4, 1)], "limited earth limited beast "), (2, &[(4, 1)], "twist xyzzy ball of "), (2, &[(0, 1)], "destroy bless other ")]),
        (2114989965, 0, "iron_leggings", 8, [(1, &[(4, 1)], "limited earth limited beast "), (2, &[(4, 1)], "twist xyzzy ball of "), (2, &[(0, 1)], "destroy bless other ")]),
        (2114989965, 0, "iron_boots", 4, [(1, &[(4, 1)], "limited earth limited beast "), (2, &[(4, 1)], "twist xyzzy ball of "), (2, &[(0, 1)], "destroy bless other ")]),
        (2114989965, 0, "turtle_helmet", 1, [(1, &[(0, 1)], "limited earth limited beast "), (2, &[(8, 1)], "twist xyzzy ball of "), (2, &[(0, 1)], "destroy bless other ")]),
        (2114989965, 0, "iron_axe", 512, [(1, &[(9, 1)], "limited earth limited beast "), (2, &[(10, 1)], "twist xyzzy ball of "), (2, &[(17, 1)], "destroy bless other ")]),
        (2114989965, 0, "iron_shovel", 2048, [(1, &[(17, 1)], "limited earth limited beast "), (2, &[(17, 1)], "twist xyzzy ball of "), (2, &[(15, 1)], "destroy bless other ")]),
        (2114989965, 0, "iron_hoe", 64, [(1, &[(17, 1)], "limited earth limited beast "), (2, &[(17, 1)], "twist xyzzy ball of "), (2, &[(15, 1)], "destroy bless other ")]),
        (2114989965, 0, "fishing_rod", 4096, [(1, &[], "range berata limited earth "), (2, &[], "towards range spirit "), (2, &[], "of elemental range ")]),
        (2114989965, 0, "crossbow", 65536, [(1, &[(34, 1)], "limited earth limited beast "), (2, &[(34, 1)], "twist xyzzy ball of "), (2, &[(34, 1)], "destroy bless other ")]),
        (2114989965, 0, "trident", 32768, [(1, &[(29, 1)], "limited earth limited beast "), (2, &[(29, 1)], "twist xyzzy ball of "), (2, &[(29, 1)], "destroy bless other ")]),
        (2114989965, 0, "carrot_on_a_stick", 8192, [(1, &[], "range berata limited earth "), (2, &[], "towards range spirit "), (2, &[], "of elemental range ")]),
        (2114989965, 15, "mace", 4194304, [(6, &[(11, 1)], "limited earth limited beast "), (14, &[(39, 2)], "twist xyzzy ball of "), (30, &[(39, 4), (13, 2)], "dry fresh stretch water ")]),
        (2114989965, 15, "book", -1, [(6, &[(11, 1)], "limited earth limited beast "), (14, &[(41, 2)], "twist xyzzy ball of "), (30, &[(0, 3), (2, 4), (12, 2)], "dry fresh stretch water ")]),
        (644406434, 15, "mace", 4194304, [(4, &[], "free klaatu of "), (9, &[(39, 1)], "self twist limited "), (30, &[(40, 3)], "light wet air ")]),
        (644406434, 15, "book", -1, [(4, &[(19, 1)], "of fire of demon "), (9, &[(34, 2)], "stale physical other "), (30, &[(8, 1), (10, 4), (15, 3)], "humanoid animal of ball ")]),
        (1577864800, 15, "mace", 4194304, [(7, &[(17, 1)], "enchant self dry ignite "), (16, &[(17, 2)], "fresh fresh hot "), (30, &[(11, 4)], "darkness hot limited ")]),
        (1577864800, 15, "book", -1, [(7, &[(39, 1)], "enchant self dry ignite "), (16, &[(41, 2)], "fresh fresh hot "), (30, &[(0, 3)], "darkness hot limited ")]),
        (773073167, 15, "mace", 4194304, [(9, &[(13, 1)], "ball imbue wet ball "), (20, &[(39, 3), (17, 3)], "snuff fresh free the "), (30, &[(11, 4)], "twist bless darkness ")]),
        (773073167, 15, "book", -1, [(9, &[(41, 1)], "ball imbue wet ball "), (20, &[(9, 2)], "snuff fresh free the "), (30, &[(39, 4)], "twist bless darkness ")]),
        (1262519264, 15, "mace", 4194304, [(7, &[(11, 1)], "light ignite grow fresh "), (15, &[(39, 3), (17, 3)], "destroy niktu animal "), (30, &[(40, 3)], "the imbue humanoid twist ")]),
        (1262519264, 15, "iron_axe", 512, [(7, &[(11, 1)], "light ignite grow fresh "), (15, &[(17, 2), (15, 2)], "destroy niktu animal "), (30, &[(10, 4), (15, 4)], "the imbue humanoid twist ")]),
        (859559743, 0, "iron_sword", 16, [(1, &[(17, 1), (9, 1)], "ball elemental imbue "), (4, &[(9, 1)], "curse shrink enchant "), (5, &[(9, 1)], "ignite water sphere ")]),
    ];

    #[test]
    fn offers_match_the_bds_captures_exactly() {
        for &(seed, shelves, item, slot_bits, expected) in BDS_OFFERS {
            let stack = stack(&format!("minecraft:{item}"), 1);
            let context = format!("{item}, seed {seed}, {shelves} bookshelves");
            assert_eq!(enchant_slot(&stack), slot_bits, "{context}");
            let expected = expected.map(|(cost, enchantments, name)| Offer {
                cost,
                enchantments: enchantments.to_vec(),
                name: name.to_string(),
            });
            assert_eq!(offers(&stack, shelves, seed), Some(expected), "{context}");
        }
    }

    #[test]
    fn bookshelves_beyond_fifteen_add_nothing() {
        let sword = stack("minecraft:iron_sword", 1);
        for seed in 0..50 {
            assert_eq!(offers(&sword, 40, seed), offers(&sword, 15, seed));
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
                    // No treasure: frost walker, mending, binding, vanishing.
                    assert!(![25, 26, 27, 28].contains(&id), "{id}");
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
        assert_eq!(offers(&stack("minecraft:elytra", 1), 15, 7), None);
        // Measured: no offers for these either.
        assert_eq!(offers(&stack("minecraft:flint_and_steel", 1), 15, 7), None);
        assert_eq!(offers(&stack("minecraft:shield", 1), 15, 7), None);
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
    fn offer_names_are_words_with_trailing_spaces() {
        let bow = stack("minecraft:bow", 1);
        for seed in 0..50 {
            for offer in offers(&bow, 15, seed).unwrap() {
                assert!(offer.name.ends_with(' '));
                let words: Vec<_> = offer.name.split_whitespace().collect();
                assert!((3..=4).contains(&words.len()));
                assert!(words.iter().all(|word| NAME_WORDS.contains(word)));
            }
        }
    }

    #[test]
    fn new_seeds_are_non_negative() {
        let mut rng = BedrockRandom::new(1);
        assert!((0..1000).all(|_| i32::try_from(new_seed(&mut rng)).is_ok()));
    }
}
