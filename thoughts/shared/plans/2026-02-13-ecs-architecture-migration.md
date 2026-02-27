# ECS Architecture Migration: Parallel Game Loop

## Overview

Migrate the unastar server from a monolithic `GameServer` struct with direct world mutations to a fully ECS-driven architecture where all game state lives inside the bevy_ecs World as Resources and all logic runs as proper systems. This unblocks bevy's multi-threaded system executor, enabling automatic parallelism for packet processing and game logic.

## Current State Analysis

### Architecture Today
- `GameServer` (`server/game/mod.rs:52-69`) owns the ECS World **and** duplicates state outside it (registries, config, tick counter, providers)
- `handle_packet` (`server/game/packets.rs:27`) takes `&mut GameServer`, doing direct `world.get_mut::<T>()` mutations — bypasses ECS scheduling entirely
- Packet routing scaffolding exists (`packet_routing.rs`, `packet_router.rs`, `packet_domains.rs`) but is dead code — packets are cloned into `PacketQueues` that nothing drains (memory leak)
- Plugin actions use `String` player UUIDs causing O(N) scans per action (`server/game/plugins.rs:30-31`)
- All game logic runs on one thread despite bevy_ecs supporting automatic parallel scheduling

### Key Discoveries
- `PacketQueues.clear_all()` is `#[allow(dead_code)]` — confirmed no consumer exists (`packet_routing.rs:55`)
- `GameServer` duplicates `items`, `blocks` as both fields AND as `ItemRegistryResource`/`BlockRegistryResource` Resources (`mod.rs:64-67` vs `mod.rs:148-151`)
- `current_tick` on GameServer duplicates `TickCounter` resource (`mod.rs:58` vs `resources.rs:7`)
- `PluginManager` is on `UnastarServer` (`runtime.rs:36`), not in ECS — called as method on `&mut self`
- Join packets read `self.config` and `self.world_template` — both already have Resource wrappers (`ServerWorldTemplate`, but `ServerConfig` missing)
- `blocks.rs` methods like `break_block`, `place_block`, `get_block_break_time` all take `&mut self` / `&self` and access `self.items`, `self.ecs.world()` interleaved — these need refactoring to be systems
- The domain types in `packet_domains.rs` are well-designed and can be reused

## Desired End State

After this plan is complete:
1. `GameServer` is a thin wrapper: `{ ecs: UnastarEcs }` — all state is ECS Resources
2. All packet handling runs as domain-specific ECS systems that drain typed queues
3. Systems with disjoint component access run in parallel automatically (movement || inventory || chat)
4. Plugin actions use `Entity` handles — O(1) lookups instead of O(N) string scans
5. The dead `PacketQueues` routing code is removed
6. `cargo clippy` passes with no warnings

### How to Verify
- Server compiles and runs: `cargo build` + manual connect test
- `cargo clippy` clean
- TPS stays at 20 with existing player load
- All existing functionality works (movement, block break/place, inventory, chat, commands, chunk loading, plugins)

## What We're NOT Doing

- **Not adding intent/replay/deterministic sorting** — server is authoritative, not lockstep
- **Not adding parallel ingest systems** — packets are already decoded by per-player tokio tasks
- **Not moving PluginManager into ECS** — WASM stores aren't Send, would require NonSend and adds complexity for no gain right now
- **Not switching to bevy Events<T> for EventBuffer** — works fine as-is, can migrate later
- **Not refactoring ChunkManager into smaller pieces** — chunk systems already work and are complex; save for a separate PR
- **Not changing the networking layer** — channels and per-player tasks stay as-is
- **Not adding bounded channel limits** — not a practical issue at current scale

## Implementation Approach

Five phases, each independently shippable. Every phase ends with a compiling, working server.

---

## Phase 0: Remove Dead Packet Routing Code (Memory Leak Fix)

### Overview
Stop the active memory leak where every packet is cloned into `PacketQueues` that nothing drains. Remove the dead routing infrastructure. Keep the domain type definitions — they'll be reused in Phase 2.

### Changes Required

#### 1. Remove routing call from `handle_packet`
**File**: `crates/unastar/src/server/game/packets.rs`
**Changes**: Delete lines 61-73 (the `route_packet` call and surrounding block)

