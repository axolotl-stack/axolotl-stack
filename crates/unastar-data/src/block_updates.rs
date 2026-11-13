//! Measured block-update facts, decoded once at startup.
//!
//! Source: `data/measured/block_updates/`, BDS 1.26.51 block scenarios (the
//! `scenarios` sweep of `tools/bds-sweep`; method and hashes in
//! `data/measured/manifest.json`). The scenario files themselves are the
//! evidence unastar's block-update tests replay; this module only exposes the
//! derived tables the server needs at runtime: `connection_faces.json` (the
//! blocks whose face a fence, pane, bars or wall treats differently from
//! their collision shape), `support_holders.json` (which blocks hold each
//! kind of attached block) and `plant_soils.json` (which blocks each plant
//! grows on).

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use serde::Deserialize;

/// How fences, panes, bars and walls treat a neighbour's face.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConnectionFaces {
    /// Blocks they never connect to, although the collision shape covers the
    /// face toward them (leaves, shulker boxes, pumpkins, ...).
    pub never: HashSet<String>,
    /// Blocks they always connect to, although the collision shape does not
    /// cover the face (honey, mud, soul sand, structure void).
    pub always: HashSet<String>,
    /// Single states that never connect although their block's other states
    /// follow the collision face: `[block, states]`.
    #[serde(default)]
    pub never_states: Vec<(String, serde_json::Map<String, serde_json::Value>)>,
    /// Single states that always connect although their block's other states
    /// follow the collision face (a full snow layer).
    #[serde(default)]
    pub always_states: Vec<(String, serde_json::Map<String, serde_json::Value>)>,
}

impl ConnectionFaces {
    /// Measured override for `identifier`: `Some(true)` always connects,
    /// `Some(false)` never, `None` follows the collision face.
    pub fn verdict(&self, identifier: &str) -> Option<bool> {
        if self.always.contains(identifier) {
            Some(true)
        } else if self.never.contains(identifier) {
            Some(false)
        } else {
            None
        }
    }

    /// The measured single-state overrides of `identifier`: each state's
    /// properties and whether it connects.
    pub fn state_verdicts<'a>(
        &'a self,
        identifier: &'a str,
    ) -> impl Iterator<Item = (&'a serde_json::Map<String, serde_json::Value>, bool)> + 'a {
        let listed = |list: &'a [(String, serde_json::Map<String, serde_json::Value>)], verdict| {
            list.iter()
                .filter(move |(block, _)| block == identifier)
                .map(move |(_, states)| (states, verdict))
        };
        listed(&self.always_states, true).chain(listed(&self.never_states, false))
    }
}

/// Decode the embedded table; consumers normally use [`connection_faces`].
pub fn decode_connection_faces() -> Result<ConnectionFaces, serde_json::Error> {
    serde_json::from_str(include_str!(
        "../data/measured/block_updates/connection_faces.json"
    ))
}

/// The measured connection-face overrides of the vanilla blocks.
pub fn connection_faces() -> &'static ConnectionFaces {
    static FACES: LazyLock<ConnectionFaces> = LazyLock::new(|| {
        decode_connection_faces().expect("embedded BDS connection-face table decodes")
    });
    &FACES
}

/// The shape test a holder passes, on its collision box face toward the
/// attached block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HolderRule {
    /// Any block but air.
    Present,
    /// Any collision box.
    Collision,
    /// The collision covers the whole face.
    FaceFull,
    /// The collision covers the middle 2/16 square of the face.
    FaceCentre,
}

/// Which blocks hold one kind of attached block.
#[derive(Debug, Clone, Deserialize)]
pub struct HolderClass {
    pub rule: HolderRule,
    /// Blocks that hold it whatever the rule says.
    pub always: HashSet<String>,
    /// Blocks that never hold it whatever the rule says.
    pub never: HashSet<String>,
    /// Single holder states (block, states) that hold it against the rule.
    pub always_states: Vec<(String, serde_json::Map<String, serde_json::Value>)>,
    /// Single holder states that do not hold it although the rule says so.
    pub never_states: Vec<(String, serde_json::Map<String, serde_json::Value>)>,
    /// Holders measured.
    pub cases: usize,
}

