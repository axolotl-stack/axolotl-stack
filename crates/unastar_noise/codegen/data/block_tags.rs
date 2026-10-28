//! Build-time Java block tags (`java_data/tags/block`), resolved to block names.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize)]
struct TagFile {
    values: Vec<TagEntry>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TagEntry {
    Id(String),
    Optional { id: String, required: bool },
}

/// Every block tag, unresolved.
pub struct BlockTags {
    tags: BTreeMap<String, Vec<(String, bool)>>,
}

impl BlockTags {
    /// Load every `*.json` under `dir` (tag name = `minecraft:` + relative path).
    pub fn load(dir: &Path) -> Result<Self, String> {
        let mut tags = BTreeMap::new();
        for entry in walkdir::WalkDir::new(dir) {
            let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
            if entry.path().extension().is_none_or(|e| e != "json") {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(dir)
                .map_err(|e| e.to_string())?
                .with_extension("");
            let name = format!(
                "minecraft:{}",
                relative.to_string_lossy().replace('\\', "/")
            );
            let text = std::fs::read_to_string(entry.path())
                .map_err(|e| format!("{}: {e}", entry.path().display()))?;
            let file: TagFile = serde_json::from_str(&text)
                .map_err(|e| format!("{}: {e}", entry.path().display()))?;
            let values = file
                .values
                .into_iter()
                .map(|v| match v {
                    TagEntry::Id(id) => (id, true),
                    TagEntry::Optional { id, required } => (id, required),
                })
                .collect();
            tags.insert(name, values);
        }
        Ok(Self { tags })
    }

    /// Block names in a tag (`#minecraft:...` or `minecraft:...`), nested tags expanded.
    pub fn resolve(&self, tag: &str) -> Result<BTreeSet<String>, String> {
        let mut out = BTreeSet::new();
        self.expand(tag.trim_start_matches('#'), &mut out, &mut Vec::new())?;
        Ok(out)
    }

    fn expand(
        &self,
        tag: &str,
        out: &mut BTreeSet<String>,
        stack: &mut Vec<String>,
    ) -> Result<(), String> {
        if stack.iter().any(|t| t == tag) {
            return Err(format!("block tag cycle through {tag}"));
        }
        let values = self
            .tags
            .get(tag)
            .ok_or_else(|| format!("unknown block tag #{tag}"))?;
        stack.push(tag.to_string());
        for (value, required) in values {
            if let Some(nested) = value.strip_prefix('#') {
                match self.expand(nested, out, stack) {
                    Err(e) if *required => return Err(e),
                    _ => {}
                }
            } else {
                out.insert(value.clone());
            }
        }
        stack.pop();
        Ok(())
    }

}
