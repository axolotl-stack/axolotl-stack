//! Lecterns (`minecraft:cardinal_direction`, `powered_bit`; the `Lectern`
//! block actor keeps `hasBook`, `book` (an item compound), `page` and
//! `totalPages`).
//!
//! - **Placement** keeps the generic state: the lectern faces the placing
//!   player.
//! - **Use** with a book and quill or a written book on an empty lectern
//!   lays the book on it (one item, used up outside creative) with
//!   `item.book.put` at the block's corner (sound name inferred). A use on
//!   a lectern holding a book opens its screen: `ContainerOpen` of type
//!   [`CONTAINER_TYPE`] at the lectern under the player's next window id
//!   (nothing when the player already has a window open); the client shows
//!   the book.
//! - **Pages** ([`turn_page`]): a page turn stores the page, clamped to
//!   0..=`totalPages` of the stored book (the count the client sends is
//!   ignored), and, one tick later, sends a redstone pulse: `powered_bit`
//!   turns on for one tick (strength 15, [`power`]) and then off.
//! - **Taking the book** ([`take_book`]): the book's "take" is the
//!   lectern's attack. A survival player punching a lectern that holds a
//!   book drops the book at the lectern's centre (as a container drops its
//!   items) and breaking goes on as usual; a creative player's break drops
//!   the book and leaves the lectern standing. Either way the lectern's
//!   centre gets a block-change vibration
//!   ([`super::sculk::GameEvent::BlockChange`]) without a source.
//! - **Breaking**: a player breaking the lectern drops the book.
//! - **Comparator output** ([`comparator_signal`]): 0 without a book;
//!   otherwise `⌊f⌋ + 1` with `f = 14` for books of at most three pages and
//!   `f = 14 × page / (totalPages / 2 − 1)` (integer halving) beyond.
//!
//! `totalPages` of a laid book is the length of its `pages` list (at least
//! 1), as BDS 1.26.51 does.

use bevy_ecs::prelude::*;
use glam::IVec3;
use zuri_nbt::{NBTTag, tag};

use super::super::{Place, actors, bit, centre, schedule_tick, with};
use crate::entity::components::{ContainerWindows, GameMode, PlayerSession, ScreenKind};
use crate::entity::systems::BlockSource;
use crate::item::ItemStack;
use crate::item::enchant::item_tag;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::item_use::{held, set_slot};
use crate::server::game::types::{BlockRegistryResource, ItemRegistryResource};

/// State property.
pub const POWERED: &str = "powered_bit";
/// The lectern's block actor id.
pub const ACTOR: &str = "Lectern";
/// `ContainerOpen` type of the lectern screen.
pub const CONTAINER_TYPE: u8 = 25;

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