Remove this block:
```rust
// Route packet to queues for future parallel processing
if let Some(mut queues) = self
    .ecs
    .world_mut()
    .get_resource_mut::<super::packet_routing::PacketQueues>()
{
    super::packet_router::PacketRouter::route_packet(
        session_id,
        entity,
        packet.clone(),
        &mut queues,
    );
}
```

Also remove the `// TEMPORARY:` comment on the line after.

#### 2. Remove PacketQueues resource insertion
**File**: `crates/unastar/src/server/game/mod.rs`
**Changes**: Delete lines 125-127

Remove:
```rust
// Initialize packet routing queues
ecs.world_mut()
    .insert_resource(packet_routing::PacketQueues::default());
```

#### 3. Delete dead routing files
**Files to delete**:
- `crates/unastar/src/server/game/packet_routing.rs` (the `PacketQueues` struct)
- `crates/unastar/src/server/game/packet_router.rs` (the `PacketRouter` and routing logic)

**Keep**: `crates/unastar/src/server/game/packet_domains.rs` — the domain enums are well-designed and will be reused in Phase 2.

#### 4. Remove module declarations
**File**: `crates/unastar/src/server/game/mod.rs`
**Changes**: Remove the `mod packet_router;` and `mod packet_routing;` declarations (keep `mod packet_domains;`).

### Success Criteria

#### Automated Verification:
- [ ] `cargo build` succeeds
- [ ] `cargo clippy` has no new warnings
- [ ] No references to `PacketQueues` or `PacketRouter` remain in codebase (except possibly in domain types)

#### Manual Verification:
- [ ] Server starts and accepts connections
- [ ] Player can move, break blocks, chat — all existing packet handlers still work

---

## Phase 1: Move GameServer State Into ECS Resources

### Overview
Move all game state from `GameServer` fields into ECS Resources. After this phase, `GameServer` becomes `{ ecs: UnastarEcs }` and all methods access state through the World.

### Changes Required

#### 1. Create new Resource wrapper types
**File**: `crates/unastar/src/server/game/types.rs`
**Changes**: Add new Resource types

```rust
/// Server configuration as an ECS Resource.
#[derive(Resource, Clone)]
pub struct ServerConfigResource(pub ServerConfig);

/// Command registry as an ECS Resource.
#[derive(Resource)]
pub struct CommandRegistryResource(pub CommandRegistry);

/// Biome registry as an ECS Resource.
#[derive(Resource)]
pub struct BiomeRegistryResource(pub Arc<BiomeRegistry>);

/// Entity registry as an ECS Resource.
#[derive(Resource)]
pub struct EntityRegistryResource(pub Arc<EntityRegistry>);

/// Counter for spawning item entities.
#[derive(Resource, Default)]
pub struct ItemEntityIdCounter(pub i64);

/// Queue of network events to process this tick.
/// Used to defer spawn/despawn from the runtime drain loop into ECS systems.
#[derive(Resource, Default)]
pub struct NetworkEventQueue {
    pub joins: Vec<PlayerSpawnData>,
    pub disconnects: Vec<SessionId>,
}

/// Player data store for persistence.
#[derive(Resource, Clone)]
pub struct PlayerDataStoreResource {
    pub store: Option<Arc<PlayerDataStore>>,
    pub save_previous_position: bool,
}

/// World provider for chunk persistence.
#[derive(Resource)]
pub struct WorldProviderResource(pub Option<Arc<dyn crate::storage::WorldProvider>>);

/// Player provider for player data persistence.
#[derive(Resource)]
pub struct PlayerProviderResource {
    pub provider: Option<Arc<dyn crate::storage::PlayerProvider>>,
    pub save_on_disconnect: bool,
}
```

Note: `WorldProviderResource` and `PlayerProviderResource` contain `Arc<dyn Trait>`. Since the traits likely require `Send + Sync`, these should work as normal Resources. If they don't implement Send, use `bevy_ecs::world::World::insert_non_send_resource()` instead.

#### 2. Simplify GameServer struct
**File**: `crates/unastar/src/server/game/mod.rs`
**Changes**: Strip GameServer down to just the ECS wrapper

```rust
pub struct GameServer {
    pub ecs: UnastarEcs,
}
```

Update `GameServer::with_config()` to insert all state as resources during world initialization. The existing `insert_resource` calls for `SessionEntityMap`, `EntityGrid`, `ChunkManager`, etc. stay — just add the new ones:

