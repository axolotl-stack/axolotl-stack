//! Validation shared by `generate` and `check`.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

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

pub(crate) fn validate_entry(root: &Path, entry: &Entry, files: &mut TestFiles) -> Vec<String> {
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
        } else {
            match files.is_tracked(root, &entry.evidence) {
                Ok(true) => {}
                Ok(false) => problems.push(format!(
                    "evidence `{}` is not tracked by git; commit it (an untracked or \
                     ignored file is not evidence anyone else can check)",
                    entry.evidence
                )),
                // Without git no evidence can be checked: say so once, not
                // once per verified row.
                Err(Some(problem)) => problems.push(problem),
                Err(None) => {}
            }
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

/// Cache of test files read while resolving `tests` references, and of the
/// paths git tracks (for `verified` evidence).
#[derive(Default)]
pub struct TestFiles {
    contents: HashMap<String, Option<String>>,
    tracked: Option<Result<Tracked, String>>,
    /// Whether the `git ls-files` failure has been reported already.
    untracked_reported: bool,
}

/// Paths in the git index: tracked files, and submodule roots (whose
/// contents are pinned by the superproject commit).
#[derive(Default)]
struct Tracked {
    files: HashSet<String>,
    submodules: Vec<String>,
}

impl Tracked {
    fn list(root: &Path) -> Result<Self, String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["ls-files", "-z", "--stage"])
            .output()
            .map_err(|e| {
                format!(
                    "cannot run `git ls-files` to check `verified` evidence ({e});                      run `parity` inside a git checkout"
                )
            })?;
        if !output.status.success() {
            return Err(format!(
                "`git ls-files` failed while checking `verified` evidence ({});                  run `parity` inside a git checkout",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(Self::parse(&String::from_utf8_lossy(&output.stdout)))
    }

    /// Parses `git ls-files -z --stage` output: `<mode> <hash> <stage>\t<path>\0`.
    fn parse(stdout: &str) -> Self {
        let mut tracked = Tracked::default();
        for record in stdout.split('\0').filter(|r| !r.is_empty()) {
            let Some((meta, path)) = record.split_once('\t') else {
                continue;
            };
            if meta.starts_with("160000 ") {
                tracked.submodules.push(path.to_owned());
            } else {
                tracked.files.insert(path.to_owned());
            }
        }
        tracked
    }

    /// Whether `path` is a tracked file, a directory holding one, or lies
    /// inside a submodule.
    fn contains(&self, path: &str) -> bool {
        let path = path.replace('\\', "/");
        let path = path.trim_end_matches('/');
        let under = |dir: &str, path: &str| {
            path.strip_prefix(dir)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        };
        self.files.contains(path)
            || self.files.iter().any(|file| under(path, file))
            || self.submodules.iter().any(|dir| under(dir, path))
    }
}

impl TestFiles {
    /// Whether git tracks `path` (relative to `root`); `git ls-files` runs once.
    /// If it cannot run, the first call returns the problem and later calls
    /// `Err(None)`, so the failure is reported once per run.
    fn is_tracked(&mut self, root: &Path, path: &str) -> Result<bool, Option<String>> {
        match self.tracked.get_or_insert_with(|| Tracked::list(root)) {
            Ok(tracked) => Ok(tracked.contains(path)),
            Err(problem) if !self.untracked_reported => {
                self.untracked_reported = true;
                Err(Some(problem.clone()))
            }
            Err(_) => Err(None),
        }
    }

    /// `path` must be a file in the repository; `path::name` must also define
    /// a `#[test]` function `name` in it.
    pub(crate) fn resolve(&mut self, root: &Path, reference: &str) -> Result<(), String> {
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
                "test `{reference}`: no `#[test]` function `{function}` in `{file}`"
            ));
        }
        Ok(())
    }
}

/// Whether `content` defines `fn name` as a test: the `fn` item starts its
/// line (after `pub`/`async`/`unsafe`), so text in comments and strings does
/// not count, and the attributes directly above it include `#[test]` or an
/// async runtime's test attribute (`#[tokio::test]`, `#[tokio::test(...)]`).
fn defines_fn(content: &str, name: &str) -> bool {
    let mut offset = 0;
    content.split_inclusive('\n').any(|line| {
        let start = offset;
        offset += line.len();
        declares_fn(line.trim_start(), name)
            && attributes_above(&content[..start])
                .iter()
                .any(|attribute| is_test_attribute(attribute))
    })
}

