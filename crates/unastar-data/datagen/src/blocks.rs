//! Normalize native BDS measurements without reordering canonical palette ordinals.

use crate::nbt::NetworkLittleEndian;
use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use zuri_nbt::NBTTag;

#[derive(Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StateData {
    destroy_speed: f64,
    explosion_resistance: f64,
    opaque_full_block: bool,
    light_emission: u8,
    light_damping: u8,
    hashed_network_id: u32,
    #[serde(flatten)]
    additional: BTreeMap<String, serde_json::Value>,
}

fn read_json<T: serde::de::DeserializeOwned>(dir: &Path, name: &str) -> miette::Result<T> {
    let path = dir.join(name);
    let bytes = std::fs::read(&path).map_err(|e| miette::miette!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| miette::miette!("{}: {e}", path.display()))
}

pub fn write_blocks_kdl(native_dir: &Path, output_dir: &Path) -> miette::Result<()> {
    let version: serde_json::Value = read_json(native_dir, "version.json")?;
    if version["version"] != "1.26.51" || version["protocol"] != 2193 {
        return Err(miette::miette!(
            "native block data must be Bedrock 1.26.51 protocol 2193"
        ));
    }
    let properties: BTreeMap<String, serde_json::Value> =
        read_json(native_dir, "block_properties_table.json")?;
    let state_data: BTreeMap<u32, StateData> = read_json(native_dir, "block_state_data.json")?;
    let tags: BTreeMap<String, Vec<String>> = read_json(native_dir, "block_tags.json")?;
    let bytes = std::fs::read(native_dir.join("canonical_block_states.nbt"))
        .map_err(|e| miette::miette!("canonical palette: {e}"))?;
    let mut input = bytes.as_slice();
    let mut doc = KdlDocument::new();
    let mut seen = BTreeSet::new();
    let mut count = 0u32;
    let mut current: Option<KdlNode> = None;
    while !input.is_empty() {
        let root = NBTTag::read(&mut input, NetworkLittleEndian)
            .map_err(|e| miette::miette!("canonical state {count}: {e}"))?;
        let NBTTag::Compound(root) = root else {
            return Err(miette::miette!("state {count} is not compound"));
        };
        let Some(NBTTag::String(name)) = root.0.get("name") else {
            return Err(miette::miette!("state {count} lacks name"));
        };
        let Some(NBTTag::Compound(states)) = root.0.get("states") else {
            return Err(miette::miette!("state {count} lacks properties"));
        };
        if !properties.contains_key(&name.0) {
            return Err(miette::miette!("state {count} unknown block {}", name.0));
        }
        let data = state_data
            .get(&count)
            .ok_or_else(|| miette::miette!("missing state measurements {count}"))?;
        if data.light_emission > 15
            || data.light_damping > 15
            || !data.destroy_speed.is_finite()
            || !data.explosion_resistance.is_finite()
        {
            return Err(miette::miette!("invalid state measurements {count}"));
        }
        let same = current
            .as_ref()
            .and_then(|n| n.get(0))
            .and_then(KdlValue::as_string)
            == Some(name.0.as_str());
        if !same {
            if let Some(node) = current.take() {
                doc.nodes_mut().push(node);
            }
            if !seen.insert(name.0.clone()) {
                return Err(miette::miette!("noncontiguous palette block {}", name.0));
            }
            let mut node = KdlNode::new("block");
            node.push(name.0.clone());
            // Registry identity is the canonical first ordinal, not a legacy Java/Prismarine ID.
            prop(&mut node, "id", KdlValue::Integer(count.into()));
            prop(&mut node, "name", KdlValue::String(name.0.clone()));
            prop(&mut node, "hardness", KdlValue::Float(data.destroy_speed));
            prop(
                &mut node,
                "resistance",
                KdlValue::Float(data.explosion_resistance),
            );
            prop(
                &mut node,
                "is_transparent",
                KdlValue::Bool(!data.opaque_full_block),
            );
            prop(
                &mut node,
                "emit_light",
                KdlValue::Integer(data.light_emission.into()),
            );
            prop(
                &mut node,
                "filter_light",
                KdlValue::Integer(data.light_damping.into()),
            );
            prop(&mut node, "min_state_id", KdlValue::Integer(count.into()));
            prop(
                &mut node,
                "default_state_id",
                KdlValue::Integer(count.into()),
            );
            prop(
                &mut node,
                "default_state_source",
                KdlValue::String("weak_first_canonical_state_not_measured_default".into()),
            );
            prop(
                &mut node,
                "native_properties_json",
                KdlValue::String(serde_json::to_string(&properties[&name.0]).unwrap()),
            );
            let mut children = KdlDocument::new();
            if let Some(tags) = tags.get(&name.0) {
                for tag in tags {
                    let mut n = KdlNode::new("tag");
                    n.push(tag.clone());
                    children.nodes_mut().push(n);
                }
            }
            node.set_children(children);
            current = Some(node);
        }
        let node = current.as_mut().unwrap();
        node.insert("max_state_id", i128::from(count));
        let first = node.get("min_state_id").unwrap().as_integer().unwrap();
        node.insert("state_id_count", i128::from(count) - first + 1);
        let mut state = KdlNode::new("state");
        state.push(i128::from(count));
        prop(
            &mut state,
            "network_hash",
            KdlValue::Integer(data.hashed_network_id.into()),
        );
        let mut values = BTreeMap::new();
        for (key, value) in &states.0 {
            let v = match value {
                NBTTag::Byte(v) => serde_json::json!({"byte": v.0}),
                NBTTag::Int(v) => serde_json::json!({"int": v.0}),
                NBTTag::String(v) => serde_json::json!({"string": v.0}),
                _ => return Err(miette::miette!("unsupported state property {key}")),
            };
            values.insert(key, v);
        }
        prop(
            &mut state,
            "properties_json",
            KdlValue::String(serde_json::to_string(&values).unwrap()),
        );
        // Preserve state-dependent physical data rather than projecting all states to the first.
        prop(
            &mut state,
            "measurements_json",
            KdlValue::String(serde_json::to_string(data).unwrap()),
        );
        node.children_mut()
            .as_mut()
            .unwrap()
            .nodes_mut()
            .push(state);
        count += 1;
    }
    if let Some(node) = current {
        doc.nodes_mut().push(node);
    }
    if count == 0
        || state_data.len() != count as usize
        || properties.keys().any(|name| !seen.contains(name))
        || tags.keys().any(|name| !seen.contains(name))
    {
        return Err(miette::miette!(
            "native block domain or state count mismatch"
        ));
    }
    doc.autoformat();
    std::fs::create_dir_all(output_dir).map_err(|e| miette::miette!("{e}"))?;
    std::fs::write(output_dir.join("blocks.kdl"), format!("// AUTO-GENERATED by unastar-data-gen\n// Source: protocolgen native 1.26.51 (protocol 2193)\n// Block count: {}\n// Canonical state ID count: {count}\n{doc}", seen.len()))
        .map_err(|e| miette::miette!("{e}"))?;
    Ok(())
}

