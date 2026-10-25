//! Build-time view of `java_data/`: Java block states, their Bedrock runtime ids and
//! their block classes.
//!
//! Emitters resolve Java block states (`{"Name": ..., "Properties": {...}}` in worldgen
//! JSON) through [`JavaBlocks::resolve`] and emit the Bedrock runtime id as a literal, so
//! generated code never looks a block up at run time.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::Deserialize;

/// Block-class bits; must match `src/block_class.rs`.
pub mod class {
    pub const AIR: u8 = 1 << 0;
    pub const FLUID: u8 = 1 << 1;
    pub const WATER: u8 = 1 << 2;
    pub const LEAVES: u8 = 1 << 3;
    pub const BLOCKS_MOTION: u8 = 1 << 4;
    pub const UNMAPPED: u8 = 1 << 7;
}

#[derive(Deserialize)]
struct BlocksFile {
    java_version: String,
    blocks: Vec<JavaBlock>,
}

/// One Java block and the layout of its state ids.
#[derive(Deserialize, Clone, Debug)]
pub struct JavaBlock {
    pub name: String,
    pub first_id: u32,
    pub default_id: u32,
    /// `(property, values)` in state-id order: the cartesian product, last property fastest.
    pub properties: Vec<(String, Vec<String>)>,
}

#[derive(Deserialize)]
struct BedrockFile {
    java_version: String,
    bedrock_ordinals: Vec<u32>,
}

#[derive(Deserialize)]
struct CollisionsFile {
    shapes: Vec<Vec<[f64; 6]>>,
    indices: Vec<u32>,
}

/// Java block states of the pinned version.
pub struct JavaBlocks {
    pub java_version: String,
    blocks: Vec<JavaBlock>,
    by_name: HashMap<String, usize>,
    bedrock: Vec<u32>,
    classes: Vec<u8>,
}

impl JavaBlocks {
    /// Load `blocks.json`, `bedrock_blocks.json` and `collisions.json` from `dir`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let read = |name: &str| {
            std::fs::read_to_string(dir.join(name)).map_err(|e| format!("{name}: {e}"))
        };
        let blocks: BlocksFile =
            serde_json::from_str(&read("blocks.json")?).map_err(|e| format!("blocks.json: {e}"))?;
        let bedrock: BedrockFile = serde_json::from_str(&read("bedrock_blocks.json")?)
            .map_err(|e| format!("bedrock_blocks.json: {e}"))?;
        let collisions: CollisionsFile = serde_json::from_str(&read("collisions.json")?)
            .map_err(|e| format!("collisions.json: {e}"))?;
        if blocks.java_version != bedrock.java_version {
            return Err(format!(
                "java_data mixes Java {} block states with a Java {} Bedrock mapping",
                blocks.java_version, bedrock.java_version
            ));
        }
        let state_count = bedrock.bedrock_ordinals.len();
        if collisions.indices.len() != state_count {
            return Err(
                "collisions.json and bedrock_blocks.json disagree on the state count".into(),
            );
        }

        let mut classes = vec![0u8; state_count];
        let mut by_name = HashMap::new();
        for (index, block) in blocks.blocks.iter().enumerate() {
            by_name.insert(block.name.clone(), index);
            let count: usize = block.properties.iter().map(|(_, v)| v.len()).product();
            for offset in 0..count {
                let id = block.first_id as usize + offset;
                let props = decode_state(block, offset);
                let shape = &collisions.shapes[collisions.indices[id] as usize];
                classes[id] = classify(&block.name, &props, shape);
            }
        }
        Ok(Self {
            java_version: blocks.java_version,
            blocks: blocks.blocks,
            by_name,
            bedrock: bedrock.bedrock_ordinals,
            classes,
        })
    }

    /// Number of Java block states.
    pub fn state_count(&self) -> usize {
        self.bedrock.len()
    }

    /// Bedrock runtime id of a Java state id.
    pub fn bedrock(&self, java_id: u32) -> u32 {
        self.bedrock[java_id as usize]
    }

    /// Every Java block, in state id order.
    pub fn blocks(&self) -> &[JavaBlock] {
        &self.blocks
    }

    /// Every Java state id of a namespaced block.
    pub fn state_ids(&self, name: &str) -> Result<std::ops::Range<u32>, String> {
        let block = self
            .by_name
            .get(name)
            .map(|&i| &self.blocks[i])
            .ok_or_else(|| format!("unknown Java block {name} ({})", self.java_version))?;
        let count: usize = block.properties.iter().map(|(_, v)| v.len()).product();
        Ok(block.first_id..block.first_id + count as u32)
    }

    /// Bedrock runtime ids of every Java state, in Java state id order.
    pub fn bedrock_table(&self) -> &[u32] {
        &self.bedrock
    }

    /// Class flags of every Java state, in Java state id order.
    pub fn class_table(&self) -> &[u8] {
        &self.classes
    }

    /// Class flags per Bedrock runtime id (`UNMAPPED` where no Java state maps). When several
    /// Java states share a Bedrock state (Bedrock keeps waterlogging in a second layer), the
    /// non-waterlogged Java state decides.
    pub fn bedrock_class_table(&self) -> Vec<u8> {
        let len = self
            .bedrock
            .iter()
            .copied()
            .max()
            .map_or(0, |m| m as usize + 1);
        let mut out = vec![class::UNMAPPED; len];
        let mut decided_by_dry = vec![false; len];
        for block in &self.blocks {
            let count: usize = block.properties.iter().map(|(_, v)| v.len()).product();
            for offset in 0..count {
                let id = block.first_id as usize + offset;
                let props = decode_state(block, offset);
                let dry = props.get("waterlogged").is_none_or(|v| v == "false");
                let target = self.bedrock[id] as usize;
                if out[target] == class::UNMAPPED || (dry && !decided_by_dry[target]) {
                    out[target] = self.classes[id];
                    decided_by_dry[target] = dry;
                }
            }
        }
        out
    }

    /// Java state id of a block state; properties missing from `properties` take the
    /// block's default values, as in Java's block-state codec.
    pub fn resolve(
        &self,
        name: &str,
        properties: &BTreeMap<String, String>,
    ) -> Result<u32, String> {
        let name = if name.contains(':') {
            name.to_string()
        } else {
            format!("minecraft:{name}")
        };
        let block = self
            .by_name
            .get(&name)
            .map(|&i| &self.blocks[i])
            .ok_or_else(|| format!("unknown Java block {name} ({})", self.java_version))?;
        let defaults = decode_state(block, (block.default_id - block.first_id) as usize);
        for key in properties.keys() {
            if !block.properties.iter().any(|(k, _)| k == key) {
                return Err(format!("{name} has no property {key}"));
            }
        }
        let mut offset = 0usize;
        for (key, values) in &block.properties {
            let value = properties.get(key).unwrap_or(&defaults[key]);
            let index = values
                .iter()
                .position(|v| v == value)
                .ok_or_else(|| format!("{name}[{key}={value}] is not a valid value"))?;
            offset = offset * values.len() + index;
        }
        Ok(block.first_id + offset as u32)
    }
}

