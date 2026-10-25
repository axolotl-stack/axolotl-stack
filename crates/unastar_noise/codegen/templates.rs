//! Structure templates (`data/minecraft/structure/**/*.nbt`) read from the
//! local Java server jar at build time (feature `java-templates`, env
//! `UNASTAR_JAVA_SERVER_JAR`) and embedded as one binary blob.
//!
//! Templates are Mojang assets: they are never committed; the build reads them
//! from the user's jar. Without the feature the generated table is empty and
//! `TEMPLATES_EMBEDDED` is `false`.
//!
//! Per palette, blocks are stored in Java's `StructureTemplate` load order:
//! full-cube blocks without block entities, then other blocks without block
//! entities, then blocks with block entities, each group sorted by (y, x, z).
//! Processors draw randomness per block in this order.
//!
//! Blob layout (little endian): `count: u32`, `offsets: [u32; count]`, then per
//! template: `size: [i32; 3]`, `palettes: u16`, per palette
//! `blocks: u32` and `blocks` records of `x, y, z: u8, state: u16, entity: u16`
//! (`0xFFFF` = none); `entities: u16` block-entity records (kind byte:
//! 0 jigsaw, 1 data marker, 2 native id + NETWORK child fields + lifted loot/spawner);
//! `entity_count: u16` entity records (`pos: [f64; 3]`, `block_pos: [i32; 3]`,
//! `id`, raw Java NBT).

use crate::codegen::java_blocks::{JavaBlocks, Result, write_pretty};
use quote::quote;
use std::collections::BTreeMap;
use std::path::Path;
use unastar_packs::{
    block_entity,
    nbt::{self, Tag},
};

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u16).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}

/// Parse a block state string (`minecraft:oak_stairs[facing=north,half=top]`).
pub fn parse_state_string(blocks: &JavaBlocks, s: &str) -> Result<u16> {
    let (name, props) = match s.find('[') {
        Some(i) => (&s[..i], s[i + 1..].trim_end_matches(']')),
        None => (s, ""),
    };
    let name = if name.contains(':') {
        name.to_string()
    } else {
        format!("minecraft:{name}")
    };
    let mut map = BTreeMap::new();
    for kv in props.split(',').filter(|kv| !kv.is_empty()) {
        let (k, v) = kv
            .split_once('=')
            .ok_or_else(|| format!("bad block state string {s}"))?;
        map.insert(k.to_string(), v.to_string());
    }
    blocks.state(&name, &map)
}

fn state_of(blocks: &JavaBlocks, entry: &Tag) -> Result<u16> {
    let name = entry
        .get("Name")
        .and_then(Tag::as_str)
        .ok_or("template palette entry without Name")?;
    let mut props = BTreeMap::new();
    if let Some(p) = entry.get("Properties").and_then(Tag::as_compound) {
        for (k, v) in p {
            props.insert(
                k.clone(),
                v.as_str().ok_or("non-string property")?.to_string(),
            );
        }
    }
    blocks.state(name, &props)
}

