use serde::{Deserialize, Serialize};

/// Events sent from Host to Plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginEvent {
    /// Server tick pulse.
    Tick {
        tick_id: u64,
        time: u64,
    },
    /// A player joined the server.
    PlayerJoin {
        player_id: String, // UUID
        username: String,
    },
    /// A player sent a chat message.
    PlayerChat {
        player_id: String,
        message: String,
    },
    /// A player broke a block.
    BlockBreak {
        player_id: String,
        x: i32,
        y: i32,
        z: i32,
        block_name: String,
    },
    /// A player placed a block.
    BlockPlace {
        player_id: String,
        x: i32,
        y: i32,
        z: i32,
        block_name: String,
    },
}

/// Actions returned from Plugin to Host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginAction {
    /// Log a message to the server console.
    Log {
        level: String, // "info", "warn", "error"
        message: String,
    },
    /// Kick a player.
    Kick {
        player_id: String,
        reason: String,
    },
    /// Send a message to a player.
    SendMessage {
        player_id: String,
        message: String,
    },
    /// Set a block in the world.
    SetBlock {
        x: i32,
        y: i32,
        z: i32,
        block_name: String,
    },
}

/// The trait that plugins must implement.
pub trait Plugin {
    fn on_load(&mut self) {}
    fn on_unload(&mut self) {}
    fn on_tick(&mut self, events: Vec<PluginEvent>) -> Vec<PluginAction>;
}

/// FFI Helper: Deallocates memory given to the host.
/// Plugins shouldn't call this directly; the host calls it.
#[no_mangle]
pub unsafe extern "C" fn dealloc_buffer(ptr: *mut u8, len: i32) {
    let _ = Vec::from_raw_parts(ptr, len as usize, len as usize);
}
