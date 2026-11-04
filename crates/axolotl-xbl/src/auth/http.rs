//! HTTP clients built on first use, so constructors never panic.
//!
//! `reqwest::ClientBuilder::build` can fail (for example when the TLS backend
//! cannot initialise). Building in the constructor would force either a panic
//! or a fallible constructor; building on first request turns the failure into
//! an `XblError` from the call that needed the client.

use std::sync::OnceLock;
use std::time::Duration;

use crate::error::XblResult;

/// A `reqwest::Client` with a fixed timeout, built on first use.
pub(crate) struct LazyClient {
    timeout: Duration,
    client: OnceLock<reqwest::Client>,
}

impl LazyClient {
    pub(crate) const fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            client: OnceLock::new(),
        }
    }

    /// A ready client (tests and callers that already hold one).
    #[cfg(test)]
    pub(crate) fn ready(client: reqwest::Client) -> Self {
        let lazy = Self::new(Duration::ZERO);
        let _ = lazy.client.set(client);
        lazy
    }

    /// The client, built now if this is the first use. A build failure is
    /// returned (and retried on the next call) instead of panicking.
    pub(crate) fn get(&self) -> XblResult<&reqwest::Client> {
        if let Some(client) = self.client.get() {
            return Ok(client);
        }
        let client = reqwest::Client::builder().timeout(self.timeout).build()?;
        Ok(self.client.get_or_init(|| client))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_once_and_reuses_the_client() {
        let lazy = LazyClient::new(Duration::from_secs(1));
        let first = lazy.get().unwrap() as *const reqwest::Client;
        let second = lazy.get().unwrap() as *const reqwest::Client;
        assert_eq!(first, second);
    }
}
