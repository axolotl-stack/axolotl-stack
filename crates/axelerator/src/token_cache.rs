//! Token cache for Xbox Live authentication.
//!
//! Provides lazy-loading and automatic refresh of OAuth tokens with file-based persistence.
//!
//! # Example
//!
//! ```no_run
//! use axelerator::TokenCache;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Load or authenticate
//! let cache = TokenCache::new("token.json");
//! let token = cache.get_or_authenticate().await?;
//!
//! // Token is now ready to use
//! println!("Authenticated as: {}", token.gamertag());
//! # Ok(())
//! # }
//! ```

use anyhow::{Context, Result};
use axolotl_xbl::auth::{DeviceCodeAuth, OAuthToken, XblToken, XblTokenClient, relying_party};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Cached token data persisted to disk. Debug output never includes credentials.
#[derive(Clone, Serialize, Deserialize)]
pub struct CachedToken {
    /// OAuth token from Microsoft.
    pub oauth: OAuthToken,
    /// Unix timestamp when token was acquired.
    pub acquired_at: u64,
    /// Unix timestamp when token expires.
    pub expires_at: u64,
}

impl std::fmt::Debug for CachedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedToken")
            .field("oauth", &"[REDACTED]")
            .field("acquired_at", &self.acquired_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl CachedToken {
    /// Create a new cached token from an OAuth token.
    pub fn new(oauth: OAuthToken) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Preserve the server's access-token lifetime without overflowing.
        let expires_at = now.saturating_add(oauth.expires_in);

        Self {
            oauth,
            acquired_at: now,
            expires_at,
        }
    }

    /// Check if the access token has expired (or is about to).
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.is_expired_at(now)
    }

    fn is_expired_at(&self, now: u64) -> bool {
        // go-xsapi xal/sisu/oauth2.go:Config.TokenSource uses oauth2.ReuseTokenSource,
        // whose default OAuth expiration margin is ten seconds.
        now.saturating_add(10) > self.expires_at
    }

    /// A refresh token has no locally knowable expiration time.
    /// Matches go-xsapi xal/sisu/oauth2.go: tokenRefresher.Token.
    pub fn can_refresh(&self) -> bool {
        !self.oauth.refresh_token.is_empty()
    }
}

/// Cached XBL token (in-memory only) with proactive expiration checking.
struct CachedXblToken {
    token: XblToken,
}

impl CachedXblToken {
    fn new(token: XblToken) -> Self {
        Self { token }
    }

    /// Check if this token is still valid (not expired).
    fn is_valid(&self) -> bool {
        self.token.is_valid()
    }
}

/// Lazy-loading token cache with file persistence.
///
/// Handles:
/// - Loading tokens from disk
/// - Checking expiration
/// - Refreshing expired tokens
/// - Device code authentication for fresh tokens
/// - Saving refreshed tokens back to disk
pub struct TokenCache {
    /// Path to the cache file.
    path: PathBuf,
    /// Cached OAuth token with expiry metadata.
    oauth: RwLock<Option<CachedToken>>,
    /// Cached XBL token for default RP (Xbox Live).
    xbl: RwLock<Option<CachedXblToken>>,
    /// XBL token client - reused to share device token across all RP requests.
    xbl_client: XblTokenClient,
}

