use std::path::{Path, PathBuf};

#[test]
fn source_boundaries_reject_raw_pmmp_runtime_consumers() {
    assert_no_forbidden_runtime_source(
        &[
            "unastar_data::CREATIVE_",
            "unastar_data::REQUIRED_",
            "unastar_data::pmmp",
        ],
        "runtime code should consume generated unastar_data artifacts, not raw PMMP JSON constants",
    );
}

#[test]
fn source_boundaries_reject_direct_valentine_block_registry_loads() {
    assert_no_forbidden_runtime_source(
        &[
            "jolyne::valentine::blocks::BLOCKS",
            "valentine::bedrock::version::v1_26_0::blocks::BLOCKS",
            "valentine_bedrock_1_26_0::blocks::BLOCKS",
        ],
        "runtime block registry code should consume unastar_data::blocks, not direct Valentine block constants",
    );
}

#[test]
fn source_boundaries_reject_runtime_generated_item_lookups_outside_registry() {
    assert_no_forbidden_runtime_source_outside(
        &["unastar_data::items::get("],
        &["registry\\item.rs", "registry/item.rs"],
        "runtime item-stack code should use ItemRegistry-cached metadata, not direct generated item lookups",
    );
}

fn assert_no_forbidden_runtime_source(forbidden: &[&str], reason: &str) {
    assert_no_forbidden_runtime_source_outside(forbidden, &[], reason);
}

fn assert_no_forbidden_runtime_source_outside(
    forbidden: &[&str],
    allowed_suffixes: &[&str],
    reason: &str,
) {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_root = manifest_dir.join("src");
    let mut violations = Vec::new();
    collect_violations_outside(&source_root, forbidden, allowed_suffixes, &mut violations);

    assert!(
        violations.is_empty(),
        "{reason}:\n{}",
        violations.join("\n")
    );
}

fn collect_violations_outside(
    path: &Path,
    forbidden: &[&str],
    allowed_suffixes: &[&str],
    violations: &mut Vec<String>,
) {
    if path.is_dir() {
        let entries = std::fs::read_dir(path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for entry in entries {
            let entry = entry.expect("failed to read source directory entry");
            collect_violations_outside(&entry.path(), forbidden, allowed_suffixes, violations);
        }
        return;
    }

    if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
        return;
    }

    let path_text = path.to_string_lossy();
    if allowed_suffixes
        .iter()
        .any(|suffix| path_text.ends_with(suffix))
    {
        return;
    }

    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    for (line_index, line) in content.lines().enumerate() {
        for pattern in forbidden {
            if line.contains(pattern) {
                violations.push(format!(
                    "{}:{} contains `{}`",
                    path.display(),
                    line_index + 1,
                    pattern
                ));
            }
        }
    }
}
