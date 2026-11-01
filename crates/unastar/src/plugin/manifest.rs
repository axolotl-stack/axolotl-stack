use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

use crate::ecs::events::EventKind;

/// Unique identifier for a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginId(pub String);

impl std::fmt::Display for PluginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The manifest file (`plugin.toml`) for a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// The unique ID of the plugin (e.g., "author.plugin-name").
    pub id: PluginId,
    /// Human-readable name.
    pub name: String,
    /// Plugin version (semver).
    pub version: String,
    /// Required host API version.
    pub api_version: String,
    /// Initialization and callback failures stop the runtime instead of being optional.
    #[serde(default)]
    pub required: bool,
    /// Optional opaque configuration file, relative to the plugin directory.
    #[serde(default)]
    pub config: Option<PathBuf>,
    /// Requested capabilities.
    #[serde(default)]
    pub capabilities: HashSet<PluginCapability>,
    /// Events this plugin subscribes to.
    #[serde(default)]
    pub subscriptions: HashSet<EventKind>,
    /// Performance limits.
    #[serde(default)]
    pub limits: PluginLimits,
}

/// Resource limits for a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginLimits {
    /// Wasmtime instruction fuel shared by all event exports in one server tick.
    /// Instantiation and on-load share a separate initialization budget of this size.
    /// Defaults to 10M; epoch interruption remains an independent per-call limit.
    #[serde(default = "default_fuel")]
    pub fuel_per_tick: u64,
    /// Maximum bytes per Wasm linear memory, enforced at creation and growth.
    /// Exceeding this limit traps. Defaults to 64MB.
    #[serde(default = "default_memory")]
    pub max_memory: usize,
    /// Maximum live component resources, including expired capabilities retained by a guest.
    #[serde(default = "default_owned_resources")]
    pub max_owned_resources: u32,
    /// Aggregate host calls in one shared tick budget.
    #[serde(default = "default_host_operations")]
    pub max_host_operations_per_tick: u32,
    /// Maximum scheduled jobs accepted per shared tick.
    #[serde(default = "default_tasks_per_tick")]
    pub max_tasks_per_tick: u32,
    /// Maximum unexecuted jobs retained by one component instance.
    #[serde(default = "default_pending_tasks")]
    pub max_pending_tasks: u32,
    /// Maximum UTF-8 bytes in one message or diagnostic.
    #[serde(default = "default_message_bytes")]
    pub max_message_bytes: u32,
    /// Maximum direct messages staged in one shared tick.
    #[serde(default = "default_messages")]
    pub max_messages_per_tick: u32,
    /// Maximum owned command roots, including provisional registrations.
    #[serde(default = "default_command_roots")]
    pub max_command_roots: u32,
}

impl Default for PluginLimits {
    fn default() -> Self {
        Self {
            fuel_per_tick: default_fuel(),
            max_memory: default_memory(),
            max_owned_resources: default_owned_resources(),
            max_host_operations_per_tick: default_host_operations(),
            max_tasks_per_tick: default_tasks_per_tick(),
            max_pending_tasks: default_pending_tasks(),
            max_message_bytes: default_message_bytes(),
            max_messages_per_tick: default_messages(),
            max_command_roots: default_command_roots(),
        }
    }
}

fn default_fuel() -> u64 {
    10_000_000
}
fn default_memory() -> usize {
    64 * 1024 * 1024
}

/// Bound resource references independently of Wasm linear memory.
fn default_owned_resources() -> u32 {
    4096
}
/// Limit cheap imported operations in addition to Wasm instruction fuel.
fn default_host_operations() -> u32 {
    20_000
}
/// Keep deferred work finite even when every callback schedules more work.
fn default_tasks_per_tick() -> u32 {
    256
}
/// Bound jobs independently of how many task handles the guest drops.
fn default_pending_tasks() -> u32 {
    1024
}
/// Allow formatted messages without unbounded host allocations.
fn default_message_bytes() -> u32 {
    4096
}
/// Bound a component's packet-producing direct message calls.
fn default_messages() -> u32 {
    256
}

/// Bound metadata retained by component-owned command declarations.
fn default_command_roots() -> u32 {
    64
}

/// Capabilities a plugin can request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginCapability {
    /// Construct and inspect bounded detached item values, without granting inventory writes.
    ItemValues,
    /// Capture complete owned inventory images for the current scoped player.
    ReadInventory,
    /// Stage bounded native status, motion, mode and flight changes.
    PlayerMechanics,
    /// Read typed actor references and current-world observations.
    ReadPlayers,
    /// Advertise owned command roots; handling still uses explicit subscriptions.
    Commands,
    /// Send bounded direct messages to exact current connections.
    SendMessages,
    /// Schedule bounded work on world owners with fresh transaction scopes.
    ScheduleTasks,
    /// Access to the filesystem (scoped).
    Filesystem,
    /// Access to network (http/sockets).
    Network,
    /// Ability to write player data.
    WritePlayerData,
    /// Ability to spawn processes (restricted).
    Process,
}
