# unastar — agent guide

Extends the root [AGENTS.md](../../AGENTS.md). Behaviour, configuration and evidence tables are in
[README.md](README.md) (long; jump by heading).

## Rules for gameplay work

- **BDS 1.26.51 is the reference.** Copy what it does, including odd behaviour, and add a row to
  [docs/quirks/bds.md](../../docs/quirks/bds.md) for anything that looks permissive or buggy
  (`log-quirk` skill). Java-style world generation copies Java Edition 1.21.11 and logs quirks
  in [docs/quirks/java.md](../../docs/quirks/java.md).
- **Data before code.** Vanilla behaviour comes from data (`unastar-data`, behaviour packs via
  `unastar-packs`, rules via `unastar-rules`) decoded at startup into registries keyed by
  identifier. Don't hard-code per-entry Rust for something vanilla ships as data. Facts BDS doesn't
  ship are measured from BDS (`bds-sweep`, `bds-extractor`). See [docs/decisions/](../../docs/decisions/).
- **Parity rows in the same commit** (`parity-update` skill). `done` needs a real test here;
  `verified` also needs committed native evidence (capture or fixture) the test compares against.
  `MobRegistry::unsupported(identifier)` (`src/entity/mob/archetype.rs`) says why the runtime
  can't spawn an actor yet; use it to refresh entity rows.
- **The server is authoritative**: inventory, reach, gamemode and item consumption are checked
  server-side; never trust a client claim to skip a check.
- **Plugins**: cancellable events are dispatched before the world is mutated; plugin
  instantiation waits for a live world. Contract: [openspec/specs/plugin-host/spec.md](../../openspec/specs/plugin-host/spec.md).
- **Persistence**: loaded chunk columns are never regenerated or marked dirty; player data is
  restored and saved symmetrically; corrupt records are preserved, not dropped.

## Code shape

- **One canonical type per domain concept.** Enchantments, mob effects, dimensions, game modes,
  damage causes and actor flags each have exactly one type; extend it, never add a parallel enum,
  `const` table or `match name { "minecraft:x" => … }`. Before adding one, `rg` for the existing
  type. A missing entry goes into its source data and is regenerated, not hand-added in a branch.
- **No hand-written id↔name tables when data exists.** Ids, names and per-entry facts come from
  `unastar-data` (vanilla packs, the BDS command capture in `output/available_commands.json`,
  `data/measured/`) or from `valentine`, through a generator that cross-checks its sources and
  fails the build on a mismatch. Protocol magic numbers (level events, ability bits, update flags)
  live in exactly one module with their provenance.
- **Resolve names when data loads, with a typed error; runtime code takes typed values.** Decoders
  turn names into ids and report the source path on failure. Gameplay code doesn't take `&str`
  identifiers where a typed id exists, doesn't match on `"minecraft:…"` strings and doesn't build
  or strip `minecraft:` by hand. No `unwrap`/`expect`/`panic!` on data from the network, disk or
  plugins outside tests.
- **Conversions are trait impls; tests reuse production code.** `From`/`TryFrom` for ids,
  `FromStr`/`Display` for names, serde for config and fixtures. Test helpers call production
  parsers and the shared test support instead of re-implementing them, and fixtures are structured
  JSON deserialized into production types, never encoded strings parsed with `split`/`trim_*`.

## Map

| Path | What |
|---|---|
| `src/ecs/` | App, schedules, resources, events (bevy_ecs 0.17) |
| `src/server/` | Connection handling and game packet handlers (`server/game/`) |
| `src/entity/` | Actors: components, definitions, mobs, goals, navigation, spawning, projectiles |
| `src/item/`, `src/crafting/`, `src/block_entity/` | Item stacks, enchanting, equipment, recipes, block entities |
| `src/command/` | Data-driven command syntax, parser, selectors, executors |
| `src/world/` | Chunks, both world generators (`world/generator/`) |
| `src/storage/` | LevelDB / BDS-compatible persistence |
| `src/plugin/` | Wasmtime plugin host |
| `src/registry/` | Block/item registries, collision |
| `tests/` | End-to-end loopback tests and parity/worldgen fixtures (`tests/fixtures/`) |

## Features and validation

- `java-worldgen` needs `UNASTAR_JAVA_SERVER_JAR` (Java 1.21.11 `server.jar`, never committed);
  `bedrock-worldgen` needs `UNASTAR_BDS_DIR` (local BDS 1.26.51). Default builds need neither.
- Never commit Mojang assets or BDS binaries; captures and caches live under the gitignored `/.cache/`.

```sh
cargo clippy -p unastar --all-targets -- -D warnings
cargo test -p unastar --lib --tests
cargo run -p parity -- check
```
