//! Derives `java_data/blocks.json`, the compact Java block table the feature and
//! structure code generators read at build time.
//!
//! Inputs (not committed):
//! - `reports/blocks.json` from the pinned Java server's data generator
//!   (`java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports`);
//! - `data/pc/<version>/blockCollisionShapes.json` from PrismarineJS
//!   minecraft-data (MIT), which lists the collision boxes of every state.
//!
//! Output per block: registry name, block type, properties in report order,
//! first state id, default state offset, and one flag byte per state (a single
//! number when every state has the same flags):
//! - bit 0: Java "legacy solid": the collision shape is non-empty and its bounds
//!   have an average edge >= 0.7291666 or a height >= 1 (blocks that force the
//!   flag on or off in code are listed in `FORCE_SOLID_*` below);
//! - bits 1..=6: the collision shape covers the full face, in `Direction` order
//!   (down, up, north, south, west, east), used for `has_sturdy_face`;
//! - bit 7: the collision shape is a full cube.
//!
//! Usage: `cargo run -p unastar_noise --example derive_java_blocks -- <reports/blocks.json> <blockCollisionShapes.json> <out.json> <source-note>`

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Blocks whose block properties force `isSolid()` on regardless of shape
/// (signs, banners and pressure plates keep the solidity they had as wooden or
/// stone materials before materials were removed).
const FORCE_SOLID_ON_SUFFIXES: &[&str] = &["_sign", "_banner", "_pressure_plate"];

const EPS: f64 = 1e-7;

fn bounds(boxes: &[Vec<f64>]) -> Option<[f64; 6]> {
    let mut b = [f64::MAX, f64::MAX, f64::MAX, f64::MIN, f64::MIN, f64::MIN];
    if boxes.is_empty() {
        return None;
    }
    for bx in boxes {
        for i in 0..3 {
            b[i] = b[i].min(bx[i]);
            b[i + 3] = b[i + 3].max(bx[i + 3]);
        }
    }
    Some(b)
}

/// Whether the union of `boxes` covers the unit square of one face.
///
/// Sampled on a 1/64 grid of cell centres: vanilla shapes use 1/16 steps, so
/// this is exact for them.
fn face_full(boxes: &[Vec<f64>], axis: usize, at_max: bool) -> bool {
    let (u, v) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let plane = if at_max { 1.0 } else { 0.0 };
    let n = 64;
    for i in 0..n {
        for j in 0..n {
            let pu = (i as f64 + 0.5) / n as f64;
            let pv = (j as f64 + 0.5) / n as f64;
            let covered = boxes.iter().any(|bx| {
                let touches = if at_max {
                    (bx[axis + 3] - plane).abs() < EPS
                } else {
                    (bx[axis] - plane).abs() < EPS
                };
                touches && bx[u] <= pu && pu <= bx[u + 3] && bx[v] <= pv && pv <= bx[v + 3]
            });
            if !covered {
                return false;
            }
        }
    }
    true
}

