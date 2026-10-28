//! I3 source comparison against the pinned BDS install (opt in with UNASTAR_BDS_DIR).
//!
//! Uses the shared WG-Bedrock archive reader. Item/loot layers override earlier
//! layers; recipes load higher-priority layers first and keep the first identifier
//! across recipe kinds. Within a layer the shared reader's file order is retained.
//! `format_version` is schema metadata, never a precedence key. In particular this
//! does not reproduce `decode_vanilla`'s separate flattened-samples preprocessing.
//!
//! All readable documents are parsed before selection. Opaque 7fMCB payloads are
//! not decoded or assigned guessed identifiers from filenames; they must match
//! the reviewed origin -> SHA-256 list exactly, so any changed or additional
//! binary document fails the comparison.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use unastar_packs::packs::{BASE_GAME_VERSION, Family, PackFile, PackStack};

const BINARY_DOCUMENT: &[u8] = b"\x7fMCB";

#[derive(Debug)]
struct CanonicalDocument {
    origin: String,
    value: Value,
}

#[derive(Debug, Default)]
struct FamilyData {
    documents: BTreeMap<String, CanonicalDocument>,
    // Every opaque entry, including lower layers. Even an overridden opaque
    // document cannot be proven overridden by identifier without decoding it.
    opaque: BTreeMap<String, String>,
    readable_entries: usize,
}

fn family(stack: &PackStack, family: Family) -> FamilyData {
    // Reverse layers, not all files: recipe order *within* a layer matters.
    let mut files = Vec::new();
    if family == Family::Recipes {
        for layer in stack.layers.iter().rev() {
            files.extend(
                PackStack {
                    layers: vec![layer.clone()],
                }
                .files(family)
                .unwrap(),
            );
        }
    } else {
        files = stack.files(family).unwrap();
    }
    resolve_files(files, family)
}

fn resolve_files(files: Vec<PackFile>, family: Family) -> FamilyData {
    let mut out = FamilyData::default();
    for file in files {
        if file.bytes.starts_with(BINARY_DOCUMENT) {
            assert!(
                out.opaque
                    .insert(file.origin.clone(), hash(&file.bytes))
                    .is_none(),
                "duplicate opaque origin: {}",
                file.origin
            );
            continue;
        }
        let value = unastar_packs::json::parse(&file.origin, &file.bytes).unwrap();
        let key = if family == Family::LootTables {
            // PackStack supplies the complete, case-preserving family-relative
            // path for loose files and entries in nested archives alike.
            file.entry.clone()
        } else {
            let document = unastar_packs::packs::parse_document(&file.origin, &file.bytes).unwrap();
            assert!(
                if family == Family::Items {
                    document.kind == "minecraft:item"
                } else {
                    document.kind.starts_with("minecraft:recipe_")
                },
                "{}: unexpected {} document in {}",
                file.origin,
                document.kind,
                family.path()
            );
            document.identifier
        };
        out.readable_entries += 1;
        let document = CanonicalDocument {
            origin: file.origin,
            value,
        };
        if family == Family::Recipes {
            out.documents.entry(key).or_insert(document);
        } else {
            out.documents.insert(key, document);
        }
    }
    out
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn diff(samples: &FamilyData, bds: &FamilyData) -> Value {
    let only = |a: &FamilyData, b: &FamilyData| -> Vec<String> {
        a.documents
            .keys()
            .filter(|key| !b.documents.contains_key(*key))
            .cloned()
            .collect()
    };
    let differing: Vec<_> = samples
        .documents
        .iter()
        .filter(|(key, value)| {
            bds.documents
                .get(*key)
                .is_some_and(|other| other.value != value.value)
        })
        .map(|(key, _)| key.clone())
        .collect();
    json!({
        "only_in_samples": only(samples, bds),
        "only_in_bds": only(bds, samples),
        "differing": differing,
    })
}

fn check_samples_pin(samples_root: &Path, pins: &Value) {
    let version: Value = serde_json::from_slice(
        &std::fs::read(samples_root.join("version.json"))
            .expect("missing bedrock-samples version.json"),
    )
    .unwrap();
    assert_eq!(
        version["latest"]["version"], pins["samples_version"],
        "bedrock-samples version changed; review the comparison pin, do not auto-bless"
    );
    let revision = Command::new("git")
        .arg("-C")
        .arg(samples_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git is required to validate the bedrock-samples comparison pin");
    assert!(
        revision.status.success(),
        "cannot read bedrock-samples pin: {}",
        String::from_utf8_lossy(&revision.stderr)
    );
    assert_eq!(
        String::from_utf8(revision.stdout).unwrap().trim(),
        pins["samples_commit"].as_str().unwrap(),
        "bedrock-samples commit changed; review the comparison pin, do not auto-bless"
    );
    let clean = Command::new("git")
        .arg("-C")
        .arg(samples_root)
        .args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            "behavior_pack",
            "version.json",
        ])
        .output()
        .expect("git is required to check local sample modifications");
    assert!(
        clean.status.success() && clean.stdout.is_empty(),
        "bedrock-samples comparison input has local changes or git status failed:\n{}\n{}",
        String::from_utf8_lossy(&clean.stdout),
        String::from_utf8_lossy(&clean.stderr)
    );
    assert_eq!(
        json!(BASE_GAME_VERSION),
        pins["bds_base_game_version"],
        "shared BDS target changed; review the native comparison baseline"
    );
}

