//! Debug worldgen to trace key density function values.
//!
//! Run with: cargo run -p unastar --example debug_worldgen

use unastar::world::generator::density::{
    compute_depth, compute_final_density, compute_preliminary_surface_level, ColumnContext, FlatCacheGrid,
    FunctionContext, NoiseRegistry,
};

fn main() {
    let seed = 0i64; // Use seed 0 for reproducibility
    let noises = NoiseRegistry::new(seed);

    // Test blended noise directly
    println!("=== Testing BlendedNoise directly ===");
    use unastar_noise::NoiseSource;
    for y in [-64, 0, 64, 128].iter() {
        let val = noises.sample_blended_noise(0.0, *y as f64, 0.0, 0.25, 0.125, 80.0, 160.0, 8.0);
        println!("  BlendedNoise at y={}: {:.6}", y, val);
    }
    println!();

    // Test a position - let's check chunk (0, 0)
    let chunk_x = 0;
    let chunk_z = 0;
    let block_x = 0;
    let block_z = 0;

    println!("=== Debug Worldgen for seed {} ===", seed);
    println!("Testing position: block ({}, {})", block_x, block_z);
    println!();

    // Create FlatCacheGrid for this chunk
    let grid = FlatCacheGrid::new(chunk_x, chunk_z, &noises);

    // Print FlatCache values at grid[0][0]
    println!("FlatCache grid values at (0,0):");
    println!("  fc_n9 (continents):  {:.6}", grid.fc_n9[0][0]);
    println!("  fc_n21 (erosion):    {:.6}", grid.fc_n21[0][0]);
    println!("  fc_n26 (ridges):     {:.6}", grid.fc_n26[0][0]);
    println!();

    // Create ColumnContext
    let col = ColumnContext::new(block_x, block_z, &noises, &grid);

    // Print ColumnContext values
    // Note: The actual meaning of these depends on the generated code
    println!("ColumnContext values:");
    println!("  c2d_n36 (offset):    {:.6}", col.c2d_n36);
    println!("  c2d_n64 (factor):    {:.6}", col.c2d_n64);
    println!();

    // Compute preliminary surface level
    let ctx = FunctionContext::new(block_x, 0, block_z);
    let preliminary_surface = compute_preliminary_surface_level(&ctx, &noises, &grid, &col);
    println!("Preliminary surface level: {:.2}", preliminary_surface);
    println!();

    // Compute density at various Y levels
    println!("Density at various Y levels:");
    for y in [-64, -40, -20, 0, 20, 40, 60, 63, 80, 100, 120].iter() {
        let ctx = FunctionContext::new(block_x, *y, block_z);
        let density = compute_final_density(&ctx, &noises, &grid, &col);
        let solid = if density > 0.0 { "SOLID" } else { "AIR" };
        println!("  y={:4}: density={:+.6} ({})", y, density, solid);
    }
    println!();

    // Find the actual surface by binary search
    let mut surface_y = -64;
    for y in (-64..320).rev() {
        let ctx = FunctionContext::new(block_x, y, block_z);
        let density = compute_final_density(&ctx, &noises, &grid, &col);
        if density > 0.0 {
            surface_y = y;
            break;
        }
    }
    println!("Actual surface (first solid from top): y={}", surface_y);
    println!();

    // Test at a few more positions to check for ocean
    println!("Testing multiple positions for ocean detection:");
    for (x, z) in [(0, 0), (100, 100), (-100, -100), (500, 500), (-500, -500)].iter() {
        let cx = x >> 4;
        let cz = z >> 4;
        let grid = FlatCacheGrid::new(cx, cz, &noises);
        let col = ColumnContext::new(*x, *z, &noises, &grid);
        let ctx = FunctionContext::new(*x, 0, *z);
        let continents = grid.fc_n9[(z.rem_euclid(16) >> 2) as usize][(x.rem_euclid(16) >> 2) as usize];
        let prelim = compute_preliminary_surface_level(&ctx, &noises, &grid, &col);

        // Find surface
        let mut surf = -64;
        for y in (-64..320).rev() {
            let ctx = FunctionContext::new(*x, y, *z);
            let density = compute_final_density(&ctx, &noises, &grid, &col);
            if density > 0.0 {
                surf = y;
                break;
            }
        }

        let terrain_type = if continents < -0.44 {
            "DEEP OCEAN"
        } else if continents < -0.18 {
            "OCEAN"
        } else if continents < -0.16 {
            "COAST"
        } else {
            "LAND"
        };

        println!("  ({:5}, {:5}): continents={:+.4}, prelim_surf={:6.2}, actual_surf={:4}, type={}",
            x, z, continents, prelim, surf, terrain_type);
    }

    // Deep dive into problematic position (-100, -100)
    println!("\n=== Deep dive: position (-100, -100) ===");
    let x = -100;
    let z = -100;
    let cx = x >> 4;
    let cz = z >> 4;
    let grid = FlatCacheGrid::new(cx, cz, &noises);
    let col = ColumnContext::new(x, z, &noises, &grid);

    println!("Chunk: ({}, {})", cx, cz);
    println!("ColumnContext:");
    println!("  c2d_n36 (offset): {:.6}", col.c2d_n36);
    println!("  c2d_n64 (factor): {:.6}", col.c2d_n64);

    // Test BlendedNoise directly at this position
    println!("\nBlendedNoise at ({}, y, {}):", x, z);
    for y in [-64, -40, -20, 0, 20, 40, 48, 60, 80, 100].iter() {
        let bn = noises.sample_blended_noise(x as f64, *y as f64, z as f64, 0.25, 0.125, 80.0, 160.0, 8.0);
        println!("  y={:4}: blended_noise={:+.6}", y, bn);
    }

    // Compute depth at various Y levels to see what's happening
    println!("\nDepth values at various Y levels:");
    for y in [-64, -40, -20, 0, 20, 40, 48, 60, 80, 100].iter() {
        let ctx = FunctionContext::new(x, *y, z);
        let depth = compute_depth(&ctx, &noises, &grid, &col);
        // y_clamped_gradient formula: y in [-64, 320] -> [1.5, -1.5]
        let y_gradient = 1.5 + ((*y as f64) - (-64.0)) / (320.0 - (-64.0)) * (-1.5 - 1.5);
        let expected_depth = y_gradient + col.c2d_n36;  // c2d_n36 is offset
        println!("  y={:4}: depth={:.6} (y_grad={:.4}, offset={:.4}, expected={:.4})",
            y, depth, y_gradient, col.c2d_n36, expected_depth);
    }

    println!("\nDensity around preliminary surface (48):");
    for y in [40, 44, 48, 52, 56, 60, 64, 68, 72, 76, 80, 90, 100].iter() {
        let ctx = FunctionContext::new(x, *y, z);
        let density = compute_final_density(&ctx, &noises, &grid, &col);
        let solid = if density > 0.0 { "SOLID" } else { "AIR" };
        println!("  y={:4}: density={:+.6} ({})", y, density, solid);
    }

    println!("\nDensity around actual surface (-13):");
    for y in [-20, -16, -13, -10, -6, 0, 10, 20, 30].iter() {
        let ctx = FunctionContext::new(x, *y, z);
        let density = compute_final_density(&ctx, &noises, &grid, &col);
        let solid = if density > 0.0 { "SOLID" } else { "AIR" };
        println!("  y={:4}: density={:+.6} ({})", y, density, solid);
    }
}
