//! Typed component storage imports, independent of world and player resources.

use std::time::Instant;

use super::{HostContext, wit_bindings};
use crate::plugin::storage::{Namespace, SharedNamespace, StoreError};
use wit_bindings::unastar::plugin::storage as wit;

impl HostContext {
    /// Keep the lease alive for this context, sharing it during staged reloads.
    fn storage_namespace(&mut self) -> Result<SharedNamespace, StoreError> {
        if let Some(namespace) = &self.storage_namespace {
            return Ok(namespace.clone());
        }
        let namespace = self
            .storage_registry
            .lock()
            .map_err(|_| StoreError::IoError)?
            .namespace(&self.plugin_id.0)?;
        self.storage_namespace = Some(namespace.clone());
        Ok(namespace)
    }

    /// Bound storage imports without renewing fuel or excluding ordinary getters.
    fn storage_call<R>(
        &mut self,
        may_write: bool,
        call: impl FnOnce(&mut Namespace) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        let started = self
            .io_budget
            .begin(Instant::now())
            .ok_or(StoreError::QuotaExceeded)?;
        let result = self.storage_namespace().and_then(|namespace| {
            let mut namespace = namespace.lock().map_err(|_| StoreError::IoError)?;
            call(&mut namespace)
        });
        if !self.io_budget.finish(started, Instant::now()) {
            if may_write && matches!(&result, Ok(_) | Err(StoreError::Uncertain)) {
                return Err(StoreError::Uncertain);
            }
            return result.and(Err(StoreError::QuotaExceeded));
        }
        result
    }
}

impl wit::Host for HostContext {
    fn limits(&mut self) -> wit::StorageLimits {
        let limits = self
            .storage_registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .limits();
        wit::StorageLimits {
            max_value_bytes: limits.value_bytes as u64,
            max_namespace_bytes: limits.namespace_bytes,
            max_keys: limits.keys as u64,
        }
    }

    fn read(&mut self, key: String) -> Result<Option<wit::Blob>, wit::StorageError> {
        self.storage_call(false, |namespace| namespace.read(&key))
            .map(|blob| {
                blob.map(|blob| wit::Blob {
                    revision: blob.revision,
                    bytes: blob.bytes,
                })
            })
            .map_err(error)
    }

    fn compare_exchange(
        &mut self,
        key: String,
        expected_revision: Option<u64>,
        bytes: Vec<u8>,
    ) -> Result<u64, wit::StorageError> {
        self.storage_call(true, |namespace| {
            namespace.compare_exchange(&key, expected_revision, &bytes)
        })
        .map_err(error)
    }
}

/// Error values carry no host paths or data belonging to another component.
fn error(error: StoreError) -> wit::StorageError {
    match error {
        StoreError::InvalidKey => wit::StorageError::InvalidKey,
        StoreError::QuotaExceeded => wit::StorageError::QuotaExceeded,
        StoreError::Conflict => wit::StorageError::Conflict,
        StoreError::IoError => wit::StorageError::IoError,
        StoreError::Uncertain => wit::StorageError::Uncertain,
    }
}
