# Tasks

## 1. Data and generator

- [x] 1.1 Move the random-enchanting rows and `/enchant` lang keys into `unastar-data/data/measured/enchantments.json` with provenance; list it in the measured README and `NOT_SWEPT`
- [x] 1.2 Generate `Enchantment` in `unastar-rules/build/enchantments.rs` from the command capture, failing the build on any disagreement with the protocol docs, the vanilla data module, `en_US.lang` or the data file
- [x] 1.3 Add `UnknownEnchantment`, `FromStr`, `Display`, serde and the `Enchantments` level table in `unastar-rules/src/ids.rs`, with tests against the capture and the data file

## 2. Migration

- [x] 2.1 `unastar_rules::enchanting` and loot: typed `EnchantmentInstance`, typed loot enchantment fields, `specific_enchants` decoded, `LootContext::enchantment_level(Enchantment)`
- [x] 2.2 `ItemStack` caches the level table; `item::enchant` keeps unknown `ench` entries; the hand enum is deleted
- [x] 2.3 Migrate loot, `/enchant`, mining, durability, drops, infinity, mob gear and tests; delete `BINDING_CURSE_ID` and `NAME_KEYS`

## 3. Validation

- [x] 3.1 `cargo fmt --all -- --check`
- [x] 3.2 `cargo clippy -p unastar -p unastar-data -p unastar-rules --all-targets -- -D warnings`
- [x] 3.3 `cargo test -p unastar --lib --tests` and `cargo test -p unastar-data -p unastar-rules -p parity`
- [x] 3.4 `cargo run -p parity -- check`, `openspec validate --all --strict`, `bun tools/agents/check-links.ts`
