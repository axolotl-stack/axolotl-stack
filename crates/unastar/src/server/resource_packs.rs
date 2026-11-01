//! Validate local pack identities and retain immutable archive bytes for joins.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use jolyne::proto::packs::{ResourcePack, ServerPacks};
use serde::Deserialize;
use thiserror::Error;
use uuid::Uuid;

use crate::config::ResourcePackConfig;

/// Only the runtime's validated immutable offer becomes an observable server fact.
#[derive(bevy_ecs::prelude::Resource, Clone)]
pub(crate) struct ConfiguredPacks(pub(crate) Arc<ServerPacks>);

/// Invalid configured packs fail before listener binding, never become an empty offer.
#[derive(Debug, Error)]
pub(crate) enum LoadError {
    #[error("resource pack {path}: {reason}")]
    Invalid { path: PathBuf, reason: String },
    #[error("resource pack {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Load each configured archive once. Existing joins never observe later file changes.
pub(crate) fn load(
    config: &ResourcePackConfig,
    required: bool,
) -> Result<Arc<ServerPacks>, LoadError> {
    let mut packs = ServerPacks {
        required,
        ..ServerPacks::default()
    };
    let mut remaining = config.max_total_bytes;
    let mut identities = BTreeSet::new();
    for path in &config.paths {
        let file = File::open(path).map_err(|source| io(path, source))?;
        let metadata = file.metadata().map_err(|source| io(path, source))?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > remaining {
            return Err(invalid(
                path,
                "archive is empty, not a regular file, or exceeds the total byte limit",
            ));
        }
        let mut content = Vec::new();
        file.take(remaining.saturating_add(1))
            .read_to_end(&mut content)
            .map_err(|source| io(path, source))?;
        if content.len() as u64 > remaining {
            return Err(invalid(path, "archives exceed the total byte limit"));
        }
        remaining -= content.len() as u64;
        let manifest = manifest(path, &content, config.max_manifest_bytes)?;
        let header_uuid = identity(path, &manifest.header.uuid)?;
        let pack_version = version(path, manifest.header.version, manifest.format_version)?;
        version(
            path,
            manifest.header.min_engine_version,
            manifest.format_version,
        )?;
        if !identities.insert(header_uuid) {
            return Err(invalid(path, "pack or module UUID appears more than once"));
        }
        if manifest.modules.is_empty() {
            return Err(invalid(path, "manifest has no resource module"));
        }
        for module in manifest.modules {
            if module.kind != "resources" {
                return Err(invalid(
                    path,
                    "only resource modules are supported; server scripts and behavior modules are not executed",
                ));
            }
            let uuid = identity(path, &module.uuid)?;
            if !identities.insert(uuid) {
                return Err(invalid(path, "pack or module UUID appears more than once"));
            }
            version(path, module.version, manifest.format_version)?;
        }
        let mut pack = ResourcePack::new(header_uuid, pack_version, content.into())
            .map_err(|error| invalid(path, error.to_string()))?;
        pack.ray_tracing = manifest.capabilities.iter().any(|name| name == "raytraced");
        pack.is_addon = manifest.metadata.product_type.as_deref() == Some("addon");
        packs.packs.push(Arc::new(pack));
    }
    Ok(Arc::new(packs))
}

#[derive(Deserialize)]
struct Manifest {
    format_version: u32,
    header: Header,
    modules: Vec<Module>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    metadata: Metadata,
}

#[derive(Deserialize)]
struct Header {
    #[serde(rename = "name")]
    _name: String,
    #[serde(rename = "description")]
    _description: String,
    uuid: String,
    version: Version,
    min_engine_version: Version,
}

#[derive(Default, Deserialize)]
struct Metadata {
    product_type: Option<String>,
}

#[derive(Deserialize)]
struct Module {
    #[serde(rename = "type")]
    kind: String,
    uuid: String,
    version: Version,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Version {
    Vector([u32; 3]),
    Text(String),
}

/// Read the root manifest, with bounded decompression and no filesystem extraction.
fn manifest(path: &Path, content: &[u8], limit: u64) -> Result<Manifest, LoadError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(content))
        .map_err(|error| invalid(path, format!("invalid ZIP archive: {error}")))?;
    let mut manifest_index = None;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| invalid(path, format!("unsupported ZIP entry: {error}")))?;
        if file.encrypted() || file.is_symlink() || !safe_name(file.name()) {
            return Err(invalid(
                path,
                "encrypted, symbolic-link or unsafe archive entry",
            ));
        }
        if file.name() == "manifest.json" && manifest_index.replace(index).is_some() {
            return Err(invalid(
                path,
                "archive has more than one root manifest.json",
            ));
        }
    }
    let index = manifest_index.ok_or_else(|| invalid(path, "archive has no root manifest.json"))?;
    let file = archive
        .by_index(index)
        .map_err(|error| invalid(path, format!("manifest cannot be opened: {error}")))?;
    if file.size() > limit {
        return Err(invalid(path, "manifest exceeds its byte limit"));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| io(path, source))?;
    if bytes.len() as u64 > limit {
        return Err(invalid(path, "manifest exceeds its byte limit"));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| invalid(path, format!("manifest is not UTF-8: {error}")))?;
    let text = unastar_packs::json::strip_comments(text.strip_prefix('\u{feff}').unwrap_or(text));
    let manifest: Manifest = serde_json::from_str(&text)
        .map_err(|error| invalid(path, format!("invalid manifest: {error}")))?;
    if !matches!(manifest.format_version, 2 | 3) {
        return Err(invalid(
            path,
            "resource manifest format_version must be 2 or 3",
        ));
    }
    Ok(manifest)
}

/// Check both path separator conventions before bytes reach any client platform.
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['/', '\\'])
        && !name.contains(['\0', ':'])
        && !name.split(['/', '\\']).any(|part| part == "..")
}

/// Preserve a valid declared semantic version; version-three manifests use text.
fn version(path: &Path, value: Version, format: u32) -> Result<String, LoadError> {
    let parsed = match value {
        Version::Vector([major, minor, patch]) if format == 2 => {
            semver::Version::new(u64::from(major), u64::from(minor), u64::from(patch))
        }
        Version::Vector(_) => {
            return Err(invalid(path, "manifest version 3 requires string versions"));
        }
        Version::Text(text) => semver::Version::parse(&text)
            .map_err(|error| invalid(path, format!("invalid semantic version: {error}")))?,
    };
    Ok(parsed.to_string())
}

/// Pack identities must name actual resources rather than an unset UUID.
fn identity(path: &Path, text: &str) -> Result<Uuid, LoadError> {
    Uuid::parse_str(text)
        .ok()
        .filter(|uuid| !uuid.is_nil())
        .ok_or_else(|| invalid(path, "invalid or nil UUID"))
}

/// Keep startup errors useful without converting malformed data to defaults.
fn invalid(path: &Path, reason: impl Into<String>) -> LoadError {
    LoadError::Invalid {
        path: path.into(),
        reason: reason.into(),
    }
}

/// Preserve the original filesystem cause for operator diagnostics.
fn io(path: &Path, source: std::io::Error) -> LoadError {
    LoadError::Io {
        path: path.into(),
        source,
    }
}

#[cfg(test)]
pub(crate) mod tests;