/// Holder classes keyed `<attached block>/<support side>` (`down`, `east`,
/// `up`), e.g. `minecraft:torch/down`.
#[derive(Debug, Clone, Deserialize)]
pub struct SupportHolders {
    pub classes: HashMap<String, HolderClass>,
}

/// Decode the embedded table; consumers normally use [`support_holders`].
pub fn decode_support_holders() -> Result<SupportHolders, serde_json::Error> {
    serde_json::from_str(include_str!(
        "../data/measured/block_updates/support_holders.json"
    ))
}

/// The measured holders of every swept kind of attached block.
pub fn support_holders() -> &'static SupportHolders {
    static HOLDERS: LazyLock<SupportHolders> = LazyLock::new(|| {
        decode_support_holders().expect("embedded BDS support-holder table decodes")
    });
    &HOLDERS
}

#[derive(Deserialize)]
struct PlantSoilsFile {
    plants: HashMap<String, HashSet<String>>,
    #[serde(default)]
    plant_states: HashMap<String, Vec<SoilState>>,
}

/// A single soil state a plant stays on: `[block, states]`.
pub type SoilState = (String, serde_json::Map<String, serde_json::Value>);

fn plant_soils_file() -> &'static PlantSoilsFile {
    static SOILS: LazyLock<PlantSoilsFile> = LazyLock::new(|| {
        serde_json::from_str::<PlantSoilsFile>(include_str!(
            "../data/measured/block_updates/plant_soils.json"
        ))
        .expect("embedded BDS plant-soil table decodes")
    });
    &SOILS
}

/// The blocks each swept plant stays on (every measured state of the soil),
/// keyed by plant identifier.
pub fn plant_soils() -> &'static HashMap<String, HashSet<String>> {
    &plant_soils_file().plants
}

/// The single soil states a plant stays on where the soil's other measured
/// states do not hold it (a lily pad on a water source, not on flowing
/// water), keyed by plant identifier.
pub fn plant_soil_states() -> &'static HashMap<String, Vec<SoilState>> {
    &plant_soils_file().plant_states
}

#[derive(Deserialize)]
struct BlockActorDefaultsFile {
    actors: HashMap<String, String>,
}

/// The block actor BDS 1.26.51 announces when a player places each of these
/// blocks (`block_actor_defaults.json`, measured from the placement
/// captures), as network NBT; the position inside is the capture's.
pub fn block_actor_default(identifier: &str) -> Option<&'static [u8]> {
    static DEFAULTS: LazyLock<HashMap<String, Vec<u8>>> = LazyLock::new(|| {
        serde_json::from_str::<BlockActorDefaultsFile>(include_str!(
            "../data/measured/block_actor_defaults.json"
        ))
        .expect("embedded BDS block-actor defaults decode")
        .actors
        .into_iter()
        .map(|(block, hex)| {
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
                .collect();
            (block, bytes)
        })
        .collect()
    });
    DEFAULTS.get(identifier).map(Vec::as_slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_connection_faces_name_vanilla_blocks() {
        let faces = connection_faces();
        assert_eq!(faces.verdict("minecraft:mud"), Some(true));
        assert_eq!(faces.verdict("minecraft:oak_leaves"), Some(false));
        assert_eq!(faces.verdict("minecraft:stone"), None);
        for name in faces.never.iter().chain(&faces.always) {
            assert!(
                crate::blocks::ALL_BLOCKS
                    .iter()
                    .any(|block| block.identifier == name),
                "{name} is not a vanilla block"
            );
        }
        assert!(faces.never.is_disjoint(&faces.always));
    }

    #[test]
    fn measured_support_tables_decode() {
        let holders = support_holders();
        let torch = &holders.classes["minecraft:torch/down"];
        assert_eq!(torch.rule, HolderRule::FaceCentre);
        assert!(torch.never.contains("minecraft:oak_leaves"));
        assert_eq!(
            holders.classes["minecraft:white_carpet/down"].rule,
            HolderRule::Present
        );
        let soils = plant_soils();
        assert!(soils["minecraft:wheat"].contains("minecraft:farmland"));
        assert!(!soils["minecraft:dandelion"].contains("minecraft:stone"));
    }
}
