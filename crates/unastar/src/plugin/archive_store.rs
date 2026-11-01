//! Immutable, module-owned native player state. Encoded payloads never enter guests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::storage::{self, StoreError};
use crate::server::game::player_state::PlayerSnapshot;

pub(crate) const DEFAULT_DIRECTORY: &str = "data/player-state-archives";
const FORMAT_VERSION: u16 = 1;
pub(crate) const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;

/// Archive failures contain no host paths or encoded native state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArchiveError {
    InvalidKey,
    Conflict,
    NotFound,
    InvalidReference,
    InvalidSnapshot,
    QuotaExceeded,
    IoError,
    Uncertain,
}

impl From<StoreError> for ArchiveError {
    /// Preserve installation uncertainty rather than acknowledging an ambiguous write.
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::InvalidKey => Self::InvalidKey,
            StoreError::QuotaExceeded => Self::QuotaExceeded,
            StoreError::Conflict => Self::Conflict,
            StoreError::IoError => Self::IoError,
            StoreError::Uncertain => Self::Uncertain,
        }
    }
}

/// Original account observation, never a capability for a current or replacement actor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Owner {
    pub uuid: String,
    pub authenticated_xuid: Option<u64>,
    pub name: String,
    pub connection_id: Option<u64>,
}

/// Durable world identity is independent of the old ECS world's runtime lifetime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceWorld {
    pub uuid: String,
    pub name: String,
    pub dimension: i32,
}

/// Only this opaque reference and typed historical facts may cross the component boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reference {
    pub key: String,
    pub revision: u64,
    pub fingerprint: String,
    pub owner: Owner,
}

/// The complete native state stays in host-owned files and memory.
/// Keeping persistence separate from capture permits future owned event payloads.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    version: u16,
    plugin_id: String,
    pub owner: Owner,
    pub world: SourceWorld,
    pub captured_tick: u64,
    pub state: PlayerSnapshot,
}

impl Record {
    /// Bind a genuine host capture to its original account, module and world.
    pub fn new(
        plugin_id: &str,
        owner: Owner,
        world: SourceWorld,
        captured_tick: u64,
        state: PlayerSnapshot,
    ) -> Result<Self, ArchiveError> {
        let record = Self {
            version: FORMAT_VERSION,
            plugin_id: plugin_id.into(),
            owner,
            world,
            captured_tick,
            state,
        };
        record.validate(plugin_id)?;
        Ok(record)
    }

    /// Stored formats and account facts are checked before any typed value is exposed.
    fn validate(&self, plugin_id: &str) -> Result<(), ArchiveError> {
        let owner =
            uuid::Uuid::parse_str(&self.owner.uuid).map_err(|_| ArchiveError::InvalidSnapshot)?;
        let world =
            uuid::Uuid::parse_str(&self.world.uuid).map_err(|_| ArchiveError::InvalidSnapshot)?;
        let native_xuid = self
            .state
            .xuid
            .as_ref()
            .map(|text| {
                text.parse::<u64>()
                    .ok()
                    .filter(|xuid| *xuid > 0 && xuid.to_string() == *text)
                    .ok_or(ArchiveError::InvalidSnapshot)
            })
            .transpose()?;
        if self.version != FORMAT_VERSION
            || self.plugin_id != plugin_id
            || self.plugin_id.is_empty()
            || owner.is_nil()
            || self.owner.uuid != owner.to_string()
            || self.owner.uuid != self.state.data.uuid
            || self.owner.authenticated_xuid != native_xuid
            || self.owner.connection_id == Some(0)
            || !label(&self.owner.name, 64)
            || world.is_nil()
            || self.world.uuid != world.to_string()
            || !label(&self.world.name, 128)
            || self.world.dimension != self.state.data.dimension
        {
            return Err(ArchiveError::InvalidSnapshot);
        }
        self.state
            .validate()
            .map_err(|_| ArchiveError::InvalidSnapshot)
    }
}

