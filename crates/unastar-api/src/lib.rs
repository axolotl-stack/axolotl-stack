use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

pub use unastar_api_macros::{event_handler, native_plugin, plugin};

// Native Rust plugin system (enabled with "native" feature)
#[cfg(feature = "native")]
pub mod native;

// ...

mod player_wrapper;
pub use player_wrapper::Player;

// ============================================================================
// Handle Types
// ============================================================================

/// Opaque handle to a player entity. Valid only during the current on_tick call.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
)]
pub struct PlayerHandle(pub u32);

/// Opaque handle to a world/level. Valid only during the current on_tick call.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
)]
pub struct WorldHandle(pub u32);

/// Player identifier wrapper (UUID string)
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct PlayerId(pub String);

/// Block ID wrapper
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
)]
pub struct BlockId(pub u32);

// ============================================================================
// Query Result Types
// ============================================================================

/// Player information snapshot
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PlayerInfo {
    pub uuid: String,
    pub name: String,
    pub position: Vec3,
    pub health: f32,
    pub max_health: f32,
}

/// 3D vector (position, velocity, etc.)
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "native", derive(abi_stable::StableAbi))]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
}

// ============================================================================
// Async Task System
// ============================================================================

/// Request for async task execution on host
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum TaskRequest {
    /// Sleep for N milliseconds (for testing)
    Sleep { duration_ms: u64 },
    /// Custom task with serialized data
    Custom { task_type: String, data: Vec<u8> },
}

/// Result from async task execution
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum TaskResult {
    Success(Vec<u8>),
    Error(String),
}

// ============================================================================
// Event Types
// ============================================================================

/// Event kind for subscription filtering
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Tick,
    PlayerJoin,
    PlayerChat,
    BlockBreak,
    BlockPlace,
    Timer,
    PlayerMove,
    PlayerJump,
    PlayerToggleSneak,
    PlayerToggleSprint,
    PlayerQuit,
    PlayerHeldSlotChange,
    PlayerStartBreak,
    PlayerInteractBlock,
    PlayerItemUse,
    PlayerSwing,
    TaskComplete,
}

/// Events sent from Host to Plugin.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum PluginEvent {
    /// Server tick pulse.
    Tick {
        tick_id: u64,
    },
    /// A player joined the server.
    PlayerJoin {
        player: PlayerHandle,
        username: String,
    },
    /// A player sent a chat message.
    PlayerChat {
        player: PlayerHandle,
        message: String,
    },
    /// A block was broken.
    BlockBreak {
        player: PlayerHandle,
        position: (i32, i32, i32),
        block_id: BlockId,
    },
    /// A block was placed.
    BlockPlace {
        player: PlayerHandle,
        position: (i32, i32, i32),
        block_id: BlockId,
    },
    /// A timer fired.
    Timer {
        id: u64,
    },
    /// A player moved.
    PlayerMove {
        player: PlayerHandle,
        from: Vec3,
        to: Vec3,
    },
    /// A player jumped.
    PlayerJump {
        player: PlayerHandle,
    },
    PlayerToggleSneak {
        player: PlayerHandle,
        is_sneaking: bool,
    },
    PlayerToggleSprint {
        player: PlayerHandle,
        is_sprinting: bool,
    },
    PlayerQuit {
        player: PlayerHandle,
    },
    PlayerHeldSlotChange {
        player: PlayerHandle,
        old_slot: u8,
        new_slot: u8,
    },
    PlayerStartBreak {
        player: PlayerHandle,
        position: (i32, i32, i32),
        face: u8,
    },
    PlayerInteractBlock {
        player: PlayerHandle,
        position: (i32, i32, i32),
        face: u8,
    },
    PlayerItemUse {
        player: PlayerHandle,
    },
    /// A player swung their arm.
    PlayerSwing {
        player: PlayerHandle,
    },
    /// Async task completed
    TaskComplete {
        task_id: u32,
        result: TaskResult,
    },
}

