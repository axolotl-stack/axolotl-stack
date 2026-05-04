//! Source manifest generation for reproducible gameplay-data artifacts.
//!
//! The manifest records where each generated artifact came from before Rust
//! codegen sees it. Runtime code should depend on normalized artifacts, not on
//! ad-hoc knowledge of PMMP, Prismarine, BDS, or behavior-pack layouts.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::info;
use walkdir::WalkDir;

use crate::bds;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestSource {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub version: Option<String>,
    pub git_commit: Option<String>,
    pub hash: Option<String>,
    pub confidence: String,
}

impl ManifestSource {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("source name is required".to_string());
        }
        if self.kind.trim().is_empty() {
            return Err(format!("source {} kind is required", self.name));
        }
        if self.path.trim().is_empty() {
            return Err(format!("source {} path is required", self.name));
        }
        if self.version.is_none() {
            return Err(format!("source {} requires version", self.name));
        }
        if self.git_commit.is_none() && self.hash.is_none() {
            return Err(format!("source {} requires git_commit or hash", self.name));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManifestArtifact {
    name: String,
    path: String,
    hash: String,
}

pub struct ManifestInputs<'a> {
    pub vanilla_path: &'a Path,
    pub vanilla_biomes_path: &'a Path,
    pub overrides_path: &'a Path,
    pub upstream_path: &'a Path,
    pub pmmp_path: &'a Path,
    pub valentine_version_path: &'a Path,
    pub bds_extraction_path: Option<&'a Path>,
}

pub fn write_manifest(output_dir: &Path, inputs: &ManifestInputs<'_>) -> miette::Result<()> {
    let mut sources = vec![
        source_from_path(
            "valentine_bedrock_1_26_0",
            "generated_protocol_data",
            inputs.valentine_version_path,
            Some("bedrock_1_26_0".to_string()),
            "high",
            false,
        )?,
        source_from_path(
            "vanilla_behavior_pack",
            "behavior_pack",
            inputs.vanilla_path,
            vanilla_version(inputs.vanilla_path)?,
            "high",
            true,
        )?,
        source_from_path(
            "vanilla_behavior_pack_biomes",
            "behavior_pack_biomes",
            inputs.vanilla_biomes_path,
            vanilla_version(inputs.vanilla_biomes_path)?,
            "high",
            true,
        )?,
        source_from_path(
            "project_overrides",
            "kdl_overrides",
            inputs.overrides_path,
            Some("local-kdl-overrides-v1".to_string()),
            "high",
            false,
        )?,
        source_from_path(
            "upstream_metadata",
            "schema_metadata",
            inputs.upstream_path,
            upstream_version(inputs.upstream_path)?,
            "medium",
            false,
        )?,
        source_from_path(
            "pmmp_bedrock_data",
            "external_json",
            inputs.pmmp_path,
            Some("pmmp-bedrockdata-embedded-v1".to_string()),
            "medium",
            false,
        )?,
    ];
    if let Some(path) = inputs.bds_extraction_path {
        sources.push(source_from_path(
            "bds_extractor_capture",
            "bds_extractor_json",
            path,
            bds::extraction_version(path)?,
            "high",
            false,
        )?);
    }

    for source in &sources {
        source
            .validate()
            .map_err(|e| miette::miette!("Invalid source manifest: {}", e))?;
    }

    let artifacts = collect_artifacts(output_dir, inputs.bds_extraction_path.is_some())?;
    let manifest = render_manifest(&sources, &artifacts);
    let path = output_dir.join("manifest.kdl");
    std::fs::write(&path, manifest)
        .map_err(|e| miette::miette!("Failed to write {}: {}", path.display(), e))?;
    info!("Wrote source manifest to {}", path.display());
    Ok(())
}

fn source_from_path(
    name: &str,
    kind: &str,
    path: &Path,
    version: Option<String>,
    confidence: &str,
    record_git_commit: bool,
) -> miette::Result<ManifestSource> {
    if !path.exists() {
        return Err(miette::miette!(
            "Required source path does not exist: {}",
            path.display()
        ));
    }

    let git_commit = record_git_commit.then(|| git_commit(path)).flatten();
    let hash = Some(hash_path(path)?);

    Ok(ManifestSource {
        name: name.to_string(),
        kind: kind.to_string(),
        path: normalize_path(path),
        version,
        git_commit,
        hash,
        confidence: confidence.to_string(),
    })
}

