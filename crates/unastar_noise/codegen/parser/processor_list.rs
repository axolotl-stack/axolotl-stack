use super::common::{RawJsonAsset, parse_raw_json_registry};
use std::collections::HashMap;
use std::path::Path;

pub type ProcessorListJson = RawJsonAsset;

pub fn parse_all(
    dir: &Path,
) -> Result<HashMap<String, ProcessorListJson>, Box<dyn std::error::Error>> {
    parse_raw_json_registry(dir)
}
