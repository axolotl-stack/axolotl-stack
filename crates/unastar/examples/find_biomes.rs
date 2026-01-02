//! Find coordinates for specific biomes.
//!
//! Usage: cargo run -p unastar --example find_biomes

use unastar::world::generator::BiomeNoise;
use unastar_noise::Biome;
use std::collections::HashMap;

fn main() {
    let seed: i64 = 0; // Change this to match your world seed
    let noise = BiomeNoise::from_seed(seed);

    // Search in a grid pattern - BIGGER radius
    let search_radius = 10000; // blocks from origin
    let step = 128; // sample every 128 blocks

    let mut biome_locations: HashMap<Biome, Vec<(i32, i32)>> = HashMap::new();
    let mut all_biomes_found: HashMap<Biome, u32> = HashMap::new();

    // Biomes we want to find
    let interesting_biomes = [
        Biome::IceSpikes,
        Biome::MushroomFields,
        Biome::Badlands,
        Biome::ErodedBadlands,
        Biome::WoodedBadlands,
        Biome::WindsweptHills,
        Biome::GravellyMountains,
        Biome::WindsweptForest,
        Biome::StonyShore,
        Biome::Swamp,
        Biome::MangroveSwamp,
        Biome::Jungle,
        Biome::BambooJungle,
        Biome::SparseJungle,
        Biome::CherryGrove,
        Biome::PaleGarden,
        Biome::Meadow,
        Biome::SnowySlopes,
        Biome::Grove,
        Biome::JaggedPeaks,
        Biome::FrozenPeaks,
        Biome::StonyPeaks,
        Biome::SunflowerPlains,
        Biome::FlowerForest,
        Biome::DarkForest,
        Biome::TallBirchForest,
        Biome::SavannaPlateau,
        Biome::WindsweptSavanna,
        Biome::River,
        Biome::FrozenRiver,
        Biome::Beach,
        Biome::SnowyBeach,
    ];

    println!("Searching for biomes (seed={})...", seed);
    println!("Search area: {}x{} blocks, step={}", search_radius * 2, search_radius * 2, step);
    println!();

    let y = 64; // Surface level

    for x in (-search_radius..=search_radius).step_by(step as usize) {
        for z in (-search_radius..=search_radius).step_by(step as usize) {
            let biome = noise.get_biome(x, y, z);

            // Count ALL biomes
            *all_biomes_found.entry(biome).or_default() += 1;

            // Only track interesting biomes, and limit to 3 locations each
            if interesting_biomes.contains(&biome) {
                let locs = biome_locations.entry(biome).or_default();
                if locs.len() < 3 {
                    locs.push((x, z));
                }
            }
        }
    }

    // Print results sorted by biome name
    let mut sorted: Vec<_> = biome_locations.iter().collect();
    sorted.sort_by_key(|(biome, _)| format!("{:?}", biome));

    println!("=== Found Biome Locations ===");
    println!();

    for (biome, locs) in sorted {
        println!("{:?}:", biome);
        for (x, z) in locs {
            println!("  /tp @s {} 100 {}", x, z);
        }
        println!();
    }

    // Print biomes NOT found
    println!("=== Biomes NOT Found ===");
    for biome in &interesting_biomes {
        if !biome_locations.contains_key(biome) {
            println!("  {:?}", biome);
        }
    }

    // Print ALL biomes distribution
    println!();
    println!("=== All Biomes Found (total distribution) ===");
    let mut all_sorted: Vec<_> = all_biomes_found.iter().collect();
    all_sorted.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    for (biome, count) in all_sorted {
        println!("  {:?}: {} samples", biome, count);
    }
}
