//! Provenance rule (see `AGENTS.md`): comments describe behaviour, never
//! engine internals. This scans every comment in the crate's sources for the
//! two shapes engine symbol names take in prose:
//!
//! - a scope-qualified type in backticks, `` `Outer::Inner` ``, outside an
//!   intra-doc link (`[`...`]` links name this crate's Rust items);
//! - a lowerCamelCase identifier in backticks, `` `doThing` `` or
//!   `` `doThing(...)` ``; this crate's own items are snake_case, and
//!   namespaced data ids (`minecraft:muttonRaw`) are not symbols.

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The text after `//` on a line, if any (comments only, not code).
fn comment(line: &str) -> Option<&str> {
    line.find("//").map(|at| &line[at + 2..])
}

/// Backticked spans in `text` that are not the target of an intra-doc link.
fn code_spans(text: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        let in_link = rest[..open].ends_with('[');
        if !in_link {
            spans.push(&after[..close]);
        }
        rest = &after[close + 1..];
    }
    spans
}

fn looks_internal(span: &str) -> bool {
    let ident: String = span
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == ':')
        .collect();
    if let Some((outer, inner)) = ident.split_once("::") {
        let upper = |s: &str| s.chars().next().is_some_and(|c| c.is_ascii_uppercase());
        return upper(outer) && upper(inner);
    }
    if ident.contains(':') {
        // A namespaced data id (`minecraft:muttonRaw`), not a symbol.
        return false;
    }
    let mut chars = ident.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && !ident.contains('_')
        && chars.any(|c| c.is_ascii_uppercase())
}

#[test]
fn comments_name_no_engine_internals() {
    let mut files = Vec::new();
    rust_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    assert!(!files.is_empty());
    let mut found = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for (number, line) in text.lines().enumerate() {
            let Some(comment) = comment(line) else {
                continue;
            };
            for span in code_spans(comment) {
                if looks_internal(span) {
                    found.push(format!("{}:{}: `{span}`", file.display(), number + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "internal-looking names:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_scanner_flags_both_shapes_and_spares_links_and_snake_case() {
    // Made-up shapes only: real engine names must not appear here either.
    assert!(looks_internal("Outer::Inner"));
    assert!(looks_internal("doThing"));
    assert!(looks_internal("doThing(50)"));
    assert!(looks_internal("nextInt(50) <= cost"));
    assert!(!looks_internal("do_thing"));
    assert!(!looks_internal("minecraft:zombie"));
    assert!(!looks_internal("minecraft:muttonRaw"));
    assert!(!looks_internal("crate::loot"));
    assert!(code_spans("see [`ValueKind::Bool`]").is_empty());
}
