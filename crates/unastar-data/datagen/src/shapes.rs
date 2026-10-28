//! Per-state block collision, outline and liquid-clip shapes.
//!
//! Input: protocolgen's generated block catalog (`generated/data/block`, the
//! pinned `crates/valentine_gen/protocolgen` submodule). Its geometry comes
//! from [CloudburstMC/Data](https://github.com/CloudburstMC/Data) (Apache
//! License 2.0) at the revision recorded in protocolgen's
//! `generated/data/semantic_sources.json`; see protocolgen's
//! `data/NOTICE.md`. The catalog is Go source with a fixed generated layout:
//!
//! - `index_generated.go`: `var blocks` (block variable names, indexed by
//!   `State.Block`), `var shapes` (box lists; entry 0 is `nil`, the
//!   *unavailable* shape), `var properties` (rows carrying the
//!   `CollisionShape`, `OutlineShape` and `LiquidClipShape` indices) and
//!   `var states` (one `<block>States[i]` reference per palette position).
//! - `<block>_generated.go`: `var <Block> = Block{Name: ...}` and
//!   `var <block>States = [...]State{{Hash, Block, Properties}, ...}`.
//!
//! Palette positions are the canonical NBT state order, so they must equal
//! unastar's state ids. The step checks every state's identifier and network
//! hash against the native palette (`canonical_block_states.nbt`,
//! `block_state_data.json`) and fails on any difference, and checks that the
//! unavailable liquid-clip shapes are exactly the omissions protocolgen
//! records in `semantic_sources.json`.
//!
//! Output: `output/shapes.json`, a deduplicated shape list (index 0 reserved
//! for "unavailable") and one `[collision, outline, liquid_clip]` index row per
//! state, decoded at startup by `unastar_data::shapes`.

use crate::nbt::NetworkLittleEndian;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;
use tracing::info;
use zuri_nbt::NBTTag;

/// Artifact file name under `output/`.
pub const ARTIFACT: &str = "shapes.json";
/// Artifact layout version; bump with `unastar_data::shapes`.
pub const FORMAT: &str = "unastar-block-shapes-v1";

/// One box: `[min_x, min_y, min_z, max_x, max_y, max_z]` in block-local units.
pub type ShapeBox = [f32; 6];

/// One palette state of the protocolgen catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogState {
    pub name: String,
    pub hash: u32,
    /// Catalog shape indices: collision, outline, liquid clip.
    pub shapes: [u16; 3],
}

/// The parsed protocolgen block catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    /// `None` is an unavailable shape (only index 0 in the catalog).
    pub shapes: Vec<Option<Vec<ShapeBox>>>,
    /// States in palette order.
    pub states: Vec<CatalogState>,
}

/// Hash protocolgen reserves for `minecraft:unknown` in place of the native
/// measurement; its own generator accepts exactly this difference
/// (`data/internal/generator/native_blocks.go`).
const UNKNOWN_BLOCK: &str = "minecraft:unknown";
const UNKNOWN_BLOCK_HASH: u32 = u32::MAX - 1;

#[derive(Deserialize)]
struct SemanticSources {
    cloudburst_ref: String,
    native_ref: String,
    liquid_clip_omissions: Vec<Omission>,
    counts: SemanticCounts,
}

#[derive(Deserialize)]
struct Omission {
    name: String,
    hash: u32,
}