```rust
ecs.world_mut().insert_resource(ServerConfigResource(config.clone()));
ecs.world_mut().insert_resource(CommandRegistryResource(CommandRegistry::with_defaults()));
ecs.world_mut().insert_resource(BiomeRegistryResource(Arc::new(biomes)));
ecs.world_mut().insert_resource(EntityRegistryResource(Arc::new(entities)));
ecs.world_mut().insert_resource(ItemEntityIdCounter(100000));
ecs.world_mut().init_resource::<NetworkEventQueue>();
ecs.world_mut().insert_resource(PlayerDataStoreResource { store: None, save_previous_position: false });
ecs.world_mut().insert_resource(PlayerProviderResource { provider: None, save_on_disconnect: false });
ecs.world_mut().insert_resource(WorldProviderResource(None));
```

Remove `ItemRegistryResource` Arc wrapping — store the registry directly or keep Arc if needed by multiple systems. Same for `BlockRegistryResource`.

#### 3. Update runtime.rs to use Resources
**File**: `crates/unastar/src/server/runtime.rs`
**Changes**:

The `UnastarServer::new()` method currently calls `server.set_player_data_store(...)`, `server.set_player_provider(...)`, `server.set_world_provider(...)`. Change these to insert into ECS Resources directly:

```rust
// Instead of:
server.set_player_data_store(player_data_store.clone(), save_previous_position);
// Do:
server.ecs.world_mut().insert_resource(PlayerDataStoreResource {
    store: Some(player_data_store.clone()),
    save_previous_position,
});
```

Similarly for player provider and world provider.

The accept loop at `runtime.rs:173` currently uses `self.server.world_template.clone()`. Change to read from the ECS Resource:
```rust
let world_template = self.server.ecs.world()
    .get_resource::<ServerWorldTemplate>()
    .unwrap().0.clone();
```

#### 4. Update `spawn_player` / `despawn_player`
**File**: `crates/unastar/src/server/game/mod.rs`
**Changes**: These methods currently access `self.config` for `default_gamemode`. Change to read from ECS:

```rust
pub fn spawn_player(&mut self, data: PlayerSpawnData) -> Entity {
    let config = self.ecs.world().get_resource::<ServerConfigResource>().unwrap().0.clone();
    // ... rest uses config.default_gamemode instead of self.config.default_gamemode
}
```

#### 5. Update join.rs
**File**: `crates/unastar/src/server/game/join.rs`
**Changes**: `send_join_packets` reads `self.config.default_chunk_radius`, `self.world_template`, and `self.current_tick`. Change to read from Resources:

```rust
pub(super) fn send_join_packets(&self, entity: bevy_ecs::entity::Entity) {
    let world = self.ecs.world();
    let config = world.get_resource::<ServerConfigResource>().unwrap();
    let world_template = world.get_resource::<ServerWorldTemplate>().unwrap();
    let tick = world.get_resource::<TickCounter>().unwrap().current;
    // ... replace self.config with config.0, self.world_template with world_template.0, self.current_tick with tick
}
```

#### 6. Update commands.rs
**File**: `crates/unastar/src/server/game/commands.rs`
**Changes**: `handle_command_request` uses `self.commands.find(...)`. Change to read from Resource:

```rust
let commands = self.ecs.world().get_resource::<CommandRegistryResource>().unwrap();
let Some(_command) = commands.0.find(&invocation.name) else { ... };
```

`handle_teleport_command` uses `self.current_tick`. Change to read from `TickCounter`.

#### 7. Update blocks.rs
**File**: `crates/unastar/src/server/game/blocks.rs`
**Changes**:

`break_block` uses `self.items` for item drops and `self.next_item_entity_id`:
```rust
// Instead of: self.items.get_by_name(name)
let items = self.ecs.world().get_resource::<ItemRegistryResource>().unwrap();
items.0.get_by_name(name)

// Instead of: self.next_item_entity_id; self.next_item_entity_id += 1;
let mut counter = self.ecs.world_mut().get_resource_mut::<ItemEntityIdCounter>().unwrap();
let item_entity_id = counter.0;
counter.0 += 1;
```

`handle_block_click` uses `self.items` and `self.blocks`:
```rust
let items = self.ecs.world().get_resource::<ItemRegistryResource>().unwrap();
let blocks = self.ecs.world().get_resource::<BlockRegistryResource>().unwrap();
```

`get_block_break_time` uses `self.ecs.world()` — already correct, just reads ChunkManager.

