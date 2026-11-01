//! Durable local signing keys, independently of short-lived identity assertions.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use p384::ecdsa::SigningKey;
use p384::elliptic_curve::rand_core::{OsRng, RngCore};
use p384::elliptic_curve::zeroize::Zeroizing;
use p384::pkcs8::{DecodePrivateKey, EncodePrivateKey};

use super::{Identity, IdentityError};

const MAX_KEY_BYTES: u64 = 4096;

/// A server's reusable P-384 key; no identity token or account credential is stored.
/// Debug output deliberately excludes both private and public key material.
#[derive(Clone)]
pub struct ServerSigningKey(SigningKey);

impl std::fmt::Debug for ServerSigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerSigningKey").finish_non_exhaustive()
    }
}

impl ServerSigningKey {
    /// Read PKCS#8 DER, or atomically publish a new private key if the path is absent.
    /// Existing unreadable, malformed or insecure files fail without replacement.
    /// Concurrent creators converge on the same fully written file; they never
    /// observe a partially written destination. File contents are synced before
    /// publication, and directory entries are also synced on Unix.
    pub fn load_or_create(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() || path.file_name().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "signing key requires a file path",
            ));
        }
        match Self::load(path) {
            Ok(key) => return Ok(key),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let parent = parent(path);
        create_directories(parent)?;
        let key = Self(SigningKey::random(&mut OsRng));
        let encoded = key.0.to_pkcs8_der().map_err(|_| invalid_key())?;
        let (temporary, mut file) = temporary_file(parent)?;
        file.write_all(encoded.as_bytes())?;
        file.sync_all()?;
        drop(file);
        match fs::hard_link(&temporary.0, path) {
            Ok(()) => {
                fs::remove_file(&temporary.0)?;
                sync_directory(parent)?;
                Ok(key)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // Only an initialized, synced key can win the no-replace publication.
                drop(temporary);
                Self::load(path)
            }
            Err(error) => Err(error),
        }
    }

    /// Issue a current short-lived self-signed identity using this stable key.
    /// Call for each negotiation rather than caching a signed token across restarts.
    pub fn identity(&self, domain: impl Into<String>) -> Result<Identity, IdentityError> {
        Identity::generate_server(self.0.clone(), domain)
    }

    /// Validate complete existing contents before returning any signing capability.
    fn load(path: &Path) -> io::Result<Self> {
        validate_file(&fs::metadata(path)?)?;
        let mut file = File::open(path)?;
        validate_file(&file.metadata()?)?;
        let mut bytes = Zeroizing::new(Vec::new());
        Read::by_ref(&mut file)
            .take(MAX_KEY_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_KEY_BYTES {
            return Err(invalid_key());
        }
        let key = SigningKey::from_pkcs8_der(&bytes).map_err(|_| invalid_key())?;
        sync_directory(parent(path))?;
        Ok(Self(key))
    }
}

/// Reject nonregular paths before opening them, and check the actual opened file too.
fn validate_file(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.is_file() {
        return Err(invalid_key());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "signing key must have private file permissions",
            ));
        }
    }
    Ok(())
}

/// The containing directory for both relative and absolute key paths.
fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Keep malformed private material out of error and logging text.
fn invalid_key() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "signing key is not a bounded PKCS#8 P-384 private key",
    )
}

/// Persist each newly created directory entry before publishing a key below it.
fn create_directories(path: &Path) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut directory = path;
    loop {
        match fs::metadata(directory) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(directory.to_path_buf());
                directory = parent(directory);
            }
            Err(error) => return Err(error),
        }
    }
    fs::create_dir_all(path)?;
    for created in missing {
        sync_directory(&created)?;
        sync_directory(parent(&created))?;
    }
    Ok(())
}

/// Own exactly one staging path so failures never remove another creator's file.
struct TemporaryKey(PathBuf);
impl Drop for TemporaryKey {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Create an unguessable, exclusive staging file beside its final destination.
fn temporary_file(parent: &Path) -> io::Result<(TemporaryKey, File)> {
    loop {
        let path = parent.join(format!(
            ".nethernet-key-{:016x}{:016x}.tmp",
            OsRng.next_u64(),
            OsRng.next_u64()
        ));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => {
                let temporary = TemporaryKey(path);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(fs::Permissions::from_mode(0o600))?;
                }
                return Ok((temporary, file));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

/// Sync namespace changes where the standard library supports directory handles.
fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn persisted_key_survives_reload_and_is_private_and_redacted() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/server.der");
        let first = ServerSigningKey::load_or_create(&path).unwrap();
        let bytes = fs::read(&path).unwrap();
        let second = ServerSigningKey::load_or_create(&path).unwrap();
        assert_eq!(first.0.verifying_key(), second.0.verifying_key());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(format!("{first:?}"), "ServerSigningKey { .. }");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn malformed_existing_key_is_never_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.der");
        ServerSigningKey::load_or_create(&path).unwrap();
        for bytes in [vec![], vec![0; 48], vec![7; MAX_KEY_BYTES as usize + 1]] {
            fs::write(&path, &bytes).unwrap();
            assert_eq!(
                ServerSigningKey::load_or_create(&path).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        assert!(ServerSigningKey::load_or_create("").is_err());
        assert!(ServerSigningKey::load_or_create(directory.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_and_permissive_existing_keys_fail_without_repair_or_rotation() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.der");
        ServerSigningKey::load_or_create(&path).unwrap();
        let bytes = fs::read(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            ServerSigningKey::load_or_create(&path).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        if File::open(&path).is_err() {
            assert!(ServerSigningKey::load_or_create(&path).is_err());
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn concurrent_creators_share_one_complete_key() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.der");
        let barrier = Arc::new(Barrier::new(8));
        let jobs: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    *ServerSigningKey::load_or_create(path)
                        .unwrap()
                        .0
                        .verifying_key()
                })
            })
            .collect();
        let keys: Vec<_> = jobs.into_iter().map(|job| job.join().unwrap()).collect();
        assert!(keys.iter().all(|key| key == &keys[0]));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn refreshed_assertions_keep_trust_but_do_not_reuse_expired_tokens() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.der");
        let first = ServerSigningKey::load_or_create(&path).unwrap();
        let later = ServerSigningKey::load_or_create(&path).unwrap();
        let now = super::super::unix_time().unwrap();
        let early = super::super::generate_server_at(first.0, "self".into(), now - 120).unwrap();
        let fresh = later.identity("self").unwrap();
        assert_ne!(early.token, fresh.token);
        let early_claims = super::super::parse_token(&early.token).unwrap();
        let fresh_claims = super::super::parse_token(&fresh.token).unwrap();
        assert!(early_claims.claims["exp"].as_i64().unwrap() < now);
        assert!(fresh_claims.claims["exp"].as_i64().unwrap() > now);
        assert_eq!(
            early.private_key.verifying_key(),
            fresh.private_key.verifying_key()
        );
    }
}
