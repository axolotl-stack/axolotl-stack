# Unastar Development Roadmap

This document outlines the development path from current state to a fully-featured Bedrock server. Phases are roughly ordered by dependency—later phases often depend on earlier ones.

See `LONGTERM.md` for the architectural vision (WASM plugins, tick phases, API design).


---

## Vanilla Compatibility Definition & Evidence Gates

Unastar should not claim broad vanilla compatibility until the gates below pass on the pinned Bedrock protocol version exposed through `jolyne::valentine` (`bedrock_1_26_40` today). The near-term target is **vanilla-compatible join, spawn, and basic creative interaction**, not full survival parity.

### Evidence Matrix

| Gate | Required evidence | Primary owners |
| --- | --- | --- |
| JOLYNE-M1 handshake smoke | Offline and online-mode login harness reaches `Play` without panics, with bounded timeouts and expected disconnect reasons. | `jolyne` |
| JOLYNE-M2 resource-pack negotiation | `ResourcePacksInfo -> ResourcePackStack -> Completed` is gated correctly; non-empty packs exercise `DataInfo`, `ChunkRequest`, and `ChunkData`; `ClientCacheStatus` is accepted/ignored safely. | `jolyne` |
| JOLYNE-M3 start-game order | Integration test asserts the documented `LOGIN_SEQUENCE.md` order: `StartGame`, `ItemRegistry`, `BiomeDefinitionList`, `AvailableEntityIdentifiers`, `CreativeContent`, `PlayStatus(Spawned)`, then `SetLocalPlayerAsInitialized`. | `jolyne`, `unastar` |
| UNASTAR-M1 boot/join/spawn | Launch a local Unastar server and one harness client; client joins, receives registries/start-game packets, requests radius, and remains connected. | `unastar` |
| UNASTAR-M2 first chunks | `RequestChunkRadius` yields `ChunkRadiusUpdated`, `NetworkChunkPublisherUpdate`, and first chunk/subchunk data without silent all-air fallback for load/encode errors. | `unastar` |
| UNASTAR-M3 creative loop | Creative inventory can place/break a block through server-authoritative validation; rejected actions resync client inventory/world state. | `unastar` |
| UNASTAR-M4 persistence smoke | Disconnect/restart/rejoin preserves player position and loaded chunk edits; later expands to inventory, effects, entities, and block entities. | `unastar` |
| DATA-M1 provenance | Blocks, items, biomes, entities, creative content, and worldgen tables map to a source artifact, generated table, protocol packet, and at least one runtime assertion. | `unastar-data`, `valentine`, `unastar` |
| CI-M1 compatibility gate | Linux and Windows CI run `cargo check/test -p unastar`, JOLYNE-M1..M3, and UNASTAR-M1..M2 before public docs use stronger compatibility wording. | workspace |

### Current Review Findings Driving The Gates

- `jolyne` has the typestate protocol foundation, RakNet/NetherNet transports, compression/encryption batching, and online/offline auth paths, but online-mode key trust, resource-pack sequencing, cache-status handling, and handshake timeouts need hardening before compatibility claims.
- `unastar` has the tick runtime, ECS packet queues, chunk radius/subchunk responses, async chunk generation, player spawn/movement/despawn broadcast, basic creative inventory/container handling, block break/place broadcasts, and LevelDB persistence scaffolding.
- `unastar` is not yet vanilla-authoritative: inventory requests, movement, and block interactions still trust too much client input; chunk streaming can hide errors; entity replication is player-centric; persistence omits several vanilla state domains.
- Existing tests are mostly unit/source-boundary level. Add scripted protocol smoke tests before marking any gate complete.

---

## Current State ✓

These are implemented foundations, not proof of the compatibility gates above. Keep each checked item paired with a smoke/integration test before using it as compatibility evidence.

- [x] Basic networking (RakNet via `jolyne`)
- [x] Player join/leave and authentication (Xbox Live + offline)
- [x] ECS architecture (`bevy_ecs`)
- [x] Chunk generation (flat world) and streaming
- [x] Player movement and position sync
- [x] Basic block breaking/placing with animations
- [x] Entity spawn/despawn broadcasting
- [x] Registry loading (blocks, items, biomes, entities)


---

## Phase -1: Compatibility Hardening Gates

Work this phase before broad feature expansion. It turns the current server foundation into a measured vanilla-client baseline.

### Jolyne Protocol Gates
- [ ] Fix online-mode trust chain validation so JWT `x5u`/`x5c` headers cannot select untrusted keys.
- [ ] Gate resource-pack progression on the correct `ResourcePackClientResponse` states and support non-empty pack `DataInfo`/`ChunkData` transfer.
- [ ] Handle `ClientCacheStatus` explicitly, even when blob cache support is disabled.
- [ ] Add deadlines and deterministic disconnect reasons for network-settings, login/auth, resource-pack, and start-game stages.
- [ ] Add `crates/jolyne/tests/login_sequence.rs` for the documented packet ordering in `docs/LOGIN_SEQUENCE.md`.