impl TokenCache {
    /// Create a new token cache backed by a file.
    ///
    /// Does not read the file immediately - use `get_or_authenticate()` to load.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            oauth: RwLock::new(None),
            xbl: RwLock::new(None),
            xbl_client: XblTokenClient::new(),
        }
    }

    /// Get or create an authenticated XBL token for the default relying party (Xbox Live).
    ///
    /// This is the main entry point. It will:
    /// 1. Return cached token if available AND not expired
    /// 2. Exchange OAuth for XBL token if not cached or expired
    /// 3. Refresh OAuth if expired but refresh token is valid
    /// 4. Do fresh device code auth only if the cache file is absent
    ///
    /// Expiration is checked proactively: one minute for XSTS, ten seconds for OAuth.
    pub async fn get_or_authenticate(&self) -> Result<XblToken> {
        // Check if we have a valid (non-expired) cached XBL token
        {
            let cached = self.xbl.read().await;
            if let Some(ref xbl) = *cached {
                if xbl.is_valid() {
                    return Ok(xbl.token.clone());
                }
                // Token expired, will refresh below
                debug!(
                    expires_at = xbl.token.expires_at,
                    "Cached XBL token expired, refreshing..."
                );
            }
        }

        // Clear expired token
        {
            let mut cached = self.xbl.write().await;
            *cached = None;
        }

        // Need to get a new XBL token
        let oauth = self.get_or_refresh_oauth().await?;

        info!("Exchanging OAuth token for XBL token...");
        let xbl_token = self
            .xbl_client
            .get_xbl_token(&oauth, Some(relying_party::XBOX_LIVE))
            .await
            .context("Failed to get XBL token")?;

        info!(
            gamertag = %xbl_token.gamertag(),
            expires_in_secs = xbl_token.seconds_until_expiry(),
            "Authenticated successfully"
        );

        // Cache the new token
        {
            let mut cached = self.xbl.write().await;
            *cached = Some(CachedXblToken::new(xbl_token.clone()));
        }

        Ok(xbl_token)
    }

    /// Get an XBL token for a specific relying party.
    ///
    /// Per-relying-party tokens are cached by the shared XBL token client.
    pub async fn get_xbl_token(&self, rp: &str) -> Result<XblToken> {
        let oauth = self.get_or_refresh_oauth().await?;

        debug!("Exchanging OAuth token for XBL token (RP: {})", rp);
        let xbl_token = self
            .xbl_client
            .get_xbl_token(&oauth, Some(rp))
            .await
            .context("Failed to get XBL token")?;

        Ok(xbl_token)
    }

    /// Resolve the URL through the title NSAL and acquire its relying-party token.
    pub async fn get_xbl_token_for_url(&self, url: &str) -> Result<XblToken> {
        let oauth = self.get_or_refresh_oauth().await?;
        self.xbl_client
            .get_xbl_token_for_url(&oauth, url)
            .await
            .context("Failed to get XBL token for service")
    }

    /// Force refresh of the cached XBL token.
    ///
    /// Use this if you encounter 401 errors that suggest token expiration.
    pub async fn force_refresh_xbl(&self) -> Result<XblToken> {
        info!("Force refreshing XBL token...");

        // Clear in-memory caches so OAuth is reloaded/refreshed too.
        {
            let mut oauth = self.oauth.write().await;
            let mut cached = self.xbl.write().await;
            *oauth = None;
            *cached = None;
            self.xbl_client.clear_cached_tokens().await;
        }

        // Get fresh token
        self.get_or_authenticate().await
    }

    /// Get or refresh the OAuth token.
    async fn get_or_refresh_oauth(&self) -> Result<OAuthToken> {
        // Check if we have a valid cached OAuth token.
        {
            let cached = self.oauth.read().await;
            if let Some(ref token) = *cached {
                if !token.is_expired() {
                    return Ok(token.oauth.clone());
                }

                debug!(
                    expires_at = token.expires_at,
                    "Cached OAuth token expired, attempting refresh..."
                );
            }
        }

        // Attempt to refresh expired in-memory OAuth token before loading from disk.
        let expired_token = {
            let cached = self.oauth.read().await;
            cached.as_ref().map(|t| t.oauth.clone())
        };

        if let Some(old_token) = expired_token {
            let refreshed = self.try_refresh(&old_token).await?;
            let mut cached = self.oauth.write().await;
            *cached = Some(CachedToken::new(refreshed.clone()));
            return Ok(refreshed);
        }

        // Need to load or create OAuth token.
        let token = self.load_or_authenticate_oauth().await?;

        // Preserve the persisted absolute expiry instead of restarting its TTL.
        let oauth = token.oauth.clone();
        {
            let mut cached = self.oauth.write().await;
            *cached = Some(token);
        }

        Ok(oauth)
    }

    /// Load OAuth from file or do fresh authentication.
    async fn load_or_authenticate_oauth(&self) -> Result<CachedToken> {
        // Try loading from file
        if let Some(token) = self.try_load_from_file().await? {
            return Ok(token);
        }

        // Only an absent cache starts fresh interactive authentication.
        self.do_device_code_auth().await.map(CachedToken::new)
    }

    /// Try to load and validate a token from the cache file.
    async fn try_load_from_file(&self) -> Result<Option<CachedToken>> {
        let data = match tokio::fs::read_to_string(&self.path).await {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).context("Failed to read token cache"),
        };
        // Serde errors can embed credential-bearing values. Retain no source.
        let cached: CachedToken = serde_json::from_str(&data)
            .map_err(|_| anyhow::anyhow!("Failed to parse token cache"))?;

        debug!(path = %self.path.display(), "Found cached token");

        // If access token is still valid, use it directly
        if !cached.is_expired() {
            info!("Using cached token (still valid)");
            return Ok(Some(cached));
        }

        // Like go-xsapi xal/sisu/oauth2.go:tokenRefresher.Token, propagate
        // refresh failures instead of falling back to interactive login.
        anyhow::ensure!(
            cached.can_refresh(),
            "Cached token expired and refresh token is not set"
        );
        info!("Token expired, attempting refresh...");
        let refreshed = self.try_refresh(&cached.oauth).await?;
        info!("Token refreshed successfully");
        Ok(Some(CachedToken::new(refreshed)))
    }

    /// Attempt to refresh an expired token.
    async fn try_refresh(&self, old_token: &OAuthToken) -> Result<OAuthToken> {
        anyhow::ensure!(
            !old_token.refresh_token.is_empty(),
            "Cached token expired and refresh token is not set"
        );
        let auth = DeviceCodeAuth::new();
        let refreshed = auth
            .refresh(&old_token.refresh_token)
            .await
            .context("Refresh failed")?;

        // Save the refreshed token
        self.save_to_file(&refreshed).await?;

        Ok(refreshed)
    }

    /// Perform fresh device code authentication.
    async fn do_device_code_auth(&self) -> Result<OAuthToken> {
        let auth = DeviceCodeAuth::new();
        let code = auth
            .start()
            .await
            .context("Failed to start device code flow")?;

        // Print user-friendly login instructions
        println!();
        println!("╔══════════════════════════════════════════════════════════╗");
        println!("║                    XBOX LIVE LOGIN                       ║");
        println!("╠══════════════════════════════════════════════════════════╣");
        println!("║                                                          ║");
        println!("║  1. Open: {:<43} ║", code.verification_uri);
        println!("║  2. Enter code: {:<37} ║", code.user_code);
        println!("║                                                          ║");
        println!("╚══════════════════════════════════════════════════════════╝");
        println!();

        info!("Waiting for user to complete login...");

        let token = auth
            .wait_for_auth(&code)
            .await
            .context("Device code authentication failed or timed out")?;

        // Save for next time
        self.save_to_file(&token).await?;

        info!(path = %self.path.display(), "Token cached for future use");
        Ok(token)
    }

    /// Save a token to the cache file.
    async fn save_to_file(&self, token: &OAuthToken) -> Result<()> {
        let cached = CachedToken::new(token.clone());
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || save_securely(&path, &cached))
            .await
            .context("Token cache writer failed")??;

        debug!(path = %self.path.display(), "Token saved to cache");
        Ok(())
    }

    /// Get the path to the cache file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Clear all cached tokens (memory only, not the file).
    /// Next call to `get_or_authenticate` will reload from file.
    pub async fn clear(&self) {
        let mut oauth = self.oauth.write().await;
        let mut xbl = self.xbl.write().await;
        *oauth = None;
        *xbl = None;
        self.xbl_client.clear_cached_tokens().await;
    }

    /// Delete the cache file.
    pub async fn delete_file(&self) -> Result<()> {
        if self.path.exists() {
            tokio::fs::remove_file(&self.path)
                .await
                .context("Failed to delete token cache")?;
            info!(path = %self.path.display(), "Token cache deleted");
        }
        Ok(())
    }
}

