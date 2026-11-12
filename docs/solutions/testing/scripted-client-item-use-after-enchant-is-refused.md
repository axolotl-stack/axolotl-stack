---
title: The scripted client's first item use after /enchant is refused, and /clear resends an empty slot
area: testing
tags: [bds-sweep, scripted-client, enchant, unbreaking, armour, flint_and_steel]
date: 2026-11-12
origin: items round 4 (Unbreaking sweep)
status: current
---

## Context

The first Unbreaking run (`tools/bds-sweep/scripts/unbreaking.txt`) gave good mining numbers,
but no armour wear at all and almost no flint and steel wear. Every armour piece's equip `use`
and most flint and steel `use-on` clicks were refused. A refused click shows up as BDS resending
the whole inventory and the clicked blocks.

## What we learned

- `/enchant` changes the held stack on the server, and BDS tells the client through an
  `InventoryTransaction` that `bds-sweep-client` does not apply. The client's next item use still
  claims the old stack, so BDS refuses it and resends the inventory. After that resend, uses
  work. Mining is not affected, because breaking does not claim the held stack.
- Fix in the script, not the client: after each `/enchant`, one throwaway `use` (a click in the
  air). Armour then needs a second `use` to equip. `unbreaking_script.py` does this.
- Reading wear back from resent inventories: a `/clear` before a fresh item resends slot 0
  empty. Taking an item's last `Damage` before the next marker therefore reads 0. Take the
  highest `Damage` instead, since wear only grows.
- Check that each section did what it was meant to before reading rates. A table with 0 wear, or
  with an armour piece missing from `PlayerArmorDamage`, means the uses were refused, not that
  Unbreaking spared everything.
- `PlayerArmorDamage` carries each worn piece's new total damage after a hit that wore it. Pieces
  that took no wear are absent from the packet.

## How to apply

Any script that `/enchant`s and then uses the item should spend one use to resync first. Count a
section's real samples from the capture (destroy particles, `ignite` sounds, armour packets),
not only from the script.