fn check_bds_pin(bds_dir: &Path, pins: &Value) {
    let version = BASE_GAME_VERSION
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".");
    let packs_dir = bds_dir.join("behavior_packs");
    let native_manifest = packs_dir.join(format!("vanilla_{version}/manifest.json"));
    let bytes = std::fs::read(&native_manifest).unwrap_or_else(|error| {
        panic!(
            "missing pinned BDS layer {}: {error}",
            native_manifest.display()
        )
    });
    let metadata =
        unastar_packs::json::parse(&native_manifest.display().to_string(), &bytes).unwrap();
    assert_eq!(
        metadata["header"]["version"], pins["bds_base_game_version"],
        "native pack manifest version does not match the comparison pin"
    );
    for entry in std::fs::read_dir(&packs_dir).unwrap() {
        let entry = entry.unwrap();
        if entry.path().is_dir()
            && let Some(pack_version) =
                unastar_packs::packs::vanilla_pack_version(&entry.file_name().to_string_lossy())
        {
            assert!(
                pack_version.as_slice() <= BASE_GAME_VERSION.as_slice(),
                "newer native layer {} is present; this is not the pinned BDS install",
                entry.path().display()
            );
        }
    }
    // Endstone installs expose this extra marker; official installs need only
    // the mandatory versioned pack manifest above.
    let marker = bds_dir.join("version.txt");
    if marker.is_file() {
        let actual = std::fs::read_to_string(&marker).unwrap();
        assert!(
            actual.trim() == version || format!("1.{}", actual.trim()) == version,
            "{} reports {}, expected {version}",
            marker.display(),
            actual.trim()
        );
    }
}

#[test]
fn bedrock_samples_compare_all_readable_bds_documents() {
    let Some(bds_dir) = std::env::var_os("UNASTAR_BDS_DIR").map(PathBuf::from) else {
        eprintln!(
            "skipped: set UNASTAR_BDS_DIR to a BDS 1.26.51 install for the I3 source comparison"
        );
        return;
    };
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let expected: Value = serde_json::from_str(include_str!("../data/bds_pack_diff.json")).unwrap();
    let samples_root = manifest.join("data/vanilla_bp");
    check_samples_pin(&samples_root, &expected["pins"]);
    check_bds_pin(&bds_dir, &expected["pins"]);
    let samples = PackStack::single_pack("samples", samples_root.join("behavior_pack"));
    let bds = PackStack::open(&bds_dir).unwrap();
    let mut actual = serde_json::Map::new();
    let mut opaque = serde_json::Map::new();
    for (name, kind) in [
        ("items", Family::Items),
        ("recipes", Family::Recipes),
        ("loot_tables", Family::LootTables),
    ] {
        let ours = family(&samples, kind);
        let theirs = family(&bds, kind);
        assert!(
            ours.opaque.is_empty(),
            "bedrock-samples must contain readable JSON: {:?}",
            ours.opaque
        );
        assert!(
            ours.readable_entries > 0 && theirs.readable_entries > 0,
            "{name}: missing or empty family in the supplied sample/native pack; refusing empty equality"
        );
        let report = diff(&ours, &theirs);
        eprintln!(
            "{name}: samples {} readable / {} selected; BDS {} readable / {} selected / {} opaque",
            ours.readable_entries,
            ours.documents.len(),
            theirs.readable_entries,
            theirs.documents.len(),
            theirs.opaque.len()
        );
        for key in report["differing"].as_array().unwrap() {
            let key = key.as_str().unwrap();
            eprintln!(
                "{name}/{key}: {} != {}",
                ours.documents[key].origin, theirs.documents[key].origin
            );
            eprintln!(
                "samples: {}\nBDS: {}",
                ours.documents[key].value, theirs.documents[key].value
            );
        }
        if !theirs.opaque.is_empty() {
            opaque.insert(name.into(), json!(theirs.opaque));
        }
        actual.insert(name.into(), report);
    }
    let actual = Value::Object(actual);
    let opaque = Value::Object(opaque);
    assert!(
        actual == expected["canonical_differences"] && opaque == expected["opaque_documents"],
        "I3 source comparison changed.\nCanonical JSON differences:\n{}\nReviewed differences:\n{}\nOpaque 7fMCB entries (exact origin -> raw SHA-256):\n{}\nReviewed opaque entries:\n{}\nReview pin/data/precedence changes explicitly; UNASTAR_BLESS is intentionally unsupported.",
        serde_json::to_string_pretty(&actual).unwrap(),
        serde_json::to_string_pretty(&expected["canonical_differences"]).unwrap(),
        serde_json::to_string_pretty(&opaque).unwrap(),
        serde_json::to_string_pretty(&expected["opaque_documents"]).unwrap(),
    );
}

