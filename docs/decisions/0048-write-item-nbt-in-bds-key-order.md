---
status: proposed
date: 2026-11-12
decision-makers: [protocol lane (proposed; awaits the owner)]
---

# Write item NBT in BDS's key order

## Context and Problem Statement

Item user data carries an NBT compound. unastar wrote it with `zuri_nbt`, whose compound is a
`HashMap` written in the map's random order. BDS 1.26.51 writes compound keys sorted and types an
empty list `End` (the `book` probe fixtures: a page is `photoname`, `text`; a signed book `author`,
`generation`, `pages`, `title`, `xuid`; an opened book's `pages` is an empty list of type 0). A
multi-key compound from unastar therefore never matched BDS byte for byte, and two encodings of the
same item could differ. Raised by the OpenSpec change `protocol-client-input`.

## Decision Drivers

- Copy BDS exactly, bytes included (decision 0013), so fixtures can be compared byte for byte.
- One canonical NBT type: keep `zuri_nbt::tag::Compound` as the in-memory form.

## Considered Options

- A small writer (`crate::item::nbt::write_sorted`) for item user data that sorts keys and types
  empty lists `End`, used by `with_item_tag`.
- Replace `zuri_nbt` with an NBT crate that keeps insertion order, and insert keys in BDS order.
- Leave the order random and compare decoded NBT instead of bytes.

## Decision Outcome

Chosen option: "a small sorted writer", because it makes every item compound deterministic and
BDS-ordered without changing the in-memory type or any reader, and insertion order would still
need every builder to insert in BDS's order.

### Consequences

- Good, because item NBT is byte-identical to BDS's for the measured items and deterministic for
  all.
- Good, because a compound that cannot be written (a string over 65535 bytes) is now a typed error
  (`NbtWriteError`), not a panic.
- Bad, because unastar keeps a second NBT writer for item user data beside `zuri_nbt`'s.

### Confirmation

`crates/unastar/src/item/nbt.rs` tests;
`crates/unastar/src/server/game/books/tests.rs::edits_match_the_bds_book_capture_byte_for_byte`
compares 23 signed and unsigned books with the BDS fixtures byte for byte.