#### 8. Update chunks.rs
**File**: `crates/unastar/src/server/game/chunks.rs`
**Changes**: `handle_chunk_radius_request` uses `self.config.max_chunk_radius`. Read from Resource instead.

#### 9. Update packets.rs (item_stack_request)
**File**: `crates/unastar/src/server/game/packets.rs`
**Changes**: `handle_item_stack_request` uses `self.world_template.creative_content`. Read from `ServerWorldTemplate` Resource instead.

#### 10. Remove `set_*` methods and duplicate fields
**File**: `crates/unastar/src/server/game/mod.rs`
**Changes**: Delete `set_player_data_store`, `set_player_provider`, `set_world_provider`, and the corresponding fields. Delete duplicate `items`, `blocks`, `biomes`, `entities`, `commands`, `config`, `world_config`, `world_template`, `current_tick`, `next_item_entity_id` fields.

### Success Criteria

#### Automated Verification:
- [ ] `cargo build` succeeds
- [ ] `cargo clippy` has no new warnings
- [ ] `GameServer` struct has only `ecs: UnastarEcs` field

#### Manual Verification:
- [ ] Server starts and accepts connections
- [ ] Players can join, move, break/place blocks, use inventory, chat, use commands, teleport
- [ ] Chunk loading/streaming works
- [ ] Plugins load and respond to events

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation before proceeding to Phase 2.

---

## Phase 2: Split Packet Handling Into Domain Systems

### Overview
Replace the monolithic `handle_packet` method with typed per-domain Resource queues and ECS systems. The runtime drain loop becomes a simple packet router. Domain systems drain their queues and process packets. Systems with disjoint component access automatically run in parallel.

### Changes Required

#### 1. Add PacketApply system set
**File**: `crates/unastar/src/ecs/schedules.rs`
**Changes**: Add new set before PhysicsSet

```rust
/// System set for applying packet-driven state changes.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PacketApplySet;
```

#### 2. Update schedule configuration
**File**: `crates/unastar/src/ecs/app.rs`
**Changes**: Add PacketApplySet to the chain

```rust
tick_schedule.configure_sets(
    (
        PacketApplySet,
        PhysicsSet,
        EntityLogicSet,
        ChunkSet,
        NetworkSendSet,
        CleanupSet,
    )
        .chain(),
);
```

#### 3. Create per-domain packet queue Resources
**File**: `crates/unastar/src/server/game/packet_queues.rs` (new file)
**Changes**: Define strongly-typed queue resources. Using separate Resources (not one struct) so bevy sees them as independent for scheduling.

```rust
use bevy_ecs::prelude::*;
use crate::network::SessionId;

/// Movement packets waiting to be processed this tick.
#[derive(Resource, Default)]
pub struct MovementPacketQueue(pub Vec<(Entity, jolyne::valentine::PlayerAuthInputPacket)>);

/// Block action packets (breaking, placing) waiting to be processed.
#[derive(Resource, Default)]
pub struct BlockPacketQueue(pub Vec<(Entity, BlockAction)>);

/// Represents a block-related action extracted from packets.
pub enum BlockAction {
    /// Block actions from PlayerAuthInput (break start/stop/crack/abort/predict)
    AuthInputActions {
        actions: Vec<jolyne::valentine::PlayerAuthInputPacketBlockActionItem>,
    },
    /// Block click from InventoryTransaction (block placement)
    BlockClick {
        data: jolyne::valentine::types::TransactionUseItem,
    },
    /// Player action related to blocks
    PlayerAction(jolyne::valentine::PlayerActionPacket),
}

/// Inventory packets waiting to be processed.
#[derive(Resource, Default)]
pub struct InventoryPacketQueue(pub Vec<(Entity, InventoryAction)>);

pub enum InventoryAction {
    ItemStackRequest(jolyne::valentine::ItemStackRequestPacket),
    ContainerClose(jolyne::valentine::ContainerClosePacket),
    MobEquipment(jolyne::valentine::MobEquipmentPacket),
    Transaction(jolyne::valentine::InventoryTransactionPacket),
    Interact(jolyne::valentine::InteractPacket),
}

/// Chat and command packets.
#[derive(Resource, Default)]
pub struct ChatPacketQueue(pub Vec<(Entity, SessionId, ChatAction)>);

pub enum ChatAction {
    Text(jolyne::valentine::TextPacket),
    Command(jolyne::valentine::CommandRequestPacket),
}

/// Chunk request packets.
#[derive(Resource, Default)]
pub struct ChunkPacketQueue(pub Vec<(Entity, ChunkAction)>);

pub enum ChunkAction {
    SubchunkRequest(jolyne::valentine::SubchunkRequestPacket),
    RadiusRequest(jolyne::valentine::RequestChunkRadiusPacket),
}
```