fn prop(node: &mut KdlNode, name: &str, value: KdlValue) {
    node.push(KdlEntry::new_prop(name, value));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_uses_native_1_26_51_ordinals_and_network_hashes() {
        let doc: KdlDocument = include_str!("../../output/blocks.kdl").parse().unwrap();
        for (name, expected, hash) in [
            ("minecraft:stone", 3317, 2150698529i128),
            ("minecraft:wooden_door", 7380, 650702320),
            ("minecraft:air", 17025, 3690217760),
        ] {
            let row = doc
                .nodes()
                .iter()
                .find(|n| n.get(0).and_then(KdlValue::as_string) == Some(name))
                .unwrap();
            assert_eq!(
                row.get("min_state_id").and_then(KdlValue::as_integer),
                Some(expected)
            );
            let state = row
                .children()
                .unwrap()
                .nodes()
                .iter()
                .find(|n| n.name().value() == "state")
                .unwrap();
            assert_eq!(
                state.get("network_hash").and_then(KdlValue::as_integer),
                Some(hash)
            );
            let measurements: serde_json::Value =
                serde_json::from_str(state.get("measurements_json").unwrap().as_string().unwrap())
                    .unwrap();
            let native: BTreeMap<u32, StateData> = read_json(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../valentine_gen/protocolgen/data/source/inputs/1.26.51/native"),
                "block_state_data.json",
            )
            .unwrap();
            assert_eq!(
                measurements["friction"],
                native[&(expected as u32)].additional["friction"]
            );
        }
    }
}
