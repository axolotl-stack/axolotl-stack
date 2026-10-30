//! Validation shared by `generate` and `check`.

use std::collections::HashMap;
use std::path::Path;

use crate::catalog::DomainRows;
use crate::status::{Entry, Status, StatusFile};

/// Problems found in one domain's status file against its generated rows.
pub fn validate(
    root: &Path,
    rows: &DomainRows,
    entries: &StatusFile,
    files: &mut TestFiles,
) -> Vec<String> {
    let domain = rows.domain;
    let mut problems = Vec::new();
    for row in &rows.rows {
        if !entries.contains_key(&row.id) {
            problems.push(format!(
                "{domain}: generated row `{}` has no status entry",
                row.id
            ));
        }
    }
    for id in unknown_ids(rows, entries) {
        problems.push(format!(
            "{domain}: status entry `{id}` names no generated row"
        ));
    }
    for (id, entry) in entries {
        for problem in validate_entry(root, entry, files) {
            problems.push(format!("{domain}: `{id}`: {problem}"));
        }
    }
    problems
}

pub fn unknown_ids<'a>(rows: &DomainRows, entries: &'a StatusFile) -> Vec<&'a str> {
    let known: std::collections::HashSet<&str> = rows.rows.iter().map(|r| r.id.as_str()).collect();
    entries
        .keys()
        .map(String::as_str)
        .filter(|id| !known.contains(id))
        .collect()
}

fn validate_entry(root: &Path, entry: &Entry, files: &mut TestFiles) -> Vec<String> {
    let mut problems = Vec::new();
    match entry.status {
        Status::Done | Status::Verified => {
            if entry.tests.is_empty() {
                problems.push(format!(
                    "`{}` needs at least one test",
                    entry.status.as_str()
                ));
            }
            if entry.owner.is_empty() {
                problems.push(format!("`{}` needs an owner", entry.status.as_str()));
            }
        }
        Status::Partial => {
            if entry.gaps.is_empty() {
                problems.push("`partial` needs `gaps` saying what is missing".to_owned());
            }
        }
        Status::NotApplicable => {
            if entry.notes.is_empty() {
                problems.push("`not_applicable` needs `notes` saying why".to_owned());
            }
        }
        Status::Missing => {}
    }
    if entry.status == Status::Verified {
        if entry.evidence.is_empty() || entry.evidence == "none" {
            problems.push("`verified` needs native `evidence`".to_owned());
        } else if !root.join(&entry.evidence).exists() {
            problems.push(format!(
                "evidence `{}` is not a path in the repository",
                entry.evidence
            ));
        }
    }
    for test in &entry.tests {
        if let Err(problem) = files.resolve(root, test) {
            problems.push(problem);
        }
    }
    for (field, text) in [
        ("gaps", &entry.gaps),
        ("notes", &entry.notes),
        ("evidence", &entry.evidence),
    ] {
        if let Some(pattern) = forbidden_provenance(text) {
            problems.push(format!(
                "`{field}` contains `{pattern}`; describe behaviour, not engine internals"
            ));
        }
    }
    problems
}

/// Text that suggests engine internals rather than observed behaviour
/// (the repository comment rule): raw addresses, recovered function names.
pub(crate) fn forbidden_provenance(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase();
    ["fun_", "0x14", "decompil", "disassembl", "rva "]
        .into_iter()
        .find(|pattern| lower.contains(pattern))
}

/// Cache of test files read while resolving `tests` references.
#[derive(Default)]
pub struct TestFiles {
    contents: HashMap<String, Option<String>>,
}

impl TestFiles {
    /// `path` must be a file in the repository; `path::name` must also define
    /// `fn name` in it.
    fn resolve(&mut self, root: &Path, reference: &str) -> Result<(), String> {
        let (file, function) = match reference.split_once("::") {
            Some((file, rest)) => (file, Some(rest.rsplit("::").next().unwrap_or(rest))),
            None => (reference, None),
        };
        let content = self
            .contents
            .entry(file.to_owned())
            .or_insert_with(|| std::fs::read_to_string(root.join(file)).ok());
        let Some(content) = content else {
            return Err(format!("test file `{file}` does not exist"));
        };
        if let Some(function) = function
            && !defines_fn(content, function)
        {
            return Err(format!(
                "test `{reference}`: `fn {function}` not found in `{file}`"
            ));
        }
        Ok(())
    }
}

fn defines_fn(content: &str, name: &str) -> bool {
    let needle = format!("fn {name}");
    content.match_indices(&needle).any(|(at, _)| {
        content[at + needle.len()..]
            .chars()
            .next()
            .is_some_and(|c| c == '(' || c == '<' || c.is_whitespace())
    })
}
