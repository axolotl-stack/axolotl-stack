use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Unique identifier for a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginId(pub String);

impl std::fmt::Display for PluginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

use unastar_api::EventKind;

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
    /// Maximum fuel (instructions) per tick. Defaults to 10M.
    #[serde(default = "default_fuel")]
    pub fuel_per_tick: u64,
    /// Maximum memory in bytes. Defaults to 64MB.
    #[serde(default = "default_memory")]
    pub max_memory: usize,
}

impl Default for PluginLimits {
    fn default() -> Self {
        Self {
            fuel_per_tick: default_fuel(),
            max_memory: default_memory(),
        }
    }
}

fn default_fuel() -> u64 { 10_000_000 }
fn default_memory() -> usize { 64 * 1024 * 1024 }

/// Capabilities a plugin can request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginCapability {
    /// Access to the filesystem (scoped).
    Filesystem,
    /// Access to network (http/sockets).
    Network,
    /// Ability to write player data.
    WritePlayerData,
    /// Ability to spawn processes (restricted).
    Process,
}
