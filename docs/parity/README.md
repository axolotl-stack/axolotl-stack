# Vanilla parity catalog

One list of everything vanilla Bedrock 1.26.51 has, and how far `unastar` is with each
thing, across every domain. The summary is [PARITY.md](PARITY.md). If you are looking for
something to work on, start at its [Where to help](PARITY.md#where-to-help) section: per
domain, the free families with the most missing or partial rows, and the areas already
claimed in [claims.toml](claims.toml).

## The rule

**A feature is not finished until its parity rows are updated in the same commit.**
That means status, tests, evidence and gaps in `docs/parity/<domain>.toml`, followed by
`cargo run -p parity -- generate` and a passing `cargo run -p parity -- check`. CI runs the check.

## Files

| File | What it is | Edited by |
|---|---|---|
| `PARITY.md` | Counts per domain and family, where to help (free and claimed areas), the most incomplete sub-row facets, and gap lists | generated; never edit |
| `<domain>.toml` | One status entry per generated row | you (values only) and `parity generate` (layout) |
| `claims.toml` | Areas someone is actively working on, and by whom | you (add a claim when you start, remove it when you stop) |
| `source/bedrock-worldgen.tsv` | Bedrock worldgen row list, generated from a BDS install | generated; never edit |

## Where the rows come from

The denominator is generated from vanilla data, never typed by hand, so a new vanilla
version adds rows automatically. The generators live in `crates/parity/src/sources/`.

| Domain | Rows | Source | Family |
|---|---|---|---|
| entities | every actor; sub-rows `<actor>/<component>` for each component in its base and component groups, `<actor>/spawn_rules` | `unastar_data::actor_definitions` (bedrock-samples behaviour pack) and the BDS 1.26.51 `AvailableActorIdentifiers` capture (`unastar_data::actor_identifiers`); `unastar_data::spawn_rules` | boss, projectile, hostile, passive, ... from the definition's components and `type_family` |
| blocks | every block | protocolgen 1.26.51 native palette (`unastar_data::blocks::ALL_BLOCKS`) | behaviour family (doors, slabs, redstone, liquids, crops_growing, functional, ...) by name/property rules in `sources/blocks.rs` |
| items | every item; sub-rows `<item>/<facet>` for each server-side behaviour | BDS 1.26.51 item registry (`unastar_data::items`), facets from native properties/tags, the measured BDS fuel/armour/melee tables, vanilla item documents and the block/actor registries | the item's most specific facet |
| commands | every advertised command | the commands lane's decoded BDS 1.26.51 `AvailableCommands` capture, `crates/unastar-data/output/available_commands.json` | permission level |
| protocol | every packet | `crates/valentine_gen/protocolgen/generated/1.26.51/manifest.json` | manifest direction (`bidirectional` includes client-to-server packets) |
| worldgen | Java 1.21.11 and Bedrock 1.26.51 biomes, features, structures, carvers and surface rules per dimension | Java: `crates/unastar_noise/worldgen_data` and its biome tags. Bedrock: the BDS install through `unastar_bedrock_codegen::model::Model` (rows cached in `source/bedrock-worldgen.tsv`) | `<edition>/<dimension>/<kind>` |
| systems | cross-cutting systems no data file enumerates (redstone, liquids, weather, lighting, enchanting, raids, ..., and speed against BDS) | hand-maintained list in `crates/parity/src/sources/systems.rs` | world, mobs, player, crafting, interface, platform, performance |

Notes on ids:

- Java surface-rule rows are `java/<dimension>/surface_rule/<index>-<block>`: the
  block-placing leaves of the dimension's `noise_settings` rule tree in rule order.
- `bedrock/unmatched/*` are feature rules whose biome filter matches no biome of any
  dimension under our filter evaluation (the same one the Bedrock generator uses).
- Bedrock worldgen data ships only inside BDS (Minecraft EULA). The snapshot holds row ids
  and short labels only. Without `UNASTAR_BDS_DIR`, both commands use the snapshot; with it,
  `generate` refreshes the snapshot and `check` fails if the snapshot is stale.

## Status entries

Every row has exactly one entry. Fields, in this order (empty ones omitted):

| Field | Meaning |
|---|---|
| `status` | `missing`, `partial`, `done`, `verified` or `not_applicable` |
| `owner` | lane or person responsible (`entities`, `items`, `blocks`, `commands`, `wg-bedrock`, `wg-java`, `protocol`, `mainline`, `platform`) |
| `tests` | `path` or `path::test_fn`, relative to the repository root |
| `evidence` | committed native evidence (BDS/Java capture or fixture path), or `none` |
| `gaps` | short text: what is still missing |
| `notes` | anything else, e.g. work that exists on an unmerged branch |

| Status | Requires | Means |
|---|---|---|
| `missing` | nothing | nothing implemented |
| `partial` | `gaps` | some behaviour exists; `gaps` says what does not |
| `done` | `owner`, `tests` | implemented and a test in the workspace exercises it |
| `verified` | `owner`, `tests`, `evidence` (an existing repository path) | done, and the test compares against that native evidence |
| `not_applicable` | `notes` | vanilla has it but a server never implements it (client-only, editor or Education Edition content); excluded from percentages |

Statuses describe the code on the branch the file is on. Work that only exists on an
unmerged branch goes in `notes` until it is merged.

Text in `gaps`, `notes` and `evidence` describes behaviour, never engine internals (no
BDS or client class/function names, addresses or offsets). Black-box provenance such as
"matches the BDS 1.26.51 capture" is fine. `parity check` rejects a few obvious patterns.

## Headline rows

Each domain's first summary line in `PARITY.md` is its headline, and the family table,
gap list and "Where to help" count those rows. Usually they are the top-level rows. Item
rows only mean the item exists in the registry, so the items headline is its behaviour
sub-rows and the registry line is labelled "registered"; entity rows mean an actor can be
summoned and is replicated, and are labelled so. `Domain::headline`,
`Domain::top_level_label` and `Domain::sub_row_label` in `crates/parity/src/catalog.rs`
decide this.

## Claims

[`claims.toml`](claims.toml) records who is working on what, so contributors do not
duplicate work. Each `[[claim]]` has:

| Field | Meaning |
|---|---|
| `area` | short description |
| `owner` | `@github-handle` for contributors, `lane NAME` for the maintainers' agent lanes |
| `since` | the date the claim was made, as a TOML date (`2026-10-29`, no quotes) |
| `domain` | status file stem (`entities`, `blocks`, `items`, `commands`, `protocol`, `worldgen`, `systems`) |
| `families` | row families of that domain, as in `PARITY.md`'s family tables |
| `ids` | row ids; each also covers the row's sub-rows |
| `prefixes` | plain prefixes of row ids (`bedrock/`, `world.`) |

A claim covers one domain; an area spanning several domains has one claim per domain.
Add a claim in your first pull request for an area and remove it when you stop. Claims
never change a row's status. Overlap is an error: to take part of a claimed family, agree
with its owner and narrow their claim to ids.

## Commands

```sh
cargo run -p parity -- generate   # refresh rows, add `missing` entries, rewrite status files and PARITY.md
cargo run -p parity -- check      # validate; what CI runs
UNASTAR_BDS_DIR=/path/to/bds/server cargo run -p parity -- generate   # also refresh the Bedrock worldgen snapshot
```

`parity check` fails when:

- a generated row has no status entry, or a status entry names a row that is no longer
  generated (rename or delete it by hand; `generate` refuses to drop it silently);
- a `done`/`verified` row lists no test, or a test file or `fn` that does not exist;
- a required field is missing (see the table above), or `verified` evidence is not a path in
  the repository;
- a claim names an unknown domain, family or row id, a prefix that matches no row, has no
  owner, no selector or a `since` that is not a date, or two claims cover the same row;
- a status file is not in canonical form, `PARITY.md` is stale, or (with `UNASTAR_BDS_DIR`)
  the Bedrock worldgen snapshot is stale.

## Evidence documents

These stay where they are and are linked from rows' `evidence`/`notes` where relevant:

- BDS behaviour quirks: [`thoughts/shared/research/bds-quirks.md`](../../thoughts/shared/research/bds-quirks.md)
- Built-in constants and their provenance (D12): [`thoughts/shared/plans/2026-10-24-data-driven-decisions.md`](../../thoughts/shared/plans/2026-10-24-data-driven-decisions.md)
- Java worldgen pipeline and evidence: [`crates/unastar_noise/README.md`](../../crates/unastar_noise/README.md)
- Bedrock worldgen measurement ledger: [`crates/unastar_bedrock_codegen/README.md`](../../crates/unastar_bedrock_codegen/README.md)
- Measured BDS tables: [`crates/unastar-data/data/measured/`](../../crates/unastar-data/data/measured/)

The former entity backlog (`thoughts/shared/plans/2026-10-28-entity-backlog.md`) and the
parity items of the mob-runtime follow-ups are folded into `entities.toml` and
`systems.toml`.
