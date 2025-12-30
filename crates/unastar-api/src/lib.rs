use serde::{Deserialize, Serialize};

pub use unastar_api_macros::plugin;

// ============================================================================
// Handle Types
// ============================================================================

/// Opaque handle to a player entity. Valid only during the current on_tick call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlayerHandle(pub u32);

/// Opaque handle to a world/level. Valid only during the current on_tick call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorldHandle(pub u32);

/// Player identifier wrapper (UUID string)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlayerId(pub String);

/// Block ID wrapper
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub u32);

// ============================================================================
// Query Result Types
// ============================================================================

/// Player information snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub uuid: String,
    pub name: String,
    pub position: Vec3,
    pub health: f32,
    pub max_health: f32,
}

/// 3D vector (position, velocity, etc.)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
// Event Types
// ============================================================================

/// Event kind for subscription filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Tick,
    PlayerJoin,
    PlayerChat,
    BlockBreak,
    BlockPlace,
    Timer,
}

/// Events sent from Host to Plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginEvent {
    /// Server tick pulse.
    Tick { tick_id: u64 },
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
    Timer { id: u64 },
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
        }
    }
}

// ============================================================================
// Action Types
// ============================================================================

/// Log level for plugin messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// Actions returned from Plugin to Host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginAction {
    /// Log a message to the server console.
    Log { level: LogLevel, message: String },
    /// Kick a player.
    Kick { player_id: String, reason: String },
    /// Send a message to a player.
    SendMessage { player_id: String, message: String },
    /// Teleport a player to a position.
    Teleport { player_id: String, position: Vec3 },
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
// Guest Memory Allocator (needed for host to allocate in guest memory)
// ============================================================================

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
pub struct GameContext;

#[cfg(target_arch = "wasm32")]
impl GameContext {
    /// Query information about a player by handle.
    ///
    /// Returns `None` if handle is invalid or player has despawned.
    pub fn player(&self, handle: PlayerHandle) -> Option<PlayerInfo> {
        let mut buf = vec![0u8; 1024]; // 1KB buffer should be enough for PlayerInfo
        let len = unsafe { player_get_info(handle.0, buf.as_mut_ptr(), buf.len() as u32) };

        if len > 0 && len <= buf.len() as u32 {
            serde_json::from_slice(&buf[..len as usize]).ok()
        } else {
            None
        }
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
            serde_json::from_slice(&buf[..len as usize]).ok()
        } else {
            None
        }
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
    fn on_tick(&mut self, events: Vec<PluginEvent>, ctx: &GameContext) -> Vec<PluginAction>;
}
