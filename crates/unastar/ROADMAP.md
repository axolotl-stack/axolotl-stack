# Unastar Development Roadmap

This document outlines the development path from current state to a fully-featured Bedrock server. Phases are roughly ordered by dependency—later phases often depend on earlier ones.

See `LONGTERM.md` for the architectural vision (WASM plugins, tick phases, API design).

---

## Current State ✓

- [x] Basic networking (RakNet via `jolyne`)
- [x] Player join/leave and authentication (Xbox Live + offline)
- [x] ECS architecture (`bevy_ecs`)
- [x] Chunk generation (flat world) and streaming
- [x] Player movement and position sync
- [x] Basic block breaking/placing with animations
- [x] Entity spawn/despawn broadcasting
- [x] Registry loading (blocks, items, biomes, entities)

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
