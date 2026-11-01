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
    /// Maximum literal block batches staged per shared component cycle, hard-capped at 128.
    #[serde(default = "default_block_edits")]
    pub max_block_edits_per_tick: u32,
    /// Maximum outstanding literal batches, including requests whose guest ticket was dropped.
    #[serde(default = "default_pending_block_edits")]
    pub max_pending_block_edits: u32,
    /// Maximum UTF-8 bytes in a connection termination notice, also capped at 4096 by the host.
    #[serde(default = "default_connection_reason_bytes")]
    pub max_connection_reason_bytes: u32,
    /// Maximum new termination requests accepted in one shared component cycle.
    #[serde(default = "default_connection_requests")]
    pub max_connection_requests_per_tick: u32,
    /// Maximum outstanding requests across callback staging and backpressure retries.
    #[serde(default = "default_pending_connection_requests")]
    pub max_pending_connection_requests: u32,
    /// Maximum affected cells lifted for one block authorization/completion.
    #[serde(default = "default_block_cells")]
    pub max_block_cells: u32,
    /// Wasmtime instruction fuel shared by all event exports in one server tick.
    /// Defaults to 10M; epoch interruption remains an independent per-call limit.
    #[serde(default = "default_fuel")]
    pub fuel_per_tick: u64,
    /// One instruction budget for instantiation and on-load together, before admission.
    #[serde(default = "default_initialization_fuel")]
    pub fuel_per_initialization: u64,
    /// One non-storage wall allowance for instantiation and on-load together.
    #[serde(default = "default_initialization_timeout")]
    pub initialization_timeout_ms: u32,
    /// Non-storage wall allowance per ordinary callback; it does not renew cycle fuel.
    #[serde(default = "default_callback_timeout")]
    pub callback_timeout_ms: u32,
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
    /// Maximum owned UI widget slots, including provisional callback updates.
    #[serde(default = "default_ui_widgets")]
    pub max_ui_widgets: u32,
    /// Maximum staged widget updates per shared component tick.
    #[serde(default = "default_ui_updates")]
    pub max_ui_updates_per_tick: u32,
    /// Maximum owned subject/viewer pairs, including provisional updates.
    #[serde(default = "default_view_pairs")]
    pub max_view_pairs: u32,
    /// Maximum viewer projection writes per shared component tick.
    #[serde(default = "default_view_updates")]
    pub max_view_updates_per_tick: u32,
    /// Maximum outstanding staged or provider-owned inventory writes per instance.
    #[serde(default = "default_pending_inventory_operations")]
    pub max_pending_inventory_operations: u32,
}

impl Default for PluginLimits {
    fn default() -> Self {
        Self {
            max_block_edits_per_tick: default_block_edits(),
            max_pending_block_edits: default_pending_block_edits(),
            max_connection_reason_bytes: default_connection_reason_bytes(),
            max_connection_requests_per_tick: default_connection_requests(),
            max_pending_connection_requests: default_pending_connection_requests(),
            max_block_cells: default_block_cells(),
            fuel_per_tick: default_fuel(),
            fuel_per_initialization: default_initialization_fuel(),
            initialization_timeout_ms: default_initialization_timeout(),
            callback_timeout_ms: default_callback_timeout(),
            max_memory: default_memory(),
            max_owned_resources: default_owned_resources(),
            max_host_operations_per_tick: default_host_operations(),
            max_tasks_per_tick: default_tasks_per_tick(),
            max_pending_tasks: default_pending_tasks(),
            max_message_bytes: default_message_bytes(),
            max_messages_per_tick: default_messages(),
            max_command_roots: default_command_roots(),
            max_ui_widgets: default_ui_widgets(),
            max_ui_updates_per_tick: default_ui_updates(),
            max_view_pairs: default_view_pairs(),
            max_view_updates_per_tick: default_view_updates(),
            max_pending_inventory_operations: default_pending_inventory_operations(),
        }
    }
}

/// Keep per-cycle native publication work bounded independently of guest fuel.
fn default_block_edits() -> u32 {
    32
}
/// Pending work retains its reservation until cancellation, rejection or actual application.
fn default_pending_block_edits() -> u32 {
    128
}

impl PluginLimits {
    /// Reject unusable or unbounded execution allowances before instantiating a guest.
    pub(crate) fn validate_execution(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.fuel_per_initialization > 0,
            "initialization fuel must be positive"
        );
        anyhow::ensure!(
            (1..=60_000).contains(&self.initialization_timeout_ms),
            "initialization timeout must be between 1 and 60000 ms"
        );
        anyhow::ensure!(
            (1..=1000).contains(&self.callback_timeout_ms),
            "callback timeout must be between 1 and 1000 ms"
        );
        Ok(())
    }
}

/// Stateful initialization has a separate finite allowance from simulation callbacks.
fn default_initialization_fuel() -> u64 {
    100_000_000
}
/// Initialization finishes before listeners and player admission open.
fn default_initialization_timeout() -> u32 {
    1000
}
/// Keep the existing short callback default, independently of startup capacity.
fn default_callback_timeout() -> u32 {
    10
}

/// Cover bounded native multi-cell growth while rejecting unbounded lifting.
fn default_block_cells() -> u32 {
    512
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

/// Keep termination display text bounded independently of general chat messages.
fn default_connection_reason_bytes() -> u32 {
    1024
}
/// Bound accepted termination calls without replenishing during nested callbacks.
fn default_connection_requests() -> u32 {
    64
}
/// Bound backpressured transport requests independently of retained ticket resources.
fn default_pending_connection_requests() -> u32 {
    128
}

/// Bound metadata retained by component-owned command declarations.
fn default_command_roots() -> u32 {
    64
}

/// Bound owned widgets independently of network speed or guest handle retention.
fn default_ui_widgets() -> u32 {
    512
}
/// Bound callback-stage allocations independently of committed widget count.
fn default_ui_updates() -> u32 {
    512
}

/// Keep pairwise formatting memory finite independently of player count.
fn default_view_pairs() -> u32 {
    4096
}
/// Bound projection recomputation independently of guest fuel.
fn default_view_updates() -> u32 {
    4096
}

/// Bound accepted asynchronous writes independently of guest resource drops.
fn default_pending_inventory_operations() -> u32 {
    64
}

/// Capabilities a plugin can request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginCapability {
    ReadBlocks,
    WriteBlocks,
    /// Durably capture and read complete immutable host-owned player state.
    PlayerStateArchives,
    /// Construct and inspect bounded detached item values, without granting inventory writes.
    ItemValues,
    /// Capture complete owned inventory images for the current scoped player.
    ReadInventory,
    /// Stage bounded native status, motion, mode and flight changes.
    PlayerMechanics,
    /// Replace exact inventory images through durable idempotent transactions.
    WriteInventory,
    /// Read typed actor references and current-world observations.
    ReadPlayers,
    /// Advertise owned command roots; handling still uses explicit subscriptions.
    Commands,
    /// Manage typed self UI bound to scoped player and world capabilities.
    Presentation,
    /// Restrict and format one player's representation to an exact scoped viewer.
    PlayerViews,
    /// Send bounded direct messages to exact current connections.
    SendMessages,
    /// Terminate exact admitted connections without granting admission publication or live state writes.
    ConnectionControl,
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