fn encode_template(origin: &str, blocks: &JavaBlocks, root: &Tag, out: &mut Vec<u8>) -> Result<()> {
    let size = root
        .get("size")
        .and_then(Tag::as_list)
        .ok_or("template without size")?;
    let size: Vec<i32> = size.iter().map(|t| t.as_int().unwrap_or(0)).collect();
    if size.len() != 3 || size.iter().any(|s| !(0..=255).contains(s)) {
        return Err(format!("template size {size:?} out of range").into());
    }
    for s in &size {
        out.extend_from_slice(&s.to_le_bytes());
    }
    let palettes: Vec<&[Tag]> = match root.get("palettes").and_then(Tag::as_list) {
        Some(ps) => ps.iter().filter_map(Tag::as_list).collect(),
        None => vec![root.get("palette").and_then(Tag::as_list).unwrap_or(&[])],
    };
    let block_list = root.get("blocks").and_then(Tag::as_list).unwrap_or(&[]);

    // Block entities are shared by every palette (they come from the block list).
    let mut be_records: Vec<Vec<u8>> = Vec::new();
    let mut be_of_block: Vec<u16> = Vec::with_capacity(block_list.len());
    for b in block_list {
        match b.get("nbt") {
            None => be_of_block.push(u16::MAX),
            Some(tag) => {
                let mut rec = Vec::new();
                let id = tag.get("id").and_then(Tag::as_str).unwrap_or("");
                match id {
                    "minecraft:jigsaw" => {
                        rec.push(0);
                        let s =
                            |k: &str| tag.get(k).and_then(Tag::as_str).unwrap_or("").to_string();
                        put_str(&mut rec, &s("name"));
                        put_str(&mut rec, &s("target"));
                        put_str(&mut rec, &s("pool"));
                        let final_state = parse_state_string(blocks, &s("final_state"))?;
                        rec.extend_from_slice(&final_state.to_le_bytes());
                        // Joint: explicit, or aligned for horizontal-facing jigsaws.
                        let joint = match tag.get("joint").and_then(Tag::as_str) {
                            Some("rollable") => 0u8,
                            Some("aligned") => 1,
                            Some(other) => {
                                return Err(format!("unknown jigsaw joint {other}").into());
                            }
                            None => 2, // resolved from the block's orientation at load
                        };
                        rec.push(joint);
                        let prio = |k: &str| tag.get(k).and_then(Tag::as_int).unwrap_or(0);
                        rec.extend_from_slice(&prio("selection_priority").to_le_bytes());
                        rec.extend_from_slice(&prio("placement_priority").to_le_bytes());
                    }
                    "minecraft:structure_block"
                        if tag.get("mode").and_then(Tag::as_str) == Some("DATA") =>
                    {
                        rec.push(1);
                        put_str(
                            &mut rec,
                            tag.get("metadata").and_then(Tag::as_str).unwrap_or(""),
                        );
                    }
                    _ => {
                        let compiled = block_entity::compile(origin, tag)?;
                        rec.push(2);
                        put_str(&mut rec, compiled.id);
                        put_bytes(&mut rec, &compiled.fields);
                        match compiled.loot {
                            Some((table, seed)) => {
                                rec.push(1);
                                put_str(&mut rec, &table);
                                rec.extend_from_slice(&seed.to_le_bytes());
                            }
                            None => rec.push(0),
                        }
                        match compiled.spawner_entity {
                            Some(entity) => {
                                rec.push(1);
                                put_str(&mut rec, &entity);
                            }
                            None => rec.push(0),
                        }
                    }
                }
                be_of_block.push(be_records.len() as u16);
                be_records.push(rec);
            }
        }
    }

    out.extend_from_slice(&(palettes.len() as u16).to_le_bytes());
    for palette in &palettes {
        let states: Vec<u16> = palette
            .iter()
            .map(|e| state_of(blocks, e))
            .collect::<Result<_>>()?;
        let mut normal = Vec::new();
        let mut special = Vec::new();
        let mut with_nbt = Vec::new();
        for (i, b) in block_list.iter().enumerate() {
            let pos: Vec<i32> = b
                .get("pos")
                .and_then(Tag::as_list)
                .ok_or("template block without pos")?
                .iter()
                .map(|t| t.as_int().unwrap_or(0))
                .collect();
            let idx = b
                .get("state")
                .and_then(Tag::as_int)
                .ok_or("block without state")? as usize;
            let state = *states.get(idx).ok_or("template state index out of range")?;
            let rec = (pos[1], pos[0], pos[2], state, be_of_block[i]);
            if be_of_block[i] != u16::MAX {
                with_nbt.push(rec);
            } else if blocks.full_cube_static(state) {
                normal.push(rec);
            } else {
                special.push(rec);
            }
        }
        for list in [&mut normal, &mut special, &mut with_nbt] {
            list.sort_by_key(|r| (r.0, r.1, r.2));
        }
        let total = normal.len() + special.len() + with_nbt.len();
        out.extend_from_slice(&(total as u32).to_le_bytes());
        for (y, x, z, state, be) in normal.into_iter().chain(special).chain(with_nbt) {
            out.extend_from_slice(&[x as u8, y as u8, z as u8]);
            out.extend_from_slice(&state.to_le_bytes());
            out.extend_from_slice(&be.to_le_bytes());
        }
    }

    out.extend_from_slice(&(be_records.len() as u16).to_le_bytes());
    for r in &be_records {
        out.extend_from_slice(r);
    }

    let entities = root.get("entities").and_then(Tag::as_list).unwrap_or(&[]);
    out.extend_from_slice(&(entities.len() as u16).to_le_bytes());
    for e in entities {
        let pos = e.get("pos").and_then(Tag::as_list).unwrap_or(&[]);
        for i in 0..3 {
            let v = match pos.get(i) {
                Some(Tag::Double(d)) => *d,
                _ => 0.0,
            };
            out.extend_from_slice(&v.to_le_bytes());
        }
        let bp = e.get("blockPos").and_then(Tag::as_list).unwrap_or(&[]);
        for i in 0..3 {
            let v = bp.get(i).and_then(Tag::as_int).unwrap_or(0);
            out.extend_from_slice(&v.to_le_bytes());
        }
        let tag = e.get("nbt");
        let id = tag
            .and_then(|t| t.get("id"))
            .and_then(Tag::as_str)
            .unwrap_or("");
        put_str(out, id);
        put_bytes(out, &tag.map(nbt::encode_java).unwrap_or_default());
    }
    Ok(())
}

