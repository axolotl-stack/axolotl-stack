//! The decoded shape table equals protocolgen's generated block catalog for
//! every state. The catalog (Go source, geometry from CloudburstMC/Data,
//! Apache-2.0) is parsed here independently of the datagen step.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use unastar_data::blocks;
use unastar_data::shapes::{self, ShapeBox, ShapeRef};

fn protocolgen_data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../valentine_gen/protocolgen/generated/data")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// Body lines of the generated composite literal opened by `header`.
fn body<'a>(src: &'a str, header: &str) -> Vec<&'a str> {
    let mut lines = src.lines().skip_while(|l| *l != header);
    assert!(lines.next().is_some(), "missing {header}");
    lines.take_while(|l| *l != "}").collect()
}

fn number_after(line: &str, key: &str) -> u64 {
    let start = line.find(key).unwrap_or_else(|| panic!("{key} in {line}")) + key.len();
    line[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap()
}

/// Catalog shapes: `None` for `nil` (unavailable).
fn catalog_shape(line: &str) -> Option<Vec<ShapeBox>> {
    let literal = line.trim().trim_end_matches(',');
    if literal == "nil" {
        return None;
    }
    let inner = &literal[1..literal.len() - 1];
    Some(
        inner
            .split(['{', '}'])
            .map(|part| part.trim_matches([',', ' ']))
            .filter(|part| !part.is_empty())
            .map(|part| {
                let values: Vec<f32> = part.split(", ").map(|v| v.parse().unwrap()).collect();
                values.try_into().unwrap()
            })
            .collect(),
    )
}

type CatalogState = (String, u32, [Option<Vec<ShapeBox>>; 3]);

struct Catalog {
    /// `(identifier, hash, [collision, outline, liquid_clip])` per palette position.
    states: Vec<CatalogState>,
}

fn catalog() -> Catalog {
    let dir = protocolgen_data().join("block");
    let index = read(&dir.join("index_generated.go"));
    let mut names: HashMap<String, String> = HashMap::new();
    let mut arrays: HashMap<String, Vec<(u32, usize, usize)>> = HashMap::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let file = path.file_name().unwrap().to_str().unwrap().to_string();
        if !file.ends_with("_generated.go") || file == "index_generated.go" {
            continue;
        }
        let src = read(&path);
        let mut lines = src.lines();
        while let Some(line) = lines.next() {
            if let Some(var) = line
                .strip_prefix("var ")
                .and_then(|l| l.strip_suffix(" = Block{"))
            {
                let name = lines.next().unwrap();
                let name = name.split('"').nth(1).unwrap();
                names.insert(var.into(), name.into());
            } else if let Some(var) = line
                .strip_prefix("var ")
                .and_then(|l| l.strip_suffix(" = [...]State{"))
            {
                let states = lines
                    .by_ref()
                    .take_while(|l| *l != "}")
                    .map(|l| {
                        (
                            number_after(l, "Hash: ") as u32,
                            number_after(l, "Block: ") as usize,
                            number_after(l, "Properties: ") as usize,
                        )
                    })
                    .collect();
                arrays.insert(var.into(), states);
            }
        }
    }
    let block_names: Vec<&String> = body(&index, "var blocks = [...]Block{")
        .iter()
        .map(|l| &names[l.trim().trim_end_matches(',')])
        .collect();
    let shapes: Vec<Option<Vec<ShapeBox>>> = body(&index, "var shapes = [...][]Box{")
        .into_iter()
        .map(catalog_shape)
        .collect();
    let properties: Vec<[usize; 3]> = body(&index, "var properties = [...]Properties{")
        .into_iter()
        .map(|l| {
            [
                number_after(l, " CollisionShape: ") as usize,
                number_after(l, " OutlineShape: ") as usize,
                number_after(l, " LiquidClipShape: ") as usize,
            ]
        })
        .collect();
    let states = body(&index, "var states = [...]State{")
        .into_iter()
        .map(|l| {
            let reference = l.trim().trim_end_matches(',').trim_end_matches(']');
            let (var, slot) = reference.split_once('[').unwrap();
            let (hash, block, props) = arrays[var][slot.parse::<usize>().unwrap()];
            (
                block_names[block].clone(),
                hash,
                properties[props].map(|i| shapes[i].clone()),
            )
        })
        .collect();
    Catalog { states }
}

fn bits(boxes: &[ShapeBox]) -> Vec<[u32; 6]> {
    boxes.iter().map(|b| b.map(f32::to_bits)).collect()
}

#[test]
fn shape_table_equals_protocolgen_catalog_for_every_state() {
    let catalog = catalog();
    assert_eq!(catalog.states.len(), 22_091);
    assert_eq!(shapes::TABLE.state_count(), 22_091);

    // Palette positions are unastar's state ids: same identifier and the
    // native network hash (protocolgen reserves u32::MAX - 1 for unknown).
    let native: BTreeMap<u32, serde_json::Value> =
        serde_json::from_str(&read(&Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../valentine_gen/protocolgen/data/source/inputs/1.26.51/native/block_state_data.json",
        )))
        .unwrap();

    let mut unavailable = Vec::new();
    for (id, (name, hash, expected)) in catalog.states.iter().enumerate() {
        let id = id as u32;
        assert_eq!(blocks::by_runtime_id(id).unwrap().identifier, name, "{id}");
        let native_hash = native[&id]["hashedNetworkId"].as_u64().unwrap() as u32;
        assert!(
            *hash == native_hash || (name == "minecraft:unknown" && *hash == u32::MAX - 1),
            "state {id} {name}: {hash} != native {native_hash}"
        );
        let actual = [
            shapes::collision(id),
            shapes::outline(id),
            shapes::liquid_clip(id),
        ];
        for (kind, (expected, actual)) in expected.iter().zip(actual).enumerate() {
            match (expected, actual.expect("state in table")) {
                (Some(expected), ShapeRef::Available(actual)) => {
                    assert_eq!(
                        bits(actual),
                        bits(expected),
                        "state {id} {name} kind {kind}"
                    );
                }
                (None, ShapeRef::Unavailable) => unavailable.push((kind, name.clone(), *hash)),
                (expected, actual) => {
                    panic!("state {id} {name} kind {kind}: {expected:?} vs {actual:?}")
                }
            }
        }
    }
    assert_eq!(shapes::collision(22_091), None);

    // Only liquid-clip shapes are unavailable: exactly protocolgen's 30
    // recorded Cloudburst omissions.
    let semantic: serde_json::Value =
        serde_json::from_str(&read(&protocolgen_data().join("semantic_sources.json"))).unwrap();
    let mut recorded: Vec<(usize, String, u32)> = semantic["liquid_clip_omissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                2,
                o["name"].as_str().unwrap().to_string(),
                o["hash"].as_u64().unwrap() as u32,
            )
        })
        .collect();
    unavailable.sort();
    recorded.sort();
    assert_eq!(unavailable.len(), 30);
    assert_eq!(unavailable, recorded);
}
