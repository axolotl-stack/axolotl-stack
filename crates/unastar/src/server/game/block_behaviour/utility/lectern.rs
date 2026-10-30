//! Lecterns (`minecraft:cardinal_direction`, `powered_bit`; the `Lectern`
//! block actor keeps `hasBook`, `book` (an item compound), `page` and
//! `totalPages`).
//!
//! - **Placement** keeps the generic state: the lectern faces the placing
//!   player.
//! - **Use** with a book and quill or a written book on an empty lectern
//!   lays the book on it (one item, used up outside creative) with
//!   `item.book.put` at the block's corner (sound name inferred). A use on
//!   a lectern holding a book is handled: the client opens the book.
//! - **Pages** ([`turn_page`]): a page turn stores the page and, one tick
//!   later, sends a redstone pulse: `powered_bit` turns on for one tick
//!   (strength 15, [`power`]) and then off.
//! - **Taking the book** ([`take_book`]) gives it to the player (dropped at
//!   their feet when it does not fit) and empties the lectern.
//! - **Breaking**: a player breaking the lectern drops the book.
//! - **Comparator output** ([`comparator_signal`]): 0 without a book;
//!   otherwise `⌊f⌋ + 1` with `f = 14` for books of at most three pages and
//!   `f = 14 × page / (totalPages / 2 − 1)` (integer halving) beyond.
//!
//! `totalPages` of a laid book is the length of its `pages` list (at least
//! 1) until the client reports its own count with a page turn.

use bevy_ecs::prelude::*;
use glam::IVec3;
use zuri_nbt::{NBTTag, tag};

use super::super::{Place, actors, bit, schedule_tick, with};
use crate::entity::components::GameMode;
use crate::entity::systems::BlockSource;
use crate::item::ItemStack;
use crate::item::enchant::item_tag;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::item_use::{give_or_drop, held, set_slot};
use crate::server::game::types::{BlockRegistryResource, ItemRegistryResource};

/// State property.
pub const POWERED: &str = "powered_bit";
/// The lectern's block actor id.
pub const ACTOR: &str = "Lectern";

/// The book on a lectern and where it is open.
#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    pub item: ItemStack,
    pub page: i32,
    pub total_pages: i32,
}

fn int_of(compound: &tag::Compound, key: &str) -> i32 {
    match compound.0.get(key) {
        Some(NBTTag::Int(value)) => value.0,
        _ => 0,
    }
}

/// The book on the lectern at `pos`.
pub fn book(world: &World, pos: IVec3) -> Option<Book> {
    let actor = actors::read_kind(world, pos, ACTOR)?;
    if !matches!(actor.0.get("hasBook"), Some(NBTTag::Byte(has)) if has.0 != 0) {
        return None;
    }
    let NBTTag::Compound(item) = actor.0.get("book")? else {
        return None;
    };
    let item = crate::block_entity::item_from_nbt(
        item,
        &world.resource::<ItemRegistryResource>().0,
        &world.resource::<BlockRegistryResource>().0,
    )?;
    Some(Book {
        item,
        page: int_of(&actor, "page"),
        total_pages: int_of(&actor, "totalPages"),
    })
}

fn store_book(world: &mut World, pos: IVec3, book: Option<&Book>) {
    let mut actor = actors::new_compound(ACTOR, pos);
    actor.0.insert(
        "hasBook".into(),
        NBTTag::Byte(tag::Byte(u8::from(book.is_some()))),
    );
    if let Some(book) = book {
        actor.0.insert(
            "book".into(),
            NBTTag::Compound(crate::block_entity::item_to_nbt(&book.item)),
        );
        actor
            .0
            .insert("page".into(), NBTTag::Int(tag::Int(book.page)));
        actor
            .0
            .insert("totalPages".into(), NBTTag::Int(tag::Int(book.total_pages)));
    }
    actors::store(world, pos, actor);
    // Comparators around read the page.
    super::super::block_changed(world, pos);
}

fn is_book(item: &str) -> bool {
    matches!(item, "minecraft:writable_book" | "minecraft:written_book")
}

/// Pages in a book's NBT (at least 1).
fn pages(stack: &ItemStack) -> i32 {
    let count = item_tag(stack).and_then(|compound| match compound.0.get("pages") {
        Some(NBTTag::List(pages)) => i32::try_from(pages.0.len()).ok(),
        _ => None,
    });
    count.unwrap_or(0).max(1)
}

/// Whether a player's `use` on `state` is a block interaction: always.
pub fn is_usable(_entry: &BlockEntry, _state: u32) -> bool {
    true
}

/// A player's `use`: lay the held book on an empty lectern.
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, _state: u32) -> bool {
    if book(world, pos).is_some() {
        return true;
    }
    let Some((slot, stack)) = held(world, player) else {
        return false;
    };
    if !is_book(stack.item_id()) {
        return false;
    }
    let laid = Book {
        item: stack.clone().with_count(1),
        page: 0,
        total_pages: pages(&stack),
    };
    store_book(world, pos, Some(&laid));
    crate::entity::mob::net::sound::play_level_sound(
        world,
        None,
        pos.as_dvec3(),
        "item.book.put",
        -1,
    );
    if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
        set_slot(world, player, slot, stack.grow(-1));
    }
    true
}