fn collect_artifacts(
    output_dir: &Path,
    include_bds_artifacts: bool,
) -> miette::Result<Vec<ManifestArtifact>> {
    let mut artifacts = Vec::new();
    let bds_artifact = output_dir.join("biome_packets.kdl");
    if bds_artifact.exists() && !include_bds_artifacts {
        return Err(miette::miette!(
            "{} exists but no --bds-extraction source was supplied for manifest provenance",
            bds_artifact.display()
        ));
    }

    for name in [
        "entities.kdl",
        "blocks.kdl",
        "biomes.kdl",
        "items.kdl",
        "creative.kdl",
    ] {
        let path = output_dir.join(name);
        if path.exists() {
            artifacts.push(ManifestArtifact {
                name: name.strip_suffix(".kdl").unwrap_or(name).to_string(),
                path: name.to_string(),
                hash: hash_path(&path)?,
            });
        }
    }
    if include_bds_artifacts && bds_artifact.exists() {
        artifacts.push(ManifestArtifact {
            name: "biome_packets".to_string(),
            path: "biome_packets.kdl".to_string(),
            hash: hash_path(&bds_artifact)?,
        });
    }

    Ok(artifacts)
}

fn git_commit(path: &Path) -> Option<String> {
    let dir = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if commit.is_empty() {
        None
    } else {
        Some(commit)
    }
}

fn vanilla_version(path: &Path) -> miette::Result<Option<String>> {
    let Some(version_path) = find_ancestor_file(path, "version.json") else {
        return Ok(None);
    };
    let json = std::fs::read_to_string(&version_path)
        .map_err(|e| miette::miette!("Failed to read {}: {}", version_path.display(), e))?;
    let value: serde_json::Value = serde_json::from_str(&json)
        .map_err(|e| miette::miette!("Failed to parse {}: {}", version_path.display(), e))?;

    Ok(value
        .get("latest")
        .and_then(|latest| latest.get("version"))
        .and_then(|version| version.as_str())
        .map(ToOwned::to_owned))
}

fn upstream_version(path: &Path) -> miette::Result<Option<String>> {
    let readme = path.join("README.txt");
    if !readme.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&readme)
        .map_err(|e| miette::miette!("Failed to read {}: {}", readme.display(), e))?;
    let mojang =
        extract_segment_after(&content, "/tree/").unwrap_or_else(|| "unknown-mojang".to_string());
    let blockception = extract_segment_after(&content, "/blob/")
        .or_else(|| extract_segment_after(&content, "/tree/"))
        .unwrap_or_else(|| "unknown-blockception".to_string());

    Ok(Some(format!(
        "mojang-creator-tools:{mojang};blockception-schema:{blockception}"
    )))
}

fn find_ancestor_file(path: &Path, filename: &str) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        let candidate = ancestor.join(filename);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn extract_segment_after(content: &str, marker: &str) -> Option<String> {
    let start = content.find(marker)? + marker.len();
    let rest = &content[start..];
    let segment = rest
        .split(['/', '\r', '\n', '#', '?'])
        .next()
        .unwrap_or_default()
        .trim();
    if segment.is_empty() {
        None
    } else {
        Some(segment.to_string())
    }
}

