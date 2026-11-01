//! Private durable storage backing the namespaced component storage interface.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"UPSTORE1";
pub(crate) const DEFAULT_DIRECTORY: &str = "data/plugins";
const MAX_KEY: usize = 128;
const HEADER: usize = 8 + 8 + 2 + 8;
const DIGEST: usize = 32;
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(1);

/// A confirmed durable revision, owned independently of an invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Blob {
    pub revision: u64,
    pub bytes: Vec<u8>,
}

/// Failures distinguish rejection from a write that may already be installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreError {
    InvalidKey,
    QuotaExceeded,
    Conflict,
    IoError,
    Uncertain,
}

/// Storage bounds apply to encoded files as well as guest-provided values.
#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub value_bytes: usize,
    pub namespace_bytes: u64,
    pub keys: usize,
}

impl Default for Limits {
    /// Default capacity is explicit; operator configuration may change it.
    fn default() -> Self {
        Self {
            value_bytes: 16 * 1024 * 1024,
            namespace_bytes: 64 * 1024 * 1024,
            keys: 1024,
        }
    }
}

impl Limits {
    /// Reject empty capacity and arithmetic overflow before opening directories.
    pub(crate) fn validate(self) -> Result<(), StoreError> {
        if self.value_bytes == 0
            || self.namespace_bytes == 0
            || self.keys == 0
            || self
                .value_bytes
                .checked_add(HEADER + MAX_KEY + DIGEST + 1)
                .is_none()
        {
            return Err(StoreError::QuotaExceeded);
        }
        Ok(())
    }
}

pub(super) type SharedNamespace = Arc<Mutex<Namespace>>;

/// Reuse a leased namespace while a replacement component is being initialized.
/// Weak entries release the operating-system lease after the last context exits.
pub(super) struct Registry {
    root: PathBuf,
    limits: Limits,
    namespaces: HashMap<String, Weak<Mutex<Namespace>>>,
}

impl Default for Registry {
    /// Tests and standalone managers open no files until a component uses storage.
    fn default() -> Self {
        Self::new(PathBuf::from(DEFAULT_DIRECTORY), Limits::default())
    }
}

impl Registry {
    /// Keep storage outside component filesystem preopens; namespaces open lazily.
    pub fn new(root: PathBuf, limits: Limits) -> Self {
        Self {
            root,
            limits,
            namespaces: HashMap::new(),
        }
    }

    /// Report the effective host limits without exposing other namespaces.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Durable component values must stay outside every filesystem preopen.
    pub fn validate_directory(&self, plugin_directory: &Path) -> Result<(), StoreError> {
        self.limits.validate()?;
        let root = resolve_directory(&self.root)?;
        let plugins = fs::canonicalize(plugin_directory).map_err(io)?;
        if root.starts_with(plugins) {
            return Err(StoreError::IoError);
        }
        Ok(())
    }

    /// Distinct typed stores cannot share or contain each other's private native files.
    pub(super) fn disjoint_from(&self, other: &Self) -> Result<bool, StoreError> {
        directories_disjoint(&self.root, &other.root)
    }

    /// Share only an exact component identity, including across a staged reload.
    pub fn namespace(&mut self, plugin_id: &str) -> Result<SharedNamespace, StoreError> {
        if let Some(namespace) = self.namespaces.get(plugin_id).and_then(Weak::upgrade) {
            return Ok(namespace);
        }
        self.namespaces
            .retain(|_, namespace| namespace.strong_count() != 0);
        let namespace = Arc::new(Mutex::new(Namespace::open(
            &self.root,
            plugin_id,
            self.limits,
        )?));
        self.namespaces
            .insert(plugin_id.into(), Arc::downgrade(&namespace));
        Ok(namespace)
    }
}

/// Resolve aliases before accepting two independently exposed storage roots.
pub(crate) fn directories_disjoint(first: &Path, second: &Path) -> Result<bool, StoreError> {
    let first = resolve_directory(first)?;
    let second = resolve_directory(second)?;
    Ok(!first.starts_with(&second) && !second.starts_with(&first))
}

