//! Build-time canonical Java states, native physics, and the final Bedrock mapping.
//!
//! Worldgen JSON resolves to canonical Java state ids. Generation keeps every source
//! property; the separate Bedrock table is consumed only at the edition boundary.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Block-class bits; must match `src/block_class.rs`.
pub mod class {
    pub const AIR: u8 = 1 << 0;
    pub const FLUID: u8 = 1 << 1;
    pub const WATER: u8 = 1 << 2;
    pub const LEAVES: u8 = 1 << 3;
    pub const BLOCKS_MOTION: u8 = 1 << 4;
    pub const FULL_TOP_FACE: u8 = 1 << 5;
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
struct PhysicsFile {
    java_version: String,
    flags_sha256: String,
    wall_above_flags_sha256: String,
    center_support_flags_sha256: String,
    behavior_flags_sha256: String,
    states: Vec<PhysicsState>,
}

#[derive(Deserialize)]
struct PhysicsState {
    id: u32,
    block: String,
    properties: Vec<(String, String)>,
    flags: u8,
    wall_above_flags: u8,
    center_support_flags: u8,
    behavior_flags: u8,
}

/// Java block states of the pinned version.
pub struct JavaBlocks {
    pub java_version: String,
    blocks: Vec<JavaBlock>,
    by_name: HashMap<String, usize>,
    bedrock: Vec<u32>,
    classes: Vec<u8>,
    shape_support: Vec<u8>,
    shape_wall_above: Vec<u8>,
    shape_center_support: Vec<u8>,
    state_physics: Vec<u8>,
}

impl JavaBlocks {
    /// Load canonical states, validated native physics, and the Bedrock mapping.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let read = |name: &str| {
            std::fs::read_to_string(dir.join(name)).map_err(|e| format!("{name}: {e}"))
        };
        let blocks: BlocksFile =
            serde_json::from_str(&read("blocks.json")?).map_err(|e| format!("blocks.json: {e}"))?;
        let bedrock: BedrockFile = serde_json::from_str(&read("bedrock_blocks.json")?)
            .map_err(|e| format!("bedrock_blocks.json: {e}"))?;
        let physics: PhysicsFile = serde_json::from_str(&read("block_physics.json")?)
            .map_err(|e| format!("block_physics.json: {e}"))?;
        if blocks.java_version != bedrock.java_version {
            return Err(format!(
                "java_data mixes Java {} block states with a Java {} Bedrock mapping",
                blocks.java_version, bedrock.java_version
            ));
        }
        let state_count = bedrock.bedrock_ordinals.len();
        if physics.java_version != blocks.java_version || physics.states.len() != state_count {
            return Err(
                "block_physics.json disagrees with the pinned Java version/state count".into(),
            );
        }
        let mut shape_support = Vec::with_capacity(state_count);
        let mut shape_wall_above = Vec::with_capacity(state_count);
        let mut shape_center_support = Vec::with_capacity(state_count);
        let mut state_physics = Vec::with_capacity(state_count);
        for (id, state) in physics.states.iter().enumerate() {
            if state.id as usize != id {
                return Err(format!(
                    "block_physics.json has an invalid state id at {id}"
                ));
            }
            shape_support.push(state.flags);
            shape_wall_above.push(state.wall_above_flags);
            shape_center_support.push(state.center_support_flags);
            state_physics.push(state.behavior_flags);
        }
        if format!("{:x}", Sha256::digest(&shape_support)) != physics.flags_sha256
            || format!("{:x}", Sha256::digest(&shape_wall_above)) != physics.wall_above_flags_sha256
            || format!("{:x}", Sha256::digest(&shape_center_support))
                != physics.center_support_flags_sha256
            || format!("{:x}", Sha256::digest(&state_physics)) != physics.behavior_flags_sha256
        {
            return Err("block_physics.json native physics checksum mismatch".into());
        }

        let mut classes = vec![0u8; state_count];
        let mut by_name = HashMap::new();
        for (index, block) in blocks.blocks.iter().enumerate() {
            by_name.insert(block.name.clone(), index);
            let count: usize = block.properties.iter().map(|(_, v)| v.len()).product();
            for offset in 0..count {
                let id = block.first_id as usize + offset;
                let props = decode_state(block, offset);
                let native = &physics.states[id];
                if native.block != block.name
                    || native.properties.len() != props.len()
                    || !native
                        .properties
                        .iter()
                        .all(|(key, value)| props.get(key) == Some(value))
                {
                    return Err(format!(
                        "block_physics.json state {id} identity disagrees with blocks.json"
                    ));
                }
                classes[id] = classify(
                    &block.name,
                    &props,
                    native.flags & 0x80 != 0,
                    native.behavior_flags & 0x10 != 0,
                );
            }
        }
        Ok(Self {
            java_version: blocks.java_version,
            blocks: blocks.blocks,
            by_name,
            bedrock: bedrock.bedrock_ordinals,
            classes,
            shape_support,
            shape_wall_above,
            shape_center_support,
            state_physics,
        })
    }

    /// Number of Java block states.
    pub fn state_count(&self) -> usize {
        self.bedrock.len()
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

    /// Actual native sturdy faces D/U/N/S/W/E, connection exception, and motion bits.
    pub fn shape_support_table(&self) -> &[u8] {
        &self.shape_support
    }

    /// Actual wall post/N/E/S/W test-shape coverage by the block above.
    pub fn shape_wall_above_table(&self) -> &[u8] {
        &self.shape_wall_above
    }

    /// Actual native CENTER sturdy faces D/U/N/S/W/E before tag-specific rules.
    pub fn shape_center_support_table(&self) -> &[u8] {
        &self.shape_center_support
    }

    /// Native legacy solidity, solid rendering, collision cube, and replaceability.
    pub fn state_physics_table(&self) -> &[u8] {
        &self.state_physics
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
/// `BLOCKS_MOTION` is captured from native `BlockState.blocksMotion`, including explicit
/// force-solid overrides that cannot be reconstructed from collision bounds.
/// `FULL_TOP_FACE` is the actual native full upper collision face, not a sturdy face.
fn classify(
    name: &str,
    props: &BTreeMap<String, String>,
    blocks_motion: bool,
    full_top_face: bool,
) -> u8 {
    let mut flags = 0u8;
    let short = name.trim_start_matches("minecraft:");
    if matches!(short, "air" | "cave_air" | "void_air") {
        flags |= class::AIR;
    }
    let waterlogged = props.get("waterlogged").is_some_and(|v| v == "true");
    if short == "water" || unastar_packs::block_state::java_secondary_water(name, waterlogged) {
        flags |= class::FLUID | class::WATER;
    }
    if short == "lava" {
        flags |= class::FLUID;
    }
    if short.ends_with("_leaves") {
        flags |= class::LEAVES;
    }
    if blocks_motion {
        flags |= class::BLOCKS_MOTION;
    }
    if full_top_face {
        flags |= class::FULL_TOP_FACE;
    }
    flags
}
