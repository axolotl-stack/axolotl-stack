use super::common::{BinaryAsset, collect_binary_assets};
use std::collections::HashMap;
use std::path::Path;

pub type StructureTemplateAsset = BinaryAsset;

pub fn parse_all(
    dir: &Path,
) -> Result<HashMap<String, StructureTemplateAsset>, Box<dyn std::error::Error>> {
    collect_binary_assets(dir, "nbt")
}