#[derive(Deserialize)]
struct SemanticCounts {
    block_states: usize,
    unavailable_liquid_clip_shapes: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeStateHash {
    hashed_network_id: u32,
}

/// Write `shapes.json` from protocolgen's catalog under `generated_data_dir`
/// (`.../protocolgen/generated/data`) after checking it against the native
/// palette in `native_dir`.
pub fn write_shapes(
    generated_data_dir: &Path,
    native_dir: &Path,
    output_dir: &Path,
) -> miette::Result<()> {
    let block_dir = generated_data_dir.join("block");
    let index_path = block_dir.join("index_generated.go");
    let index = read_text(&index_path)?;
    let catalog = parse_catalog(&block_dir)?;
    let semantic: SemanticSources = serde_json::from_str(&read_text(
        &generated_data_dir.join("semantic_sources.json"),
    )?)
    .map_err(|e| miette::miette!("semantic_sources.json: {e}"))?;

    if catalog.states.len() != semantic.counts.block_states {
        return Err(miette::miette!(
            "catalog has {} states, semantic_sources.json counts {}",
            catalog.states.len(),
            semantic.counts.block_states
        ));
    }
    check_native_alignment(&catalog, &native_palette(native_dir)?)?;
    check_liquid_clip_omissions(&catalog, &semantic)?;

    let json = render(&catalog, &semantic, &sha256_hex(index.as_bytes()));
    std::fs::create_dir_all(output_dir).map_err(|e| miette::miette!("{e}"))?;
    let path = output_dir.join(ARTIFACT);
    std::fs::write(&path, json).map_err(|e| miette::miette!("{}: {e}", path.display()))?;
    info!(
        "Wrote {} ({} states) to {}",
        ARTIFACT,
        catalog.states.len(),
        path.display()
    );
    Ok(())
}

/// Read a text input with CRLF folded to LF (Windows `core.autocrlf`
/// checkouts of the submodule), so parsing and digests match the committed
/// bytes.
fn read_text(path: &Path) -> miette::Result<String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| miette::miette!("{}: {e}", path.display()))?;
    Ok(text.replace("\r\n", "\n"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// Parse protocolgen's generated block catalog in `block_dir`.
pub fn parse_catalog(block_dir: &Path) -> miette::Result<Catalog> {
    let index = read_text(&block_dir.join("index_generated.go"))?;
    let mut names = HashMap::new();
    let mut state_arrays = HashMap::new();
    let entries = std::fs::read_dir(block_dir)
        .map_err(|e| miette::miette!("{}: {e}", block_dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| miette::miette!("{e}"))?.path();
        let Some(file) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if file.ends_with("_generated.go") && file != "index_generated.go" {
            parse_block_file(&read_text(&path)?, &mut names, &mut state_arrays)
                .map_err(|e| miette::miette!("{file}: {e}"))?;
        }
    }
    build_catalog(&index, &names, &state_arrays)
}

/// Parsed `{Hash, Block, Properties}` state literal.
type StateLiteral = (u32, usize, usize);

fn build_catalog(
    index: &str,
    names: &HashMap<String, String>,
    state_arrays: &HashMap<String, Vec<StateLiteral>>,
) -> miette::Result<Catalog> {
    let blocks = section(index, "var blocks = [...]Block{")?
        .into_iter()
        .map(|line| {
            let var = line.trim().trim_end_matches(',');
            names
                .get(var)
                .cloned()
                .ok_or_else(|| miette::miette!("block {var} has no definition"))
        })
        .collect::<miette::Result<Vec<_>>>()?;
    let shapes = section(index, "var shapes = [...][]Box{")?
        .into_iter()
        .map(parse_shape)
        .collect::<miette::Result<Vec<_>>>()?;
    if shapes.first() != Some(&None) || shapes[1..].iter().any(Option::is_none) {
        return Err(miette::miette!(
            "catalog shape 0 must be the only unavailable (nil) shape"
        ));
    }
    let properties = section(index, "var properties = [...]Properties{")?
        .into_iter()
        .map(|line| {
            Ok([
                field(line, "CollisionShape")?,
                field(line, "OutlineShape")?,
                field(line, "LiquidClipShape")?,
            ])
        })
        .collect::<miette::Result<Vec<[u16; 3]>>>()?;

    let mut states = Vec::new();
    for (position, line) in section(index, "var states = [...]State{")?
        .into_iter()
        .enumerate()
    {
        let reference = line.trim().trim_end_matches(',');
        let (var, slot) = reference
            .strip_suffix(']')
            .and_then(|r| r.split_once('['))
            .ok_or_else(|| miette::miette!("state {position}: bad reference {reference}"))?;
        let slot: usize = slot
            .parse()
            .map_err(|e| miette::miette!("state {position}: {e}"))?;
        let &(hash, block, props) = state_arrays
            .get(var)
            .and_then(|array| array.get(slot))
            .ok_or_else(|| miette::miette!("state {position}: {reference} is undefined"))?;
        let name = blocks
            .get(block)
            .ok_or_else(|| miette::miette!("state {position}: block {block} out of range"))?;
        let row = *properties
            .get(props)
            .ok_or_else(|| miette::miette!("state {position}: properties {props} out of range"))?;
        if let Some(bad) = row.iter().find(|&&i| usize::from(i) >= shapes.len()) {
            return Err(miette::miette!(
                "state {position}: shape {bad} out of range"
            ));
        }
        states.push(CatalogState {
            name: name.clone(),
            hash,
            shapes: row,
        });
    }
    Ok(Catalog { shapes, states })
}

/// Lines between `header` and the closing `}` of a generated Go composite literal.
fn section<'a>(src: &'a str, header: &str) -> miette::Result<Vec<&'a str>> {
    let start = src
        .lines()
        .position(|line| line == header)
        .ok_or_else(|| miette::miette!("missing `{header}`"))?;
    let lines: Vec<&str> = src
        .lines()
        .skip(start + 1)
        .take_while(|line| *line != "}")
        .collect();
    if lines.is_empty() {
        return Err(miette::miette!("`{header}` is empty"));
    }
    Ok(lines)
}

