//! Benchmarks for world generation performance.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use unastar::world::generator::VanillaGenerator;

/// Benchmark full chunk generation
fn bench_chunk_generation(c: &mut Criterion) {
    let generator = VanillaGenerator::new(12345);

    c.bench_function("generate_chunk", |b| {
        b.iter(|| black_box(generator.generate_chunk(black_box(0), black_box(0))))
    });
}

/// Benchmark chunk generation at different positions
fn bench_chunk_generation_positions(c: &mut Criterion) {
    let generator = VanillaGenerator::new(12345);

    let mut group = c.benchmark_group("chunk_positions");

    for (x, z) in [(0, 0), (100, 100), (-50, 50), (1000, 1000)] {
        group.bench_with_input(
            BenchmarkId::new("generate", format!("({},{})", x, z)),
            &(x, z),
            |b, &(x, z)| b.iter(|| black_box(generator.generate_chunk(black_box(x), black_box(z)))),
        );
    }

    group.finish();
}

fn bench_noise_sampling(c: &mut Criterion) {
    use unastar::world::generator::BiomeNoise;

    let biome_noise = BiomeNoise::from_seed(12345);
    let mut group = c.benchmark_group("noise_sampling");

    // Single sample
    group.bench_function("sample_climate_single", |b| {
        b.iter(|| {
            black_box(biome_noise.sample_climate(black_box(100), black_box(64), black_box(100)))
        })
    });

    // Batch sample (4 at a time)
    group.bench_function("sample_climate_4", |b| {
        let x = [100, 101, 102, 103];
        let z = [100, 100, 100, 100];
        b.iter(|| {
            black_box(biome_noise.sample_climate_4(black_box(x), black_box(64), black_box(z)))
        })
    });

    // Compare: 4 single samples vs 1 batch sample
    group.bench_function("sample_climate_4x_single", |b| {
        b.iter(|| {
            // FIX: Wrap arguments in black_box so the compiler re-evaluates them every loop
            let r1 = biome_noise.sample_climate(black_box(100), black_box(64), black_box(100));
            let r2 = biome_noise.sample_climate(black_box(101), black_box(64), black_box(100));
            let r3 = biome_noise.sample_climate(black_box(102), black_box(64), black_box(100));
            let r4 = biome_noise.sample_climate(black_box(103), black_box(64), black_box(100));
            black_box([r1, r2, r3, r4])
        })
    });

    group.finish();
}

fn bench_perlin_noise(c: &mut Criterion) {
    use unastar::world::generator::noise::PerlinNoise;
    use unastar::world::generator::xoroshiro::Xoroshiro128;

    let mut rng = Xoroshiro128::from_seed(12345);
    let noise = PerlinNoise::new(&mut rng);

    let mut group = c.benchmark_group("perlin_noise");

    // Single sample
    group.bench_function("sample_single", |b| {
        b.iter(|| black_box(noise.sample(black_box(1.5), black_box(2.5), black_box(3.5))))
    });

    // SIMD batch sample
    group.bench_function("sample_4_simd", |b| {
        let x = [1.5, 2.5, 3.5, 4.5];
        let z = [3.5, 3.5, 3.5, 3.5];
        b.iter(|| black_box(noise.sample_4_arrays(black_box(x), black_box(2.5), black_box(z))))
    });

    // Compare: 4 single samples
    group.bench_function("sample_4x_single", |b| {
        b.iter(|| {
            // FIX: Wrap arguments in black_box
            let r1 = noise.sample(black_box(1.5), black_box(2.5), black_box(3.5));
            let r2 = noise.sample(black_box(2.5), black_box(2.5), black_box(3.5));
            let r3 = noise.sample(black_box(3.5), black_box(2.5), black_box(3.5));
            let r4 = noise.sample(black_box(4.5), black_box(2.5), black_box(3.5));
            black_box([r1, r2, r3, r4])
        })
    });

    group.finish();
}

/// Benchmark OctaveNoise
fn bench_octave_noise(c: &mut Criterion) {
    use unastar::world::generator::noise::OctaveNoise;
    use unastar::world::generator::xoroshiro::Xoroshiro128;

    let mut rng = Xoroshiro128::from_seed(12345);
    let amplitudes = [1.0, 1.0, 2.0, 2.0, 2.0, 1.0];
    let noise = OctaveNoise::new(&mut rng, &amplitudes, -9);

    let mut group = c.benchmark_group("octave_noise");

    // Single sample
    group.bench_function("sample_single", |b| {
        b.iter(|| black_box(noise.sample(black_box(0.1), black_box(0.2), black_box(0.3))))
    });

    // SIMD batch sample
    group.bench_function("sample_4_simd", |b| {
        let x = [0.1, 0.2, 0.3, 0.4];
        let z = [0.3, 0.3, 0.3, 0.3];
        b.iter(|| black_box(noise.sample_4_arrays(black_box(x), black_box(0.2), black_box(z))))
    });

    // Compare: 4 single samples
    group.bench_function("sample_4x_single", |b| {
        b.iter(|| {
            // Wrap inputs in black_box to force recalculation every time
            let r1 = noise.sample(black_box(1.5), black_box(2.5), black_box(3.5));
            let r2 = noise.sample(black_box(2.5), black_box(2.5), black_box(3.5));
            let r3 = noise.sample(black_box(3.5), black_box(2.5), black_box(3.5));
            let r4 = noise.sample(black_box(4.5), black_box(2.5), black_box(3.5));
            black_box([r1, r2, r3, r4])
        })
    });

    group.finish();
}

/// Benchmark multiple chunks (like a player loading area)
fn bench_multi_chunk(c: &mut Criterion) {
    let generator = VanillaGenerator::new(12345);

    let mut group = c.benchmark_group("multi_chunk");
    group.sample_size(10); // Fewer samples for expensive benchmarks

    // 9 chunks (3x3 area)
    group.bench_function("generate_3x3", |b| {
        b.iter(|| {
            for x in -1..=1 {
                for z in -1..=1 {
                    black_box(generator.generate_chunk(x, z));
                }
            }
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_perlin_noise,
    bench_octave_noise,
    bench_noise_sampling,
    bench_chunk_generation,
    bench_chunk_generation_positions,
    bench_multi_chunk,
);
criterion_main!(benches);
