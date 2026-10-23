//! Deterministic `OpenIdProvider` cache tests on Tokio's paused clock.

use super::security_tests::{claims, client_data, key, sign_with, verifier_for};
use crate::auth::Trust;
use crate::auth::openid::{KeySetSource, OpenIdProvider, OpenIdVerifier, RefreshPolicy};
use crate::error::AuthError;
use futures::future::{BoxFuture, join_all};
use p384::SecretKey;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Semaphore;

const GATE_OPEN: usize = Semaphore::MAX_PERMITS;

/// Published keys: `(key, kid)`.
type Keys = Vec<(SecretKey, Option<&'static str>)>;

/// Key-set source with a fetch counter, a latency, a gate and swappable keys
/// (`None` makes fetches fail).
#[derive(Clone)]
struct TestSource {
    fetches: Arc<AtomicUsize>,
    keys: Arc<Mutex<Option<Keys>>>,
    latency: Duration,
    gate: Arc<Semaphore>,
}

impl TestSource {
    fn new(keys: Keys, latency: Duration) -> Self {
        Self {
            fetches: Arc::default(),
            keys: Arc::new(Mutex::new(Some(keys))),
            latency,
            gate: Arc::new(Semaphore::new(GATE_OPEN)),
        }
    }
    fn fetches(&self) -> usize {
        self.fetches.load(Ordering::SeqCst)
    }
    fn set_keys(&self, keys: Option<Keys>) {
        *self.keys.lock() = keys;
    }
}

impl KeySetSource for TestSource {
    fn fetch(&self) -> BoxFuture<'_, Result<OpenIdVerifier, AuthError>> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            drop(self.gate.acquire().await.unwrap());
            tokio::time::sleep(self.latency).await;
            let keys = self.keys.lock().clone();
            let keys = keys.ok_or_else(|| AuthError::ProviderUnavailable("test outage".into()))?;
            let keys: Vec<_> = keys.iter().map(|(key, kid)| (key, *kid)).collect();
            Ok(verifier_for(&keys))
        })
    }
}

struct Login {
    token: String,
    client: String,
}

fn login(provider_key: &SecretKey, kid: Option<&str>) -> Login {
    let identity = key(1);
    Login {
        token: sign_with(provider_key, kid, &claims(&identity)),
        client: client_data(&identity),
    }
}

async fn validate(provider: &OpenIdProvider, login: &Login) -> Result<(), AuthError> {
    provider
        .validate(&login.token, &login.client)
        .await
        .map(|login| assert_eq!(login.identity.trust(), Trust::AwaitingKeyProof))
}

#[tokio::test(start_paused = true)]
async fn concurrent_cold_logins_share_one_fetch() {
    let provider_key = key(2);
    let source = TestSource::new(
        vec![(provider_key.clone(), None)],
        Duration::from_millis(100),
    );
    let provider = OpenIdProvider::new(source.clone(), RefreshPolicy::default());
    let login = login(&provider_key, None);
    let started = tokio::time::Instant::now();
    let results = join_all((0..32).map(|_| validate(&provider, &login))).await;
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    assert_eq!(source.fetches(), 1);
    // One fetch latency in total, not one per login.
    assert_eq!(started.elapsed(), Duration::from_millis(100));
}