fn declares_fn(line: &str, name: &str) -> bool {
    let mut rest = line;
    for qualifier in ["pub(crate) ", "pub(super) ", "pub ", "async ", "unsafe "] {
        if let Some(stripped) = rest.strip_prefix(qualifier) {
            rest = stripped.trim_start();
        }
    }
    rest.strip_prefix("fn ")
        .and_then(|rest| rest.trim_start().strip_prefix(name))
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c == '(' || c == '<' || c.is_whitespace())
}

/// The outer attributes directly above an item whose text starts after
/// `before`, nearest first, skipping line comments and doc comments between
/// them. Attributes may span lines.
fn attributes_above(before: &str) -> Vec<&str> {
    let mut attributes = Vec::new();
    let mut rest = before.trim_end();
    loop {
        let last_line = rest.rsplit('\n').next().unwrap_or(rest);
        if last_line.trim_start().starts_with("//") {
            rest = rest[..rest.len() - last_line.len()].trim_end();
            continue;
        }
        if !rest.ends_with(']') {
            break;
        }
        // Find the `#[` that opens the attribute ending here.
        let mut depth = 0usize;
        let mut open = None;
        for (at, c) in rest.char_indices().rev() {
            match c {
                ']' => depth += 1,
                '[' => {
                    depth -= 1;
                    if depth == 0 {
                        open = Some(at);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(at) = open.filter(|&at| rest[..at].ends_with('#')) else {
            break;
        };
        attributes.push(&rest[at - 1..]);
        rest = rest[..at - 1].trim_end();
    }
    attributes
}

fn is_test_attribute(attribute: &str) -> bool {
    let inner: String = attribute.chars().filter(|c| !c.is_whitespace()).collect();
    inner == "#[test]" || inner == "#[tokio::test]" || inner.starts_with("#[tokio::test(")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway directory, removed by the caller.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("parity-check-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, path: &str, text: &str) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn entry(status: Status) -> Entry {
        Entry {
            status,
            ..Entry::missing()
        }
    }

    const TESTS: &str = "\
        // fn mentioned_in_a_comment() is not a test\n\
        fn helper() {}\n\
        \n\
        #[test]\n\
        fn plain_test() {}\n\
        \n\
        /// Docs and other attributes may sit between.\n\
        #[tokio::test(flavor = \"multi_thread\")]\n\
        #[cfg_attr(\n\
            miri,\n\
            ignore\n\
        )]\n\
        async fn async_test() {}\n\
        \n\
        #[test]\n\
        pub fn generic_test<T>() {}\n";

    #[test]
    fn test_references_must_name_test_functions() {
        assert!(defines_fn(TESTS, "plain_test"));
        assert!(defines_fn(TESTS, "async_test"));
        assert!(defines_fn(TESTS, "generic_test"));
        assert!(!defines_fn(TESTS, "helper"), "a non-test fn");
        assert!(!defines_fn(TESTS, "mentioned_in_a_comment"), "comment text");
        assert!(!defines_fn(TESTS, "plain"), "a prefix of a test name");
        assert!(!defines_fn("let s = \"fn quoted() {}\";", "quoted"));
    }

    #[test]
    fn resolve_checks_the_file_and_the_function() {
        let root = scratch("resolve");
        write(&root, "tests/a.rs", TESTS);
        let mut files = TestFiles::default();
        assert_eq!(files.resolve(&root, "tests/a.rs"), Ok(()));
        assert_eq!(files.resolve(&root, "tests/a.rs::plain_test"), Ok(()));
        assert_eq!(
            files.resolve(&root, "tests/a.rs::module::async_test"),
            Ok(())
        );
        let helper = files.resolve(&root, "tests/a.rs::helper").unwrap_err();
        assert!(
            helper.contains("no `#[test]` function `helper`"),
            "{helper}"
        );
        let missing = files.resolve(&root, "tests/b.rs::plain_test").unwrap_err();
        assert!(missing.contains("does not exist"), "{missing}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn forbidden_provenance_flags_engine_internals_case_insensitively() {
        assert_eq!(forbidden_provenance("see FUN_1234"), Some("fun_"));
        assert_eq!(forbidden_provenance("at 0x1400a0"), Some("0x14"));
        assert_eq!(forbidden_provenance("Decompiled from"), Some("decompil"));
        assert_eq!(
            forbidden_provenance("from the disassembly"),
            Some("disassembl")
        );
        assert_eq!(forbidden_provenance("RVA 1234"), Some("rva "));
        assert_eq!(
            forbidden_provenance("matches the BDS 1.26.51 capture"),
            None
        );
        assert_eq!(forbidden_provenance(""), None);
    }

    #[test]
    fn validate_entry_requires_each_status_fields() {
        let root = scratch("entry");
        let mut files = TestFiles::default();
        let mut problems = |entry: &Entry| validate_entry(&root, entry, &mut files);

        assert!(problems(&entry(Status::Missing)).is_empty());
        let done = problems(&entry(Status::Done));
        assert!(
            done.iter().any(|p| p.contains("needs at least one test")),
            "{done:?}"
        );
        assert!(
            done.iter().any(|p| p.contains("needs an owner")),
            "{done:?}"
        );
        let partial = problems(&entry(Status::Partial));
        assert!(partial.iter().any(|p| p.contains("`gaps`")), "{partial:?}");
        let not_applicable = problems(&entry(Status::NotApplicable));
        assert!(not_applicable.iter().any(|p| p.contains("`notes`")));

        let verified = Entry {
            evidence: "none".to_owned(),
            ..entry(Status::Verified)
        };
        assert!(
            problems(&verified)
                .iter()
                .any(|p| p.contains("native `evidence`"))
        );
        let absent = Entry {
            evidence: "captures/absent.bin".to_owned(),
            ..entry(Status::Verified)
        };
        assert!(
            problems(&absent)
                .iter()
                .any(|p| p.contains("is not a path in the repository"))
        );

        let internals = Entry {
            gaps: "port FUN_00 next".to_owned(),
            ..entry(Status::Partial)
        };
        let found = problems(&internals);
        assert!(
            found.iter().any(|p| p.contains("`gaps` contains `fun_`")),
            "{found:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verified_evidence_must_be_tracked_by_git() {
        let root = scratch("tracked");
        git(&root, &["init", "-q"]);
        write(&root, "tests/a.rs", TESTS);
        write(&root, "captures/tracked.bin", "x");
        write(&root, "captures/untracked.bin", "x");
        write(&root, ".gitignore", "ignored.bin\n");
        write(&root, "captures/ignored.bin", "x");
        git(
            &root,
            &["add", "tests/a.rs", "captures/tracked.bin", ".gitignore"],
        );

        let verified = |evidence: &str| Entry {
            status: Status::Verified,
            owner: "someone".to_owned(),
            tests: vec!["tests/a.rs::plain_test".to_owned()],
            evidence: evidence.to_owned(),
            ..Entry::missing()
        };
        let mut files = TestFiles::default();
        let mut check = |evidence: &str| validate_entry(&root, &verified(evidence), &mut files);
        assert!(check("captures/tracked.bin").is_empty());
        assert!(
            check("captures").is_empty(),
            "a directory holding tracked files"
        );
        for evidence in ["captures/untracked.bin", "captures/ignored.bin"] {
            let problems = check(evidence);
            assert!(
                problems.len() == 1 && problems[0].contains("is not tracked by git"),
                "{evidence}: {problems:?}"
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_git_checkout_is_reported_once() {
        let root = scratch("no-git");
        let inside_a_repo = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rev-parse", "--git-dir"])
            .output()
            .is_ok_and(|output| output.status.success());
        if inside_a_repo {
            eprintln!("skipping: the temp directory is inside a git repository");
            let _ = std::fs::remove_dir_all(root);
            return;
        }
        write(&root, "tests/a.rs", TESTS);
        let mut problems = Vec::new();
        let mut files = TestFiles::default();
        for name in ["a.bin", "b.bin", "c.bin"] {
            write(&root, &format!("captures/{name}"), "x");
            let entry = Entry {
                status: Status::Verified,
                owner: "someone".to_owned(),
                tests: vec!["tests/a.rs::plain_test".to_owned()],
                evidence: format!("captures/{name}"),
                ..Entry::missing()
            };
            problems.extend(validate_entry(&root, &entry, &mut files));
        }
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("run `parity` inside a git checkout"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn submodule_contents_count_as_tracked() {
        let tracked =
            Tracked::parse("100644 aaaa 0\tdocs/a.md\x00160000 bbbb 0\tdata/vanilla_bp\x00");
        assert!(tracked.contains("docs/a.md"));
        assert!(tracked.contains("docs"));
        assert!(tracked.contains("data/vanilla_bp/entities/pig.json"));
        assert!(!tracked.contains("data/vanilla_bp_extra/pig.json"));
        assert!(!tracked.contains("doc"));
    }
}
