---
# MADR 4 — https://adr.github.io/madr/
status: accepted
date: 2026-11-12
decision-makers: [Jviguy]
supersedes:
---

# Generate one canonical Enchantment type from source-attributed data

## Context and Problem Statement

An audit found five enchantment tables on main: the random-enchanting table in
`unastar_rules::enchanting` (numeric ids), a hand-written seven-variant `Enchantment` enum in
`unastar::item::enchant` with its own name match, a second name list and a lang-key table in
`/enchant`, a numeric-id switch in the loot adapter plus an id table in the loot decoder, and a
string match in the drop-sweep test helper. Open lanes extended the hand enum independently. Every
new enchantment rule meant another table, and an unknown name in loot data only failed when rolled.

## Decision Drivers

- One type per concept; no stringly-typed lookups on hot paths (owner rule).
- Vanilla facts come from data with recorded provenance (0013, 0014); generated code is never
  edited by hand.
- Bevy-free `unastar-rules` and the server must share the type without a dependency cycle
  (`unastar-data` depends on `unastar-rules`).
- Sharpness and the other combat enchantments are read on every hit.

## Considered Options

- Keep extending the hand-written enum and tables.
- Generate the enum in `unastar-data`'s build or codegen.
- Generate the enum in `unastar-rules`' build script, beside the filter-subject enum.

## Decision Outcome

Chosen option: generate `unastar_rules::ids::Enchantment` in `unastar-rules/build.rs`
(`build/enchantments.rs`), chosen by the owner on 2026-11-12, because it is the one crate that
`unastar-rules`, `unastar-data` and `unastar` can all depend on, and the build script already
generates schema enums from the same data directories.

- Ids and names come from the BDS 1.26.51 AvailableCommands capture (command enum `Enchant`,
  id = index). The build fails unless Mojang's bedrock-protocol-docs `Enchant__Type`, the vanilla
  `mojang-enchantments.json` data module (which adds the alias `bow_infinity`) and `en_US.lang`
  agree.
- Rules BDS doesn't ship as data (weights, slots, costs, exclusive groups, lists, treasure,
  max levels, lang keys) moved unchanged into `unastar-data/data/measured/enchantments.json`
  with their provenance.
- The generated surface: `#[repr(i16)]` ids, `ALL`, `COUNT`, `NAMES`, `name()`, `identifier()`,
  `max_level()`, `lang_key()`, checked `TryFrom<i16>` with the typed `UnknownEnchantment` error,
  `From<Enchantment> for i16`, `FromStr` (bare, `minecraft:` and aliases; other namespaces
  rejected), `Display` and serde by name.
- Data that names enchantments holds `Enchantment` values. Loot tables decode them through serde
  at load, so a bad name in the embedded vanilla tables fails the `unastar-data` build and one in
  any other table is a `ParseError` naming the table.
- `Enchantments([u8; Enchantment::COUNT])` is the per-stack level table. `ItemStack` decodes it
  once whenever its user data is set and keeps it beside the bytes; equality and stacking still
  compare only the bytes. `ench` entries with unknown ids stay in the buffer and never enter the
  table.

### Consequences

- Good, because there is one table: adding a rule is a data edit, and every enchantment
  (all 42) is available to every lane.
- Good, because hot-path queries are an array index and untrusted ids fail through a typed error.
- Bad, because `ItemStack` grows by 42 bytes and its user data is now private behind
  `nbt()`/`with_nbt()`/`with_raw_nbt()`.
- Bad, because the drop and mining sweep fixtures still spell tools as `diamond_pickaxe+fortune3`;
  tests parse them through `FromStr` instead of converting 1,476 hashed sweep artefacts.
- Neutral: vanilla gameplay is unchanged, but malformed or foreign item NBT now reads through the
  level table, which treats level 0 as absent and accepts any integer id tag. A Curse of Binding
  entry without a level (or at level 0) no longer stops a mob from swapping the armour piece.
  `/enchant` on an item with an unknown `ench` id keeps that entry (moved after the known ones)
  where it used to panic. Loot names that only failed when rolled (and `bow_infinity`, which
  used to be unsupported) are now checked at load. None of this is measured against BDS.

### Confirmation

`unastar-rules` tests compare every id and name with the capture and every rule with the data
file; `unastar` tests round-trip the level table through item NBT. `review-change` checks that
no new enchantment table or name match appears.
