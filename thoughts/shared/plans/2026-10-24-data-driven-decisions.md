# Data-driven gameplay: decisions (2026-10-24)

Decided by the project owner after the VANILLA-ITEMS research
(`thoughts/shared/research/2026-10-24-data-driven-items.md`). They apply to every
gameplay area (entities, items, blocks, world, survival) unless a later decision says
otherwise.

| # | Decision |
|---|---|
| D1 | Bump the shared vanilla pack input (`crates/unastar-data/data/vanilla_bp`, bedrock-samples) to 1.26.50 or newer, so pack data matches BDS 1.26.51. |
| D2 | Build-time codegen for the schemas (typed Rust structs/enums per data family); the vanilla data itself is embedded and decoded at startup. No per-entry Rust code is generated. |
| D3 | Copy BDS exactly, including permissive or quirky behaviour. Every weird or edge-case behaviour we copy is recorded in `thoughts/shared/research/bds-quirks.md` (what BDS does, how it was observed, why it looks odd). We refine those later, one decision at a time. |
| D4 | Built-in facts that BDS does not ship as data (armour values, tool tiers, block fuels, block drops, and similar) are measured from BDS with sweeps/extraction, not taken from wiki tables. |
| D5 | Scope is regular vanilla only for now. The registries and APIs are designed so that addon packs (behavior/resource packs loaded at runtime) and WASM mods (the existing wasmtime plugin system) can be bolted on later without redesign: data families are keyed by identifier, loaded into registries that can accept extra or overriding rows before they are frozen, and gameplay hooks go through explicit events/components rather than per-entity code. |

Evidence rule: nothing committed references decompilation (no tool names, addresses or
recovered symbols). Repo text describes behaviour and cites observed captures, public
Mojang documentation, public data sets or open-source code.

## Worldgen (2026-10-24, later the same day)

| # | Decision |
|---|---|
| D6 | Worldgen is ahead-of-time compiled end to end. Density functions already are; surface rules/builders, carvers, placed/configured features, feature rules, and structures (sets, placement, templates, jigsaw pools) are compiled from their data at build time into straight-line Rust, the way `unastar_noise` does today. For worldgen this overrides D2's "no per-entry generated code": the goal is the most efficient generator possible. A later addon/WASM overlay may use an interpreted path for extra content, but vanilla is compiled. |
| D7 | unastar ships two overworld generators: **Java-style** (Java Edition parity, the existing `unastar_noise` design finished to 100 %: terrain, biomes, surface rules, carvers, features, structures) and **Bedrock-style** (BDS 1.26.51 parity: the shared Java noise core with Bedrock seeding/biomes where they differ, Bedrock surface builders, feature rules/features, carvers and structures from the BDS install). Both share one generator trait, chunk pipeline, AOT toolchain and caches; the world config picks one. Finishing the Java-style generator comes first. |
| D8 | Each generator is gated by reference fixtures from its own truth source: a Java Edition server (run locally, loopback only) for Java-style, and the shared BDS runtime for Bedrock-style. Parity metrics ratchet: a change may only raise them. Performance is measured (ms/chunk, allocations) and ratchets too. |

## Gameplay architecture (2026-10-24, after the combined architecture review)

Recorded in full in `thoughts/shared/plans/2026-10-24-data-driven-architecture.md` §10.

| # | Decision |
|---|---|
| D9 | Entity data comes from bedrock-samples (the `vanilla_bp` submodule, 1.26.50.4+), checked against the BDS install's layered packs where present. |
| D10 | Filters, events, Molang and loot live in a separate bevy-free `unastar-rules` crate shared by runtime, datagen and worldgen codegen. |
| D11 | First mobs: zombie, skeleton, creeper, spider, cow, pig, sheep, chicken; then by component coverage. |
| D12 | Parked gameplay branches land with their non-BDS numbers recorded as known deviations, replaced by BDS sweeps later. |
| D13 | Artefacts derived from bedrock-samples are generated at build time, not committed (manifests only). Java worldgen data stays committed. |