#### 4. Replace `handle_packet` with packet router
**File**: `crates/unastar/src/server/game/packets.rs`
**Changes**: Replace the entire `handle_packet` method with a simple router that pushes into typed queues. No `packet.clone()` — move the packet data.

```rust
impl GameServer {
    pub fn route_packet(&mut self, session_id: SessionId, entity: Entity, packet: McpePacket) {
        let world = self.ecs.world_mut();

        match packet.data {
            McpePacketData::PacketDisconnect(_) => {
                // Immediate handling (unchanged)
                if let Some(mut event_buffer) = world.get_resource_mut::<EventBuffer>() {
                    event_buffer.push(ServerEvent::PlayerQuit { entity });
                }
                // Queue despawn
                if let Some(mut queue) = world.get_resource_mut::<NetworkEventQueue>() {
                    queue.disconnects.push(session_id);
                }
            }
            McpePacketData::PacketPlayerAuthInput(pk) => {
                // Split: movement data goes to movement queue
                if let Some(mut q) = world.get_resource_mut::<MovementPacketQueue>() {
                    q.0.push((entity, *pk));
                }
                // Note: block actions from AuthInput are extracted inside the movement system
                // and forwarded to block queue, OR we extract here during routing
            }
            McpePacketData::PacketItemStackRequest(pk) => {
                if let Some(mut q) = world.get_resource_mut::<InventoryPacketQueue>() {
                    q.0.push((entity, InventoryAction::ItemStackRequest(pk)));
                }
            }
            // ... etc for each packet type, matching the existing handle_packet cases
        }
    }
}
```

Update `runtime.rs` drain loop to call `route_packet` instead of `handle_packet`:
```rust
NetworkEvent::Packet { session_id, packet } => {
    let entity = {
        let map = self.server.ecs.world().get_resource::<SessionEntityMap>().unwrap();
        map.get(session_id)
    };
    if let Some(entity) = entity {
        self.server.route_packet(session_id, entity, packet);
    }
}
```

#### 5. Create domain systems
Convert each `handle_*` method to an ECS system. Key systems:

**`apply_movement`** — extracts from `handle_player_auth_input`:
```rust
pub fn apply_movement(
    mut queue: ResMut<MovementPacketQueue>,
    mut players: Query<(
        &mut Position, &mut Rotation, &mut PlayerState,
        &mut PlayerInput, &mut BreakingState, &GameMode,
    )>,
    mut event_buffer: ResMut<EventBuffer>,
    // block actions get forwarded to BlockPacketQueue
    mut block_queue: ResMut<BlockPacketQueue>,
) {
    for (entity, pk) in queue.0.drain(..) {
        if let Ok((mut pos, mut rot, mut state, mut input, mut breaking, game_mode)) = players.get_mut(entity) {
            // ... movement logic from handle_player_auth_input
            // Extract block actions and push to block_queue
            if let Some(block_actions) = pk.block_action {
                block_queue.0.push((entity, BlockAction::AuthInputActions { actions: block_actions }));
            }
        }
    }
}
```

**`apply_inventory`** — extracts from `handle_item_stack_request`, `handle_container_close`, `handle_mob_equipment`, `handle_interact`, `handle_inventory_transaction`:
```rust
pub fn apply_inventory(
    mut queue: ResMut<InventoryPacketQueue>,
    mut players: Query<(
        &mut MainInventory, &mut CursorItem, &mut HeldSlot,
        &mut InventoryOpened, &mut ItemStackRequestState,
        &PlayerSession, &Position,
    )>,
    items: Res<ItemRegistryResource>,
    blocks: Res<BlockRegistryResource>,
    world_template: Res<ServerWorldTemplate>,
    mut event_buffer: ResMut<EventBuffer>,
) { ... }
```

