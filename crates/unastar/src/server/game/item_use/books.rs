//! Writing books: `BookEdit`.
//!
//! The client edits a book and quill (`minecraft:writable_book`) in its
//! inventory slot `book_slot` and tells the server each change: replace a
//! page (missing pages up to it are added empty), add a page before an
//! index, delete a page, swap two existing pages, or sign. Signing turns the
//! book into a `minecraft:written_book` carrying the pages, the `title`,
//! the `author` (any text the player typed) and the `xuid` from the packet
//! and `generation` 0 (original). Pages are `{text, photoname}` compounds
//! in the item's `pages` list (Bedrock item format).
//!
//! Limits (public reference, Minecraft Wiki, Book and Quill): at most
//! [`MAX_PAGES`] pages of [`MAX_PAGE_CHARS`] characters, a title and an
//! author of at most [`MAX_NAME_CHARS`] characters. An edit that breaks a
//! limit, names a missing page or a slot without a book and quill is
//! ignored and the slot is sent back to the client; a signed book is sent
//! to the client. The page and signed-book NBT match BDS 1.26.51
//! (client-diff `itemuses-misc`), which answers every edit with an s2c
//! `InventoryTransaction` replacing the book; this server sends
//! `InventorySlot` only for signing and rejections.

use bevy_ecs::prelude::*;
use jolyne::valentine::BookEditPacket;
use jolyne::valentine::types::BookEditPacketOperation as Operation;
use zuri_nbt::{NBTTag, tag};

use crate::entity::components::{GameMode, MainInventory};
use crate::item::ItemStack;
use crate::item::enchant::{item_tag, with_item_tag};

pub const WRITABLE_BOOK: &str = "minecraft:writable_book";
pub const WRITTEN_BOOK: &str = "minecraft:written_book";
pub const MAX_PAGES: usize = 50;
pub const MAX_PAGE_CHARS: usize = 256;
pub const MAX_NAME_CHARS: usize = 16;

/// `BookEdit` packets waiting for [`apply_book_edits`].
#[derive(Resource, Debug, Default)]
pub struct BookEditQueue(pub Vec<(Entity, BookEditPacket)>);

/// One page: its text and photo name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Page {
    pub text: String,
    pub photo: String,
}

/// The pages of a book.
pub fn pages(stack: &ItemStack) -> Vec<Page> {
    let Some(NBTTag::List(list)) = item_tag(stack).and_then(|t| t.0.get("pages").cloned()) else {
        return Vec::new();
    };
    let string = |page: &tag::Compound, key: &str| match page.0.get(key) {
        Some(NBTTag::String(value)) => value.0.clone(),
        _ => String::new(),
    };
    list.0
        .iter()
        .filter_map(|page| match page {
            NBTTag::Compound(page) => Some(Page {
                text: string(page, "text"),
                photo: string(page, "photoname"),
            }),
            _ => None,
        })
        .collect()
}

fn string_tag(value: &str) -> NBTTag {
    NBTTag::String(tag::String(value.to_owned()))
}

/// `stack` with `pages` stored.
fn with_pages(stack: &ItemStack, pages: &[Page]) -> ItemStack {
    let mut compound = item_tag(stack).unwrap_or_default();
    let list = pages
        .iter()
        .map(|page| {
            let mut entry = std::collections::HashMap::new();
            entry.insert("photoname".to_owned(), string_tag(&page.photo));
            entry.insert("text".to_owned(), string_tag(&page.text));
            NBTTag::Compound(tag::Compound(entry))
        })
        .collect();
    compound
        .0
        .insert("pages".to_owned(), NBTTag::List(tag::List(list)));
    with_item_tag(stack, compound)
}

fn fits(text: &str, limit: usize) -> bool {
    text.chars().count() <= limit
}

fn index(value: i32) -> Option<usize> {
    usize::try_from(value).ok().filter(|i| *i < MAX_PAGES)
}

/// Apply `operation` to the book and quill `stack`; `None` when it is
/// rejected.
pub fn edit(stack: &ItemStack, operation: &Operation, world: &World) -> Option<ItemStack> {
    if stack.is_empty() || stack.item_id() != WRITABLE_BOOK {
        return None;
    }
    let mut book = pages(stack);
    match operation {
        Operation::ReplacePage(replace) => {
            let at = index(replace.page_index)?;
            let text = replace.page_text.to_string();
            if !fits(&text, MAX_PAGE_CHARS) {
                return None;
            }
            if book.len() <= at {
                book.resize(at + 1, Page::default());
            }
            book[at] = Page {
                text,
                photo: replace.photo_name.to_string(),
            };
        }
        Operation::AddPage(add) => {
            let at = index(add.page_index).filter(|at| *at <= book.len())?;
            let text = add.page_text.to_string();
            if book.len() >= MAX_PAGES || !fits(&text, MAX_PAGE_CHARS) {
                return None;
            }
            book.insert(
                at,
                Page {
                    text,
                    photo: add.photo_name.to_string(),
                },
            );
        }
        Operation::DeletePage(delete) => {
            let at = index(delete.page_index).filter(|at| *at < book.len())?;
            book.remove(at);
        }
        Operation::SwapPages(swap) => {
            let a = index(swap.page_index).filter(|at| *at < book.len())?;
            let b = index(swap.swap_with_index).filter(|at| *at < book.len())?;
            book.swap(a, b);
        }
        Operation::Finalize(sign) => {
            let (title, author) = (sign.title.to_string(), sign.author.to_string());
            if !fits(&title, MAX_NAME_CHARS) || !fits(&author, MAX_NAME_CHARS) {
                return None;
            }
            let written = crate::server::game::world_stack(world, WRITTEN_BOOK, 1)?;
            let mut compound = item_tag(&with_pages(stack, &book)).unwrap_or_default();
            compound.0.insert("title".to_owned(), string_tag(&title));
            compound.0.insert("author".to_owned(), string_tag(&author));
            compound
                .0
                .insert("xuid".to_owned(), string_tag(&sign.xuid.to_string()));
            compound
                .0
                .insert("generation".to_owned(), NBTTag::Int(tag::Int(0)));
            return Some(with_item_tag(&written, compound));
        }
    }
    Some(with_pages(stack, &book))
}

/// System (exclusive, `PacketApplySet`): apply queued book edits in order.
pub fn apply_book_edits(world: &mut World) {
    let edits = std::mem::take(&mut world.get_resource_or_init::<BookEditQueue>().0);
    for (player, packet) in edits {
        if world.get::<GameMode>(player) == Some(&GameMode::Spectator) {
            continue;
        }
        let Some(slot) = usize::try_from(packet.book_slot).ok() else {
            continue;
        };
        let Some(stack) = world
            .get::<MainInventory>(player)
            .and_then(|inv| inv.0.item(slot).cloned())
        else {
            continue;
        };
        match edit(&stack, &packet.operation, world) {
            Some(edited) => {
                if let Some(mut inv) = world.get_mut::<MainInventory>(player) {
                    let _ = inv.0.set_item(slot, edited.clone());
                }
                if matches!(packet.operation, Operation::Finalize(_)) {
                    crate::server::game::send_slot(world, player, slot, &edited);
                }
            }
            None => crate::server::game::send_slot(world, player, slot, &stack),
        }
    }
}
