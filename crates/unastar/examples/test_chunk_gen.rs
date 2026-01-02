//! Test chunk generation with aquifer to verify preliminary_surface_level is working.
//!
//! Run with: cargo run --example test_chunk_gen

use unastar::world::generator::VanillaGenerator;

fn main() {
    println!("Generating chunk (0, 0) to test aquifer...\n");

    let generator = VanillaGenerator::new(0);
    let chunk = generator.generate_chunk(0, 0);

    println!("\nChunk generated successfully!");
    println!("Chunk size: 16x384x16 blocks (y=-64 to y=319)");

    // Count water blocks to see if aquifer is working
    let mut water_count = 0;
    let mut stone_count = 0;
    let mut air_count = 0;

    for y in -64..320 {
        for x in 0..16 {
            for z in 0..16 {
                let block = chunk.get_block(x, y, z);
                match block {
                    1 => stone_count += 1, // Stone
                    8 | 9 => water_count += 1, // Water (still or flowing)
                    0 => air_count += 1, // Air
                    _ => {}
                }
            }
        }
    }

    println!("\nBlock counts:");
    println!("  Air: {}", air_count);
    println!("  Stone/terrain: {}", stone_count);
    println!("  Water: {}", water_count);

    // Check water distribution by Y level
    println!("\nWater distribution by Y level:");
    for y in [-64, -40, -20, 0, 20, 40, 60, 80] {
        let mut count = 0;
        for x in 0..16 {
            for z in 0..16 {
                let block = chunk.get_block(x, y, z);
                if block == 8 || block == 9 {
                    count += 1;
                }
            }
        }
        if count > 0 {
            println!("  Y={:>3}: {} water blocks", y, count);
        }
    }
}