impl PluginEvent {
    /// Get the kind of this event for subscription filtering
    pub fn kind(&self) -> EventKind {
        match self {
            PluginEvent::Tick { .. } => EventKind::Tick,
            PluginEvent::PlayerJoin { .. } => EventKind::PlayerJoin,
            PluginEvent::PlayerChat { .. } => EventKind::PlayerChat,
            PluginEvent::BlockBreak { .. } => EventKind::BlockBreak,
            PluginEvent::BlockPlace { .. } => EventKind::BlockPlace,
            PluginEvent::Timer { .. } => EventKind::Timer,
            PluginEvent::PlayerMove { .. } => EventKind::PlayerMove,
            PluginEvent::PlayerJump { .. } => EventKind::PlayerJump,
            PluginEvent::PlayerToggleSneak { .. } => EventKind::PlayerToggleSneak,
            PluginEvent::PlayerToggleSprint { .. } => EventKind::PlayerToggleSprint,
            PluginEvent::PlayerQuit { .. } => EventKind::PlayerQuit,
            PluginEvent::PlayerHeldSlotChange { .. } => EventKind::PlayerHeldSlotChange,
            PluginEvent::PlayerStartBreak { .. } => EventKind::PlayerStartBreak,
            PluginEvent::PlayerInteractBlock { .. } => EventKind::PlayerInteractBlock,
            PluginEvent::PlayerItemUse { .. } => EventKind::PlayerItemUse,
            PluginEvent::PlayerSwing { .. } => EventKind::PlayerSwing,
            PluginEvent::TaskComplete { .. } => EventKind::TaskComplete,
        }
    }
}

// ============================================================================
// Action Types
// ============================================================================

/// Log level for plugin messages
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// Actions returned from Plugin to Host.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum PluginAction {
    /// Log a message to the server console.
    Log { level: LogLevel, message: String },
    /// Kick a player.
    Kick { player_id: String, reason: String },
    /// Send a message to a player.
    SendMessage { player_id: String, message: String },
    /// Teleport a player to a position.
    Teleport { player_id: String, position: Vec3 },
    /// Give an item to a player.
    GiveItem {
        player_id: String,
        item_id: String,
        count: u8,
    },
    /// Cancel the event with the given ID.
    Cancel { event_id: u32 },
}

// ============================================================================
// Host Function Imports (calls into Rust host)
// ============================================================================

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "unastar")]
extern "C" {
    /// Get player information. Returns length of JSON written to buffer, or 0 on error.
    fn player_get_info(handle: u32, buf_ptr: *mut u8, buf_len: u32) -> u32;

    /// Get block at world position. Returns block runtime ID.
    fn world_get_block(x: i32, y: i32, z: i32) -> u32;

    /// Get world spawn position. Returns length of JSON written to buffer.
    fn world_get_spawn(buf_ptr: *mut u8, buf_len: u32) -> u32;
}

// ============================================================================
// Guest Memory Allocator and Shared Buffer
// ============================================================================

/// Shared state written by host, read by plugin (zero-copy)
/// Layout: [SharedState (fixed size)] [events...] [scratch space...]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SharedState {
    /// Global Metrics
    pub tick_id: u64,
    pub world_time: u64,
    pub current_tps: f32,
    pub player_count: u32,

    /// World State
    pub spawn_x: f64,
    pub spawn_y: f64,
    pub spawn_z: f64,
    pub weather: u8,    // 0: Clear, 1: Rain, 2: Thunder
    pub difficulty: u8, // 0: Peaceful, 1: Easy, 2: Normal, 3: Hard

    /// Active Context (Triggering Player)
    /// This is populated with the info of the player related to the current batch of events
    pub active_player_id: u32,
    pub active_player_x: f64,
    pub active_player_y: f64,
    pub active_player_z: f64,
    pub active_player_yaw: f32,
    pub active_player_pitch: f32,
    pub active_player_health: f32,

    /// Padding to keep alignment and future proofing
    pub _reserved: [u8; 16],
}

impl SharedState {
    pub const SIZE: usize = std::mem::size_of::<SharedState>();

    /// Get spawn position as Vec3 (zero-copy read)
    pub fn spawn(&self) -> Vec3 {
        Vec3 {
            x: self.spawn_x,
            y: self.spawn_y,
            z: self.spawn_z,
        }
    }
}

#[repr(align(8))]
struct SharedBuffer([u8; 64 * 1024]);

#[cfg(target_arch = "wasm32")]
static mut SHARED_BUFFER: SharedBuffer = SharedBuffer([0u8; 64 * 1024]); // 64KB shared buffer

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn get_shared_buffer_ptr() -> *mut u8 {
    unsafe { SHARED_BUFFER.0.as_mut_ptr() }
}

/// Get shared state from the buffer (zero-copy read)
#[cfg(target_arch = "wasm32")]
pub fn get_shared_state() -> &'static SharedState {
    unsafe { &*(SHARED_BUFFER.0.as_ptr() as *const SharedState) }
}