fn hash_path(path: &Path) -> miette::Result<String> {
    let mut hasher = Sha256::new();

    if path.is_file() {
        hasher.update(
            std::fs::read(path)
                .map_err(|e| miette::miette!("Failed to hash file {}: {}", path.display(), e))?,
        );
    } else if path.is_dir() {
        let mut files: Vec<PathBuf> = WalkDir::new(path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.into_path())
            .collect();
        files.sort();

        for file in files {
            let relative = file.strip_prefix(path).unwrap_or(&file);
            hasher.update(relative.to_string_lossy().as_bytes());
            hasher.update([0]);
            hasher.update(
                std::fs::read(&file).map_err(|e| {
                    miette::miette!("Failed to hash file {}: {}", file.display(), e)
                })?,
            );
            hasher.update([0]);
        }
    } else {
        hasher.update(b"missing");
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn render_manifest(sources: &[ManifestSource], artifacts: &[ManifestArtifact]) -> String {
    let generated_at =
        std::env::var("UNASTAR_DATA_GENERATED_AT").unwrap_or_else(|_| "unknown".to_string());
    let mut out = String::new();
    out.push_str("// AUTO-GENERATED by unastar-data-gen\n");
    out.push_str("// Records source versions and artifact hashes for generated gameplay data.\n\n");
    out.push_str(&format!(
        "manifest schema_version=\"1\" generated_at=\"{}\" {{\n",
        escape(&generated_at)
    ));

    for source in sources {
        out.push_str(&format!(
            "    source \"{}\" kind=\"{}\" path=\"{}\" confidence=\"{}\"",
            escape(&source.name),
            escape(&source.kind),
            escape(&source.path),
            escape(&source.confidence)
        ));
        if let Some(version) = &source.version {
            out.push_str(&format!(" version=\"{}\"", escape(version)));
        }
        if let Some(git_commit) = &source.git_commit {
            out.push_str(&format!(" git_commit=\"{}\"", escape(git_commit)));
        }
        if let Some(hash) = &source.hash {
            out.push_str(&format!(" hash=\"{}\"", escape(hash)));
        }
        out.push('\n');
    }

    for artifact in artifacts {
        out.push_str(&format!(
            "    artifact \"{}\" path=\"{}\" hash=\"{}\"\n",
            escape(&artifact.name),
            escape(&artifact.path),
            escape(&artifact.hash)
        ));
    }

    out.push_str("}\n");
    out
}

fn normalize_path(path: &Path) -> String {
    let normalized = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let relative = std::env::current_dir()
        .ok()
        .and_then(|cwd| cwd.canonicalize().ok())
        .and_then(|cwd| normalized.strip_prefix(cwd).ok().map(Path::to_path_buf))
        .unwrap_or(normalized);

    relative
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/")
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_requires_version() {
        let source = ManifestSource {
            name: "vanilla".to_string(),
            kind: "behavior_pack".to_string(),
            path: "data/vanilla".to_string(),
            version: None,
            git_commit: None,
            hash: None,
            confidence: "high".to_string(),
        };

        let error = source.validate().unwrap_err();
        assert!(error.contains("requires version"));
    }

    #[test]
    fn source_accepts_version_and_git_commit() {
        let source = ManifestSource {
            name: "vanilla".to_string(),
            kind: "behavior_pack".to_string(),
            path: "data/vanilla".to_string(),
            version: Some("1.21.130.3".to_string()),
            git_commit: Some("abc123".to_string()),
            hash: None,
            confidence: "high".to_string(),
        };

        source.validate().unwrap();
    }

    #[test]
    fn render_manifest_includes_sources_and_artifacts() {
        let source = ManifestSource {
            name: "vanilla".to_string(),
            kind: "behavior_pack".to_string(),
            path: "data/vanilla".to_string(),
            version: Some("1.21.130.3".to_string()),
            git_commit: Some("abc123".to_string()),
            hash: None,
            confidence: "high".to_string(),
        };
        let artifact = ManifestArtifact {
            name: "entities".to_string(),
            path: "entities.kdl".to_string(),
            hash: "def456".to_string(),
        };

        let manifest = render_manifest(&[source], &[artifact]);
        assert!(manifest.contains("source \"vanilla\""));
        assert!(manifest.contains("artifact \"entities\""));
        assert!(manifest.contains("schema_version=\"1\""));
    }

    #[test]
    fn collect_artifacts_rejects_bds_artifact_without_capture_source() {
        let temp_dir = std::env::temp_dir().join(format!(
            "unastar-manifest-bds-test-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        std::fs::write(
            temp_dir.join("biome_packets.kdl"),
            "biome_packet minecraft:plains\n",
        )
        .expect("write stale artifact");

        let err = collect_artifacts(&temp_dir, false).expect_err("missing BDS source is invalid");

        assert!(err.to_string().contains("no --bds-extraction source"));
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    fn unique_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    }
}