fn flags(name: &str, boxes: &[Vec<f64>]) -> u8 {
    let mut f = 0u8;
    let short = name.trim_start_matches("minecraft:");
    let force_on = FORCE_SOLID_ON_SUFFIXES.iter().any(|s| short.ends_with(s));
    if let Some(b) = bounds(boxes) {
        let (dx, dy, dz) = (b[3] - b[0], b[4] - b[1], b[5] - b[2]);
        let size = (dx + dy + dz) / 3.0;
        if size >= 0.7291666666666666 || dy >= 1.0 {
            f |= 1;
        }
        // Direction order: down, up, north (-z), south (+z), west (-x), east (+x).
        let faces = [
            (1, false),
            (1, true),
            (2, false),
            (2, true),
            (0, false),
            (0, true),
        ];
        for (i, (axis, max)) in faces.into_iter().enumerate() {
            if face_full(boxes, axis, max) {
                f |= 1 << (i + 1);
            }
        }
        let full = boxes.iter().any(|bx| {
            bx[0].abs() < EPS
                && bx[1].abs() < EPS
                && bx[2].abs() < EPS
                && (bx[3] - 1.0).abs() < EPS
                && (bx[4] - 1.0).abs() < EPS
                && (bx[5] - 1.0).abs() < EPS
        });
        if full {
            f |= 1 << 7;
        }
    }
    if force_on {
        f |= 1;
    }
    f
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!(
            "usage: derive_java_blocks <reports/blocks.json> <blockCollisionShapes.json> <out.json> <source-note>"
        );
        std::process::exit(2);
    }
    let report: Map<String, Value> =
        serde_json::from_str(&std::fs::read_to_string(&args[1]).expect("read report"))
            .expect("parse report");
    let shapes: Value =
        serde_json::from_str(&std::fs::read_to_string(&args[2]).expect("read shapes"))
            .expect("parse shapes");
    let shape_of_block = shapes["blocks"].as_object().expect("blocks");
    let shape_table = shapes["shapes"].as_object().expect("shapes");

    // Sort by first state id so the table index order is the Java registry order.
    let mut blocks: Vec<(&String, &Value)> = report.iter().collect();
    blocks.sort_by_key(|(_, v)| v["states"][0]["id"].as_u64().unwrap());

    let mut out = Vec::new();
    let mut expected_id = 0u64;
    for (name, def) in blocks {
        let states = def["states"].as_array().unwrap();
        let first = states[0]["id"].as_u64().unwrap();
        assert_eq!(first, expected_id, "{name}: state ids are not contiguous");
        expected_id += states.len() as u64;
        let short = name.trim_start_matches("minecraft:");
        let shape_ids: Vec<u64> = match shape_of_block.get(short) {
            Some(Value::Number(n)) => vec![n.as_u64().unwrap(); states.len()],
            Some(Value::Array(a)) => a.iter().map(|v| v.as_u64().unwrap()).collect(),
            _ => panic!("{name}: no collision shape in minecraft-data"),
        };
        assert_eq!(shape_ids.len(), states.len(), "{name}: state count mismatch");
        let state_flags: Vec<u8> = shape_ids
            .iter()
            .map(|id| {
                let boxes: Vec<Vec<f64>> =
                    serde_json::from_value(shape_table[&id.to_string()].clone()).unwrap();
                flags(name, &boxes)
            })
            .collect();
        let default = states
            .iter()
            .position(|s| s.get("default").and_then(Value::as_bool) == Some(true))
            .unwrap();
        let props: Vec<Value> = def
            .get("properties")
            .and_then(Value::as_object)
            .map(|m| m.iter().map(|(k, v)| json!([k, v])).collect())
            .unwrap_or_default();
        // Verify the mixed-radix layout (last property varies fastest), which the
        // generated state index computation relies on.
        let prop_values: Vec<(String, Vec<String>)> = props
            .iter()
            .map(|p| {
                (
                    p[0].as_str().unwrap().to_string(),
                    serde_json::from_value(p[1].clone()).unwrap(),
                )
            })
            .collect();
        for (i, s) in states.iter().enumerate() {
            let mut idx = 0usize;
            for (k, vals) in &prop_values {
                let v = s["properties"][k].as_str().unwrap();
                idx = idx * vals.len() + vals.iter().position(|x| x == v).unwrap();
            }
            assert_eq!(idx, i, "{name}: state {i} is not in mixed-radix order");
        }
        let flags_json = if state_flags.iter().all(|f| *f == state_flags[0]) {
            json!(state_flags[0])
        } else {
            json!(state_flags)
        };
        let mut entry = BTreeMap::new();
        entry.insert("name", json!(name));
        entry.insert("type", def["definition"]["type"].clone());
        entry.insert("props", json!(props));
        entry.insert("default", json!(default));
        entry.insert("flags", flags_json);
        out.push(entry);
    }
    let doc = json!({ "source": args[4], "state_count": expected_id, "blocks": out });
    let mut text = serde_json::to_string(&doc).unwrap();
    // One block per line keeps diffs reviewable.
    text = text.replace("},{\"default\"", "},\n{\"default\"");
    std::fs::write(&args[3], text + "\n").expect("write output");
    eprintln!("wrote {} blocks, {} states", report.len(), expected_id);
}
