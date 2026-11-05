//! HTTP clients built on first use, so constructors never panic.
//!
//! `reqwest::ClientBuilder::build` (and `reqwest::Client::new`, which panics on
//! the same failure) can fail, for example when the TLS backend cannot
//! initialise. Building in a constructor would force either a panic or a
//! fallible constructor; building on first request turns the failure into
//! [`XblError::HttpClientBuild`] from the call that needed the client, distinct
//! from a network failure (`XblError::Http`).

use std::sync::OnceLock;
use std::time::Duration;

use crate::error::{XblError, XblResult};

/// A `reqwest::Client`, built on first use with an optional request timeout.
#[derive(Debug, Clone)]
pub(crate) struct LazyClient {
    timeout: Option<Duration>,
    client: OnceLock<reqwest::Client>,
}

impl LazyClient {
    /// A client whose requests time out after `timeout`.
    pub(crate) const fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout: Some(timeout),
            client: OnceLock::new(),
        }
    }

    /// A client with reqwest's defaults (no request timeout), as
    /// `reqwest::Client::new()` builds it.
    pub(crate) const fn without_timeout() -> Self {
        Self {
            timeout: None,
            client: OnceLock::new(),
        }
    }

    /// A ready client (tests and callers that supply their own).
    pub(crate) fn ready(client: reqwest::Client) -> Self {
        let lazy = Self::without_timeout();
        let _ = lazy.client.set(client);
        lazy
    }

    /// The client, built now if this is the first use. A build failure is
    /// returned as [`XblError::HttpClientBuild`] (and retried on the next call)
    /// instead of panicking.
    pub(crate) fn load(&self) -> XblResult<&reqwest::Client> {
        if let Some(client) = self.client.get() {
            return Ok(client);
        }
        let mut builder = reqwest::Client::builder();
        if let Some(timeout) = self.timeout {
            builder = builder.timeout(timeout);
        }
        let client = builder.build().map_err(build_error)?;
        Ok(self.client.get_or_init(|| client))
    }
}

/// The error for a `reqwest::Client` that could not be built. The reqwest error
/// is dropped: like every `XblError` it must not carry remote or local details.
pub(crate) fn build_error(_error: reqwest::Error) -> XblError {
    XblError::HttpClientBuild
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_once_and_reuses_the_client() {
        for lazy in [
            LazyClient::with_timeout(Duration::from_secs(1)),
            LazyClient::without_timeout(),
        ] {
            let first = lazy.load().unwrap() as *const reqwest::Client;
            let second = lazy.load().unwrap() as *const reqwest::Client;
            assert_eq!(first, second);
        }
    }

    /// A client build failure is its own error, not a network failure: no
    /// status, not `XblError::Http`, and a message that says what failed.
    #[test]
    fn a_build_failure_is_not_reported_as_a_network_error() {
        // Any real reqwest error stands in for a failed TLS initialisation,
        // which cannot be forced portably.
        let reqwest_error = reqwest::Client::builder()
            .build()
            .unwrap()
            .get("not a url")
            .build()
            .unwrap_err();
        let error = build_error(reqwest_error);
        assert!(matches!(error, XblError::HttpClientBuild));
        assert_eq!(error.status(), None);
        assert!(error.to_string().contains("HTTP client"), "{error}");
    }
}