/// Get spawn position directly from shared memory (zero-copy, no host call!)
#[cfg(target_arch = "wasm32")]
pub fn world_spawn_fast() -> Vec3 {
    get_shared_state().spawn()
}

/// Get current tick ID directly from shared memory
#[cfg(target_arch = "wasm32")]
pub fn tick_id_fast() -> u64 {
    get_shared_state().tick_id
}

/// Get world time directly from shared memory
#[cfg(target_arch = "wasm32")]
pub fn world_time_fast() -> u64 {
    get_shared_state().world_time
}

/// Get server TPS directly from shared memory
#[cfg(target_arch = "wasm32")]
pub fn current_tps_fast() -> f32 {
    get_shared_state().current_tps
}

/// Get online player count directly from shared memory
#[cfg(target_arch = "wasm32")]
pub fn player_count_fast() -> u32 {
    get_shared_state().player_count
}

/// Get the active player's position (zero-copy)
#[cfg(target_arch = "wasm32")]
pub fn active_player_pos_fast() -> Vec3 {
    let state = get_shared_state();
    Vec3::new(
        state.active_player_x,
        state.active_player_y,
        state.active_player_z,
    )
}

/// Get the active player's rotation (yaw, pitch)
#[cfg(target_arch = "wasm32")]
pub fn active_player_rot_fast() -> (f32, f32) {
    let state = get_shared_state();
    (state.active_player_yaw, state.active_player_pitch)
}

#[cfg(target_arch = "wasm32")]
pub fn active_player_health_fast() -> f32 {
    get_shared_state().active_player_health
}

#[cfg(not(target_arch = "wasm32"))]
pub fn current_tps_fast() -> f32 {
    0.0
}

#[cfg(not(target_arch = "wasm32"))]
pub fn player_count_fast() -> u32 {
    0
}

