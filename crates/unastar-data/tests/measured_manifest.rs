//! `data/measured/manifest.json` records the SHA-256 of every sweep artefact
//! (decision D4). These tests keep the hashes honest: a hand-edited or
//! re-measured file must come with its manifest update.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

fn measured_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("data/measured")
}

/// Files in `data/measured` that are not sweep artefacts: the manifest, its
/// README, and the capture-derived tables that record their source capture's
/// SHA-256 inside the file (see `data/measured/README.md`).
const NOT_SWEPT: [&str; 6] = [
    "README.md",
    "manifest.json",
    "actor_pick_items.json",
    "builtin_recipes.json",
    "potion_types.json",
    "recipe_legacy_identities.json",
];

/// `(path, sha256)` for every file the manifest lists.
fn listed(manifest: &serde_json::Value) -> Vec<(String, String)> {
    let mut files = Vec::new();
    for (family, body) in manifest["families"].as_object().expect("families") {
        let entries = body["files"]
            .as_array()
            .unwrap_or_else(|| panic!("{family}: no files"));
        assert!(!entries.is_empty(), "{family}: no files");
        for file in entries {
            files.push((
                file["path"].as_str().expect("path").to_owned(),
                file["sha256"].as_str().expect("sha256").to_owned(),
            ));
        }
    }
    files
}

/// Editor and OS litter that is never data (`.DS_Store`, `Thumbs.db`,
/// `desktop.ini`, `file~`, `.file.swp`).
fn is_junk(file_name: &str) -> bool {
    matches!(file_name, ".DS_Store" | "Thumbs.db" | "desktop.ini")
        || file_name.ends_with('~')
        || file_name.ends_with(".swp")
        || file_name.ends_with(".swo")
}

fn on_disk(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let file_name = entry.file_name().to_str().unwrap().to_owned();
        if is_junk(&file_name) {
            continue;
        }
        let name = format!("{prefix}{file_name}");
        if entry.file_type().unwrap().is_dir() {
            on_disk(&entry.path(), &format!("{name}/"), out);
        } else {
            out.insert(name);
        }
    }
}

/// Hash mismatches and missing files, one line each.
fn mismatches(dir: &Path, files: &[(String, String)]) -> Vec<String> {
    files
        .iter()
        .filter_map(|(path, expected)| match std::fs::read(dir.join(path)) {
            Ok(bytes) => {
                let actual = format!("{:x}", Sha256::digest(&bytes));
                (actual != *expected).then(|| format!("{path}: manifest {expected}, file {actual}"))
            }
            Err(error) => Some(format!("{path}: {error}")),
        })
        .collect()
}

#[test]
fn every_measured_file_matches_its_manifest_hash() {
    let dir = measured_dir();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["bds_version"], "1.26.51");
    let files = listed(&manifest);
    let problems = mismatches(&dir, &files);
    assert!(problems.is_empty(), "{}", problems.join("\n"));

    let mut present = BTreeSet::new();
    on_disk(&dir, "", &mut present);
    let listed: BTreeSet<String> = files.into_iter().map(|(path, _)| path).collect();
    let unlisted: Vec<&String> = present
        .iter()
        .filter(|path| !listed.contains(*path) && !NOT_SWEPT.contains(&path.as_str()))
        .collect();
    assert!(
        unlisted.is_empty(),
        "measured files missing from manifest.json: {unlisted:?}"
    );
    for path in NOT_SWEPT {
        assert!(
            present.contains(path),
            "{path} is gone; drop it from NOT_SWEPT"
        );
    }
}

#[test]
fn an_edited_measured_file_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.json"), b"{}\n").unwrap();
    let hash = format!("{:x}", Sha256::digest(b"{}\n"));
    let files = vec![("a.json".to_owned(), hash)];
    assert!(mismatches(dir.path(), &files).is_empty());
    std::fs::write(dir.path().join("a.json"), b"{\"edited\":1}\n").unwrap();
    assert_eq!(mismatches(dir.path(), &files).len(), 1);
    std::fs::remove_file(dir.path().join("a.json")).unwrap();
    assert!(mismatches(dir.path(), &files)[0].starts_with("a.json: "));
}

#[test]
fn editor_and_os_junk_is_not_reported_as_unlisted() {
    let dir = tempfile::tempdir().unwrap();
    for name in [".DS_Store", "a.json~", ".a.json.swp", "Thumbs.db", "a.json"] {
        std::fs::write(dir.path().join(name), b"x").unwrap();
    }
    let mut present = BTreeSet::new();
    on_disk(dir.path(), "", &mut present);
    assert_eq!(present.into_iter().collect::<Vec<_>>(), ["a.json"]);
}