/// `nil`, `{}` or `{{x0, y0, z0, x1, y1, z1}, ...}` (with a trailing comma).
fn parse_shape(line: &str) -> miette::Result<Option<Vec<ShapeBox>>> {
    let literal = line.trim().trim_end_matches(',');
    if literal == "nil" {
        return Ok(None);
    }
    let inner = literal
        .strip_prefix('{')
        .and_then(|l| l.strip_suffix('}'))
        .ok_or_else(|| miette::miette!("bad shape literal {literal}"))?;
    let mut boxes = Vec::new();
    for group in inner.split('}') {
        let group = group.trim_start_matches([',', ' ']);
        if group.is_empty() {
            continue;
        }
        let values = group
            .strip_prefix('{')
            .ok_or_else(|| miette::miette!("bad box in {literal}"))?
            .split(", ")
            .map(|v| v.parse::<f32>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| miette::miette!("bad box value in {literal}: {e}"))?;
        let b: ShapeBox = values
            .try_into()
            .map_err(|_| miette::miette!("box without 6 values in {literal}"))?;
        if b.iter().any(|v| !v.is_finite()) || (0..3).any(|axis| b[axis] > b[axis + 3]) {
            return Err(miette::miette!("invalid box in {literal}"));
        }
        boxes.push(b);
    }
    Ok(Some(boxes))
}