#[tokio::test(start_paused = true)]
async fn stale_keys_keep_serving_while_one_caller_refreshes() {
    let provider_key = key(2);
    let source = TestSource::new(vec![(provider_key.clone(), None)], Duration::ZERO);
    let policy = RefreshPolicy::default();
    let provider = Arc::new(OpenIdProvider::new(source.clone(), policy));
    let login = Arc::new(login(&provider_key, None));
    validate(&provider, &login).await.unwrap();
    assert_eq!(source.fetches(), 1);

    tokio::time::advance(policy.max_age).await;
    // Hang the next fetch until released.
    let permits = source.gate.available_permits();
    source.gate.forget_permits(permits);
    let leader = tokio::spawn({
        let (provider, login) = (provider.clone(), login.clone());
        async move { validate(&provider, &login).await }
    });
    while source.fetches() < 2 {
        tokio::task::yield_now().await;
    }
    // The refresh is in flight and blocked; other logins must not wait for it.
    let others = tokio::time::timeout(
        Duration::from_secs(1),
        join_all((0..32).map(|_| validate(&provider, &login))),
    )
    .await
    .expect("logins waited for the in-flight refresh");
    assert!(others.iter().all(Result::is_ok));
    assert_eq!(source.fetches(), 2);

    source.gate.add_permits(GATE_OPEN);
    leader.await.unwrap().unwrap();
    assert_eq!(source.fetches(), 2);
}

#[tokio::test(start_paused = true)]
async fn unknown_kid_refreshes_once_per_interval() {
    let (key_a, key_b, key_c) = (key(2), key(3), key(4));
    let source = TestSource::new(vec![(key_a.clone(), Some("a"))], Duration::from_millis(50));
    let policy = RefreshPolicy::default();
    let provider = OpenIdProvider::new(source.clone(), policy);
    validate(&provider, &login(&key_a, Some("a")))
        .await
        .unwrap();
    assert_eq!(source.fetches(), 1);

    // Rotation: the issuer now publishes "b".
    source.set_keys(Some(vec![
        (key_a.clone(), Some("a")),
        (key_b.clone(), Some("b")),
    ]));
    let rotated = login(&key_b, Some("b"));
    // Within the interval an unknown kid does not refresh (gophertunnel jwks.go).
    assert!(validate(&provider, &rotated).await.is_err());
    assert_eq!(source.fetches(), 1);

    tokio::time::advance(policy.unknown_kid_interval).await;
    let results = join_all((0..16).map(|_| validate(&provider, &rotated))).await;
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    assert_eq!(
        source.fetches(),
        2,
        "concurrent unknown-kid logins share one refresh"
    );

    // A forged kid right after a refresh cannot force another fetch.
    for _ in 0..8 {
        assert!(
            validate(&provider, &login(&key_c, Some("c")))
                .await
                .is_err()
        );
    }
    assert_eq!(source.fetches(), 2);
    validate(&provider, &login(&key_a, Some("a")))
        .await
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn refresh_failure_keeps_serving_cached_keys_until_max_stale() {
    let provider_key = key(2);
    let source = TestSource::new(vec![(provider_key.clone(), None)], Duration::ZERO);
    let policy = RefreshPolicy::default();
    let provider = OpenIdProvider::new(source.clone(), policy);
    let login = login(&provider_key, None);
    validate(&provider, &login).await.unwrap();

    source.set_keys(None);
    tokio::time::advance(policy.max_age).await;
    validate(&provider, &login).await.expect("stale keys serve");
    assert_eq!(source.fetches(), 2);
    validate(&provider, &login).await.expect("stale keys serve");
    assert_eq!(source.fetches(), 2, "failed refresh is backed off");
    tokio::time::advance(policy.retry_backoff).await;
    validate(&provider, &login).await.expect("stale keys serve");
    assert_eq!(source.fetches(), 3);

    tokio::time::advance(policy.max_stale).await;
    assert!(matches!(
        validate(&provider, &login).await,
        Err(AuthError::ProviderUnavailable(_))
    ));
    source.set_keys(Some(vec![(provider_key.clone(), None)]));
    tokio::time::advance(policy.retry_backoff).await;
    validate(&provider, &login).await.expect("recovered");
}

#[tokio::test(start_paused = true)]
async fn failed_cold_fetch_is_shared_and_backed_off() {
    let provider_key = key(2);
    let source = TestSource::new(vec![], Duration::from_millis(10));
    source.set_keys(None);
    let policy = RefreshPolicy::default();
    let provider = OpenIdProvider::new(source.clone(), policy);
    let login = login(&provider_key, None);
    let results = join_all((0..16).map(|_| validate(&provider, &login))).await;
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(AuthError::ProviderUnavailable(_))))
    );
    assert_eq!(source.fetches(), 1);
    assert!(validate(&provider, &login).await.is_err());
    assert_eq!(source.fetches(), 1);
    source.set_keys(Some(vec![(provider_key.clone(), None)]));
    tokio::time::advance(policy.retry_backoff).await;
    validate(&provider, &login).await.unwrap();
    assert_eq!(source.fetches(), 2);
}

