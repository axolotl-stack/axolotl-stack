//! Report how far unastar's vanilla generator is from the BDS 1.26.51 fixture.
//!
//! Research tool (parked WIP): prints column terrain/surface height, surface
//! block and surface biome match rates plus whole-chunk block histograms for
//! `tests/fixtures/bds_1_26_51_seed_12345.json`.
//! `cargo run -p unastar --example worldgen_parity_report` (from the repo root).

use std::collections::BTreeMap;
use unastar::registry::BlockRegistry;
use unastar::world::VanillaGenerator;

fn is_terrain(name: &str) -> bool {
    let n = name.trim_start_matches("minecraft:");
    matches!(
        n,
        "stone" | "deepslate" | "dirt" | "coarse_dirt" | "grass_block" | "podzol" | "mycelium"
            | "sand" | "red_sand" | "sandstone" | "red_sandstone" | "gravel" | "terracotta"
            | "snow" | "ice" | "packed_ice" | "powder_snow" | "calcite" | "mud" | "bedrock"
            | "tuff" | "granite" | "raw_iron_block" | "raw_copper_block" | "iron_ore"
            | "copper_ore" | "deepslate_iron_ore" | "deepslate_copper_ore"
    ) || n.ends_with("_terracotta")
}

fn main() {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}

fn run() {
    let fx: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string("crates/unastar/tests/fixtures/bds_1_26_51_seed_12345.json")
            .unwrap(),
    )
    .unwrap();
    let seed = fx["seed"].as_i64().unwrap();
    let palette: Vec<String> = fx["palette"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let reg = BlockRegistry::vanilla();
    let generator = VanillaGenerator::new(seed);
    let t0 = std::time::Instant::now();
    let (mut cols, mut h_eq, mut h_close, mut h_abs, mut top_eq, mut biome_eq, mut surf_eq) =
        (0u64, 0u64, 0u64, 0i64, 0u64, 0u64, 0u64);
    let mut hist_bds: BTreeMap<String, i64> = BTreeMap::new();
    let mut hist_una: BTreeMap<String, i64> = BTreeMap::new();
    let mut top_confusion: BTreeMap<(String, String), u32> = BTreeMap::new();
    let mut ocean_bds = 0;
    let mut ocean_una = 0;
    for c in fx["chunks"].as_array().unwrap() {
        let (cx, cz) = (c["x"].as_i64().unwrap() as i32, c["z"].as_i64().unwrap() as i32);
        let chunk = generator.generate_chunk(cx, cz);
        let biome = chunk_biome(&chunk);
        for (k, v) in c["histogram"].as_object().unwrap() {
            *hist_bds.entry(k.clone()).or_default() += v.as_i64().unwrap();
        }
        for i in 0..256usize {
            let (x, z) = ((i & 15) as u8, (i >> 4) as u8);
            let mut top = None;
            let mut terrain = -65;
            for y in (-64..320).rev() {
                let id = chunk.get_block(x, y as i16, z);
                let name = reg.get_by_runtime_id(id).map(|b| b.string_id.as_str()).unwrap_or("?");
                if name == "minecraft:air" {
                    continue;
                }
                if top.is_none() {
                    top = Some((y, name.to_string()));
                }
                if is_terrain(name) {
                    terrain = y;
                    break;
                }
            }
            let (sy, sname) = top.unwrap_or((-64, "minecraft:air".into()));
            let bds_terrain = c["terrain"][i].as_i64().unwrap() as i32;
            let bds_surface = c["surface"][i].as_i64().unwrap() as i32;
            let bds_top = &palette[c["surface_block"][i].as_u64().unwrap() as usize];
            let bds_biome = c["surface_biome"][i].as_u64().unwrap() as u32;
            cols += 1;
            if terrain == bds_terrain {
                h_eq += 1;
            }
            if (terrain - bds_terrain).abs() <= 2 {
                h_close += 1;
            }
            h_abs += (terrain - bds_terrain).abs() as i64;
            if sy == bds_surface {
                surf_eq += 1;
            }
            if &sname == bds_top {
                top_eq += 1;
            }
            if bds_top == "minecraft:water" {
                ocean_bds += 1;
            }
            if sname == "minecraft:water" {
                ocean_una += 1;
            }
            *top_confusion.entry((bds_top.clone(), sname.clone())).or_default() += 1;
            if biome == bds_biome {
                biome_eq += 1;
            }
        }
        for y in -64..320 {
            for x in 0..16u8 {
                for z in 0..16u8 {
                    let id = chunk.get_block(x, y as i16, z);
                    let name = reg.get_by_runtime_id(id).map(|b| b.string_id.as_str()).unwrap_or("?");
                    if name != "minecraft:air" {
                        *hist_una.entry(name.to_string()).or_default() += 1;
                    }
                }
            }
        }
    }
    let n = cols as f64;
    println!("chunks={} columns={} gen_time={:?}", cols / 256, cols, t0.elapsed());
    println!("terrain height exact {:.1}% within2 {:.1}% mean|d| {:.2}", 100.0 * h_eq as f64 / n, 100.0 * h_close as f64 / n, h_abs as f64 / n);
    println!("surface height exact {:.1}%", 100.0 * surf_eq as f64 / n);
    println!("surface block match {:.1}%", 100.0 * top_eq as f64 / n);
    println!("surface biome id match {:.1}%", 100.0 * biome_eq as f64 / n);
    println!("water-top columns bds {} unastar {}", ocean_bds, ocean_una);
    let mut conf: Vec<_> = top_confusion.into_iter().collect();
    conf.sort_by_key(|(_, v)| std::cmp::Reverse(*v));
    for ((b, u), v) in conf.iter().take(15) {
        println!("  top bds={b} unastar={u}: {v}");
    }
    let mut keys: Vec<_> = hist_bds.keys().chain(hist_una.keys()).cloned().collect();
    keys.sort();
    keys.dedup();
    let mut rows: Vec<_> = keys
        .into_iter()
        .map(|k| (hist_bds.get(&k).copied().unwrap_or(0), hist_una.get(&k).copied().unwrap_or(0), k))
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.0.max(r.1)));
    for (b, u, k) in rows.iter().take(40) {
        println!("  hist {k}: bds {b} unastar {u}");
    }
}

fn chunk_biome(chunk: &unastar::world::Chunk) -> u32 {
    let enc = chunk.encode_biomes();
    // first storage: header byte (bits<<1|1), single value as varint (zigzag)
    let mut v: u32 = 0;
    let mut shift = 0;
    for b in &enc[1..] {
        v |= ((b & 0x7f) as u32) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            break;
        }
    }
    v >> 1
}