/// Resolve existing symlinks while allowing a storage directory not yet created.
fn resolve_directory(path: &Path) -> Result<PathBuf, StoreError> {
    let absolute = std::path::absolute(path).map_err(io)?;
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            component => {
                resolved.push(component.as_os_str());
                if resolved.try_exists().map_err(io)? {
                    resolved = fs::canonicalize(resolved).map_err(io)?;
                }
            }
        }
    }
    Ok(resolved)
}

/// One leased namespace shared by every invocation of a component identity.
pub(super) struct Namespace {
    directory: PathBuf,
    _lease: File,
    limits: Limits,
    uncertain: bool,
    #[cfg(test)]
    fault: Option<Fault>,
}

impl Namespace {
    /// Open an isolated namespace, rejecting concurrent owners and corrupt blobs.
    pub fn open(root: &Path, plugin_id: &str, limits: Limits) -> Result<Self, StoreError> {
        limits.validate()?;
        if plugin_id.is_empty() {
            return Err(StoreError::QuotaExceeded);
        }
        create_durable_directory(root)?;
        let directory = root.join(fingerprint(plugin_id.as_bytes()));
        create_durable_directory(&directory)?;
        if fs::symlink_metadata(&directory)
            .map_err(io)?
            .file_type()
            .is_symlink()
        {
            return Err(StoreError::IoError);
        }
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join(".lease"))
            .map_err(io)?;
        lease.try_lock().map_err(|_| StoreError::IoError)?;
        sync_directory(root)?;
        sync_directory(&directory)?;
        let namespace = Self {
            directory,
            _lease: lease,
            limits,
            uncertain: false,
            #[cfg(test)]
            fault: None,
        };
        let mut used = 0u64;
        let mut keys = 0usize;
        for entry in fs::read_dir(&namespace.directory).map_err(io)? {
            let entry = entry.map_err(io)?;
            if entry.path().extension().is_none_or(|value| value != "blob") {
                continue;
            }
            let bytes = namespace.read_file(&entry.path())?;
            let (key, blob) = decode(&bytes)?;
            if blob.bytes.len() > limits.value_bytes {
                return Err(StoreError::QuotaExceeded);
            }
            if entry.path() != namespace.path(&key)? {
                return Err(StoreError::IoError);
            }
            used = used
                .checked_add(bytes.len() as u64)
                .ok_or(StoreError::QuotaExceeded)?;
            keys = keys.checked_add(1).ok_or(StoreError::QuotaExceeded)?;
        }
        if used > limits.namespace_bytes || keys > limits.keys {
            return Err(StoreError::QuotaExceeded);
        }
        Ok(namespace)
    }

    /// A successful read resolves any previous installation uncertainty durably.
    pub fn read(&mut self, key: &str) -> Result<Option<Blob>, StoreError> {
        let path = self.path(key)?;
        if self.uncertain {
            sync_directory(&self.directory).map_err(|_| StoreError::Uncertain)?;
            self.uncertain = false;
        }
        let bytes = match self.read_file(&path) {
            Ok(bytes) => bytes,
            Err(StoreError::Conflict) => return Ok(None),
            Err(error) => return Err(error),
        };
        let (stored_key, blob) = decode(&bytes)?;
        if stored_key != key {
            return Err(StoreError::IoError);
        }
        if blob.bytes.len() > self.limits.value_bytes {
            return Err(StoreError::QuotaExceeded);
        }
        Ok(Some(blob))
    }

    /// Enumerate bounded, validated keys without exposing encoded value buffers.
    /// Internal typed stores use this to validate every retained record on opening.
    pub(super) fn keys(&mut self) -> Result<Vec<String>, StoreError> {
        if self.uncertain {
            sync_directory(&self.directory).map_err(|_| StoreError::Uncertain)?;
            self.uncertain = false;
        }
        let mut keys = Vec::new();
        for entry in fs::read_dir(&self.directory).map_err(io)? {
            let entry = entry.map_err(io)?;
            if entry.path().extension().is_none_or(|value| value != "blob") {
                continue;
            }
            let (key, _) = decode(&self.read_file(&entry.path())?)?;
            if entry.path() != self.path(&key)? || keys.len() >= self.limits.keys {
                return Err(StoreError::IoError);
            }
            keys.push(key);
        }
        keys.sort_unstable();
        Ok(keys)
    }

    /// Inject one write boundary for internal durable-store regression tests.
    #[cfg(test)]
    pub(super) fn inject_fault(&mut self, fault: Fault) {
        self.fault = Some(fault);
    }

    /// Compare the last acknowledged revision before atomically replacing a value.
    /// Writes confirmed by this call have flushed both the file and directory.
    pub fn compare_exchange(
        &mut self,
        key: &str,
        expected: Option<u64>,
        bytes: &[u8],
    ) -> Result<u64, StoreError> {
        let path = self.path(key)?;
        if self.uncertain {
            return Err(StoreError::Uncertain);
        }
        if bytes.len() > self.limits.value_bytes {
            return Err(StoreError::QuotaExceeded);
        }
        let previous = self.read(key)?;
        if previous.as_ref().map(|blob| blob.revision) != expected {
            return Err(StoreError::Conflict);
        }
        let revision = expected
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(StoreError::QuotaExceeded)?;
        let encoded = encode(key, revision, bytes)?;
        let mut used = encoded.len() as u64;
        let mut keys = 1usize;
        for entry in fs::read_dir(&self.directory).map_err(io)? {
            let entry = entry.map_err(io)?;
            if entry.path() == path || entry.path().extension().is_none_or(|value| value != "blob")
            {
                continue;
            }
            used = used
                .checked_add(entry.metadata().map_err(io)?.len())
                .ok_or(StoreError::QuotaExceeded)?;
            keys = keys.checked_add(1).ok_or(StoreError::QuotaExceeded)?;
        }
        if used > self.limits.namespace_bytes || keys > self.limits.keys {
            return Err(StoreError::QuotaExceeded);
        }
        let temporary_path = self.directory.join(format!(
            ".put-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let mut temporary = Temporary(temporary_path);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary.0)
            .map_err(io)?;
        file.write_all(&encoded).map_err(io)?;
        file.sync_all().map_err(io)?;
        drop(file);
        #[cfg(test)]
        if self.fault == Some(Fault::BeforeRename) {
            self.fault = None;
            return Err(StoreError::IoError);
        }
        fs::rename(&temporary.0, &path).map_err(io)?;
        temporary.0 = PathBuf::new();
        self.uncertain = true;
        #[cfg(test)]
        if self.fault == Some(Fault::AfterRename) {
            self.fault = None;
            return Err(StoreError::Uncertain);
        }
        sync_directory(&self.directory).map_err(|_| StoreError::Uncertain)?;
        self.uncertain = false;
        Ok(revision)
    }

    /// Reject invalid logical keys before touching the filesystem.
    fn path(&self, key: &str) -> Result<PathBuf, StoreError> {
        if key.is_empty()
            || key.len() > MAX_KEY
            || !key
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || b"._-".contains(&value))
            || matches!(key, "." | "..")
        {
            return Err(StoreError::InvalidKey);
        }
        Ok(self
            .directory
            .join(format!("{}.blob", fingerprint(key.as_bytes()))))
    }

    /// Bound disk reads before allocating or decoding stored lengths.
    fn read_file(&self, path: &Path) -> Result<Vec<u8>, StoreError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.file_type().is_file() => return Err(StoreError::IoError),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(StoreError::Conflict);
            }
            Err(error) => return Err(io(error)),
        }
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(StoreError::Conflict);
            }
            Err(error) => return Err(io(error)),
        };
        let limit = self
            .limits
            .value_bytes
            .saturating_add(HEADER + MAX_KEY + DIGEST);
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() > limit {
            return Err(StoreError::QuotaExceeded);
        }
        Ok(bytes)
    }
}

