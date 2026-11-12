//! Writing books: `BookEdit`, as BDS 1.26.51 handles it.
//!
//! Measured with `tools/bds-sweep/protocol_probe.py book` (2026-11-12, a
//! client sending `PlayerAuthInput`; fixtures in
//! `crates/bds-extractor/captures/1.26.51/protocol/book/`):
//!
//! - A player edits only the book it opened last: using a book and quill on
//!   air ([`open`]) opens it, signing closes it, and opening another book
//!   moves the gate to that slot. Selecting another slot keeps it. Edits of
//!   any other slot, a slot without a book and quill, or before any book was
//!   opened get no answer and change nothing (the round 2 relay runs never
//!   opened the book, so BDS ignored every edit).
//! - Opening a book with no `pages` gives it an empty `pages` list and
//!   resends the inventory (the four containers and `PlayerHotbar`).
//! - `ReplacePage` and `AddPage` past the last page pad the book with empty
//!   pages; a book holds at most [`MAX_PAGES`] pages, so an index of 50 or
//!   more, or adding to a full book, is refused. `DeletePage` and `SwapPages`
//!   need existing pages. Page text, titles and authors longer than the
//!   client allows are kept up to 768 characters; longer ones never get
//!   here (`crate::network::packet_limits` refuses the packet unread).
//! - Signing keeps the `author` and `xuid` the client sent.
//! - Signing makes a written book with the pages, `title`, `author` and
//!   `xuid` from the packet and `generation` 0.
//! - Every accepted edit is answered with a server `InventoryTransaction`
//!   ([`edit_transaction`]): the slot goes from the old book to the new one
//!   (with a new stack network id), and two creative-inventory actions move
//!   the old book in and the new one out.
//! - The player's interact text (`SetActorData`, actor data 100) follows the
//!   held item: `action.interact.write` for a book and quill, `Read` (not a
//!   translation key) for a written book, empty for anything else. BDS sends
//!   it only when it changes: on selecting a slot, on a command replacing the
//!   held item and on signing the held book (`book/held` session,
//!   [`sync_interact_text`]).

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{
    ActorRuntimeId, DataItemEntry, DataItemEntryPayload, DataItemStringPayload, EnumsDataItemType,
    PropertySyncData, SynchedActorDataCopyableDataList,
};
use jolyne::valentine::types::{
    BookEditPacketOperation as Operation, EnumsInventorySourceType, InventoryAction,
    InventorySource, InventoryTransaction, InventoryTransactionPacketTransaction,
    NormalTransactionData,
};
use jolyne::valentine::{
    BookEditPacket, InventoryTransactionPacket, McpePacket, SetActorDataPacket,
    actor_data_i_ds as actor_data_ids,
};
use zuri_nbt::{NBTTag, tag};

use crate::entity::components::{
    HeldSlot, MainInventory, PlayerInput, PlayerSession, RuntimeEntityId,
};
use crate::item::ItemStack;
use crate::item::enchant::{item_tag, try_with_item_tag};
use crate::registry::{ItemRegistry, item::ItemType};

/// The most pages a book holds.
pub const MAX_PAGES: usize = 50;

const WRITABLE_BOOK: &str = "minecraft:writable_book";
const WRITTEN_BOOK: &str = "minecraft:written_book";

/// The book items, resolved from the item registry at startup.
#[derive(Resource, Debug, Clone)]
pub struct BookItems {
    writable: ItemType,
    written: ItemType,
}

/// A book item the registry does not have.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("item registry has no `{0}`")]
pub struct MissingBookItem(pub &'static str);

impl BookItems {
    pub fn resolve(items: &ItemRegistry) -> Result<Self, MissingBookItem> {
        let item = |name| items.item_type(name).ok_or(MissingBookItem(name));
        Ok(Self {
            writable: item(WRITABLE_BOOK)?,
            written: item(WRITTEN_BOOK)?,
        })
    }

    /// Whether `stack` is a book and quill.
    pub fn is_writable(&self, stack: &ItemStack) -> bool {
        stack
            .item()
            .is_some_and(|entry| entry.id == self.writable.id)
    }

    /// The interact text of a player holding `held` (module docs).
    pub fn interact_text(&self, held: &ItemStack) -> InteractText {
        match held.item().map(|entry| entry.id) {
            Some(id) if id == self.writable.id => InteractText::Write,
            Some(id) if id == self.written.id => InteractText::Read,
            _ => InteractText::None,
        }
    }
}

/// The interact text BDS shows a player for its held item.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InteractText {
    #[default]
    None,
    /// A book and quill.
    Write,
    /// A written book.
    Read,
}

impl InteractText {
    pub fn text(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Write => "action.interact.write",
            Self::Read => "Read",
        }
    }
}