**`apply_chat`** — extracts from `handle_text` and `handle_command_request`:
```rust
pub fn apply_chat(
    mut queue: ResMut<ChatPacketQueue>,
    players: Query<(&PlayerName, &PlayerUuid, &PlayerSession)>,
    session_map: Res<SessionEntityMap>,
    commands: Res<CommandRegistryResource>,
    mut event_buffer: ResMut<EventBuffer>,
) { ... }
```

**`apply_block_actions`** — extracts from `handle_block_actions`, `break_block`, `place_block`, `handle_block_click`:
```rust
pub fn apply_block_actions(
    mut queue: ResMut<BlockPacketQueue>,
    mut players: Query<(&mut BreakingState, &GameMode, &PlayerSession, &PlayerUuid)>,
    chunk_manager: Res<ChunkManager>,
    items: Res<ItemRegistryResource>,
    blocks: Res<BlockRegistryResource>,
    mut counter: ResMut<ItemEntityIdCounter>,
    tick: Res<TickCounter>,
    mut event_buffer: ResMut<EventBuffer>,
    // Need world access for chunk data mutations and triggers
    // This system may need to use Commands or direct world access
) { ... }
```

Note: `apply_block_actions` is the trickiest system because `break_block` and `place_block` do:
- `world.get_mut::<ChunkData>(chunk_entity)` — mutates chunk data
- `world.trigger(BlockChanged { ... })` — fires observer
- `world.write_message(BlockBroadcastEvent { ... })` — writes message
- Reads `ChunkViewers` for broadcasting

This may need to be an exclusive system (`fn(world: &mut World)`) or use `Commands` with deferred application. Start with exclusive system, optimize later.

**`apply_chunk_requests`** — extracts from `handle_subchunk_request`, `handle_chunk_radius_request`:
```rust
pub fn apply_chunk_requests(
    mut queue: ResMut<ChunkPacketQueue>,
    mut players: Query<(&mut ChunkRadius, &PlayerSession)>,
    chunk_manager: Res<ChunkManager>,
    config: Res<ServerConfigResource>,
    // Also needs chunk data access for subchunk responses
) { ... }
```

#### 6. Register systems
**File**: `crates/unastar/src/server/game/mod.rs`
**Changes**: Register domain systems in `PacketApplySet`

```rust
ecs.schedule_mut().add_systems(
    (
        apply_movement,
        apply_inventory,
        apply_chat,
    ).in_set(PacketApplySet)
);

// Block actions depend on movement (AuthInput extracts block actions into the queue)
ecs.schedule_mut().add_systems(
    apply_block_actions
        .after(apply_movement)
        .in_set(PacketApplySet)
);

// Chunk requests are independent
ecs.schedule_mut().add_systems(
    apply_chunk_requests.in_set(PacketApplySet)
);
```

#### 7. Delete old packet handler methods
After the systems are working, delete the `impl GameServer` methods in `packets.rs`, `blocks.rs`, `chunks.rs`, `commands.rs` that are now replaced by systems.

#### 8. Remove `packet_domains.rs` if not reused
If the new queue types in `packet_queues.rs` fully replace the domain enums, delete `packet_domains.rs`. Otherwise keep it.

### Parallelism Achieved

With these systems registered without explicit ordering (except block after movement):

| System | Components Written | Can Run With |
|--------|-------------------|--------------|
| `apply_movement` | Position, Rotation, PlayerState, PlayerInput, BreakingState | `apply_inventory`, `apply_chat` |
| `apply_inventory` | MainInventory, CursorItem, HeldSlot, InventoryOpened, ItemStackRequestState | `apply_movement`, `apply_chat` |
| `apply_chat` | (reads only: PlayerName, PlayerSession) | Everything |
| `apply_block_actions` | ChunkData, BreakingState (exclusive system) | Nothing (runs alone) |
| `apply_chunk_requests` | ChunkRadius, ChunkViewers | `apply_movement`, `apply_inventory`, `apply_chat` |

Movement, inventory, and chat run in parallel. Block actions run after movement completes. This is the primary throughput win.

### Success Criteria

#### Automated Verification:
- [ ] `cargo build` succeeds
- [ ] `cargo clippy` has no new warnings
- [ ] No `handle_packet` method exists on `GameServer`
- [ ] All packet types are routed through typed queues

