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

fn assert_no_forbidden_runtime_source(forbidden: &[&str], reason: &str) {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_root = manifest_dir.join("src");
    let mut violations = Vec::new();
    collect_violations(&source_root, forbidden, &mut violations);

    assert!(
        violations.is_empty(),
        "{reason}:\n{}",
        violations.join("\n")
    );
}

fn collect_violations(path: &Path, forbidden: &[&str], violations: &mut Vec<String>) {
    if path.is_dir() {
        let entries = std::fs::read_dir(path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for entry in entries {
            let entry = entry.expect("failed to read source directory entry");
            collect_violations(&entry.path(), forbidden, violations);
        }
        return;
    }

    if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
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
