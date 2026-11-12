# Design: generate one canonical enchantment type

## Decisions

### Where the type is generated

Options: `unastar-data` codegen; `unastar-rules/build.rs`. `unastar-data` depends on
`unastar-rules`, so a type generated there could not be used by the loot and enchanting rules.
Chosen: `unastar-rules/build.rs`, beside the filter-subject enum, which already reads
`unastar-data/data/`. Recorded as
[decision 0052](../../../docs/decisions/0052-generate-one-enchantment-type.md) (owner, 2026-11-12).

### Where the per-stack levels live

Options: parse NBT on every query (as before); cache a level table on `ItemStack`. Chosen: cache
`Enchantments` beside the user data, decoded whenever the user data is set through `with_nbt` or
`with_raw_nbt`; the user-data field became private so the two cannot drift. Equality and stacking
compare only the user data, of which the table is a function. Unknown `ench` ids stay in the buffer.

### Sweep fixture tool keys

Options: convert the 1,476 hashed drop-sweep artefacts and the summariser to structured tools;
parse the `diamond_pickaxe+fortune3` keys through `Enchantment::from_str`. Chosen: parse, because
converting rewrites every hashed sweep artefact and collides with open lanes that regenerate
`mining_ticks.json` in the same key format.