/// Remove only this write's temporary file if installation did not consume it.
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.0);
        }
    }
}

/// Encode a self-identifying, checksummed record without trusting stored paths.
fn encode(key: &str, revision: u64, bytes: &[u8]) -> Result<Vec<u8>, StoreError> {
    let mut encoded = Vec::with_capacity(HEADER + key.len() + bytes.len() + DIGEST);
    encoded.extend_from_slice(MAGIC);
    encoded.extend_from_slice(&revision.to_le_bytes());
    encoded.extend_from_slice(
        &u16::try_from(key.len())
            .map_err(|_| StoreError::InvalidKey)?
            .to_le_bytes(),
    );
    encoded.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    encoded.extend_from_slice(key.as_bytes());
    encoded.extend_from_slice(bytes);
    let checksum = Sha256::digest(&encoded);
    encoded.extend_from_slice(&checksum);
    Ok(encoded)
}

/// Corrupt records fail reads and startup; they never become an absent value.
fn decode(bytes: &[u8]) -> Result<(String, Blob), StoreError> {
    if bytes.len() < HEADER + DIGEST || &bytes[..8] != MAGIC {
        return Err(StoreError::IoError);
    }
    let revision = u64::from_le_bytes(bytes[8..16].try_into().map_err(|_| StoreError::IoError)?);
    let key_size =
        u16::from_le_bytes(bytes[16..18].try_into().map_err(|_| StoreError::IoError)?) as usize;
    let value_size = u64::from_le_bytes(bytes[18..26].try_into().map_err(|_| StoreError::IoError)?);
    let value_size = usize::try_from(value_size).map_err(|_| StoreError::IoError)?;
    let end = HEADER
        .checked_add(key_size)
        .and_then(|size| size.checked_add(value_size))
        .ok_or(StoreError::IoError)?;
    if revision == 0
        || key_size == 0
        || key_size > MAX_KEY
        || end.checked_add(DIGEST) != Some(bytes.len())
    {
        return Err(StoreError::IoError);
    }
    if Sha256::digest(&bytes[..end])[..] != bytes[end..] {
        return Err(StoreError::IoError);
    }
    let key = String::from_utf8(bytes[HEADER..HEADER + key_size].to_vec())
        .map_err(|_| StoreError::IoError)?;
    Ok((
        key,
        Blob {
            revision,
            bytes: bytes[HEADER + key_size..end].to_vec(),
        },
    ))
}

