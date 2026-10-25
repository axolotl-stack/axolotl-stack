//! Error type shared by every ingestion and emission phase.
//!
//! Every error carries enough context (file, pack, identifier) to fix the
//! input without a debugger: the build script prints it verbatim.

use std::path::PathBuf;

/// Result alias for the codegen crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Ingestion or emission failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file or directory could not be read.
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The shared pack reader rejected an input.
    #[error(transparent)]
    Packs(#[from] unastar_packs::Error),
    /// A JSON document could not be parsed.
    #[error("{origin}: invalid JSON: {source}")]
    Json {
        origin: String,
        #[source]
        source: serde_json::Error,
    },
    /// An NBT document is malformed.
    #[error("{origin}: malformed NBT: {reason}")]
    Nbt { origin: String, reason: String },
    /// The data is well formed but violates a schema or cross-reference rule.
    #[error("{origin}: {reason}")]
    Data { origin: String, reason: String },
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub(crate) fn data(origin: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Data {
            origin: origin.into(),
            reason: reason.into(),
        }
    }
}
