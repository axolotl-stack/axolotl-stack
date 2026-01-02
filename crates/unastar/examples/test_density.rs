//! Test density function evaluation to understand why preliminary_surface_level returns -64.
//!
//! Run with: cargo run --example test_density

use unastar::world::generator::density::{
    NoiseRegistry, FlatCacheGrid, ColumnContext, FunctionContext,
    compute_final_density, compute_preliminary_surface_level,
};
use unastar_noise::NoiseSource;

fn main() {
    println!("Testing density function evaluation...\n");

    let seed = 0i64;
    let noises = NoiseRegistry::new(seed);

    // Test chunk (0, 0) at block position (8, ?, 8) - center of chunk
    let chunk_x = 0;
    let chunk_z = 0;
    let block_x = 8;
    let block_z = 8;

    // Create FlatCacheGrid for the chunk
    let grid = FlatCacheGrid::new(chunk_x, chunk_z, &noises);
    let col = ColumnContext::new(block_x, block_z, &noises, &grid);

    println!("Testing position ({}, ?, {})", block_x, block_z);
    println!("\nDensity values at different Y levels:");
    println!("{:<6} {:<15}", "Y", "Final Density");
    println!("{:-<25}", "");

    // Test from y=128 down to y=-64 (same as find_top_surface search range)
    for y in [128, 100, 80, 64, 40, 20, 0, -20, -40, -64] {
        let ctx = FunctionContext::new(block_x, y, block_z);
        let density = compute_final_density(&ctx, &noises, &grid, &col);
        println!("{:<6} {:<15.6}", y, density);
    }

    // Now test preliminary_surface_level with grid-based ColumnContext
    println!("\n\nTesting preliminary_surface_level with grid-based ColumnContext:");
    let ctx = FunctionContext::new(block_x, 64, block_z);
    let surface_y = compute_preliminary_surface_level(&ctx, &noises, &grid, &col);
    println!("preliminary_surface_level({}, 64, {}) = {} (using ColumnContext::new with grid)", block_x, block_z, surface_y);

    // Test with standalone ColumnContext (like aquifer uses)
    println!("\nTesting preliminary_surface_level with standalone ColumnContext:");
    let col_standalone = ColumnContext::new_standalone(block_x, block_z, &noises);
    let surface_y_standalone = compute_preliminary_surface_level(&ctx, &noises, &grid, &col_standalone);
    println!("preliminary_surface_level({}, 64, {}) = {} (using ColumnContext::new_standalone)", block_x, block_z, surface_y_standalone);

    println!("\n\nExpected: Surface should be around y=60-80 for ocean/plains terrain");
    println!("Actual: {}", surface_y);

    if surface_y == -64.0 {
        println!("\n⚠️  WARNING: preliminary_surface_level returned -64 (lower bound)");
        println!("This means find_top_surface never found density > 0.0");
        println!("Checking if ALL densities are negative...");

        let all_negative = (128..=-64).step_by(8).all(|y| {
            let ctx = FunctionContext::new(block_x, y, block_z);
            compute_final_density(&ctx, &noises, &grid, &col) <= 0.0
        });

        if all_negative {
            println!("✗ All densities are ≤ 0.0 in search range!");
        } else {
            println!("✓ Some densities are > 0.0, but find_top_surface missed them");
        }
    }
}