/// A player's `use`: lay the held book on an empty lectern, or open the
/// lectern screen when it holds one.
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, _state: u32) -> bool {
    if book(world, pos).is_some() {
        open_screen(world, player, pos);
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
    crate::server::game::blocks::queue_level_sound(
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

/// Open the lectern screen at `pos` for `player` under its next window id.
fn open_screen(world: &mut World, player: Entity, pos: IVec3) {
    let Some(mut windows) = world.get_mut::<ContainerWindows>(player) else {
        return;
    };
    // A second ContainerOpen while a window is open crashes the client.
    if windows.open_id().is_some() {
        return;
    }
    // The screen has no slots: item requests keep the HUD's rules.
    let container_id = windows.open(ScreenKind::None);
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(jolyne::valentine::McpePacket::from(
            jolyne::valentine::ContainerOpenPacket {
                container_id,
                container_type: CONTAINER_TYPE,
                position: jolyne::valentine::types::BlockPos {
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                },
                target_actor_id: jolyne::valentine::types::ActorUniqueId {
                    actor_unique_id: -1,
                },
            },
        ));
    }
}

/// A reader turned the book on the lectern at `pos` to `page`: store it,
/// clamped to the stored book's `totalPages`, and send the redstone pulse
/// one tick later. Returns whether the page changed.
pub fn turn_page(world: &mut World, pos: IVec3, page: i32) -> bool {
    let Some(mut open) = book(world, pos) else {
        return false;
    };
    let page = page.clamp(0, open.total_pages.max(0));
    if open.page == page {
        return false;
    }
    open.page = page;
    store_book(world, pos, Some(&open));
    schedule_tick(world, pos, 1);
    true
}

/// Take the book off the lectern at `pos`: it drops at the lectern's
/// centre and the centre gets a block-change vibration. Returns whether
/// there was one.
pub fn take_book(world: &mut World, pos: IVec3) -> bool {
    let Some(open) = book(world, pos) else {
        return false;
    };
    store_book(world, pos, None);
    crate::server::game::block_drops::pop_resource(world, pos, open.item);
    super::sculk::game_event(
        world,
        centre(pos),
        super::sculk::GameEvent::BlockChange,
        None,
        None,
    );
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

    fn lay(h: &mut Harness, item: &str) {
        h.put(AT, state("minecraft:lectern", &[]));
        h.hold(item, 1);
        h.click(AT, 1, TOP);
    }

    /// The player punches the lectern (`StartDestroyBlock` in its auth
    /// input).
    fn punch(h: &mut Harness) {
        use crate::server::game::packet_queues::{BlockAction, BlockPacketQueue};
        let action = jolyne::valentine::PlayerBlockActionData {
            player_action_type: jolyne::valentine::types::EnumsPlayerActionType::Startdestroyblock,
            position: jolyne::valentine::types::BlockPos {
                x: AT.x,
                y: AT.y,
                z: AT.z,
            },
            facing: 1,
        };
        h.world
            .get_resource_or_init::<BlockPacketQueue>()
            .0
            .push((h.player, BlockAction::AuthInputActions(vec![action])));
        crate::server::game::blocks::apply_block_actions(&mut h.world);
    }

    /// Where the dropped items are (their feet).
    fn drop_positions(h: &mut Harness) -> Vec<DVec3> {
        use crate::entity::components::{DroppedItem, Position};
        let mut query = h.world.query_filtered::<&Position, With<DroppedItem>>();
        query.iter(&h.world).map(|position| position.0).collect()
    }

    #[test]
    fn a_survival_punch_drops_the_book_at_the_centre_and_breaking_goes_on() {
        let mut h = Harness::new();
        lay(&mut h, "minecraft:writable_book");
        let laid = book(&h.world, AT).expect("a book on the lectern");
        assert_eq!(laid.item.item_id(), "minecraft:writable_book");
        assert_eq!((laid.page, laid.total_pages), (0, 1));
        assert!(crate::server::game::item_use::held(&h.world, h.player).is_none());
        assert_eq!(comparator_signal(&h.world, AT), 15);
        punch(&mut h);
        assert!(book(&h.world, AT).is_none());
        assert_eq!(h.name(AT), "minecraft:lectern");
        assert_eq!(h.drops(), [("minecraft:writable_book".to_string(), 1)]);
        let [feet] = drop_positions(&mut h)[..] else {
            panic!("one dropped book");
        };
        let off = feet - centre(AT);
        assert!(off.x.abs() <= 0.25 && off.z.abs() <= 0.25, "{feet}");
        assert!(crate::server::game::item_use::held(&h.world, h.player).is_none());
        let breaking = h
            .world
            .get::<crate::entity::components::BreakingState>(h.player)
            .is_none_or(|state| state.position == Some((AT.x, AT.y, AT.z)));
        assert!(breaking, "the punch still starts breaking the lectern");
        // An empty lectern drops nothing more.
        punch(&mut h);
        assert_eq!(h.drops().len(), 1);
    }

    #[test]
    fn a_creative_break_drops_the_book_and_leaves_the_lectern() {
        let mut h = Harness::new();
        lay(&mut h, "minecraft:written_book");
        *h.world.get_mut::<GameMode>(h.player).unwrap() = GameMode::Creative;
        h.break_at(AT);
        assert_eq!(h.name(AT), "minecraft:lectern");
        assert!(book(&h.world, AT).is_none());
        assert_eq!(h.drops(), [("minecraft:written_book".to_string(), 1)]);
        // Without a book the lectern breaks.
        h.break_at(AT);
        assert_eq!(h.name(AT), "minecraft:air");
    }

    #[test]
    fn using_a_lectern_with_a_book_opens_its_screen() {
        use jolyne::valentine::McpePacketData;
        let mut h = Harness::new();
        h.world
            .entity_mut(h.player)
            .insert(ContainerWindows::default());
        lay(&mut h, "minecraft:written_book");
        h.packets();
        h.click(AT, 1, TOP);
        let opened: Vec<_> = h
            .packets()
            .into_iter()
            .filter_map(|packet| match packet {
                McpePacketData::ContainerOpenPacket(open) => Some((
                    open.container_type,
                    IVec3::new(open.position.x, open.position.y, open.position.z),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(opened, [(CONTAINER_TYPE, AT)]);
        // A second use while the screen is open sends nothing.
        h.click(AT, 1, TOP);
        assert!(
            !h.packets()
                .iter()
                .any(|packet| matches!(packet, McpePacketData::ContainerOpenPacket(_)))
        );
    }

    #[test]
    fn other_items_are_not_laid() {
        let mut h = Harness::new();
        lay(&mut h, "minecraft:book");
        assert!(book(&h.world, AT).is_none());
    }

    #[test]
    fn turning_a_page_pulses_redstone_for_one_tick() {
        let mut h = Harness::new();
        lay(&mut h, "minecraft:written_book");
        let mut open = book(&h.world, AT).unwrap();
        open.total_pages = 10;
        store_book(&mut h.world, AT, Some(&open));
        assert!(turn_page(&mut h.world, AT, 2));
        assert!(!turn_page(&mut h.world, AT, 2));
        assert_eq!(h.value(AT, POWERED), Some(StateValue::Byte(0)));
        h.tick(1);
        assert_eq!(h.value(AT, POWERED), Some(StateValue::Byte(1)));
        h.tick(1);
        assert_eq!(h.value(AT, POWERED), Some(StateValue::Byte(0)));
        // 14 × 2 / (10 / 2 − 1) = 7.
        assert_eq!(comparator_signal(&h.world, AT), 8);
    }

    #[test]
    fn a_page_turn_is_clamped_to_the_stored_book() {
        let mut h = Harness::new();
        lay(&mut h, "minecraft:written_book");
        let mut open = book(&h.world, AT).unwrap();
        open.total_pages = 4;
        store_book(&mut h.world, AT, Some(&open));
        // The client's own page count is not read: past the end lands on
        // the stored last page, below zero on the first.
        assert!(turn_page(&mut h.world, AT, 200));
        assert_eq!(book(&h.world, AT).unwrap().page, 4);
        assert!(!turn_page(&mut h.world, AT, 9));
        assert!(turn_page(&mut h.world, AT, -3));
        assert_eq!(book(&h.world, AT).unwrap().page, 0);
        assert_eq!(book(&h.world, AT).unwrap().total_pages, 4);
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
