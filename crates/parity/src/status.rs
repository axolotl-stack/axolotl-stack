//! Status files: `docs/parity/<domain>.toml`, one entry per generated row.
//!
//! The file is a flat TOML table keyed by row id. `parity generate` rewrites
//! it in canonical form (catalog order, family headers, one inline table per
//! row), keeping every field and adding `{ status = "missing" }` for new rows.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::Context as _;
use serde::Deserialize;

use crate::catalog::{DomainRows, Row};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Nothing implemented.
    Missing,
    /// Some behaviour exists; `gaps` says what is not.
    Partial,
    /// Implemented and covered by at least one test in the workspace.
    Done,
    /// Done, and the test compares against native evidence (a BDS or Java
    /// capture/fixture committed in the repository).
    Verified,
    /// Vanilla has it but a server never implements it (client-only packets,
    /// Education Edition content, ...); `notes` says why. Excluded from
    /// percentages.
    NotApplicable,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Missing => "missing",
            Status::Partial => "partial",
            Status::Done => "done",
            Status::Verified => "verified",
            Status::NotApplicable => "not_applicable",
        }
    }

    pub fn is_complete(self) -> bool {
        matches!(self, Status::Done | Status::Verified)
    }
}

/// One row's status entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub status: Status,
    /// Lane or person responsible.
    #[serde(default)]
    pub owner: String,
    /// `path` or `path::test_fn`, relative to the repository root.
    #[serde(default)]
    pub tests: Vec<String>,
    /// Native evidence: a committed capture/fixture path, or `none`.
    #[serde(default)]
    pub evidence: String,
    /// What is still missing.
    #[serde(default)]
    pub gaps: String,
    #[serde(default)]
    pub notes: String,
}

impl Entry {
    pub fn missing() -> Self {
        Entry {
            status: Status::Missing,
            owner: String::new(),
            tests: Vec::new(),
            evidence: String::new(),
            gaps: String::new(),
            notes: String::new(),
        }
    }
}

pub type StatusFile = BTreeMap<String, Entry>;

pub fn path(root: &Path, domain: crate::catalog::Domain) -> std::path::PathBuf {
    root.join("docs/parity")
        .join(format!("{}.toml", domain.name()))
}

/// Reads a status file; an absent file is an empty one.
pub fn load(path: &Path) -> anyhow::Result<StatusFile> {
    if !path.exists() {
        return Ok(StatusFile::new());
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Canonical text of a status file for `rows`; every row must have an entry.
pub fn render(rows: &DomainRows, entries: &StatusFile) -> String {
    let domain = rows.domain;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Parity status: {} ({}).",
        domain.title(),
        domain.name()
    );
    for line in wrap(domain.row_meaning(), 78) {
        let _ = writeln!(out, "# {line}");
    }
    out.push_str(
        "#\n\
         # Rows are generated from vanilla data by `cargo run -p parity -- generate`, which\n\
         # also rewrites this file in canonical form. Edit only the values; comments are\n\
         # regenerated. Schema and rules: docs/parity/README.md.\n",
    );
    let mut family = None;
    for row in &rows.rows {
        if family != Some(&row.family) {
            family = Some(&row.family);
            let count = rows.rows.iter().filter(|r| r.family == row.family).count();
            let _ = write!(out, "\n# {} ({count})\n", row.family);
        }
        let entry = &entries[&row.id];
        out.push_str(&render_line(row, entry));
        out.push('\n');
    }
    out
}

fn render_line(row: &Row, entry: &Entry) -> String {
    let mut fields = vec![format!("status = {}", quote(entry.status.as_str()))];
    if !entry.owner.is_empty() {
        fields.push(format!("owner = {}", quote(&entry.owner)));
    }
    if !entry.tests.is_empty() {
        let tests: Vec<String> = entry.tests.iter().map(|t| quote(t)).collect();
        fields.push(format!("tests = [{}]", tests.join(", ")));
    }
    if !entry.evidence.is_empty() {
        fields.push(format!("evidence = {}", quote(&entry.evidence)));
    }
    if !entry.gaps.is_empty() {
        fields.push(format!("gaps = {}", quote(&entry.gaps)));
    }
    if !entry.notes.is_empty() {
        fields.push(format!("notes = {}", quote(&entry.notes)));
    }
    let mut line = format!("{} = {{ {} }}", quote(&row.id), fields.join(", "));
    if let Some(label) = &row.label {
        let _ = write!(line, "  # {label}");
    }
    line
}

fn quote(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