/// Property values of the state at `offset` from the block's first id.
fn decode_state(block: &JavaBlock, mut offset: usize) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (key, values) in block.properties.iter().rev() {
        out.insert(key.clone(), values[offset % values.len()].clone());
        offset /= values.len();
    }
    out
}

/// Class flags of one Java state.
///
/// `BLOCKS_MOTION` follows Java's "solid" test on the collision shape: a non-empty shape
/// whose bounding box has an average edge of at least 0.7291666666666666 or a height of at
/// least 1, excluding cobweb and bamboo saplings. Heightmaps built from it are checked
/// against the Java reference world (`tests/fixtures/java_1_21_11_seed_12345.json`).
fn classify(name: &str, props: &BTreeMap<String, String>, shape: &[[f64; 6]]) -> u8 {
    let mut flags = 0u8;
    let short = name.trim_start_matches("minecraft:");
    if matches!(short, "air" | "cave_air" | "void_air") {
        flags |= class::AIR;
    }
    let waterlogged = props.get("waterlogged").is_some_and(|v| v == "true");
    // Water plants and bubble columns always carry a water source.
    let water_plant = matches!(
        short,
        "bubble_column" | "kelp" | "kelp_plant" | "seagrass" | "tall_seagrass"
    );
    if short == "water" || water_plant || waterlogged {
        flags |= class::FLUID | class::WATER;
    }
    if short == "lava" {
        flags |= class::FLUID;
    }
    if short.ends_with("_leaves") {
        flags |= class::LEAVES;
    }
    if !shape.is_empty() && !matches!(short, "cobweb" | "bamboo_sapling") {
        let mut bounds = [f64::MAX, f64::MAX, f64::MAX, f64::MIN, f64::MIN, f64::MIN];
        for b in shape {
            for axis in 0..3 {
                bounds[axis] = bounds[axis].min(b[axis]);
                bounds[axis + 3] = bounds[axis + 3].max(b[axis + 3]);
            }
        }
        let (sx, sy, sz) = (
            bounds[3] - bounds[0],
            bounds[4] - bounds[1],
            bounds[5] - bounds[2],
        );
        if (sx + sy + sz) / 3.0 >= 0.7291666666666666 || sy >= 1.0 {
            flags |= class::BLOCKS_MOTION;
        }
    }
    flags
}
