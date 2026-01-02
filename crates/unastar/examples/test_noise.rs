//! Test noise sampling to verify values are sensible.
//!
//! Run with: cargo run --example test_noise

use unastar::world::generator::density::NoiseRegistry;
use unastar_noise::{NoiseRef, NoiseSource};

fn main() {
    println!("Testing noise sampling...\n");

    let seed = 0i64;
    let registry = NoiseRegistry::new(seed);

    // Test coordinates: center of chunk (0, 0) at various Y levels
    let test_coords = [
        (8, 64, 8),   // Near sea level
        (8, 128, 8),  // High up
        (8, 0, 8),    // Underground
        (8, -32, 8),  // Deep underground
    ];

    // Key noises used in terrain generation
    let key_noises = [
        (NoiseRef::Continentalness, "Continentalness"),
        (NoiseRef::Erosion, "Erosion"),
        (NoiseRef::Ridge, "Ridge"),
        (NoiseRef::Offset, "Offset"),
    ];

    for (x, y, z) in test_coords {
        println!("Position ({}, {}, {}):", x, y, z);

        for (noise_ref, name) in &key_noises {
            let value = registry.sample(*noise_ref, x as f64, y as f64, z as f64);
            println!("  {}: {:.6}", name, value);
        }
        println!();
    }

    // Also test the 3D noises used in density calculation
    println!("\n3D Terrain Noises at (8, 64, 8):");
    let pos = (8.0, 64.0, 8.0);

    let three_d_noises = [
        (NoiseRef::Offset, "Offset"),
        (NoiseRef::Jagged, "Jagged"),
        (NoiseRef::Temperature, "Temperature"),
        (NoiseRef::Vegetation, "Vegetation"),
    ];

    for (noise_ref, name) in &three_d_noises {
        let value = registry.sample(*noise_ref, pos.0, pos.1, pos.2);
        println!("  {}: {:.6}", name, value);
    }
}
