# Mob runtime: follow-ups and design notes (2026-10-28)

Tracking list for the definition-driven mob runtime (`crates/unastar/src/entity/mob/`),
started with the vanilla cow. Each row says what, why, and its state. Update the state
column when an item lands; delete rows only when done.

## ECS cost

| # | Item | Why | State |
|---|---|---|---|
| E1 | Transient mob state (`InLove`, `BreedCooldown`, `LastHurt`, `Persistent`, `Dead`, hurt timer, baby age) uses sparse-set storage | Table storage moves all ~40 components of a mob to a new archetype on every insert/remove; these come and go during a mob's life | Done for `mob/state.rs` (df2679cc); hurt timer and baby age in the combat/interaction slices |
| E2 | Timers are absolute deadlines on `TickCounter::current`, not per-tick countdowns | Countdown mutates every component every tick and trips change detection for nothing; a deadline is set once and compared read-only | Done for love/cooldown contract (df2679cc); hurt and death timers in the combat slice |
| E3 | No lazy "insert next tick": state that exists from spawn or composition is inserted with it (`MobBrain` at spawn, `BabyAge` with `minecraft:ageable`) | Each lazy insert is an extra archetype move per mob | In the AI and interaction slices |
| E4 | Neighbour searches (tempting player, parent, breeding partner) bucketed by chunk | Full scans are O(mobs × actors) per tick | AI slice (chunk-bucketed index) |
| E5 | AI, breeding and ageing are exclusive (`&mut World`) systems | Pathfinding reads blocks while mutating mobs; simplest correct form | Open: convert to parallel systems (`WorldBlocks` + queries) once a profile shows the serial point matters |
| E6 | Per-tick mob benchmark (N cows, ms/tick, allocations) | Turn the reasoning above into numbers and a ratchet | Open |
| E7 | Definition composition inserts/removes components (e.g. baby → adult) | One batched change per entity at the apply barrier, once per lifetime; acceptable | Note only |

## Component placement

| # | Item | Why | State |
|---|---|---|---|
| C1 | Behaviour-pack component state lives in `entity/mob/config.rs`; shared mob runtime state in `entity/mob/state.rs`; engine-level components stay in `entity/components` | Pack-derived components are created and removed by the definition stack next to their registration; players/items never carry them | Note only |
| C2 | Move generic damage state (last hurt, hurt timer, dead) to `entity/components/living.rs` and share one damage path between players and mobs | Players need the same hurt/invulnerability rules when player combat is built | Open (after the combat slice lands; player combat is another lane's scope) |
| C3 | Remove legacy `MobType`, `AiState`, `Hostile`, `Tameable` and `MobBundle` | Unused, duplicated by `MobKind` and the AI brain | Done |
| C4 | `LivingBundle` in `entity/bundles.rs` is unused | Dead code | Open: delete or adopt when players/mobs share spawn bundles |
| C5 | Typed errors (`thiserror` enums) instead of `Result<_, String>`. Spine first: `ComponentKindsBuilder::register` compile closures return a typed `ComponentCompileError`; `CompiledDefinition::compile` → `DefinitionError`; `CompiledEvents`/`fire_event` → `EventError`; `MobRegistry`/`spawn_mob` → `MobSpawnError`; `families`/`flags`/`definition_components` follow. Then every slice wrapper (`Spawn(String)` etc.) becomes the typed source. Invariant violations take guaranteed data or `expect` | String errors can't be matched on, carry no structure and blur invariants with recoverable failures | Next, after milestone-2 slices land (~45 sites) |
| C6 | Restructure `entity/mob/` (28 files flat) into: `mob/{mod, archetype, derive, state, constants, native, events}.rs`; `mob/components/` (pack-component configs split out of the 1000-line `config.rs`, one file per family: goals, movement, body, breeding, loot/xp, interact, color, shearing, spawn_entity, damage_sensor, ambient, despawn); `mob/combat/` (combat, damage_sensor, fall, death); `mob/life/` (ageing, breeding, egg laying, despawn); `mob/interaction/` (actions, interact); `mob/net/` (network, sound, ambient data); `mob/persistence.rs`; `mob/tests/` (spawn, chicken, interaction, passive). The thrown-egg projectile moves to `entity/projectile/` (not mob-specific). The generic definition layer (`definition*`, `events`, `families`, `flags`, `properties`, `attributes`, `metadata`) moves under `entity/definition/` | A flat folder of 28 files mixes data, systems and tests; modules should follow behaviour boundaries | Next, with C5, after milestone-2 slices land |

## Data and codegen

| # | Item | Why | State |
|---|---|---|---|
| D1 | Split actor-component codegen out of `unastar-data` into its own small crate | Any edit under `unastar-data/build/` reruns the whole build script (actor definitions, loot, recipes, biomes, defaults, typed components); iteration takes minutes | Open (owner request) |
| D2 | Codegen judgement calls to confirm natively: omitted `health.max` equals `value`; `damage_sensor.deals_damage` defaults to yes; panic `damage_sources` uses the 18-cause legacy-form list | Public documentation contradicts itself or vanilla data | Open |
| D3 | Components accepted without server behaviour: `leashable`, `leashable_to`, `balloonable`, `rideable`, `behavior.mount_pathing`, `conditional_bandwidth_optimization` (`mob/config.rs` `ACCEPTED_WITHOUT_BEHAVIOUR`) | Needed for the cow definition to compile; leads and riding are not implemented | Open: implement leads/riding; mirror in decisions doc D12 |
| D4 | Unknown fields in component bodies reject the definition | Closed typed bodies (`deny_unknown_fields`) | Note only |
| D5 | Vanilla pack is embedded at build time; changing a definition needs a rebuild | Runtime addon/behaviour-pack loading not wired yet | Open |

## Native constants

| # | Item | Why | State |
|---|---|---|---|
| N1 | Built-in mob constants in `entity/mob/constants.rs` were confirmed against native behaviour of the 26.30 game build | Closest build with inspectable behaviour; not yet re-checked on 1.26.51 | Open: re-check on 1.26.51 when possible; mirror in decisions doc D12 |
| N2 | Lava drag is provisional (Java Edition value) | Not confirmed natively | Open |

## Gameplay surface

| # | Item | Why | State |
|---|---|---|---|
| G1 | Operators: nothing in production granted `Operator`, so `/summon` was unusable | Needed for client checks | Interaction slice adds an `operators` config list |
| G2 | No `/gamemode` command; creative only via `default_gamemode` (new players; saved players keep their mode) | Testing convenience | Open |
| G3 | Chicken (milestone 2), natural spawning of passive mobs | Milestone 2 | Open |