#### Manual Verification:
- [ ] Movement works smoothly (walk, sprint, sneak, swim, fly, glide)
- [ ] Block breaking works (creative instant + survival timed)
- [ ] Block placing works with correct face calculations
- [ ] Inventory operations work (creative picks, hotbar changes, container open/close)
- [ ] Chat messages broadcast to all players
- [ ] Commands work (/tp)
- [ ] Chunk loading/subchunk requests work
- [ ] Plugin events fire correctly (movement, chat, block break/place)
- [ ] Item drops spawn on block break in survival

**Implementation Note**: This is the largest phase. Consider splitting into sub-PRs: movement system first, then inventory, then blocks, then chat/commands, then chunks. Each sub-PR should be independently testable.

---

## Phase 3: Fix Plugin O(N) Lookups

### Overview
Change `PluginAction` to carry `Entity` instead of `String` player_id. Resolve the handle-to-Entity mapping at the WASM host boundary (where the entity is already known), not in the apply system.

### Changes Required

#### 1. Update PluginAction enum
**File**: `crates/unastar/src/ecs/events.rs`
**Changes**: Replace `player_id: String` with `entity: Entity`

```rust
#[derive(Debug, Clone)]
pub enum PluginAction {
    SendMessage {
        entity: Entity,
        message: String,
    },
    Teleport {
        entity: Entity,
        position: (f64, f64, f64),
    },
    GiveItem {
        entity: Entity,
        item_id: String,
        count: u8,
    },
    Kick {
        entity: Entity,
        reason: String,
    },
    SetBlock {
        position: (i32, i32, i32),
        block_id: u32,
    },
}
```

#### 2. Update host trait implementations to use Entity
**File**: `crates/unastar/src/plugin/manager.rs`
**Changes**: In `HostPlayer` impl, push Entity directly instead of UUID string:

```rust
fn send_message(&mut self, self_: Resource<wit_types::Player>, message: String) {
    let entity = self.player_entity(&self_);
    self.pending_actions.push(PluginAction::SendMessage {
        entity,
        message,
    });
}

fn teleport(&mut self, self_: Resource<wit_types::Player>, position: wit_types::Vec3) {
    let entity = self.player_entity(&self_);
    self.pending_actions.push(PluginAction::Teleport {
        entity,
        position: (position.x, position.y, position.z),
    });
}

fn give_item(&mut self, self_: Resource<wit_types::Player>, item_id: String, count: u8) {
    let entity = self.player_entity(&self_);
    self.pending_actions.push(PluginAction::GiveItem {
        entity,
        item_id,
        count,
    });
}

fn kick(&mut self, self_: Resource<wit_types::Player>, reason: String) {
    let entity = self.player_entity(&self_);
    self.pending_actions.push(PluginAction::Kick {
        entity,
        reason,
    });
}
```

This removes the `player_uuid()` call entirely from these methods.

#### 3. Update process_plugin_actions system
**File**: `crates/unastar/src/server/game/plugins.rs`
**Changes**: Replace O(N) iteration with direct entity lookup

```rust
pub fn process_plugin_actions(
    mut action_queue: ResMut<ActionQueue>,
    item_registry: Res<super::types::ItemRegistryResource>,
    block_registry: Res<super::types::BlockRegistryResource>,
    mut players: Query<(
        &mut Position,
        &mut Rotation,
        &RuntimeEntityId,
        &PlayerSession,
        &mut MainInventory,
    )>,
) {
    for action in action_queue.drain() {
        match action {
            PluginAction::SendMessage { entity, message } => {
                if let Ok((_, _, _, session, _)) = players.get(entity) {
                    let packet = system_text(&message);
                    let _ = session.send(McpePacket::from(packet));
                }
            }
            PluginAction::Teleport { entity, position: pos } => {
                if let Ok((mut player_pos, rot, rid, session, _)) = players.get_mut(entity) {
                    let new_pos = DVec3::new(pos.0, pos.1, pos.2);
                    player_pos.0 = new_pos;
                    // ... send teleport packet (same as before)
                }
            }
            PluginAction::GiveItem { entity, item_id, count } => {
                if let Ok((_, _, _, session, mut inv)) = players.get_mut(entity) {
                    // ... give item logic (same as before, minus the UUID scan)
                }
            }
            PluginAction::Kick { entity, reason } => {
                if let Ok((_, _, _, _session, _)) = players.get(entity) {
                    warn!(entity=?entity, reason=%reason, "Plugin kick requested (not impl)");
                }
            }
            PluginAction::SetBlock { position, block_id } => {
                info!(pos=?position, block_id, "Plugin set block (not yet impl)");
            }
        }
    }
}
```