#[cfg(not(target_arch = "wasm32"))]
pub fn active_player_pos_fast() -> Vec3 {
    Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn active_player_rot_fast() -> (f32, f32) {
    (0.0, 0.0)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn active_player_health_fast() -> f32 {
    20.0
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn alloc(size: u32) -> *mut u8 {
    let mut buf = Vec::with_capacity(size as usize);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn dealloc(ptr: *mut u8, size: u32) {
    unsafe {
        let _ = Vec::from_raw_parts(ptr, 0, size as usize);
    }
}

// ============================================================================
// GameContext - Query API
// ============================================================================

/// Context provided during on_tick for querying game state.
///
/// This allows plugins to query player and world state on-demand,
/// while keeping the event-driven architecture for notifications.
use std::cell::RefCell;

/// Context provided during on_tick for querying game state.
///
/// This allows plugins to query player and world state on-demand,
/// while keeping the event-driven architecture for notifications.
pub struct GameContext {
    pub actions: RefCell<Vec<PluginAction>>,
}

impl GameContext {
    pub fn new() -> Self {
        Self {
            actions: RefCell::new(Vec::new()),
        }
    }

    /// Push an action to the buffer.
    pub fn push_action(&self, action: PluginAction) {
        #[cfg(target_arch = "wasm32")]
        {
            // Debug: print to stdout (visible in WASM logs if captured)
            use std::io::Write;
            let _ = writeln!(std::io::stdout(), "push_action called!");
        }
        self.actions.borrow_mut().push(action);
    }

    /// Schedule an async task to be executed on the host.
    /// Returns a task ID that will be included in the TaskComplete event.
    #[cfg(target_arch = "wasm32")]
    pub fn schedule_task(&self, request: TaskRequest) -> u32 {
        let data = borsh::to_vec(&request).unwrap_or_default();
        unsafe { host_schedule_task(data.as_ptr(), data.len() as u32) }
    }

    /// Non-WASM stub (for compilation only, not used at runtime)
    #[cfg(not(target_arch = "wasm32"))]
    pub fn schedule_task(&self, _request: TaskRequest) -> u32 {
        0
    }
}

// Host function imports for WASM
#[cfg(target_arch = "wasm32")]
extern "C" {
    /// Schedule an async task on the host. Returns task_id.
    fn host_schedule_task(data_ptr: *const u8, data_len: u32) -> u32;
}

// ============================================================================
// Typed Events (for #[event_handler])
// ============================================================================

/// Typed event for player chat.
pub struct ChatEvent<'a> {
    pub player: PlayerHandle,
    pub message: String,
    pub event_id: u32,
    pub ctx: &'a GameContext,
}

impl<'a> ChatEvent<'a> {
    pub fn cancel(&self) {
        self.ctx.push_action(PluginAction::Cancel {
            event_id: self.event_id,
        });
    }
}

pub struct JoinEvent {
    pub player: PlayerHandle,
    pub username: String,
}

pub struct QuitEvent {
    pub player: PlayerHandle,
}

pub struct BlockBreakEvent {
    pub player: PlayerHandle,
    pub position: (i32, i32, i32),
    pub block_id: BlockId,
}

pub struct BlockPlaceEvent {
    pub player: PlayerHandle,
    pub position: (i32, i32, i32),
    pub block_id: BlockId,
}

pub struct MoveEvent {
    pub player: PlayerHandle,
    pub from: Vec3,
    pub to: Vec3,
}

pub struct JumpEvent {
    pub player: PlayerHandle,
}

pub struct SneakEvent {
    pub player: PlayerHandle,
    pub is_sneaking: bool,
}

pub struct SprintEvent {
    pub player: PlayerHandle,
    pub is_sprinting: bool,
}

pub struct HeldSlotChangeEvent {
    pub player: PlayerHandle,
    pub old_slot: u8,
    pub new_slot: u8,
}

pub struct StartBreakEvent {
    pub player: PlayerHandle,
    pub position: (i32, i32, i32),
    pub face: u8,
}

pub struct InteractBlockEvent {
    pub player: PlayerHandle,
    pub position: (i32, i32, i32),
    pub face: u8,
}

pub struct ItemUseEvent {
    pub player: PlayerHandle,
}

pub struct SwingEvent {
    pub player: PlayerHandle,
}

pub struct TaskCompleteEvent {
    pub task_id: u32,
    pub result: TaskResult,
}

#[cfg(target_arch = "wasm32")]
impl GameContext {
    /// Query information about a player by handle.
    ///
    /// Returns `None` if handle is invalid or player has despawned.
    /// Query information about a player by handle (raw data).
    pub fn player_info(&self, handle: PlayerHandle) -> Option<PlayerInfo> {
        let mut buf = vec![0u8; 1024]; // 1KB buffer should be enough for PlayerInfo
        let len = unsafe { player_get_info(handle.0, buf.as_mut_ptr(), buf.len() as u32) };

        if len > 0 && len <= buf.len() as u32 {
            BorshDeserialize::try_from_slice(&buf[..len as usize]).ok()
        } else {
            None
        }
    }

    /// Get a wrapper for a player to perform actions.
    pub fn player(&self, handle: PlayerHandle) -> Option<Player> {
        Player::new(handle, self)
    }

    /// Get the block at a world position.
    pub fn get_block(&self, x: i32, y: i32, z: i32) -> BlockId {
        BlockId(unsafe { world_get_block(x, y, z) })
    }

    /// Get the world spawn position.
    pub fn world_spawn(&self) -> Option<Vec3> {
        let mut buf = vec![0u8; 256];
        let len = unsafe { world_get_spawn(buf.as_mut_ptr(), buf.len() as u32) };

        if len > 0 && len <= buf.len() as u32 {
            BorshDeserialize::try_from_slice(&buf[..len as usize]).ok()
        } else {
            None
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl GameContext {
    pub fn player_info(&self, _handle: PlayerHandle) -> Option<PlayerInfo> {
        None
    }

    pub fn player(&self, _handle: PlayerHandle) -> Option<Player> {
        None
    }
    pub fn get_block(&self, _x: i32, _y: i32, _z: i32) -> BlockId {
        BlockId(0)
    }
    pub fn world_spawn(&self) -> Option<Vec3> {
        None
    }
}

// ============================================================================
// Plugin Trait
// ============================================================================

/// The trait that plugins must implement.
pub trait Plugin {
    /// Called when the plugin is loaded.
    fn on_load(&mut self) {}

    /// Called when the plugin is unloaded.
    fn on_unload(&mut self) {}

    /// Called every server tick with batched events and a query context.
    ///
    /// # Arguments
    /// * `events` - Batch of events that occurred since last tick
    /// * `ctx` - Context for querying game state (players, world, etc.)
    ///
    /// # Returns
    /// Vector of actions to execute (teleports, kicks, messages, etc.)
    fn on_tick(&mut self, _events: Vec<PluginEvent>, _ctx: &GameContext) -> Vec<PluginAction> {
        Vec::new()
    }
}