## Known deviations (parked work)

Work written before these decisions and parked unmerged until its area's design is
approved. Each row names the slice that removes it.

| Branch | Deviation | Against | Removed by |
|---|---|---|---|
| `Jviguy/stab-vanilla-entities` | **X1:** 199 block loot tables (`unastar-data/data/overrides/loot_tables/blocks.json`) are authored from Java Edition tables, not measured from BDS | D4 | Entities E9: BDS block-drop sweep |
| `Jviguy/stab-vanilla-entities` | **X2:** tool-tier speeds/harvest levels, sword/shears speeds, mining wear and the break-time formula use unmeasured vanilla constants | D4 | Entities E9: BDS mining/tool-wear sweep; see `thoughts/shared/research/2026-10-24-data-driven-entities.md` §1.5 |

E0 removed the former D2 loot deviation: canonical pack/override JSON is built
into `OUT_DIR`, embedded, and decoded once into `unastar-rules::loot` types.
The evaluator moved to that bevy-free crate; X1/X2 above remain until E9.

### Mob runtime constants (D12)

Built-in mob behaviour that pack data does not carry lives in
`crates/unastar/src/entity/mob/constants.rs` (runtime) and
`crates/unastar-data/src/actor_components/provisional.rs` (defaults of component
fields that have no published schema). Status per group:

