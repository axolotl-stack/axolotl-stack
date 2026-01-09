//! Configuration for Axelerator.
//!
//! Configuration can be loaded from a TOML file or constructed programmatically.
//! CLI arguments override config file values.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Configuration for the Axelerator broadcast server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AxeleratorConfig {
    /// Name shown in friends list.
    pub host_name: String,

    /// World name displayed.
    pub world_name: String,

    /// Minecraft version string.
    pub version: String,

    /// Protocol version (must match client).
    pub protocol: i32,

    /// Target server IP address (where players will be transferred).
    pub server_ip: String,

    /// Target server port.
    pub server_port: u16,

    /// Maximum players to display.
    pub max_players: i32,

    /// Current player count to display (cosmetic only, Xbox doesn't validate this).
    pub display_players: i32,

    /// Path to cached OAuth token.
    pub token_cache_path: String,

    /// Auto-accept pending friend requests.
    pub auto_accept_friends: bool,

    /// Interval for periodic friend sync (seconds).
    /// This checks for new followers and auto-follows them back.
    /// Set to 0 to disable periodic sync (only use RTA notifications).
    /// Default: 10 seconds.
    pub friend_sync_interval: u64,

    /// Heartbeat interval for presence (seconds).
    pub presence_heartbeat: u64,

    /// Enable session tampering detection and auto-repair.
    pub monitor_tampering: bool,

    /// Interval for tampering checks (seconds).
    pub monitor_interval: u64,

    /// Automatically block (unfriend) detected attackers.
    pub auto_block_attackers: bool,

    /// Logging configuration.
    #[serde(default)]
    pub logging: LoggingConfig,

    /// Screenshot/showcase image configuration.
    #[serde(default)]
    pub screenshots: ScreenshotConfig,
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Log level: "error", "warn", "info", "debug", "trace".
    /// Can also use module-specific levels like "info,axelerator=debug".
    pub level: String,

    /// Include target module in log output.
    pub show_target: bool,

    /// Include timestamps in log output (useful for file logging, not needed for journald).
    pub show_timestamp: bool,

    /// Use JSON format for structured logging.
    pub json_format: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
            show_target: false,
            show_timestamp: false, // journald adds timestamps
            json_format: false,
        }
    }
}

/// Screenshot/showcase image configuration.
///
/// Screenshots are displayed as the server's showcase image in the Xbox friend list.
/// You can either set a single static image or cycle through multiple images from a directory.
///
/// Recommended image specifications:
/// - Resolution: 1200x675 pixels
/// - Format: JPEG (.jpg)
/// - Quality: 90
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenshotConfig {
    /// Enable screenshot uploading feature.
    pub enabled: bool,

    /// Path to a single screenshot image file.
    /// If set, this image will be used as the showcase.
    /// If `directory` is also set, `directory` takes precedence for cycling.
    pub path: Option<String>,

    /// Directory containing screenshot images for cycling.
    /// Supported formats: .jpg, .jpeg, .png
    /// Images will be cycled in alphabetical order.
    pub directory: Option<String>,

    /// Interval (in seconds) between screenshot cycles.
    /// Only used when `directory` is set with multiple images.
    /// Default: 300 (5 minutes)
    pub cycle_interval: u64,

    /// Whether to randomize the image order when cycling.
    /// If false, images are cycled in alphabetical order.
    pub randomize_order: bool,

    /// Upload on startup even if the image hasn't changed.
    /// Useful to ensure the showcase is always set after restart.
    pub upload_on_startup: bool,
}

impl Default for ScreenshotConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: None,
            directory: None,
            cycle_interval: 300, // 5 minutes
            randomize_order: false,
            upload_on_startup: true,
        }
    }
}

impl Default for AxeleratorConfig {
    fn default() -> Self {
        Self {
            host_name: "Axelerator Server".into(),
            world_name: "Minecraft World".into(),
            version: jolyne::valentine::GAME_VERSION.into(),
            protocol: jolyne::valentine::PROTOCOL_VERSION,
            server_ip: "127.0.0.1".into(),
            server_port: 19132,
            max_players: 10,
            display_players: 1,
            token_cache_path: "token.json".into(),
            auto_accept_friends: false,
            friend_sync_interval: 10,
            presence_heartbeat: 300,
            monitor_tampering: false,
            monitor_interval: 30,
            auto_block_attackers: false,
            logging: LoggingConfig::default(),
            screenshots: ScreenshotConfig::default(),
        }
    }
}

impl AxeleratorConfig {
    /// Load configuration from a TOML file.
    ///
    /// If the file doesn't exist, returns default config.
    /// If the file exists but is invalid, returns an error.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let path = path.as_ref();

        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
            path: path.display().to_string(),
            source: e,
        })?;

        toml::from_str(&contents).map_err(|e| ConfigError::Parse {
            path: path.display().to_string(),
            source: e,
        })
    }

    /// Load from file, falling back to default on any error.
    ///
    /// Logs warnings if the file exists but can't be parsed.
    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Self {
        match Self::load(&path) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Warning: Failed to load config: {}", e);
                Self::default()
            }
        }
    }

    /// Save configuration to a TOML file.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), ConfigError> {
        let path = path.as_ref();
        let contents =
            toml::to_string_pretty(self).map_err(|e| ConfigError::Serialize { source: e })?;

        std::fs::write(path, contents).map_err(|e| ConfigError::Io {
            path: path.display().to_string(),
            source: e,
        })
    }

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

    /// Generate an example config file contents.
    pub fn example_toml() -> String {
        let example = Self {
            host_name: "My Awesome Server".into(),
            world_name: "Survival World".into(),
            server_ip: "play.example.com".into(),
            server_port: 19132,
            max_players: 20,
            display_players: 5, // Show "5/20" in friends list (cosmetic)
            monitor_tampering: true,
            monitor_interval: 30,
            logging: LoggingConfig {
                level: "info,axelerator=debug".into(),
                ..Default::default()
            },
            screenshots: ScreenshotConfig {
                enabled: true,
                path: Some("screenshot.jpg".into()),
                directory: None,
                cycle_interval: 300,
                randomize_order: false,
                upload_on_startup: true,
            },
            ..Default::default()
        };

        format!(
            r#"# Axelerator Configuration
# See DEPLOYMENT.md for full documentation

{}
"#,
            toml::to_string_pretty(&example).unwrap()
        )
    }
}

/// Configuration loading errors.
#[derive(Debug)]
pub enum ConfigError {
    Io {
        path: String,
        source: std::io::Error,
    },
    Parse {
        path: String,
        source: toml::de::Error,
    },
    Serialize {
        source: toml::ser::Error,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io { path, source } => {
                write!(f, "Failed to read config file '{}': {}", path, source)
            }
            ConfigError::Parse { path, source } => {
                write!(f, "Failed to parse config file '{}': {}", path, source)
            }
            ConfigError::Serialize { source } => {
                write!(f, "Failed to serialize config: {}", source)
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Io { source, .. } => Some(source),
            ConfigError::Parse { source, .. } => Some(source),
            ConfigError::Serialize { source } => Some(source),
        }
    }
}
