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