### Unastar Gameplay Authority Gates
- [ ] Replace client-authoritative inventory writes with a server-side transaction engine and mismatch resync packets.
- [ ] Validate movement against server collision/abilities/game mode and send corrections for invalid movement.
- [ ] Route block break/place through an authoritative action pipeline with reach, gamemode, tool, collision, permission, and rollback checks.
- [ ] Make chunk delivery explicit about loading/not-found/encode-error states; remove silent empty-payload/all-air fallbacks for unexpected errors.
- [ ] Expand replication from player-only broadcasts to generic actor/item entity lifecycle packets tied to chunk viewers.
- [ ] Persist vanilla player/world state beyond position: inventory, effects, abilities, block entities, entities, and biome data.

### Compatibility Test Gates
- [ ] Add `crates/unastar/tests/smoke_join.rs` to boot a server, join one harness client, request chunks, and assert no disconnect.
- [ ] Add chunk-streaming regression tests for duplicate viewer registration, disconnect cleanup, send failure retry, and max subchunk-request behavior.
- [ ] Add inventory/block/movement rejection tests that verify corrective packet output and no server-state mutation.
- [ ] Add CI jobs for `cargo check -p unastar --all-targets`, `cargo test -p unastar --lib`, source-boundary tests, and the smoke gates above.

---

## Phase 0: Configuration & Server Setup

Clean, validated configuration system.

### Config Architecture
- [ ] Unified config crate (`unastar-config`) with typed structs
- [ ] TOML-based config files with hot-reload support
- [ ] Config validation on load (port ranges, paths exist, etc.)
- [ ] Environment variable overrides for containerized deployments
- [ ] Default config generation on first run

### Server Config
- [ ] Network settings (bind address, max players, MOTD, online mode)
- [ ] Performance tuning (view distance, simulation distance, tick budgets)
- [ ] Security (whitelist, banned players/IPs, rate limits)

### World Config
- [ ] Per-world settings file (`worlds/<name>/world.toml`)
- [ ] Generator selection (flat, noise, void, custom)
- [ ] World-specific spawn point, game rules, difficulty
- [ ] Dimension type assignment (overworld, nether, end, custom)

---

## Phase 1: Multi-World & Persistence

Foundation for multiple independent worlds with persistence.

> **Design note:** "Dimensions" (overworld, nether, end) are not special—they are just worlds with a dimension type that affects lighting, sky, and coordinate scaling. The protocol sends a dimension ID; the server decides which world backs it.

### Multi-World Architecture
- [ ] `World` as the fundamental unit (owns chunks, entities, tick state)
- [ ] `WorldManager` to register/load/unload worlds dynamically
- [ ] Per-world ECS `World` instance or partitioned entity storage
- [ ] Player ↔ World association (transfer between worlds = chunk unload + reload)
- [ ] Cross-world entity references (for projectiles, etc.)

### World Storage
- [x] Chunk save/load (LevelDB-based `WorldProvider`)
- [x] Dirty chunk tracking (`ChunkModified` component)
- [x] Async save on chunk unload (save modified chunks before despawn)
- [x] Save all chunks on shutdown
- [ ] World metadata file (seed, spawn point, time, weather, game rules)
- [ ] World format versioning for future migrations

### Player Data
- [x] Position persistence (save on disconnect, load on join)
- [x] LevelDB-based player storage (`LevelDBPlayerProvider`)
- [ ] Full player save format (inventory, health, hunger, XP, effects) — *requires Phase 2*
- [ ] Per-world vs global player data (inventory per-world or shared?)
- [ ] Periodic autosave
- [ ] Player data migration/versioning

### Block Entities
- [ ] Chest, furnace, sign, etc. data storage
- [ ] Tile entity serialization with chunks

---

## Phase 2: Inventory & Items

Player and container inventory systems.

### Player Inventory
- [ ] Inventory component with proper slot layout (hotbar, main, armor, offhand)
- [ ] Held item tracking and switch handling
- [ ] Creative inventory packet support
- [ ] Survival inventory crafting grid

### Container Interactions
- [ ] Open/close container packets (chest, furnace, crafting table, etc.)
- [ ] Container transaction handling (click, drag, shift-click)
- [ ] Inventory sync on open + change broadcasting

### Item Stack Behavior
- [ ] Stack splitting, merging, swapping
- [ ] Item metadata/NBT (enchantments, damage, custom name)
- [ ] Durability and tool wear

---

## Phase 3: Commands & Chat

In-game text communication and command execution.

### Chat System
- [ ] Chat packet handling and broadcasting
- [ ] Chat formatting (colors, styles)
- [ ] Chat message types (system, whisper, announcement)
- [ ] Mute/ignore basics