#[cfg(test)]
mod regressions {
    use super::*;
    use unastar_packs::brarchive::{Entry, encode};

    fn recipe(id: &str, kind: &str, version: &str, marker: u32) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "format_version": version,
            kind: {"description": {"identifier": id}, "marker": marker}
        }))
        .unwrap()
    }

    fn file(entry: &str, bytes: Vec<u8>) -> PackFile {
        PackFile {
            origin: format!("test/recipes:{entry}"),
            layer: "test".into(),
            entry: entry.into(),
            bytes,
        }
    }

    #[test]
    fn first_recipe_identifier_wins_across_kinds_regardless_of_format_version() {
        let data = resolve_files(
            vec![
                file(
                    "a.json",
                    recipe("test:shared", "minecraft:recipe_shaped", "1.12.0", 1),
                ),
                file(
                    "b.json",
                    recipe("test:shared", "minecraft:recipe_shapeless", "1.26.50", 2),
                ),
            ],
            Family::Recipes,
        );
        assert_eq!(data.readable_entries, 2);
        assert_eq!(data.documents.len(), 1);
        assert_eq!(
            data.documents["test:shared"].value["minecraft:recipe_shaped"]["marker"],
            1
        );
        assert_eq!(data.documents["test:shared"].origin, "test/recipes:a.json");
    }

    #[test]
    fn binary_paths_are_not_guessed_identifiers_or_silently_equal() {
        let data = resolve_files(
            vec![file("arbitrary.json", b"\x7fMCBopaque".to_vec())],
            Family::Items,
        );
        assert!(data.documents.is_empty());
        assert_eq!(
            data.opaque["test/recipes:arbitrary.json"],
            hash(b"\x7fMCBopaque")
        );
        let samples = resolve_files(
            vec![file(
                "arbitrary.json",
                serde_json::to_vec(&json!({
                    "format_version": "1.26.50",
                    "minecraft:item": {"description": {"identifier": "test:not_the_filename"}}
                }))
                .unwrap(),
            )],
            Family::Items,
        );
        assert_eq!(
            diff(&samples, &data)["only_in_samples"],
            json!(["test:not_the_filename"])
        );
    }

    #[test]
    fn priority_layers_do_not_reverse_archive_entry_order() {
        let root =
            std::env::temp_dir().join(format!("unastar-i3-precedence-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("definitions")).unwrap();
        for (layer, entries) in [
            (
                "vanilla",
                vec![Entry {
                    name: "base.json".into(),
                    data: recipe("test:x", "minecraft:recipe_shaped", "99.0.0", 0),
                }],
            ),
            (
                "vanilla_1.26.50",
                vec![
                    Entry {
                        name: "z_first.json".into(),
                        data: recipe("test:x", "minecraft:recipe_shapeless", "1.12.0", 1),
                    },
                    Entry {
                        name: "a_second.json".into(),
                        data: recipe("test:x", "minecraft:recipe_shaped", "1.26.50", 2),
                    },
                ],
            ),
        ] {
            let dir = root.join("behavior_packs").join(layer).join("__brarchive");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("recipes.brarchive"), encode(&entries)).unwrap();
        }
        let stack = PackStack::open(&root).unwrap();
        let data = family(&stack, Family::Recipes);
        assert_eq!(data.readable_entries, 3);
        assert_eq!(
            data.documents["test:x"].value["minecraft:recipe_shapeless"]["marker"],
            1
        );
        assert_eq!(
            data.documents["test:x"].origin,
            "vanilla_1.26.50/recipes:z_first.json"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn last_item_identifier_wins_without_format_version_selection() {
        let item = |version: &str, marker| {
            serde_json::to_vec(&json!({
                "format_version": version,
                "minecraft:item": {"description": {"identifier": "test:item"}, "marker": marker}
            }))
            .unwrap()
        };
        let data = resolve_files(
            vec![
                file("a.json", item("99.0.0", 1)),
                file("b.json", item("1.12.0", 2)),
            ],
            Family::Items,
        );
        assert_eq!(
            data.documents["test:item"].value["minecraft:item"]["marker"],
            2
        );
    }
}
