# Tasks

Each group may become its own OpenSpec change when work starts. Every group ends with
`parity-update` for the rows it touches.

## A. Measured drops and mining (deviations X1, X2)

- [ ] A.1 Map each measured drop case onto the loot IR (exact / empirical / unstable); delete the Java-authored block tables; update `server/game/block_drops.rs` docs and the item/block parity rows
- [ ] A.2 Extend the mining sweep beyond the 10-block slice and to airborne mining; replace the constants in `server/game/mining.rs`
- [ ] A.3 Mark X1/X2 resolved in decision 0023's consequences via a new record if the policy changes, otherwise in `docs/research/mob-runtime-constants.md`

## B. Loot completion

- [ ] B.1 MT19937 loot random in BDS draw order; seeded chest fixture
- [ ] B.2 Implement the unsupported loot functions and conditions listed in the `crafting.loot_tables` row

## C. Addon packs

- [ ] C.1 Startup pack loader for actors, items, blocks, recipes before freeze
- [ ] C.2 Fixture pack that overrides one mob and adds one custom mob; `platform.addons` row

## D. World ticking and protocol leftovers

- [ ] D.1 Random-tick sweep, then scheduler, then crop/leaf/grass behaviour (`world.random_ticks`)
- [ ] D.2 Autosave cadence capture, then periodic save of dirty chunks
- [ ] D.3 `SetHealthPacket`; test for `SetCommandsEnabledPacket`
- [ ] D.4 `UnlockedRecipes` and recipe-book auto-craft (`CraftRecipeAuto` layout from a captured client packet, via `valentine_gen` overrides if the layout is wrong)

## E. Engineering follow-ups

- [ ] E.1 Make `tick_breeding` and `tick_baby_growth` non-exclusive systems
- [ ] E.2 Per-tick mob benchmark (ms and allocations per 100 mobs)
- [ ] E.3 Move shared damage state (hurt timer, `LastHurt`, `Dead`) out of `entity/mob/`
- [ ] E.4 Use or remove `LivingBundle` (`entity/bundles.rs`)
- [ ] E.5 Split actor-component codegen out of `crates/unastar-data/build/` into its own crate

## F. Constants

- [ ] F.1 Re-check rows confirmed on the 26.30 build against 1.26.51; replace provisional rows with measurements

## Validate (each group)

- [ ] V.1 `cargo fmt --all -- --check`; `cargo clippy` on touched crates with `-D warnings`; their `cargo test`
- [ ] V.2 `cargo run -p parity -- generate` and `-- check`
- [ ] V.3 `review-change`, then archive with the compound step