/// Reject unbounded or ambiguous labels while retaining ordinary display names.
fn label(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

pub(crate) type SharedNamespace = Arc<Mutex<Namespace>>;

/// Archive namespaces use the same atomic write, checksum and lease implementation as storage.
/// They live under a separate protected root; generic storage cannot read their native payloads.
pub(crate) struct Registry {
    backend: storage::Registry,
    namespaces: HashMap<String, Weak<Mutex<Namespace>>>,
}

impl Default for Registry {
    /// No files open before a component requests its archive capability.
    fn default() -> Self {
        Self::new(PathBuf::from(DEFAULT_DIRECTORY), storage::Limits::default())
    }
}

impl Registry {
    /// Keep capacity explicit and independent from guest byte-storage quotas.
    pub fn new(directory: PathBuf, limits: storage::Limits) -> Self {
        Self {
            backend: storage::Registry::new(directory, limits),
            namespaces: HashMap::new(),
        }
    }

    /// Report the effective file bounds without exposing another module's records.
    pub fn limits(&self) -> storage::Limits {
        self.backend.limits()
    }

    /// Native snapshot payloads must never enter a guest filesystem preopen.
    pub fn validate_directory(&self, plugin: &Path) -> Result<(), ArchiveError> {
        self.backend.validate_directory(plugin).map_err(Into::into)
    }

    /// Raw guest storage must never address the files containing native snapshots.
    pub(super) fn validate_storage(&self, storage: &storage::Registry) -> Result<(), ArchiveError> {
        if !self.backend.disjoint_from(storage)? {
            return Err(ArchiveError::IoError);
        }
        Ok(())
    }

    /// Share the existing lease across reloads and strictly validate all retained records.
    pub fn namespace(&mut self, plugin_id: &str) -> Result<SharedNamespace, ArchiveError> {
        if self.backend.limits().value_bytes > MAX_SNAPSHOT_BYTES {
            return Err(ArchiveError::QuotaExceeded);
        }
        let namespace =
            if let Some(namespace) = self.namespaces.get(plugin_id).and_then(Weak::upgrade) {
                namespace
            } else {
                self.namespaces
                    .retain(|_, namespace| namespace.strong_count() != 0);
                let namespace = Arc::new(Mutex::new(Namespace {
                    plugin_id: plugin_id.into(),
                    backend: self.backend.namespace(plugin_id)?,
                    value_bytes: self.backend.limits().value_bytes,
                }));
                self.namespaces
                    .insert(plugin_id.into(), Arc::downgrade(&namespace));
                namespace
            };
        namespace
            .lock()
            .map_err(|_| ArchiveError::IoError)?
            .validate_all()?;
        Ok(namespace)
    }
}

/// One create-only module archive. There is no deletion, eviction or key reuse in this API.
pub(crate) struct Namespace {
    plugin_id: String,
    backend: storage::SharedNamespace,
    value_bytes: usize,
}

/// A complete immutable record plus its exact encoded size for retained-value accounting.
pub(crate) struct Loaded {
    pub reference: Reference,
    pub record: Record,
    pub bytes: usize,
}

impl Namespace {
    /// Reject corrupt native envelopes on every fresh instance/reload before guest initialization.
    fn validate_all(&mut self) -> Result<(), ArchiveError> {
        let keys = self
            .backend
            .lock()
            .map_err(|_| ArchiveError::IoError)?
            .keys()?;
        for key in keys {
            self.lookup(&key)?.ok_or(ArchiveError::IoError)?;
        }
        Ok(())
    }

    /// Acknowledgement means the immutable native payload and its directory are durable.
    /// If installation is uncertain, callers must look up the key instead of assuming failure.
    pub fn capture(&mut self, key: &str, record: &Record) -> Result<Reference, ArchiveError> {
        record.validate(&self.plugin_id)?;
        let bytes = encode(record, self.value_bytes)?;
        let revision = self
            .backend
            .lock()
            .map_err(|_| ArchiveError::IoError)?
            .compare_exchange(key, None, &bytes)?;
        if revision != 1 {
            return Err(ArchiveError::IoError);
        }
        Ok(reference(key, revision, &bytes, &record.owner))
    }

    /// Reconcile an unknown installation without changing or recapturing the original state.
    pub fn lookup(&mut self, key: &str) -> Result<Option<Reference>, ArchiveError> {
        self.load(key)
            .map(|loaded| loaded.map(|loaded| loaded.reference))
    }

    /// Validate the whole reference, including original account, before returning historical state.
    pub fn read(&mut self, expected: &Reference) -> Result<Loaded, ArchiveError> {
        let loaded = self.load(&expected.key)?.ok_or(ArchiveError::NotFound)?;
        if loaded.reference != *expected {
            return Err(ArchiveError::InvalidReference);
        }
        Ok(loaded)
    }

    /// Checksummed framing and semantic native validation are both mandatory.
    fn load(&mut self, key: &str) -> Result<Option<Loaded>, ArchiveError> {
        let Some(blob) = self
            .backend
            .lock()
            .map_err(|_| ArchiveError::IoError)?
            .read(key)?
        else {
            return Ok(None);
        };
        if blob.revision != 1 {
            return Err(ArchiveError::InvalidSnapshot);
        }
        let mut decoder = serde_json::Deserializer::from_slice(&blob.bytes);
        let mut ignored = false;
        let record: Record = serde_ignored::deserialize(&mut decoder, |_| ignored = true)
            .map_err(|_| ArchiveError::InvalidSnapshot)?;
        if ignored || decoder.end().is_err() {
            return Err(ArchiveError::InvalidSnapshot);
        }
        record.validate(&self.plugin_id)?;
        Ok(Some(Loaded {
            reference: reference(key, blob.revision, &blob.bytes, &record.owner),
            record,
            bytes: blob.bytes.len(),
        }))
    }
}

/// Bound encoded output before growing a buffer; native source bytes remain opaque.
fn encode(record: &Record, limit: usize) -> Result<Vec<u8>, ArchiveError> {
    struct Output {
        bytes: Vec<u8>,
        limit: usize,
        exceeded: bool,
    }
    impl std::io::Write for Output {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self
                .bytes
                .len()
                .checked_add(bytes.len())
                .is_none_or(|size| size > self.limit)
                || self.bytes.try_reserve(bytes.len()).is_err()
            {
                self.exceeded = true;
                return Err(std::io::Error::other("snapshot byte limit exceeded"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Output {
        bytes: Vec::new(),
        limit,
        exceeded: false,
    };
    if serde_json::to_writer(&mut output, record).is_err() {
        return Err(if output.exceeded {
            ArchiveError::QuotaExceeded
        } else {
            ArchiveError::InvalidSnapshot
        });
    }
    Ok(output.bytes)
}

/// Fingerprints bind the exact immutable bytes, including original module/account/world facts.
fn reference(key: &str, revision: u64, bytes: &[u8], owner: &Owner) -> Reference {
    let mut digest = Sha256::new();
    digest.update(b"unastar-player-state-archive-reference-v1");
    digest.update((key.len() as u64).to_be_bytes());
    digest.update(key.as_bytes());
    digest.update(revision.to_be_bytes());
    digest.update(bytes);
    Reference {
        key: key.into(),
        revision,
        fingerprint: format!("{:x}", digest.finalize()),
        owner: owner.clone(),
    }
}

#[cfg(test)]
mod tests;
