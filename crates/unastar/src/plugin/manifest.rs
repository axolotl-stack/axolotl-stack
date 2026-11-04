use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::ecs::events::EventKind;

/// Unique identifier for a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginId(pub String);

impl std::fmt::Display for PluginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Version of the `unastar:plugin` WIT package this host implements. Must match
/// the `package` line in `crates/unastar-api/wit/*.wit` (checked by a test).
pub const HOST_API_VERSION: &str = "0.2.0";

/// The manifest file (`plugin.toml`) for a plugin. Unknown keys are rejected,
/// like the server config, so a misspelt key fails loading instead of being ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

impl PluginManifest {
    /// Check that this host can run a plugin built against `api_version`.
    ///
    /// Compatibility follows Cargo's caret rule against [`HOST_API_VERSION`]: the
    /// major version must match (and the minor version too while the major is 0),
    /// and the plugin must not need a newer version than the host provides.
    pub fn check_api_version(&self) -> anyhow::Result<()> {
        let wanted = parse_version(&self.api_version)?;
        let host = parse_version(HOST_API_VERSION)?;
        let same_line = wanted.0 == host.0 && (wanted.0 > 0 || wanted.1 == host.1);
        if same_line && wanted <= host {
            Ok(())
        } else {
            anyhow::bail!(
                "plugin {} needs plugin API {}, but this server provides {}",
                self.id,
                self.api_version,
                HOST_API_VERSION
            )
        }
    }
}

/// Parse a `MAJOR.MINOR.PATCH` version with no pre-release or build suffix.
fn parse_version(version: &str) -> anyhow::Result<(u64, u64, u64)> {
    let mut parts = version.split('.').map(str::parse::<u64>);
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor)), Some(Ok(patch)), None) => Ok((major, minor, patch)),
        _ => anyhow::bail!("api_version {version:?} is not MAJOR.MINOR.PATCH"),
    }
}

/// Resource limits for a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
}

impl Default for PluginLimits {
    fn default() -> Self {
        Self {
            fuel_per_tick: default_fuel(),
            max_memory: default_memory(),
        }
    }
}

fn default_fuel() -> u64 {
    10_000_000
}
fn default_memory() -> usize {
    64 * 1024 * 1024
}

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

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
id = "test.plugin"
name = "Test"
version = "1.0.0"
api_version = "0.2.0"
"#;

    fn parse(toml: &str) -> Result<PluginManifest, toml_edit::de::Error> {
        toml_edit::de::from_str(toml)
    }

    #[test]
    fn minimal_manifest_parses_with_default_limits() {
        let manifest = parse(MINIMAL).unwrap();
        assert_eq!(manifest.limits.fuel_per_tick, default_fuel());
        assert!(manifest.subscriptions.is_empty());
    }

    fn with_api(version: &str) -> PluginManifest {
        let mut manifest = parse(MINIMAL).unwrap();
        manifest.api_version = version.into();
        manifest
    }

    #[test]
    fn api_version_must_be_compatible_with_the_host() {
        assert!(with_api("0.2.0").check_api_version().is_ok());
        for bad in ["0.1.0", "0.3.0", "1.2.0", "0.2.1", "0.2", "0.2.0-beta", "x"] {
            assert!(with_api(bad).check_api_version().is_err(), "{bad}");
        }
    }

    #[test]
    fn host_api_version_matches_the_wit_package() {
        for wit in [
            include_str!("../../../unastar-api/wit/world.wit"),
            include_str!("../../../unastar-api/wit/host.wit"),
            include_str!("../../../unastar-api/wit/types.wit"),
        ] {
            assert!(
                wit.starts_with(&format!("package unastar:plugin@{HOST_API_VERSION};")),
                "WIT package version and HOST_API_VERSION differ"
            );
        }
    }

    #[test]
    fn unknown_top_level_key_is_rejected() {
        let error = parse(&format!("{MINIMAL}subscription = [\"tick\"]\n")).unwrap_err();
        assert!(error.to_string().contains("subscription"), "{error}");
    }

    #[test]
    fn unknown_limits_key_is_rejected() {
        let error = parse(&format!("{MINIMAL}[limits]\nmax_memroy = 1\n")).unwrap_err();
        assert!(error.to_string().contains("max_memroy"), "{error}");
    }
}