/// A reader turned the book on the lectern at `pos` to `page` of
/// `total_pages`: store it and send the redstone pulse one tick later.
/// Returns whether the page changed.
pub fn turn_page(world: &mut World, pos: IVec3, page: i32, total_pages: i32) -> bool {
    let Some(mut open) = book(world, pos) else {
        return false;
    };
    if open.page == page {
        return false;
    }
    open.page = page;
    open.total_pages = total_pages.max(1);
    store_book(world, pos, Some(&open));
    schedule_tick(world, pos, 1);
    true
}

/// `player` takes the book from the lectern at `pos`. Returns whether there
/// was one.
pub fn take_book(world: &mut World, player: Entity, pos: IVec3) -> bool {
    let Some(open) = book(world, pos) else {
        return false;
    };
    store_book(world, pos, None);
    give_or_drop(world, player, open.item);
    true
}

/// Redstone power of the lectern `state`: 15 during a page-turn pulse.
pub fn power(entry: &BlockEntry, state: u32) -> u8 {
    if bit(entry, state, POWERED) { 15 } else { 0 }
}

/// Comparator output from the open page (see the module docs).
pub fn comparator_signal(world: &World, pos: IVec3) -> u8 {
    let Some(open) = book(world, pos) else {
        return 0;
    };
    let f = if open.total_pages <= 3 {
        14.0
    } else {
        14.0 * open.page as f32 / ((open.total_pages / 2) as f32 - 1.0)
    };
    (f.floor() as i32 + 1).clamp(0, 15) as u8
}

/// Placement keeps the generic state (facing the player).
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    Some(place.state)
}

/// A lectern stands anywhere.
pub fn can_survive(
    _blocks: &impl BlockSource,
    _pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    true
}

/// Nothing happens on placement.
pub fn placed(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Neighbours do not affect a lectern.
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// The page-turn pulse: power on for one tick, then off.
pub fn scheduled_tick(world: &mut World, pos: IVec3, state: u32) {
    let Some(entry) = world.registry().get_by_runtime_id(state).cloned() else {
        return;
    };
    let powered = bit(&entry, state, POWERED);
    let next = with(&entry, state, POWERED, StateValue::Byte(u8::from(!powered)));
    set_block(world, pos, next, false);
    if !powered {
        schedule_tick(world, pos, 1);
    }
}

/// Lecterns have no random tick.
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Redstone power does not act on lecterns.
pub fn power_changed(_world: &mut World, _pos: IVec3, _state: u32, _powered: bool) {}

/// A player breaks the lectern: the book drops.
pub fn breaking(world: &mut World, pos: IVec3, _state: u32) {
    if let Some(open) = book(world, pos) {
        store_book(world, pos, None);
        crate::server::game::block_drops::pop_resource(world, pos, open.item);
    }
}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::*;

    const AT: IVec3 = IVec3::new(2, FLOOR + 1, 2);
    const TOP: DVec3 = DVec3::new(0.5, 0.9, 0.5);

    #[test]
    fn holds_a_book_and_gives_it_back() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:lectern", &[]));
        h.hold("minecraft:writable_book", 1);
        h.click(AT, 1, TOP);
        let laid = book(&h.world, AT).expect("a book on the lectern");
        assert_eq!(laid.item.item_id(), "minecraft:writable_book");
        assert_eq!((laid.page, laid.total_pages), (0, 1));
        assert!(crate::server::game::item_use::held(&h.world, h.player).is_none());
        assert_eq!(comparator_signal(&h.world, AT), 15);
        assert!(take_book(&mut h.world, h.player, AT));
        assert!(book(&h.world, AT).is_none());
        let held = crate::server::game::item_use::held(&h.world, h.player)
            .unwrap()
            .1;
        assert_eq!(held.item_id(), "minecraft:writable_book");
    }

    #[test]
    fn other_items_are_not_laid() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:lectern", &[]));
        h.hold("minecraft:book", 1);
        h.click(AT, 1, TOP);
        assert!(book(&h.world, AT).is_none());
    }

    #[test]
    fn turning_a_page_pulses_redstone_for_one_tick() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:lectern", &[]));
        h.hold("minecraft:written_book", 1);
        h.click(AT, 1, TOP);
        assert!(turn_page(&mut h.world, AT, 2, 10));
        assert!(!turn_page(&mut h.world, AT, 2, 10));
        assert_eq!(h.value(AT, POWERED), Some(StateValue::Byte(0)));
        h.tick(1);
        assert_eq!(h.value(AT, POWERED), Some(StateValue::Byte(1)));
        h.tick(1);
        assert_eq!(h.value(AT, POWERED), Some(StateValue::Byte(0)));
        // 14 × 2 / (10 / 2 − 1) = 7.
        assert_eq!(comparator_signal(&h.world, AT), 8);
    }

    #[test]
    fn breaking_drops_the_book() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:lectern", &[]));
        h.hold("minecraft:written_book", 1);
        h.click(AT, 1, TOP);
        h.break_at(AT);
        let drops = h.drops();
        assert!(drops.contains(&("minecraft:written_book".to_string(), 1)));
    }
}
