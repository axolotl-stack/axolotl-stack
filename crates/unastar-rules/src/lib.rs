//! Rules engines for Bedrock pack data, shared by every consumer (decision D10).
//!
//! The crate has no ECS dependency so the runtime (`unastar`), the pack loaders
//! (`unastar-packs`, also used by build scripts) and the worldgen codegen use the
//! same parsers:
//!
//! - [`filter`]: filter groups and the vanilla filter tests behind a registry.
//!   Parsing accepts every spelling BDS accepts (readable and compact forms,
//!   operator and subject aliases) and [`filter::Filter::to_json`] writes the one
//!   canonical form the runtime decodes. Evaluation asks a caller-implemented
//!   [`filter::FilterContext`] for facts and applies subjects and operators here.
//! - [`event`]: event trees (`sequence`, `randomize`, `first_valid`, leaf
//!   actions), normalised the same way, evaluated into ordered steps that the
//!   entity layer applies in one batch.
//! - [`molang`]: one Molang lexer/parser for runtime evaluation and for the
//!   ahead-of-time worldgen compiler, with case-insensitive names and the
//!   `q.`/`v.`/`t.`/`c.` aliases resolved at parse time.
//!
//! Parse rules follow the parser-fidelity requirement of the entities research
//! (`thoughts/shared/research/2026-10-24-data-driven-entities.md` §4.7): what a
//! parser tolerates is listed next to it, and every tolerated oddity of vanilla
//! data is reported as a [`Diagnostic`] instead of being silently dropped.

pub mod event;
pub mod filter;
pub mod molang;

use std::fmt;

/// A non-fatal parse finding: BDS logs these to the content log and keeps
/// loading. `path` is a JSON pointer-like location inside the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub path: String,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// A fatal parse error: the document (or the containing definition) is rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{path}: {message}")]
pub struct ParseError {
    pub path: String,
    pub message: String,
}

impl ParseError {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

/// Collects diagnostics while parsing one document.
#[derive(Debug, Default)]
pub struct Diagnostics {
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn push(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.items.push(Diagnostic {
            path: path.into(),
            message: message.into(),
        });
    }
}
