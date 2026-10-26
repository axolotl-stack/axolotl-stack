//! Per-state block collision, outline and liquid-clip shapes.
//!
//! The data is `output/shapes.json`, written by `unastar-data-gen --shapes-only`
//! from protocolgen's generated block catalog. Block shape geometry comes from
//! [CloudburstMC/Data](https://github.com/CloudburstMC/Data), licensed under
//! the Apache License, Version 2.0 (revision recorded in the artifact's
//! `source` object and in `output/manifest.kdl`).
//!
//! The artifact is embedded and decoded once, on first use, into a frozen
//! [`ShapeTable`]; lookups afterwards are two slice indexings. State ids are
//! unastar's canonical block state ids (the native NBT palette order, the
//! same ids as [`crate::blocks::by_runtime_id`]); datagen verifies every
//! state's identifier and network hash against that palette.
//!
//! A shape is either a list of boxes (possibly empty, e.g. the collision of
//! air) or [`ShapeRef::Unavailable`]: the source has no valid geometry. In the
//! 1.26.51 data only 30 liquid-clip shapes are unavailable (invalid bounds in
//! the Cloudburst extract, recorded by protocolgen); they are kept explicit,
//! never replaced by a guessed box.

use serde::Deserialize;
use std::sync::LazyLock;

/// Embedded artifact.
const ARTIFACT: &str = include_str!("../output/shapes.json");
/// Artifact layout this decoder understands.
const FORMAT: &str = "unastar-block-shapes-v1";

/// One box: `[min_x, min_y, min_z, max_x, max_y, max_z]` in block-local units
/// (a full block is `[0, 0, 0, 1, 1, 1]`).
pub type ShapeBox = [f32; 6];

/// A state's shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeRef<'a> {
    /// Known geometry; an empty slice is a known empty shape.
    Available(&'a [ShapeBox]),
    /// The source has no valid geometry for this shape.
    Unavailable,
}

impl<'a> ShapeRef<'a> {
    /// The boxes, or `None` for an unavailable shape.
    pub fn boxes(self) -> Option<&'a [ShapeBox]> {
        match self {
            Self::Available(boxes) => Some(boxes),
            Self::Unavailable => None,
        }
    }
}

/// Which per-state shape to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    /// Entity collision.
    Collision = 0,
    /// Selection outline.
    Outline = 1,
    /// Liquid clipping.
    LiquidClip = 2,
}

/// Decoded, immutable shape table.
#[derive(Debug)]
pub struct ShapeTable {
    /// `(start, len)` into `boxes` per shape; `None` = unavailable.
    shapes: Vec<Option<(u32, u32)>>,
    boxes: Vec<ShapeBox>,
    /// Shape index per state and [`ShapeKind`].
    states: Vec<[u16; 3]>,
}

#[derive(Deserialize)]
struct RawArtifact {
    format: String,
    state_count: usize,
    state_columns: [String; 3],
    shapes: Vec<Option<Vec<ShapeBox>>>,
    states: Vec<[u16; 3]>,
}

impl ShapeTable {
    /// Decode an artifact. Fails on any structural inconsistency.
    fn decode(json: &str) -> Result<Self, String> {
        let raw: RawArtifact =
            serde_json::from_str(json).map_err(|e| format!("shapes.json: {e}"))?;
        if raw.format != FORMAT {
            return Err(format!("shapes.json format {} != {FORMAT}", raw.format));
        }
        if raw.state_columns != ["collision", "outline", "liquid_clip"] {
            return Err(format!(
                "shapes.json columns {:?} are not collision/outline/liquid_clip",
                raw.state_columns
            ));
        }
        if raw.states.len() != raw.state_count {
            return Err(format!(
                "shapes.json has {} state rows, header says {}",
                raw.states.len(),
                raw.state_count
            ));
        }
        if raw.shapes.first() != Some(&None) || raw.shapes[1..].iter().any(Option::is_none) {
            return Err("shapes.json: shape 0 must be the only unavailable shape".into());
        }
        let mut boxes = Vec::new();
        let mut shapes = Vec::with_capacity(raw.shapes.len());
        for (index, shape) in raw.shapes.into_iter().enumerate() {
            let Some(shape) = shape else {
                shapes.push(None);
                continue;
            };
            if let Some(bad) = shape.iter().find(|b| {
                b.iter().any(|v| !v.is_finite()) || (0..3).any(|axis| b[axis] > b[axis + 3])
            }) {
                return Err(format!(
                    "shapes.json: shape {index} has invalid box {bad:?}"
                ));
            }
            let start = u32::try_from(boxes.len()).map_err(|e| e.to_string())?;
            let len = u32::try_from(shape.len()).map_err(|e| e.to_string())?;
            boxes.extend(shape);
            shapes.push(Some((start, len)));
        }
        if let Some((state, _)) = raw
            .states
            .iter()
            .enumerate()
            .find(|(_, row)| row.iter().any(|&i| usize::from(i) >= shapes.len()))
        {
            return Err(format!("shapes.json: state {state} names a missing shape"));
        }
        Ok(Self {
            shapes,
            boxes,
            states: raw.states,
        })
    }

