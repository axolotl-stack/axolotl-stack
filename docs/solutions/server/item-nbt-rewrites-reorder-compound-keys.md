---
title: Item NBT rewrites reorder compound keys; never re-encode user data to read it
area: server
tags: [item, nbt, user data, enchantments, zuri_nbt, byte identical]
date: 2026-11-12
origin: openspec/changes/archive/2026-11-12-generate-enchantment-type
status: current
---

## Context

While moving item enchantments to a cached level table (decision 0052) we wanted a test that
writing the same enchantments back yields the same user-data bytes. It failed intermittently.

## What we learned

`zuri_nbt`'s `tag::Compound` is a `HashMap`, so every `with_item_tag` rewrite (enchantments,
`Damage`, block lists) serialises compound keys in hash order, which differs between maps. Two
rewrites of equal NBT are equal as NBT but not byte for byte. That was already true before the
level table. What keeps client and disk bytes stable is never re-encoding on a read path: the
stack stores the buffer it was given (`with_nbt`/`with_raw_nbt`) and only decodes from it.
Compare rewritten NBT with `item_tag(a) == item_tag(b)`; compare bytes only for buffers nothing
rewrote. A byte-exact rewrite would need an order-preserving compound type.

## Where it lives

`crates/unastar/src/item/enchant.rs` (`with_item_tag`, `decode_levels`, test
`level_table_round_trips_through_nbt`), `crates/unastar/src/item/stack.rs` (`with_raw_nbt`).
