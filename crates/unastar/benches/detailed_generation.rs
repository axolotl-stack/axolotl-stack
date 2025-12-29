use criterion::{criterion_group, criterion_main, Criterion, black_box};
use unastar::world::generator::density::{
    build_overworld_router, NoiseChunk, WrapVisitor, NoiseRouter,
    SinglePointContext, ContextProvider
};
use unastar::world::generator::surface::{build_overworld_surface_rule, SurfaceSystem};
use unastar::world::generator::BiomeNoise;
use unastar::world::chunk::{Chunk, blocks};

fn bench_density_router_compute(c: &mut Criterion) {
    let seed = 12345;
    let router = build_overworld_router(seed);
    
    // 1. Single Point Compute (Direct)
    c.bench_function("router_compute_single_point", |b| {
        b.iter(|| {
            // Pick a typical coordinate
            let ctx = SinglePointContext::new(100, 64, 100);
            black_box(router.final_density.compute(&ctx));
        })
    });

    // 2. Interpolated Compute (closer to real world usage)
    let chunk_x = 0;
    let chunk_z = 0;
    let cell_width = 4;
    let cell_height = 8;
    let min_y = -64;
    let height = 384;
    
    let mut noise_chunk = NoiseChunk::new(
        chunk_x, 
        chunk_z, 
        cell_width, 
        cell_height, 
        min_y, 
        height
    );
    
    // Wire up caching
    let visitor = WrapVisitor::new(&noise_chunk);
    let mapped_router = router.map_all(&visitor);
    
    // Prepare state for a typical cell
    noise_chunk.initialize_for_first_cell_x();
    noise_chunk.advance_cell_x(0);
    noise_chunk.select_cell_yz(0, 0); // Bottom cell
    noise_chunk.update_for_y(0, 0.5); // mid-cell Y
    noise_chunk.update_for_x(0, 0.5); // mid-cell X
    noise_chunk.update_for_z(0, 0.5); // mid-cell Z

    c.bench_function("router_compute_interpolated", |b| {
        b.iter(|| {
             black_box(mapped_router.final_density.compute(&noise_chunk));
        })
    });
}

fn bench_surface_system(c: &mut Criterion) {
    let seed = 12345;
    let biome_noise = BiomeNoise::from_seed(seed);
    let rule = build_overworld_surface_rule(seed);
    let surface_system = SurfaceSystem::new(seed, rule, biome_noise);
    
    // Setup a dummy chunk
    let mut chunk = Chunk::new(0, 0);
    // Fill with stone so surface rules have something to work on
    for x in 0..16 {
        for z in 0..16 {
            // Set a basic terrain shape
            for y in -64..70 {
                chunk.set_block(x, y, z, *blocks::STONE);
            }
        }
    }
    
    c.bench_function("surface_system_build", |b| {
        b.iter_batched(
            || chunk.clone(),
            |mut c| {
                surface_system.build_surface(&mut c, 0, 0);
            },
            criterion::BatchSize::SmallInput
        )
    });
}

fn bench_full_generation(c: &mut Criterion) {
    use unastar::world::generator::VanillaGenerator;
    let seed = 12345;
    let generator = VanillaGenerator::new(seed);
    
    c.bench_function("generate_chunk_full", |b| {
        b.iter(|| {
            black_box(generator.generate_chunk(0, 0));
        })
    });
}

criterion_group!(benches, bench_density_router_compute, bench_surface_system, bench_full_generation);
criterion_main!(benches);