/// The integer value of `Name: N` in a generated struct literal line.
fn field(line: &str, name: &str) -> miette::Result<u16> {
    let key = format!(" {name}: ");
    let start = line
        .find(&key)
        .ok_or_else(|| miette::miette!("missing {name} in {line}"))?
        + key.len();
    let digits: String = line[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits
        .parse()
        .map_err(|e| miette::miette!("bad {name} in {line}: {e}"))
}

fn parse_block_file(
    src: &str,
    names: &mut HashMap<String, String>,
    state_arrays: &mut HashMap<String, Vec<StateLiteral>>,
) -> miette::Result<()> {
    let mut lines = src.lines();
    while let Some(line) = lines.next() {
        if let Some(var) = line
            .strip_prefix("var ")
            .and_then(|l| l.strip_suffix(" = Block{"))
        {
            let name_line = lines
                .next()
                .ok_or_else(|| miette::miette!("{var} lacks Name"))?;
            let name = name_line
                .trim()
                .strip_prefix("Name:")
                .map(str::trim)
                .and_then(|n| n.strip_suffix("\","))
                .and_then(|n| n.strip_prefix('"'))
                .ok_or_else(|| miette::miette!("{var}: bad Name line {name_line}"))?;
            names.insert(var.to_string(), name.to_string());
        } else if let Some(var) = line
            .strip_prefix("var ")
            .and_then(|l| l.strip_suffix(" = [...]State{"))
        {
            let mut states = Vec::new();
            for state in lines.by_ref().take_while(|l| *l != "}") {
                states.push((
                    u32::try_from(state_field(state, "Hash")?)
                        .map_err(|e| miette::miette!("{var}: {e}"))?,
                    usize::try_from(state_field(state, "Block")?)
                        .map_err(|e| miette::miette!("{var}: {e}"))?,
                    usize::try_from(state_field(state, "Properties")?)
                        .map_err(|e| miette::miette!("{var}: {e}"))?,
                ));
            }
            state_arrays.insert(var.to_string(), states);
        }
    }
    Ok(())
}

/// `Name: N` in a `{Hash: H, Block: B, Properties: P},` state literal.
fn state_field(line: &str, name: &str) -> miette::Result<u64> {
    let key = format!("{name}: ");
    let start = line
        .find(&key)
        .ok_or_else(|| miette::miette!("missing {name} in {line}"))?
        + key.len();
    let digits: String = line[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits
        .parse()
        .map_err(|e| miette::miette!("bad {name} in {line}: {e}"))
}

/// `(identifier, network hash)` per native canonical state, in palette order.
fn native_palette(native_dir: &Path) -> miette::Result<Vec<(String, u32)>> {
    let state_data: BTreeMap<u32, NativeStateHash> =
        serde_json::from_str(&read_text(&native_dir.join("block_state_data.json"))?)
            .map_err(|e| miette::miette!("block_state_data.json: {e}"))?;
    let bytes = std::fs::read(native_dir.join("canonical_block_states.nbt"))
        .map_err(|e| miette::miette!("canonical palette: {e}"))?;
    let mut input = bytes.as_slice();
    let mut palette = Vec::new();
    while !input.is_empty() {
        let id = palette.len() as u32;
        let NBTTag::Compound(root) = NBTTag::read(&mut input, NetworkLittleEndian)
            .map_err(|e| miette::miette!("canonical state {id}: {e}"))?
        else {
            return Err(miette::miette!("canonical state {id} is not a compound"));
        };
        let Some(NBTTag::String(name)) = root.0.get("name") else {
            return Err(miette::miette!("canonical state {id} lacks a name"));
        };
        let hash = state_data
            .get(&id)
            .ok_or_else(|| miette::miette!("missing state measurements {id}"))?
            .hashed_network_id;
        palette.push((name.0.clone(), hash));
    }
    if palette.len() != state_data.len() {
        return Err(miette::miette!(
            "{} canonical states but {} state measurements",
            palette.len(),
            state_data.len()
        ));
    }
    Ok(palette)
}

/// Every catalog palette position must be the same state as unastar's state
/// id: same identifier and same network hash.
fn check_native_alignment(catalog: &Catalog, native: &[(String, u32)]) -> miette::Result<()> {
    if catalog.states.len() != native.len() {
        return Err(miette::miette!(
            "catalog has {} states, native palette {}",
            catalog.states.len(),
            native.len()
        ));
    }
    for (id, (state, (name, hash))) in catalog.states.iter().zip(native).enumerate() {
        let hash_matches = state.hash == *hash
            || (state.name == UNKNOWN_BLOCK && state.hash == UNKNOWN_BLOCK_HASH);
        if state.name != *name || !hash_matches {
            return Err(miette::miette!(
                "state {id}: catalog {} {:#x} differs from native {name} {hash:#x}",
                state.name,
                state.hash
            ));
        }
    }
    Ok(())
}

/// Unavailable liquid-clip shapes are exactly the recorded upstream omissions;
/// collision and outline shapes are always available.
fn check_liquid_clip_omissions(
    catalog: &Catalog,
    semantic: &SemanticSources,
) -> miette::Result<()> {
    let mut unavailable: Vec<(&str, u32)> = Vec::new();
    for (id, state) in catalog.states.iter().enumerate() {
        if state.shapes[0] == 0 || state.shapes[1] == 0 {
            return Err(miette::miette!(
                "state {id} ({}) has no collision or outline shape",
                state.name
            ));
        }
        if state.shapes[2] == 0 {
            unavailable.push((&state.name, state.hash));
        }
    }
    let mut recorded: Vec<(&str, u32)> = semantic
        .liquid_clip_omissions
        .iter()
        .map(|o| (o.name.as_str(), o.hash))
        .collect();
    unavailable.sort_unstable();
    recorded.sort_unstable();
    if unavailable != recorded
        || unavailable.len() != semantic.counts.unavailable_liquid_clip_shapes
    {
        return Err(miette::miette!(
            "{} unavailable liquid-clip shapes do not match the {} recorded omissions",
            unavailable.len(),
            recorded.len()
        ));
    }
    Ok(())
}

/// Deduplicate referenced shapes (by exact bits, first use in state order;
/// index 0 stays "unavailable") and render the artifact.
fn render(catalog: &Catalog, semantic: &SemanticSources, index_sha256: &str) -> String {
    let mut shapes: Vec<&[ShapeBox]> = Vec::new();
    let mut by_bits: HashMap<Vec<[u32; 6]>, usize> = HashMap::new();
    let mut remap: HashMap<u16, usize> = HashMap::from([(0, 0)]);
    let mut rows = Vec::with_capacity(catalog.states.len());
    for state in &catalog.states {
        let row = state.shapes.map(|source| {
            *remap.entry(source).or_insert_with(|| {
                let boxes = catalog.shapes[usize::from(source)]
                    .as_deref()
                    .expect("available shape");
                let bits = boxes.iter().map(|b| b.map(f32::to_bits)).collect();
                *by_bits.entry(bits).or_insert_with(|| {
                    shapes.push(boxes);
                    shapes.len()
                })
            })
        });
        rows.push(row);
    }

    let mut out = String::new();
    out.push_str("{\n");
    let _ = writeln!(out, "  \"format\": \"{FORMAT}\",");
    out.push_str("  \"generator\": \"unastar-data-gen --shapes-only\",\n");
    out.push_str("  \"source\": {\n");
    out.push_str(
        "    \"catalog\": \"cinnabar-mc/protocolgen generated/data/block (crates/valentine_gen/protocolgen)\",\n",
    );
    let _ = writeln!(out, "    \"catalog_index_sha256\": \"{index_sha256}\",");
    out.push_str(
        "    \"geometry\": \"CloudburstMC/Data (https://github.com/CloudburstMC/Data)\",\n",
    );
    let _ = writeln!(
        out,
        "    \"geometry_revision\": \"{}\",",
        semantic.cloudburst_ref
    );
    out.push_str("    \"geometry_licence\": \"Apache-2.0\",\n");
    let _ = writeln!(out, "    \"native_revision\": \"{}\",", semantic.native_ref);
    out.push_str("    \"attribution\": \"Block shape geometry from CloudburstMC/Data, licensed under the Apache License, Version 2.0, as projected by cinnabar-mc/protocolgen. See crates/unastar-data/README.md.\"\n");
    out.push_str("  },\n");
    let _ = writeln!(out, "  \"state_count\": {},", rows.len());
    out.push_str("  \"state_columns\": [\"collision\", \"outline\", \"liquid_clip\"],\n");
    out.push_str("  \"shapes\": [\n    null");
    for boxes in &shapes {
        out.push_str(",\n    [");
        for (i, b) in boxes.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push('[');
            for (j, v) in b.iter().enumerate() {
                if j > 0 {
                    out.push_str(", ");
                }
                // Debug keeps the shortest round-trip form and the sign of -0.
                let _ = write!(out, "{v:?}");
            }
            out.push(']');
        }
        out.push(']');
    }
    // One `[collision,outline,liquid_clip]` row per state id, unindented to
    // keep the 22k-row artifact compact while diffs stay one line per state.
    out.push_str("\n  ],\n  \"states\": [");
    for (i, [c, o, l]) in rows.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "\n[{c},{o},{l}]");
    }
    out.push_str("\n  ]\n}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = "package block\n\nvar blocks = [...]Block{\n\tStone,\n\tSlab,\n}\n\nvar shapes = [...][]Box{\n\tnil,\n\t{{0, 0, 0, 1, 1, 1}},\n\t{},\n\t{{0, 0, -0, 1, 0.5, 9.9e-05}, {0, 0.5, 0, 1, 1, 0.5}},\n\t{{0, 0, 0, 1, 1, 1}},\n}\n\nvar properties = [...]Properties{\n\t{Hardness: 1, CollisionShape: 1, OutlineShape: 4, VisualShape: 1, UIShape: 1, LiquidClipShape: 2, Solid: true},\n\t{Hardness: 1, CollisionShape: 3, OutlineShape: 1, VisualShape: 1, UIShape: 1, LiquidClipShape: 0, Solid: false},\n}\n\nvar states = [...]State{\n\tstoneStates[0],\n\tslabStates[1],\n\tslabStates[0],\n}\n";
    const STONE: &str = "package block\n\nvar Stone = Block{\n\tName:           \"minecraft:stone\",\n\tTranslationKey: \"tile.stone.name\",\n}\n\nvar stoneStates = [...]State{\n\t{Hash: 7, Block: 0, Properties: 0},\n}\n";
    const SLAB: &str = "package block\n\nvar Slab = Block{\n\tName:           \"minecraft:slab\",\n\tTranslationKey: \"tile.slab.name\",\n}\n\nvar slabStates = [...]State{\n\t{Hash: 8, Block: 1, Properties: 0},\n\t{Hash: 4294967294, Block: 1, Properties: 1},\n}\n";

    fn catalog() -> Catalog {
        let mut names = HashMap::new();
        let mut arrays = HashMap::new();
        parse_block_file(STONE, &mut names, &mut arrays).unwrap();
        parse_block_file(SLAB, &mut names, &mut arrays).unwrap();
        build_catalog(INDEX, &names, &arrays).unwrap()
    }

    #[test]
    fn catalog_states_resolve_through_block_files_in_palette_order() {
        let catalog = catalog();
        assert_eq!(
            catalog.states,
            vec![
                CatalogState {
                    name: "minecraft:stone".into(),
                    hash: 7,
                    shapes: [1, 4, 2],
                },
                CatalogState {
                    name: "minecraft:slab".into(),
                    hash: 4294967294,
                    shapes: [3, 1, 0],
                },
                CatalogState {
                    name: "minecraft:slab".into(),
                    hash: 8,
                    shapes: [1, 4, 2],
                },
            ]
        );
        assert_eq!(catalog.shapes[0], None);
        assert_eq!(catalog.shapes[2], Some(vec![]));
        let two_boxes = catalog.shapes[3].as_ref().unwrap();
        assert_eq!(two_boxes[0][2].to_bits(), (-0.0f32).to_bits());
        assert_eq!(two_boxes[0][5], 9.9e-5);
    }

    #[test]
    fn render_dedupes_by_bits_and_keeps_unavailable_at_zero() {
        let semantic = SemanticSources {
            cloudburst_ref: "c".into(),
            native_ref: "n".into(),
            liquid_clip_omissions: vec![],
            counts: SemanticCounts {
                block_states: 3,
                unavailable_liquid_clip_shapes: 0,
            },
        };
        let json: serde_json::Value =
            serde_json::from_str(&render(&catalog(), &semantic, "h")).unwrap();
        // Catalog shapes 1 and 4 are identical; the unreferenced shape is dropped.
        assert_eq!(
            json["shapes"],
            serde_json::json!([
                null,
                [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]],
                [],
                [
                    [0.0, 0.0, -0.0, 1.0, 0.5, 9.9e-5],
                    [0.0, 0.5, 0.0, 1.0, 1.0, 0.5]
                ]
            ])
        );
        assert_eq!(
            json["states"],
            serde_json::json!([[1, 1, 2], [3, 1, 0], [1, 1, 2]])
        );
    }

    #[test]
    fn misaligned_palettes_and_unrecorded_omissions_are_rejected() {
        let catalog = catalog();
        let native = |second: &str, hash: u32| {
            vec![
                ("minecraft:stone".to_string(), 7),
                (second.to_string(), hash),
                ("minecraft:slab".to_string(), 8),
            ]
        };
        // Only minecraft:unknown may carry protocolgen's reserved hash.
        assert!(check_native_alignment(&catalog, &native("minecraft:slab", 9)).is_err());
        assert!(check_native_alignment(&catalog, &native("minecraft:stone", 4294967294)).is_err());
        let semantic = |omissions: Vec<Omission>| SemanticSources {
            cloudburst_ref: String::new(),
            native_ref: String::new(),
            counts: SemanticCounts {
                block_states: 3,
                unavailable_liquid_clip_shapes: omissions.len(),
            },
            liquid_clip_omissions: omissions,
        };
        assert!(check_liquid_clip_omissions(&catalog, &semantic(vec![])).is_err());
        assert!(
            check_liquid_clip_omissions(
                &catalog,
                &semantic(vec![Omission {
                    name: "minecraft:slab".into(),
                    hash: 4294967294,
                }])
            )
            .is_ok()
        );
    }
}