| Group | Values | Status |
|---|---|---|
| Move control | speed ramp 10 %/tick, arrival slow-down floor 0.5, descend cap 0.25, arrive at dist² < 0.1, step 0.5625, water jump trigger 0.009, jump cooldown 10 ticks, liquid jump impulse 0.04, default jump power 0.42, default max turn 30° | Confirmed against native behaviour (26.30 game build); re-check on 1.26.51 |
| Living physics | gravity 0.08, vertical drag 0.98, friction block × 0.91 (air block 0.6) sampled at feet − 0.1, ground acceleration × (0.546/friction)³, water drag 0.8, liquid gravity 0.02, underwater speed 0.02 | Confirmed (26.30) |
| Lava drag | 0.5 | Provisional: Java Edition value |
| Goals | stroll 10 candidates; panic 40-tick hurt window, ±5/±4, re-path 8; tempt radius 16 when unset, stop 1.5, cooldown 100, re-path 8–11, head 30°/tick, scare 6 blocks / 0.1 / 5°; follow_parent box 8/4/8, 3–16 blocks, re-path 10; look_at_player head 10°/tick, `look_time` in seconds; random_look_around `look_time` in ticks; breed search 8, check every 60 ticks, touch 0.4, partner within 3 | Confirmed (26.30) |
| Pathing | search range 16, max drop 3, waypoint reach 0.75/1.0, stuck check 100 ticks, damaging-block list, grass walk-target score | Provisional: Microsoft Learn navigation docs / Java Edition |
| Look idle head turn, entity push 0.05 | | Provisional: Java Edition |
| Combat | hurt timer 10 ticks with excess-only damage, knockback retain 0.5 + 0.4 horizontal + 0.4×1.2 vertical capped 0.4, death animation 20 ticks, loot at death, XP at removal, orb sizes 2477…1 | Confirmed (26.30) |
| Melee reach 6, last-hit-by-player window 100 ticks, XP orb lifetime/gravity/drag/follow | | Provisional: Java Edition / public client reach |
| Breeding/ageing | love 600 ticks, `breed_cooldown` and `ageable.duration` in seconds ×20 | Confirmed (26.30) |
| Feed growth 0.1, legacy bucket aux mapping, /summon whole-number centring | | Provisional: Microsoft Learn / Minecraft Wiki |
| Despawn | 32/128 blocks, 44 when tick range < 5, inactivity 30 s, 1/800 chance, persistent exemption | Confirmed (26.30) |
| Replication | MoveActorDelta past 4° summed rotation or 1e-4 position²; motion packet past Δv² 0.04 | Confirmed (26.30) |
| Living actor data at spawn | every non-derived actor-data key equals the native cow/chicken `AddActor` (`EntityMetadata::living_base`) | Measured from retained BDS 1.26.51 captures |
| Eating blocks (`behavior.eat_block`) | attempt when U(0,1) < `success_chance`; feet block, else the block below; timer `time_until_eat` × 20 ticks; `EatGrass` actor event on start; on finish, with `mobgriefing` on, destroy particles (level event 2001, placed at the feet block) and the replacement's default state, then `on_eat` either way; controls move/look/jump unless authored | Confirmed (26.30) |
| `mobgriefing` default true; legacy block names `grass` → `grass_block`, `tallgrass` → `short_grass` | | Provisional: Microsoft Learn game rules / pmmp BedrockBlockUpgradeSchema |
| Interactions | row cooldown gates only rows with a cooldown; `hurt_item` wears once (none in creative, unbreaking spares with n/(n+1)); `spawn_items` at feet + `y_offset`, pop velocity + U·0.05 up and (U−U)·0.1 sideways, pickup delay 10; event fires last | Confirmed (26.30); cooldown seconds × 20 provisional |
| Colour | `set_data_from_color_index` reads the THIS actor's colour and re-resolves `minecraft:wool` by it; dye sets colour 15 − the dye's item colour, a same-colour dye does nothing; only the sixteen `*_dye` items dye (the legacy dye items are gated on a world feature) | Confirmed (26.30) |
| Wool/dye item per colour (magenta, lime, purple, green), `muttonRaw` → `mutton` | | Provisional: Minecraft Wiki wool/dye data values / pmmp BedrockItemUpgradeSchema |
| Chicken class | wing-flap drag: airborne negative vy × (1 − 0.4) after gravity and drag; `FALL_DAMAGE_MULTIPLIER` actor data 0 | Drag confirmed (26.30 + 1.26.50.26); multiplier measured from the retained 1.26.51 chicken summon |
| Fall damage | safe 3 blocks, `ceil((d − 3 − jump boost) × data 111 × block)`, landing block 0.2 below the feet, `fall.big` above 4, ground dust (actor event 38) past 3 except on air/barrier/invisible bedrock; hay 0.2, honey/bed 0.5, slime/powder snow/end portal 0, dripstone tip +2 and × 2 | Confirmed (26.30) |
| `spawn_entity` | wait `min + random(max − min)` s × 20 with the rolling tick first; filters only at expiry; `plop` at mid-body; item at the feet with the item toss; saved per rule as `SpawnTimer`/`StopSpawning` | Confirmed (26.30 + 1.26.50.26) |
| Projectiles (egg) | flight: hit test, move without collision, × inertia (liquid inertia in water), − gravity, rotation 20 % towards the heading; target boxes grow (t − 2) × 0.05 up to 0.3; invisible on the first tick; on_hit in alphabetical order; spawn_chance second roll only after the first; iconcrack particle id 14 \| item id << 16 on level event 2009; `throw` sound | Confirmed (26.30); throw sound height (eye − 0.5) provisional |
| Component field defaults without a published schema | 124 consts in `provisional.rs` | Provisional: Microsoft Learn component pages (sources per const) |

Components accepted without server behaviour so the vanilla cow, pig and
sheep compile (`entity/mob/components/mod.rs` `ACCEPTED_WITHOUT_BEHAVIOUR`):
`leashable`, `leashable_to`, `balloonable`, `rideable`,
`behavior.mount_pathing`, `conditional_bandwidth_optimization`,
`game_event_movement_tracking` (only vibration listeners consume its game
events), and for the pig `boostable`, `item_controllable`,
`behavior.controlled_by_player` (all need riding) and `transformation` (only
gained from a lightning strike). Removed when leads, riding, vibrations and
lightning land.
