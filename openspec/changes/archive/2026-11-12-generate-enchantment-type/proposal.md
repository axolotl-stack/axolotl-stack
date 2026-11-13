# Proposal: generate one canonical enchantment type

## Why

Main had five enchantment tables: the random-enchanting table in `unastar_rules::enchanting`, a
hand-written seven-variant enum in `unastar::item::enchant`, the `/enchant` name enum and lang-key
table, the loot decoder's and loot adapter's id switches, and the drop-sweep test helper's name
match. Open lanes kept extending the hand enum separately. Loot tables kept unknown enchantment
names as strings and only failed when a roll reached them.

Prior work: [decision 0021](../../../../docs/decisions/0021-shared-rules-engines-in-bevy-free-crate.md)
put shared rules in `unastar-rules`; [0013](../../../../docs/decisions/0013-copy-bds-behaviour-exactly.md)
and [0014](../../../../docs/decisions/0014-measure-built-in-facts-from-bds.md) set the data and
provenance rules. Nothing in `docs/solutions` or `docs/quirks` covers enchantments.

## What Changes

- Generate `unastar_rules::ids::Enchantment` (all 42 Bedrock 1.26.51 enchantments) in
  `unastar-rules/build.rs` from the BDS command capture, cross-checked against the protocol docs,
  the vanilla data module and `en_US.lang`; its rules come from the new
  `unastar-data/data/measured/enchantments.json`.
- Loot data holds `Enchantment` values decoded through serde: an unknown name is a `ParseError` at
  load (a build failure for the embedded vanilla tables). `specific_enchants` is decoded so its
  names are checked; rolling it stays unsupported.
- `ItemStack` decodes a per-stack level table once from its NBT; every user (loot, `/enchant`,
  mining, durability, drops, infinity, mob gear, tests) moves to the one type.
- No gameplay behaviour changes.

Out of scope: rolling `specific_enchants`, the enchanting table menu, new enchantment effects, and
converting the drop and mining sweep fixtures' `+fortune3` tool keys to structured JSON.

## Capabilities

- Modified: `rules-engine` (new generated enchantment type; loot enchantment names typed at load; random-enchanting evidence).

## Impact

Crates: `unastar-rules`, `unastar-data` (data file, manifest test), `unastar`. Parity rows: none,
the change is a refactor.
