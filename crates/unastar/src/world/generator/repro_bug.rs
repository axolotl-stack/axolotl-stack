#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use crate::world::generator::density::{DensityFunction, FunctionContext, NoiseParams, SinglePointContext};
    use crate::world::generator::density::noise_funcs::{Noise, NoiseHolder};
    use crate::world::generator::ore_veinifier::OreVeinifier;
    use crate::world::generator::noise::DoublePerlinNoise;
    use crate::world::generator::xoroshiro::Xoroshiro128;

    // Helper to match overworld.rs hash_seed
    fn hash_seed(seed: i64, salt: &str) -> i64 {
        let mut hash = seed;
        for byte in salt.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(byte as i64);
        }
        hash
    }

    fn create_noise(params: NoiseParams, seed: i64, salt: &str) -> NoiseHolder {
        let salted_seed = hash_seed(seed, salt);
        let mut rng = Xoroshiro128::from_seed(salted_seed);
        let noise = DoublePerlinNoise::new(&mut rng, &params.amplitudes, params.first_octave);
        NoiseHolder::with_noise(params, noise)
    }

    #[test]
    fn test_ore_veinifier_line_z() {
        let seed = 12345;
        
        // Params from overworld.rs
        let toggle_params = NoiseParams::new(-8, vec![1.0]);
        let ridged_params = NoiseParams::new(-7, vec![1.0]);
        let gap_params = NoiseParams::new(-5, vec![1.0]);

        // Create actual noise instances used in generation
        let vein_toggle_holder = create_noise(toggle_params, seed, "ore_vein_a");
        let vein_ridged_holder = create_noise(ridged_params, seed, "ore_vein_b");
        let vein_gap_holder = create_noise(gap_params, seed, "ore_gap");

        let vein_toggle = Arc::new(Noise::with_holder(vein_toggle_holder));
        let vein_ridged = Arc::new(Noise::with_holder(vein_ridged_holder));
        let vein_gap = Arc::new(Noise::with_holder(vein_gap_holder));

        let veinifier = OreVeinifier::new(vein_toggle, vein_ridged, vein_gap, seed);

        // Scan a line in Z at fixed X, Y (deepslate iron level)
        let y = -30;
        let x = 100;
        
        let mut streak = 0;
        println!("Scanning Z 0..200 at X={}, Y={}", x, y);
        for z in 0..200 {
            let ctx = SinglePointContext::new(x, y, z);
            let result = veinifier.compute(&ctx);
            
            if let Some(block) = result {
                // Check if it's Tuff (Iron filler)
                // In test context we don't have block IDs, but any Some is a vein block
                streak += 1;
            } else {
                if streak > 10 {
                    println!("Found streak of length {} ending at Z={}", streak, z);
                }
                streak = 0;
            }
        }
        if streak > 10 {
            println!("Found streak of length {} ending at Z=200", streak);
        }
    }
}