/// The `SetActorData` that gives a player its interact text.
pub fn interact_text_packet(
    runtime_id: RuntimeEntityId,
    text: InteractText,
    tick: jolyne::valentine::types::PlayerInputTick,
) -> SetActorDataPacket {
    SetActorDataPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        },
        actor_data: SynchedActorDataCopyableDataList {
            data: vec![DataItemEntry {
                id: actor_data_ids::INTERACT_TEXT,
                payload: DataItemEntryPayload::DataItemStringPayload(DataItemStringPayload {
                    type_: EnumsDataItemType::String,
                    value: text.text().into(),
                }),
            }],
        },
        synched_properties: PropertySyncData::default(),
        tick,
    }
}

/// System: sends each player its interact text when its held item changed
/// it (the component holds the text last sent; a joining player starts with
/// none).
pub fn sync_interact_text(
    books: Option<Res<BookItems>>,
    mut players: Query<(
        &RuntimeEntityId,
        &PlayerSession,
        &PlayerInput,
        &MainInventory,
        &HeldSlot,
        &mut InteractText,
    )>,
) {
    let Some(books) = books else {
        return;
    };
    for (runtime_id, session, input, inventory, held, mut sent) in &mut players {
        let text = inventory
            .0
            .item(usize::from(held.0))
            .map_or(InteractText::None, |stack| books.interact_text(stack));
        if text == *sent {
            continue;
        }
        *sent = text;
        let _ = session.send(interact_text_packet(*runtime_id, text, input.wire_tick()).into());
    }
}

/// The inventory slot of the book the player opened last; edits of other
/// slots are ignored.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct OpenBook {
    pub slot: u8,
}

/// One page of a book.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Page {
    pub text: String,
    pub photo_name: String,
}

impl From<&Page> for NBTTag {
    fn from(page: &Page) -> Self {
        NBTTag::Compound(tag::Compound(
            [
                ("photoname", page.photo_name.as_str()),
                ("text", page.text.as_str()),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), string(value)))
            .collect(),
        ))
    }
}

fn string(value: &str) -> NBTTag {
    NBTTag::String(tag::String(value.to_owned()))
}

fn text_of(page: &tag::Compound, key: &str) -> String {
    match page.0.get(key) {
        Some(NBTTag::String(value)) => value.0.clone(),
        _ => String::new(),
    }
}

/// The pages of a book; `None` when it has no `pages` list yet.
pub fn pages(stack: &ItemStack) -> Option<Vec<Page>> {
    let Some(NBTTag::List(list)) = item_tag(stack)?.0.get("pages").cloned() else {
        return None;
    };
    Some(
        list.0
            .iter()
            .map(|page| match page {
                NBTTag::Compound(page) => Page {
                    text: text_of(page, "text"),
                    photo_name: text_of(page, "photoname"),
                },
                _ => Page::default(),
            })
            .collect(),
    )
}

/// `stack` with `pages` stored, under a new stack network id.
fn with_pages(stack: &ItemStack, pages: &[Page]) -> Option<ItemStack> {
    let mut compound = item_tag(stack).unwrap_or_default();
    compound.0.insert(
        "pages".to_owned(),
        NBTTag::List(tag::List(pages.iter().map(NBTTag::from).collect())),
    );
    Some(fresh(try_with_item_tag(stack, &compound).ok()?))
}

fn fresh(stack: ItemStack) -> ItemStack {
    stack.with_net_id(0).into_slot()
}

fn index(value: i32) -> Option<usize> {
    usize::try_from(value).ok().filter(|at| *at < MAX_PAGES)
}

/// `pages` after `operation`, or `None` when BDS refuses it. Signing leaves
/// the pages as they are.
pub fn edit_pages(mut pages: Vec<Page>, operation: &Operation) -> Option<Vec<Page>> {
    let page = |text: &str, photo: &str| Page {
        text: text.to_owned(),
        photo_name: photo.to_owned(),
    };
    match operation {
        Operation::ReplacePage(replace) => {
            let at = index(replace.page_index)?;
            if pages.len() <= at {
                pages.resize(at + 1, Page::default());
            }
            pages[at] = page(&replace.page_text, &replace.photo_name);
        }
        Operation::AddPage(add) => {
            let at = index(add.page_index)?;
            if pages.len() >= MAX_PAGES {
                return None;
            }
            if pages.len() < at {
                pages.resize(at, Page::default());
            }
            pages.insert(at, page(&add.page_text, &add.photo_name));
        }
        Operation::DeletePage(delete) => {
            let at = index(delete.page_index).filter(|at| *at < pages.len())?;
            pages.remove(at);
        }
        Operation::SwapPages(swap) => {
            let a = index(swap.page_index).filter(|at| *at < pages.len())?;
            let b = index(swap.swap_with_index).filter(|at| *at < pages.len())?;
            pages.swap(a, b);
        }
        Operation::Finalize(_) => {}
    }
    Some(pages)
}