### Command Framework
- [ ] Proper argument parsing (players, coordinates, selectors like `@a`, `@p`)
- [ ] Tab completion support
- [ ] Command permission levels
- [ ] Help and usage generation

### Built-in Commands
- [ ] `/gamemode`, `/tp`, `/give`, `/kill`, `/time`, `/weather`
- [ ] `/say`, `/tell`, `/me`
- [ ] `/kick`, `/ban`, `/op`, `/deop`
- [ ] `/setblock`, `/fill`, `/clone`

---

## Phase 4: Entities & Mobs

Non-player entities in the world.

### Entity Framework
- [ ] Entity spawning/despawning with proper IDs
- [ ] Entity metadata sync (health, flags, equipment)
- [ ] Entity movement and position broadcasting
- [ ] Pathfinding basics

### Passive Mobs
- [ ] Spawn mechanics (light level, biome, mob caps)
- [ ] Basic AI (wander, flee, follow)
- [ ] Breeding and baby entities

### Hostile Mobs
- [ ] Aggro and target tracking
- [ ] Attack patterns
- [ ] Drops on death

### Items on Ground
- [ ] Dropped item entities
- [ ] Pickup mechanics with delay
- [ ] Despawn timer

---

## Phase 5: Combat & Health

Damage, healing, and combat mechanics.

### Health System
- [ ] Player health component + sync
- [ ] Damage sources (fall, attack, fire, drowning, void)
- [ ] Death and respawn handling
- [ ] Regeneration mechanics

### Combat
- [ ] Melee attack handling with cooldown
- [ ] Knockback
- [ ] Critical hits
- [ ] Armor damage reduction

### Status Effects
- [ ] Effect component (type, duration, amplifier)
- [ ] Effect application (potions, beacons, etc.)
- [ ] Effect visuals (particles, icons)

---

## Phase 6: World Mechanics

Gameplay systems tied to world state.

### Time & Weather
- [ ] Day/night cycle with lighting updates
- [ ] Weather (rain, thunder) with sync
- [ ] Sleeping to skip night

### Block Updates
- [ ] Block tick scheduling (crops, liquids)
- [ ] Redstone basics (power propagation, torches, repeaters)
- [ ] Liquid flow (water, lava)
- [ ] Fire spread and burnout

### Physics
- [ ] Gravity for sand/gravel
- [ ] Explosion handling
- [ ] Piston push/pull

---

## Phase 7: Advanced Features

Polish and feature completeness.

### Portals & World Transfer
> Portals are just triggers for cross-world player transfer. The "nether" and "end" are separate `World` instances with their own chunks.

- [ ] Nether portal block detection and linking algorithm
- [ ] End portal activation (eye of ender placement)
- [ ] Player world transfer (unload current chunks → send dimension change → load new chunks)
- [ ] Coordinate scaling for nether (8:1 ratio)
- [ ] Spawn platform generation for end

### Scoreboard & Bossbar
- [ ] Scoreboard objectives and display
- [ ] Team support
- [ ] Bossbar display

### Forms & UI
- [ ] Form packet handling (simple, modal, custom)
- [ ] Server settings UI

### Resource Packs
- [ ] Pack advertisement and download
- [ ] Encryption support
- [ ] Required pack enforcement

---

## Phase 8: Extension Boundary (Pre-WASM)

Internal architecture for future plugins.

### Event System
- [ ] `GameEvent` enum for semantic events (block break, player move, chat, etc.)
- [ ] Event bus with priority ordering
- [ ] Cancellable vs monitor event types

### Action Queue
- [ ] `GameAction` enum for deferred mutations
- [ ] Action validation and application phase
- [ ] Batching for network output

### Hook Points
- [ ] Packet filter trait (pre-dispatch interception)
- [ ] Dynamic command registration
- [ ] Permission provider trait

---

## Phase 9: WASM Plugin Runtime

See `LONGTERM.md` §Plugin system for full design.

- [ ] Embed Wasmtime behind feature flag
- [ ] Plugin manifest parsing (`plugin.toml`)
- [ ] Host API definition (WIT or equivalent)
- [ ] Sandbox: time budgets, memory caps, capability gating
- [ ] Event delivery and action collection

---

## Phase 10: Ecosystem & Tooling

Developer experience and community support.

- [ ] Plugin SDK crate with bindings and templates
- [ ] Example plugins (permissions, anti-cheat, economy hooks)
- [ ] Plugin hot-reload (optional)
- [ ] Rich logging and diagnostics per plugin
- [ ] API versioning and compatibility policy

---

## Notes

- **Phases are not strictly sequential.** Work on Phase 3 (commands) can start before Phase 2 (inventory) is complete.
- **Prioritize based on user demand.** If commands/chat are more urgent, tackle Phase 3 earlier.
- **Each phase should have tests.** Unit tests for logic, integration tests for packet flows.
- **Keep `LONGTERM.md` as the architectural north star.** This roadmap is the tactical checklist.
