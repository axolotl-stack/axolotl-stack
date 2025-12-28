//! Configuration for Axelerator.
//!
//! Axelerator uses WebRTC (NetherNet) to advertise sessions to Xbox Live friends,
//! then transfers players to the actual RakNet server.

use serde::{Deserialize, Serialize};

/// Configuration for the Axelerator broadcast server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxeleratorConfig {
    /// Name shown in friends list.
    #[serde(default = "default_host_name")]
    pub host_name: String,

    /// World name displayed.
    #[serde(default = "default_world_name")]
    pub world_name: String,

    /// Minecraft version string.
    #[serde(default = "default_version")]
    pub version: String,

    /// Protocol version (must match client).
    #[serde(default = "default_protocol")]
    pub protocol: i32,

    /// Target server IP address (where players will be transferred).
    pub server_ip: String,

    /// Target server port.
    #[serde(default = "default_port")]
    pub server_port: u16,

    /// Maximum players to display.
    #[serde(default = "default_max_players")]
    pub max_players: i32,

    /// Path to cached OAuth token.
    #[serde(default = "default_token_path")]
    pub token_cache_path: String,

    /// Auto-accept pending friend requests.
    #[serde(default)]
    pub auto_accept_friends: bool,

    /// Heartbeat interval for presence (seconds).
    #[serde(default = "default_heartbeat")]
    pub presence_heartbeat: u64,
}

fn default_host_name() -> String {
    "Axelerator Server".into()
}
fn default_world_name() -> String {
    "Minecraft World".into()
}
fn default_version() -> String {
    jolyne::valentine::GAME_VERSION.into()
}
fn default_protocol() -> i32 {
    jolyne::valentine::PROTOCOL_VERSION
}
fn default_port() -> u16 {
    19132
}
fn default_max_players() -> i32 {
    10
}
fn default_token_path() -> String {
    "token.json".into()
}
fn default_heartbeat() -> u64 {
    300
}

impl Default for AxeleratorConfig {
    fn default() -> Self {
        Self {
            host_name: default_host_name(),
            world_name: default_world_name(),
            version: default_version(),
            protocol: default_protocol(),
            server_ip: "127.0.0.1".into(),
            server_port: default_port(),
            max_players: default_max_players(),
            token_cache_path: default_token_path(),
            auto_accept_friends: false,
            presence_heartbeat: default_heartbeat(),
        }
    }
}

impl AxeleratorConfig {
    /// Create config for a specific server.
    pub fn for_server(ip: &str, port: u16) -> Self {
        Self {
            server_ip: ip.into(),
            server_port: port,
            ..Default::default()
        }
    }

    /// Set host name.
    pub fn with_host_name(mut self, name: &str) -> Self {
        self.host_name = name.into();
        self
    }
}