/// Namespace names never interpret component identities or keys as paths.
fn fingerprint(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A directory flush completes the durability acknowledgement after rename.
fn sync_directory(path: &Path) -> Result<(), StoreError> {
    let path = if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    };
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(io)
}

/// Persist newly created parent entries before acknowledging any contained blob.
fn create_durable_directory(path: &Path) -> Result<(), StoreError> {
    let mut missing = Vec::new();
    let mut current = Some(path);
    while let Some(directory) = current {
        if directory.as_os_str().is_empty() || directory.try_exists().map_err(io)? {
            break;
        }
        missing.push(directory);
        current = directory.parent();
    }
    fs::create_dir_all(path).map_err(io)?;
    for directory in missing {
        sync_directory(directory)?;
        if let Some(parent) = directory.parent() {
            sync_directory(parent)?;
        }
    }
    Ok(())
}

/// Map filesystem failures to the coarse category exposed to components.
fn io(_: std::io::Error) -> StoreError {
    StoreError::IoError
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Fault {
    BeforeRename,
    AfterRename,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_directory_cannot_enter_a_component_filesystem_preopen() {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("plugins");
        fs::create_dir(&plugins).unwrap();
        for directory in [plugins.clone(), plugins.join("uncreated/nested")] {
            let registry = Registry::new(directory, Limits::default());
            assert_eq!(
                registry.validate_directory(&plugins),
                Err(StoreError::IoError)
            );
        }
        let separate = root.path().join("data/plugins");
        Registry::new(separate.clone(), Limits::default())
            .validate_directory(&plugins)
            .unwrap();
        assert!(!separate.exists());
        #[cfg(unix)]
        {
            let alias = root.path().join("alias");
            std::os::unix::fs::symlink(&plugins, &alias).unwrap();
            let registry = Registry::new(alias.join("new"), Limits::default());
            assert_eq!(
                registry.validate_directory(&plugins),
                Err(StoreError::IoError)
            );
        }
    }

    #[test]
    fn confirmed_revisions_survive_reopen_and_reject_stale_writers() {
        let root = tempfile::tempdir().unwrap();
        let mut store =
            Namespace::open(root.path(), "example.component", Limits::default()).unwrap();
        assert_eq!(store.read("state"), Ok(None));
        assert_eq!(store.compare_exchange("state", None, b"first"), Ok(1));
        assert_eq!(
            store.compare_exchange("state", None, b"lost"),
            Err(StoreError::Conflict)
        );
        assert_eq!(store.compare_exchange("state", Some(1), b"second"), Ok(2));
        drop(store);
        let mut restored =
            Namespace::open(root.path(), "example.component", Limits::default()).unwrap();
        assert_eq!(
            restored.read("state"),
            Ok(Some(Blob {
                revision: 2,
                bytes: b"second".to_vec()
            }))
        );
    }

    #[test]
    fn identities_are_isolated_and_a_namespace_has_one_owner() {
        let root = tempfile::tempdir().unwrap();
        let mut first = Namespace::open(root.path(), "../first", Limits::default()).unwrap();
        assert!(Namespace::open(root.path(), "../first", Limits::default()).is_err());
        let mut other = Namespace::open(root.path(), "second", Limits::default()).unwrap();
        first.compare_exchange("state", None, b"private").unwrap();
        assert_eq!(other.read("state"), Ok(None));
        for invalid in ["", ".", "..", "../state", "a/b", "a\\b", "\0"] {
            assert_eq!(
                first.compare_exchange(invalid, None, b"data"),
                Err(StoreError::InvalidKey)
            );
        }
    }

    #[test]
    fn corrupt_and_truncated_data_never_reset_to_missing_state() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Namespace::open(root.path(), "example", Limits::default()).unwrap();
        store.compare_exchange("state", None, b"valid").unwrap();
        let path = store.path("state").unwrap();
        let original = fs::read(&path).unwrap();
        let mut corrupt = original.clone();
        corrupt[HEADER + "state".len()] ^= 1;
        fs::write(&path, &corrupt).unwrap();
        assert_eq!(store.read("state"), Err(StoreError::IoError));
        drop(store);
        assert!(Namespace::open(root.path(), "example", Limits::default()).is_err());
        fs::write(&path, &original[..original.len() - 1]).unwrap();
        assert!(Namespace::open(root.path(), "example", Limits::default()).is_err());
    }

    #[test]
    fn rejected_write_preserves_the_acknowledged_value_and_revision() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Namespace::open(root.path(), "example", Limits::default()).unwrap();
        store.compare_exchange("state", None, b"old").unwrap();
        store.fault = Some(Fault::BeforeRename);
        assert_eq!(
            store.compare_exchange("state", Some(1), b"new"),
            Err(StoreError::IoError)
        );
        assert_eq!(
            store.read("state"),
            Ok(Some(Blob {
                revision: 1,
                bytes: b"old".to_vec()
            }))
        );
    }

    #[test]
    fn uncertain_installation_blocks_new_writes_until_a_durable_read_back() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Namespace::open(root.path(), "example", Limits::default()).unwrap();
        store.compare_exchange("state", None, b"old").unwrap();
        store.fault = Some(Fault::AfterRename);
        assert_eq!(
            store.compare_exchange("state", Some(1), b"new"),
            Err(StoreError::Uncertain)
        );
        assert_eq!(
            store.compare_exchange("state", Some(2), b"later"),
            Err(StoreError::Uncertain)
        );
        assert_eq!(
            store.read("state"),
            Ok(Some(Blob {
                revision: 2,
                bytes: b"new".to_vec()
            }))
        );
        assert_eq!(store.compare_exchange("state", Some(2), b"later"), Ok(3));
    }

    #[test]
    fn quotas_count_replacements_once_and_reject_without_consuming_revisions() {
        let root = tempfile::tempdir().unwrap();
        let limits = Limits {
            value_bytes: 5,
            namespace_bytes: 256,
            keys: 1,
        };
        let mut store = Namespace::open(root.path(), "example", limits).unwrap();
        assert_eq!(store.compare_exchange("state", None, b"first"), Ok(1));
        assert_eq!(
            store.compare_exchange("other", None, b"data"),
            Err(StoreError::QuotaExceeded)
        );
        assert_eq!(
            store.compare_exchange("state", Some(1), b"longer"),
            Err(StoreError::QuotaExceeded)
        );
        assert_eq!(store.compare_exchange("state", Some(1), b"next"), Ok(2));
        assert_eq!(
            store.read("state"),
            Ok(Some(Blob {
                revision: 2,
                bytes: b"next".to_vec()
            }))
        );
        drop(store);
        let tighter = Limits {
            value_bytes: 3,
            ..limits
        };
        assert!(Namespace::open(root.path(), "example", tighter).is_err());
    }

    #[test]
    fn installed_file_size_includes_record_overhead_and_wrong_file_names_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let limits = Limits {
            value_bytes: 10,
            namespace_bytes: (HEADER + DIGEST + 5 + 2) as u64,
            keys: 1,
        };
        let mut store = Namespace::open(root.path(), "example", limits).unwrap();
        assert_eq!(store.compare_exchange("state", None, b"ok"), Ok(1));
        assert_eq!(
            store.compare_exchange("state", Some(1), b"big"),
            Err(StoreError::QuotaExceeded)
        );
        let path = store.path("state").unwrap();
        let wrong_path = store.directory.join("incorrect.blob");
        fs::rename(path, wrong_path).unwrap();
        drop(store);
        assert!(Namespace::open(root.path(), "example", limits).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_do_not_become_component_values() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Namespace::open(root.path(), "example", Limits::default()).unwrap();
        let external = root.path().join("external");
        fs::write(&external, b"outside").unwrap();
        std::os::unix::fs::symlink(&external, store.path("state").unwrap()).unwrap();
        assert_eq!(store.read("state"), Err(StoreError::IoError));
        assert_eq!(
            store.compare_exchange("state", None, b"new"),
            Err(StoreError::IoError)
        );
        assert_eq!(fs::read(external).unwrap(), b"outside");
    }

    #[test]
    fn malformed_declared_lengths_and_overflowing_limits_fail_without_reset() {
        let root = tempfile::tempdir().unwrap();
        let limits = Limits {
            value_bytes: usize::MAX,
            ..Limits::default()
        };
        assert!(Namespace::open(root.path(), "example", limits).is_err());
        let mut store = Namespace::open(
            &root.path().join("nested/new"),
            "example",
            Limits::default(),
        )
        .unwrap();
        store.compare_exchange("state", None, b"valid").unwrap();
        let path = store.path("state").unwrap();
        let mut record = fs::read(&path).unwrap();
        record[18..26].copy_from_slice(&u64::MAX.to_le_bytes());
        let end = record.len() - DIGEST;
        let checksum = Sha256::digest(&record[..end]);
        record[end..].copy_from_slice(&checksum);
        fs::write(&path, record).unwrap();
        assert_eq!(store.read("state"), Err(StoreError::IoError));
        assert_eq!(
            store.compare_exchange("state", None, b"reset"),
            Err(StoreError::IoError)
        );
    }

    #[test]
    fn staged_component_reloads_share_the_lease_and_keep_one_revision_history() {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::new(root.path().to_owned(), Limits::default());
        let active = registry.namespace("example").unwrap();
        active
            .lock()
            .unwrap()
            .compare_exchange("state", None, b"first")
            .unwrap();
        let replacement = registry.namespace("example").unwrap();
        assert!(Arc::ptr_eq(&active, &replacement));
        assert_eq!(
            replacement
                .lock()
                .unwrap()
                .compare_exchange("state", Some(1), b"next"),
            Ok(2)
        );
        assert_eq!(
            active.lock().unwrap().read("state"),
            Ok(Some(Blob {
                revision: 2,
                bytes: b"next".to_vec()
            }))
        );
        drop(active);
        drop(replacement);
        let reopened = registry.namespace("example").unwrap();
        assert_eq!(
            reopened.lock().unwrap().read("state"),
            Ok(Some(Blob {
                revision: 2,
                bytes: b"next".to_vec()
            }))
        );
        let other = registry.namespace("another.component").unwrap();
        assert_eq!(other.lock().unwrap().read("state"), Ok(None));
    }
}