Every action is now O(1) via `players.get(entity)` instead of O(N) via `players.iter()` + string comparison.

#### 4. Remove PlayerUuid from the query
The `process_plugin_actions` system no longer needs `&PlayerUuid` in its query since it doesn't do UUID-based lookups.

### Success Criteria

#### Automated Verification:
- [ ] `cargo build` succeeds
- [ ] `cargo clippy` has no new warnings
- [ ] No `player_id: String` fields in `PluginAction` enum (except `SetBlock` which uses position, not player)
- [ ] No `.iter()` loop in `process_plugin_actions`

#### Manual Verification:
- [ ] Plugin `send_message` works (test with example plugin)
- [ ] Plugin `teleport` works
- [ ] Plugin `give_item` works
- [ ] Plugin events still fire correctly

---

## Phase 4: Validate Multi-Threaded Execution

### Overview
Bevy's default schedule executor is already multi-threaded. After Phases 1-2, systems with disjoint access will automatically parallelize. This phase validates that it's actually happening and measures the impact.

### Changes Required

#### 1. Add tracing spans to domain systems
**File**: Each domain system file
**Changes**: Add `tracing::info_span!` to each system to see overlap in traces

```rust
pub fn apply_movement(...) {
    let _span = tracing::info_span!("apply_movement").entered();
    // ... system body
}
```

#### 2. Run with tracing subscriber and verify
Use `RUST_LOG=info` and check that `apply_movement` and `apply_inventory` spans overlap temporally. Or use `tracing-timing` / `tracy` for visual confirmation.

#### 3. Benchmark with multiple players
Connect 10+ bot clients (or use a load testing tool) and compare:
- TPS under load before migration (single-threaded packet handling)
- TPS under load after migration (parallel domain systems)

### Success Criteria

#### Automated Verification:
- [ ] `cargo build` succeeds (no new code, just tracing additions)

#### Manual Verification:
- [ ] Tracing output confirms parallel system execution
- [ ] TPS is stable at 20 with connected players
- [ ] No deadlocks, panics, or data races under load

---

## Testing Strategy

### Per-Phase Testing
Each phase has its own success criteria above. The key manual test after every phase:
1. Start server
2. Connect with Bedrock client
3. Walk around (movement + chunk loading)
4. Break and place blocks
5. Open inventory, move items
6. Send chat message
7. Use /tp command
8. Verify no console errors/warnings beyond expected

### Regression Concerns
- **Phase 0**: Minimal risk — just removing dead code
- **Phase 1**: Medium risk — resource access patterns change, possible borrow issues at compile time
- **Phase 2**: Highest risk — logic moves between paradigms. Each domain system should be tested individually before removing the old handler
- **Phase 3**: Low risk — data flow change, same logic
- **Phase 4**: No risk — observation only

## Performance Considerations

- **Phase 0**: Removes per-packet `clone()` — immediate memory and CPU win
- **Phase 1**: No performance change — same logic, different access pattern
- **Phase 2**: Primary performance win — parallel packet processing. With 100 players, movement (writing Position/Rotation) and inventory (writing MainInventory/CursorItem) can execute on different cores simultaneously
- **Phase 3**: Removes O(N) string comparison per plugin action — constant-time entity lookup
- **Phase 4**: Validation only

## Migration Notes

- Each phase is independently shippable and produces a working server
- Phase 0 should be done immediately (active memory leak)
- Phase 1 is prerequisite for Phase 2
- Phase 3 can be done at any time (independent of Phases 1-2)
- Phase 2 is the largest and can be split into sub-PRs per domain
- No database migrations needed
- No protocol changes needed
- Plugin API unchanged (WIT interface stays the same)

## References

- Existing system sets: `crates/unastar/src/ecs/schedules.rs`
- Current GameServer: `crates/unastar/src/server/game/mod.rs:52-69`
- Packet handler monolith: `crates/unastar/src/server/game/packets.rs`
- Plugin actions: `crates/unastar/src/ecs/events.rs:6-28`
- Plugin host impl: `crates/unastar/src/plugin/manager.rs:109-211`
- Process plugin actions: `crates/unastar/src/server/game/plugins.rs:13-162`
- Runtime tick loop: `crates/unastar/src/server/runtime.rs:200-318`
- Domain type definitions: `crates/unastar/src/server/game/packet_domains.rs`
