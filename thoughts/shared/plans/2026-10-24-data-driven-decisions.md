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