#[tokio::test(start_paused = true)]
async fn fetch_timeout_bounds_a_hung_source() {
    let provider_key = key(2);
    let source = TestSource::new(vec![(provider_key.clone(), None)], Duration::ZERO);
    let permits = source.gate.available_permits();
    source.gate.forget_permits(permits);
    let policy = RefreshPolicy::default();
    let provider = OpenIdProvider::new(source.clone(), policy);
    let started = tokio::time::Instant::now();
    assert!(matches!(
        validate(&provider, &login(&provider_key, None)).await,
        Err(AuthError::ProviderUnavailable(_))
    ));
    assert_eq!(started.elapsed(), policy.fetch_timeout);
}

/// The fetch runs in its own task: cancelling the login that started it
/// neither aborts the fetch nor loses its outcome, and a failed outcome still
/// arms the retry backoff.
#[tokio::test(start_paused = true)]
async fn cancelled_login_does_not_abort_or_forget_the_fetch() {
    let provider_key = key(2);
    let policy = RefreshPolicy::default();
    let login = login(&provider_key, None);
    for (keys, expect_ok) in [
        (None, false),
        (Some(vec![(provider_key.clone(), None)]), true),
    ] {
        let source = TestSource::new(vec![], Duration::from_millis(10));
        source.set_keys(keys);
        let provider = OpenIdProvider::new(source.clone(), policy);
        // Cancel the triggering login while its fetch is in flight.
        let cancelled =
            tokio::time::timeout(Duration::from_millis(5), validate(&provider, &login)).await;
        assert!(cancelled.is_err(), "login should still be waiting");
        assert_eq!(source.fetches(), 1);
        // The fetch completes without any waiter; the next login shares its
        // recorded outcome instead of fetching again.
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(validate(&provider, &login).await.is_ok(), expect_ok);
        assert_eq!(source.fetches(), 1, "outcome or backoff was lost");
    }
}

/// Panics on its first fetch, then serves `keys`.
struct PanicsOnce {
    fetches: AtomicUsize,
    keys: Keys,
}

impl KeySetSource for Arc<PanicsOnce> {
    fn fetch(&self) -> BoxFuture<'_, Result<OpenIdVerifier, AuthError>> {
        let first = self.fetches.fetch_add(1, Ordering::SeqCst) == 0;
        Box::pin(async move {
            assert!(!first, "key source failure");
            let keys: Vec<_> = self.keys.iter().map(|(key, kid)| (key, *kid)).collect();
            Ok(verifier_for(&keys))
        })
    }
}

#[tokio::test(start_paused = true)]
async fn panicking_fetch_frees_the_refresh_slot() {
    let provider_key = key(2);
    let source = Arc::new(PanicsOnce {
        fetches: AtomicUsize::new(0),
        keys: vec![(provider_key.clone(), None)],
    });
    let policy = RefreshPolicy::default();
    let provider = OpenIdProvider::new(source.clone(), policy);
    let login = login(&provider_key, None);

    assert!(matches!(
        validate(&provider, &login).await,
        Err(AuthError::ProviderUnavailable(_))
    ));
    tokio::time::advance(policy.retry_backoff).await;
    validate(&provider, &login)
        .await
        .expect("a new fetch after the panicked one");
    assert_eq!(source.fetches.load(Ordering::SeqCst), 2);
}