/// Emit `templates.rs` (+ `templates.bin`). With `jar = None` the table is empty.
pub fn emit(out_dir: &Path, blocks: &JavaBlocks, jar: Option<&Path>) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let mut blob = Vec::new();
    if let Some(jar) = jar {
        let zip = crate::codegen::jar::open_server_data(jar)?;
        let prefix = "data/minecraft/structure/";
        let members: Vec<String> = zip
            .names()
            .filter(|n| n.starts_with(prefix) && n.ends_with(".nbt"))
            .map(str::to_string)
            .collect();
        if members.is_empty() {
            return Err(format!("{}: no structure templates under {prefix}", jar.display()).into());
        }
        let mut records = Vec::with_capacity(members.len());
        for m in &members {
            let gz = zip.read(m)?;
            let root = nbt::read_gzip_java(m, &gz)?;
            let mut rec = Vec::new();
            encode_template(m, blocks, &root, &mut rec).map_err(|e| format!("{m}: {e}"))?;
            let name = format!("minecraft:{}", &m[prefix.len()..m.len() - 4]);
            names.push(name);
            records.push(rec);
        }
        let count = records.len() as u32;
        blob.extend_from_slice(&count.to_le_bytes());
        let mut offset = 4 + 4 * records.len();
        for r in &records {
            blob.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += r.len();
        }
        for r in records {
            blob.extend_from_slice(&r);
        }
    } else {
        blob.extend_from_slice(&0u32.to_le_bytes());
    }
    std::fs::write(out_dir.join("templates.bin"), &blob)?;
    let embedded = jar.is_some();
    let n = names.len();
    let tokens = quote! {
        /// Whether structure templates were embedded (feature `java-templates`).
        pub const TEMPLATES_EMBEDDED: bool = #embedded;
        /// Template ids (`minecraft:igloo/top`), sorted; index = template id.
        pub static TEMPLATE_NAMES: [&str; #n] = [#(#names),*];
        /// Encoded templates (see `codegen/templates.rs` for the layout).
        pub static TEMPLATE_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/templates.bin"));
    };
    write_pretty(&out_dir.join("templates.rs"), tokens)?;
    Ok(names)
}