/// The book and quill `stack` after `operation`: a new book and quill, or a
/// written book for a signature; `None` when BDS refuses the edit.
pub fn edit(books: &BookItems, stack: &ItemStack, operation: &Operation) -> Option<ItemStack> {
    if !books.is_writable(stack) {
        return None;
    }
    let pages = edit_pages(pages(stack).unwrap_or_default(), operation)?;
    let Operation::Finalize(sign) = operation else {
        return with_pages(stack, &pages);
    };
    let compound = tag::Compound(
        [
            ("author", string(&sign.author)),
            ("generation", NBTTag::Int(tag::Int(0))),
            (
                "pages",
                NBTTag::List(tag::List(pages.iter().map(NBTTag::from).collect())),
            ),
            ("title", string(&sign.title)),
            ("xuid", string(&sign.xuid)),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect(),
    );
    let written = ItemStack::new(books.written.clone(), 1);
    Some(fresh(try_with_item_tag(&written, &compound).ok()?))
}

/// The `InventoryTransaction` BDS answers an accepted edit of inventory
/// `slot` with: the slot from `before` to `after`, then the creative
/// inventory taking `before` in (slot 0) and giving `after` out (slot 1).
/// Only the slot's new item carries a stack network id.
pub fn edit_transaction(
    slot: u32,
    before: &ItemStack,
    after: &ItemStack,
    block_ids: crate::registry::block::BlockNetworkIds,
) -> Option<InventoryTransactionPacket> {
    let wire = |stack| super::inventory::wire_item(stack, block_ids);
    let without_net_id = |stack| {
        wire(stack).map(|mut item| {
            item.net_id_variant = None;
            item
        })
    };
    let creative = |slot, from_item, to_item| InventoryAction {
        source: InventorySource {
            source_type: EnumsInventorySourceType::Creativeinventory,
            container_id: None,
            bit_flags: None,
        },
        slot,
        from_item,
        to_item,
    };
    let air = Default::default;
    Some(InventoryTransactionPacket {
        legacy_request_id: Default::default(),
        legacy_set_item_slots: None,
        transaction: InventoryTransactionPacketTransaction::NormalTransactionData(
            NormalTransactionData {
                actions: InventoryTransaction {
                    actions: vec![
                        InventoryAction {
                            source: InventorySource {
                                source_type: EnumsInventorySourceType::Containerinventory,
                                container_id: Some(0),
                                bit_flags: None,
                            },
                            slot,
                            from_item: without_net_id(before)?,
                            to_item: wire(after)?,
                        },
                        creative(0, air(), without_net_id(before)?),
                        creative(1, without_net_id(after)?, air()),
                    ],
                },
            },
        ),
    })
}

/// Opens the book and quill in hotbar `slot` for editing (the item use of a
/// book on air). A book without pages gets an empty page list, and the client
/// its inventory again, as BDS does.
pub fn open(world: &mut World, player: Entity, slot: u8, stack: &ItemStack) {
    world.entity_mut(player).insert(OpenBook { slot });
    if pages(stack).is_some() {
        return;
    }
    let Some(opened) = with_pages(stack, &[]) else {
        return;
    };
    if let Some(mut inventory) = world.get_mut::<MainInventory>(player) {
        let _ = inventory.0.set_item(usize::from(slot), opened);
    }
    super::join::send_inventory_contents(world, player);
}

/// Applies a player's `BookEdit` (module docs).
pub fn apply_book_edit(world: &mut World, player: Entity, packet: &BookEditPacket) {
    let Some(slot) = u8::try_from(packet.book_slot).ok() else {
        return;
    };
    if world.get::<OpenBook>(player).map(|open| open.slot) != Some(slot) {
        tracing::debug!(
            ?player,
            slot,
            "Ignoring BookEdit of a book that is not open"
        );
        return;
    }
    let Some(before) = world
        .get::<MainInventory>(player)
        .and_then(|inventory| inventory.0.item(usize::from(slot)).cloned())
    else {
        return;
    };
    let Some(after) = edit(world.resource::<BookItems>(), &before, &packet.operation) else {
        return;
    };
    let Some(transaction) = edit_transaction(
        u32::from(slot),
        &before,
        &after,
        super::types::block_network_ids(world),
    ) else {
        return;
    };
    if let Some(mut inventory) = world.get_mut::<MainInventory>(player) {
        let _ = inventory.0.set_item(usize::from(slot), after);
    }
    if matches!(packet.operation, Operation::Finalize(_)) {
        world.entity_mut(player).remove::<OpenBook>();
    }
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(McpePacket::from(transaction));
    }
}

#[cfg(test)]
mod tests;