/// Write a new, private inode and rename it over the destination. Never truncate
/// the destination: existing readers keep the old complete JSON until reopened.
fn save_securely(path: &Path, cached: &CachedToken) -> Result<()> {
    use std::io::Write;

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temporary = parent.join(format!(".token-{}.tmp", uuid::Uuid::new_v4()));
    let mut file =
        create_private_file(&temporary).context("Failed to create private token cache")?;
    struct RemoveTemporary(PathBuf);
    impl Drop for RemoveTemporary {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _cleanup = RemoveTemporary(temporary.clone());
    // The access restrictions already apply before any credential bytes exist.
    serde_json::to_writer_pretty(&mut file, cached)
        .map_err(|_| anyhow::anyhow!("Failed to serialize token cache"))?;
    file.flush().context("Failed to flush token cache")?;
    file.sync_all().context("Failed to sync token cache")?;
    drop(file);
    std::fs::rename(&temporary, path).context("Failed to replace token cache")?;
    Ok(())
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(windows)]
fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    windows_cache::create_private_file(path)
}

#[cfg(not(any(unix, windows)))]
fn create_private_file(_: &Path) -> std::io::Result<std::fs::File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Private token cache is unsupported on this platform",
    ))
}

#[cfg(windows)]
mod windows_cache {
    use std::{
        ffi::c_void,
        fs::File,
        io,
        os::windows::{
            ffi::{OsStrExt, OsStringExt},
            io::FromRawHandle,
        },
        path::Path,
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree},
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SDDL_REVISION_1,
            },
            GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        Storage::FileSystem::{CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    struct LocalAllocation(*mut c_void);
    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }

    fn current_user_sid() -> io::Result<Vec<u16>> {
        struct TokenHandle(HANDLE);
        impl Drop for TokenHandle {
            fn drop(&mut self) {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
        // TOKEN_USER contains a pointer, so use pointer-aligned storage rather
        // than a byte vector for GetTokenInformation's variable-sized result.
        unsafe {
            let mut handle = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = TokenHandle(handle);
            let mut needed = 0;
            GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut needed);
            if needed == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
            if GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let mut sid = ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
                return Err(io::Error::last_os_error());
            }
            let _sid = LocalAllocation(sid.cast());
            let mut len = 0;
            while *sid.add(len) != 0 {
                len += 1;
            }
            Ok(std::slice::from_raw_parts(sid, len).to_vec())
        }
    }

    pub(super) fn create_private_file(path: &Path) -> io::Result<File> {
        let sid = current_user_sid()?;
        // P disables inherited ACEs. The only grant is full access for the
        // current process user, even when the destination directory is public.
        let descriptor: Vec<u16> = "D:P(A;;FA;;;"
            .encode_utf16()
            .chain(sid)
            .chain(")".encode_utf16())
            .chain(Some(0))
            .collect();
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            let mut security = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                descriptor.as_ptr(),
                SDDL_REVISION_1,
                &mut security,
                ptr::null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let _security = LocalAllocation(security);
            let attributes = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: security,
                bInheritHandle: 0,
            };
            // CREATE_NEW refuses pre-existing files/reparse points; no sharing
            // permits another handle to read the temporary while it is written.
            let handle = CreateFileW(
                path.as_ptr(),
                GENERIC_WRITE | windows_sys::Win32::Storage::FileSystem::READ_CONTROL,
                0,
                &attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                ptr::null_mut(),
            );
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let file = File::from_raw_handle(handle);
            if let Err(error) = check_private_acl(handle) {
                drop(file);
                // The file is still empty if the filesystem cannot enforce ACLs.
                let _ =
                    std::fs::remove_file(std::ffi::OsString::from_wide(&path[..path.len() - 1]));
                return Err(error);
            }
            Ok(file)
        }
    }

    fn check_private_acl(handle: HANDLE) -> io::Result<()> {
        use windows_sys::Win32::Security::{
            ACCESS_ALLOWED_ACE, ACL,
            Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
            DACL_SECURITY_INFORMATION, GetAce, GetSecurityDescriptorControl, SE_DACL_PROTECTED,
        };
        let invalid = || {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Token cache requires a protected current-user-only ACL",
            )
        };
        unsafe {
            let mut descriptor = ptr::null_mut();
            let mut dacl: *mut ACL = ptr::null_mut();
            let status = GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            );
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status as i32));
            }
            let _descriptor = LocalAllocation(descriptor);
            let mut control = 0;
            let mut revision = 0;
            if GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) == 0
                || control & SE_DACL_PROTECTED == 0
                || dacl.is_null()
                || (*dacl).AceCount != 1
            {
                return Err(invalid());
            }
            let mut ace = ptr::null_mut();
            if GetAce(dacl, 0, &mut ace) == 0 {
                return Err(invalid());
            }
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            if ace.Header.AceType != 0
                || ace.Mask != windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS
            {
                return Err(invalid());
            }
            let mut sid = ptr::null_mut();
            if ConvertSidToStringSidW((&ace.SidStart as *const u32).cast_mut().cast(), &mut sid)
                == 0
            {
                return Err(io::Error::last_os_error());
            }
            let _sid = LocalAllocation(sid.cast());
            let mut length = 0;
            while *sid.add(length) != 0 {
                length += 1;
            }
            if std::slice::from_raw_parts(sid, length) != current_user_sid()? {
                return Err(invalid());
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn assert_private_acl(path: &Path) {
        use std::os::windows::io::AsRawHandle;
        let file = File::open(path).unwrap();
        check_private_acl(file.as_raw_handle()).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximum_oauth_lifetime_saturates_without_panicking() {
        let mut oauth = test_oauth("secret");
        oauth.expires_in = u64::MAX;
        let cached = CachedToken::new(oauth);
        assert_eq!(cached.expires_at, u64::MAX);
        assert!(!cached.is_expired());
    }

    #[test]
    fn oauth_expiration_uses_ten_second_margin() {
        let mut cached = CachedToken::new(test_oauth("secret"));
        cached.expires_at = 100;
        assert!(!cached.is_expired_at(89));
        assert!(!cached.is_expired_at(90));
        assert!(cached.is_expired_at(91));
        assert!(cached.is_expired_at(u64::MAX));
    }

    #[tokio::test]
    async fn malformed_cache_is_a_safe_error_not_a_cache_miss() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let contents = br#"{"oauth":"secret-sentinel"}"#;
        std::fs::write(&path, contents).unwrap();
        let error = TokenCache::new(&path)
            .try_load_from_file()
            .await
            .unwrap_err();
        assert!(error.to_string().contains("parse token cache"));
        for cause in error.chain() {
            assert!(!format!("{cause} {cause:?}").contains("secret-sentinel"));
        }
        assert_eq!(std::fs::read(&path).unwrap(), contents);
    }

    #[tokio::test]
    async fn cache_read_errors_are_not_cache_misses() {
        let dir = tempfile::tempdir().unwrap();
        // A directory is an existing but unreadable-as-file path on Unix/Windows.
        let error = TokenCache::new(dir.path())
            .try_load_from_file()
            .await
            .unwrap_err();
        assert!(error.to_string().contains("read token cache"));
    }

    #[tokio::test]
    async fn absent_cache_is_a_cache_miss() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            TokenCache::new(dir.path().join("missing.json"))
                .try_load_from_file()
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn expired_cache_without_refresh_token_returns_error_and_preserves_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let mut token = CachedToken::new(test_oauth("secret-sentinel"));
        token.expires_at = 1;
        token.oauth.refresh_token.clear();
        save_securely(&path, &token).unwrap();
        let original = std::fs::read(&path).unwrap();
        let error = TokenCache::new(&path)
            .try_load_from_file()
            .await
            .unwrap_err();
        assert!(!format!("{error:#} {error:?}").contains("secret-sentinel"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[tokio::test]
    async fn failed_in_memory_refresh_preserves_cache_without_disk_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        save_securely(&path, &CachedToken::new(test_oauth("disk-token"))).unwrap();
        let original = std::fs::read(&path).unwrap();
        let cache = TokenCache::new(&path);
        let mut expired = CachedToken::new(test_oauth("secret-sentinel"));
        expired.expires_at = 1;
        expired.oauth.refresh_token.clear();
        *cache.oauth.write().await = Some(expired);
        let error = cache.get_or_refresh_oauth().await.unwrap_err();
        assert!(!format!("{error:#} {error:?}").contains("secret-sentinel"));
        assert_eq!(
            cache
                .oauth
                .read()
                .await
                .as_ref()
                .unwrap()
                .oauth
                .access_token,
            "secret-sentinel"
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn private_permissions_exist_before_credentials_are_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.json");
        let file = create_private_file(&path).unwrap();
        assert_eq!(file.metadata().unwrap().len(), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
        drop(file);
        #[cfg(windows)]
        windows_cache::assert_private_acl(&path);
    }

    #[test]
    fn failed_replacement_cleans_temporary_and_preserves_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("destination");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("marker"), b"unchanged").unwrap();
        let error =
            save_securely(&path, &CachedToken::new(test_oauth("secret-sentinel"))).unwrap_err();
        assert!(!format!("{error:#}").contains("secret-sentinel"));
        assert_eq!(std::fs::read(path.join("marker")).unwrap(), b"unchanged");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn loading_preserves_original_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let mut saved = CachedToken::new(test_oauth("secret"));
        saved.expires_at -= 1200;
        save_securely(&path, &saved).unwrap();
        let cache = TokenCache::new(&path);
        cache.get_or_refresh_oauth().await.unwrap();
        assert_eq!(
            cache.oauth.read().await.as_ref().unwrap().expires_at,
            saved.expires_at
        );
    }

    fn test_oauth(secret: &str) -> OAuthToken {
        OAuthToken {
            access_token: secret.into(),
            token_type: "bearer".into(),
            refresh_token: format!("{secret}-refresh"),
            expires_in: 3600,
            user_id: None,
        }
    }

    #[test]
    fn cached_debug_redacts_credentials() {
        let token = CachedToken::new(test_oauth("secret-sentinel"));
        assert!(!format!("{token:?}").contains("secret-sentinel"));
    }

    #[test]
    fn refresh_eligibility_uses_presence_not_assumed_lifetime() {
        let mut token = CachedToken::new(test_oauth("secret"));
        token.acquired_at = 0;
        assert!(token.can_refresh());
        token.oauth.refresh_token.clear();
        assert!(!token.can_refresh());
    }

    #[tokio::test]
    async fn atomic_replacement_preserves_complete_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let cache = TokenCache::new(&path);
        cache.save_to_file(&test_oauth("first")).await.unwrap();
        let old = std::fs::File::open(&path).unwrap();
        cache.save_to_file(&test_oauth("second")).await.unwrap();
        let previous: CachedToken = serde_json::from_reader(old).unwrap();
        let current: CachedToken =
            serde_json::from_reader(std::fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(previous.oauth.access_token, "first");
        assert_eq!(current.oauth.access_token, "second");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn replacement_corrects_insecure_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        TokenCache::new(&path)
            .save_to_file(&test_oauth("secret"))
            .await
            .unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn replacement_has_only_current_user_acl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        std::fs::write(&path, b"old").unwrap();
        TokenCache::new(&path)
            .save_to_file(&test_oauth("secret"))
            .await
            .unwrap();
        windows_cache::assert_private_acl(&path);
    }

    #[test]
    fn test_cached_token_expiry() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Token that expires in 1 hour
        let valid = CachedToken {
            oauth: OAuthToken {
                access_token: "test".into(),
                token_type: "bearer".into(),
                refresh_token: "test_refresh".into(),
                expires_in: 3600,
                user_id: None,
            },
            acquired_at: now,
            expires_at: now + 3600,
        };
        assert!(!valid.is_expired());
        assert!(valid.can_refresh());

        // Token that expired 1 hour ago
        let expired = CachedToken {
            oauth: OAuthToken {
                access_token: "test".into(),
                token_type: "bearer".into(),
                refresh_token: "test_refresh".into(),
                expires_in: 3600,
                user_id: None,
            },
            acquired_at: now - 7200,
            expires_at: now - 3600,
        };
        assert!(expired.is_expired());
        assert!(expired.can_refresh()); // Refresh token still valid
    }
}