    /// Number of states in the table.
    pub fn state_count(&self) -> usize {
        self.states.len()
    }

    /// Shape `kind` of `state_id`; `None` if the state id is outside the palette.
    pub fn get(&self, state_id: u32, kind: ShapeKind) -> Option<ShapeRef<'_>> {
        let index = self.states.get(state_id as usize)?[kind as usize];
        Some(match self.shapes[usize::from(index)] {
            Some((start, len)) => {
                ShapeRef::Available(&self.boxes[start as usize..(start + len) as usize])
            }
            None => ShapeRef::Unavailable,
        })
    }
}

/// The decoded table, decoded from the embedded artifact on first use.
///
/// Dereferencing panics if the embedded artifact is malformed; that is a
/// build defect caught by this crate's tests.
pub static TABLE: LazyLock<ShapeTable> =
    LazyLock::new(|| ShapeTable::decode(ARTIFACT).unwrap_or_else(|e| panic!("{e}")));

/// Collision shape of `state_id`; `None` if the state id is outside the palette.
pub fn collision(state_id: u32) -> Option<ShapeRef<'static>> {
    TABLE.get(state_id, ShapeKind::Collision)
}

/// Selection outline shape of `state_id`; `None` if the state id is outside the palette.
pub fn outline(state_id: u32) -> Option<ShapeRef<'static>> {
    TABLE.get(state_id, ShapeKind::Outline)
}

/// Liquid-clip shape of `state_id`; `None` if the state id is outside the palette.
pub fn liquid_clip(state_id: u32) -> Option<ShapeRef<'static>> {
    TABLE.get(state_id, ShapeKind::LiquidClip)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(shapes: &str, states: &str, count: usize) -> String {
        format!(
            r#"{{"format":"{FORMAT}","state_count":{count},"state_columns":["collision","outline","liquid_clip"],"shapes":{shapes},"states":{states}}}"#
        )
    }

    #[test]
    fn decode_keeps_unavailable_distinct_from_empty() {
        let table = ShapeTable::decode(&artifact(
            "[null,[],[[0.0,0.0,-0.0,1.0,0.5,1.0]]]",
            "[[1,2,0],[2,2,1]]",
            2,
        ))
        .unwrap();
        assert_eq!(
            table.get(0, ShapeKind::Collision),
            Some(ShapeRef::Available(&[]))
        );
        assert_eq!(
            table.get(0, ShapeKind::LiquidClip),
            Some(ShapeRef::Unavailable)
        );
        let Some(ShapeRef::Available([b])) = table.get(1, ShapeKind::Outline) else {
            panic!("one box");
        };
        assert_eq!(b[2].to_bits(), (-0.0f32).to_bits());
        assert_eq!(table.get(2, ShapeKind::Collision), None);
    }

    #[test]
    fn decode_rejects_inconsistent_artifacts() {
        for (shapes, states, count) in [
            // Row count differs from the header.
            ("[null,[]]", "[[1,1,1]]", 2),
            // Index past the shape list.
            ("[null,[]]", "[[1,1,2]]", 1),
            // A second unavailable shape.
            ("[null,null]", "[[1,1,1]]", 1),
            // Shape 0 must be the unavailable one.
            ("[[],[]]", "[[1,1,1]]", 1),
            // Inverted box.
            ("[null,[[1.0,0.0,0.0,0.0,1.0,1.0]]]", "[[1,1,1]]", 1),
        ] {
            assert!(
                ShapeTable::decode(&artifact(shapes, states, count)).is_err(),
                "{shapes} {states} {count}"
            );
        }
    }
}
