use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::Arc;
use unastar::world::generator::density::{
    build_overworld_router, NoiseChunk, SinglePointContext, DensityFunction,
};

fn bench_router_single_point(c: &mut Criterion) {
    let router = build_overworld_router(12345);
    let density = router.final_density;
    let ctx = SinglePointContext::new(100, 64, 100);

    c.bench_function("router_single_point", |b| {
        b.iter(|| black_box(density.compute(black_box(&ctx))))
    });
}

fn bench_noise_chunk_interpolation(c: &mut Criterion) {
    // Setup a NoiseChunk which handles the caching/interpolation logic
    let chunk = Arc::new(NoiseChunk::new(0, 0, 4, 8, -64, 384));
    
    // In a real scenario, we would wire the router to the chunk here
    // but the current implementation might not fully support easy public wiring for benches yet.
    // For now, let's benchmark the raw router evaluation which is the "heavy lifting" 
    // inside the corners of the interpolation.
    
    let router = build_overworld_router(12345);
    let density = router.final_density;
    let ctx = SinglePointContext::new(100, 64, 100);

    c.bench_function("router_compute_heavy", |b| {
        b.iter(|| black_box(density.compute(black_box(&ctx))))
    });
}

fn bench_aquifer_router(c: &mut Criterion) {
    let router = build_overworld_router(12345);
    let barrier = router.barrier_noise;
    let ctx = SinglePointContext::new(100, 64, 100);

    c.bench_function("aquifer_barrier_compute", |b| {
        b.iter(|| black_box(barrier.compute(black_box(&ctx))))
    });
}

criterion_group!(
    benches,
    bench_router_single_point,
    bench_noise_chunk_interpolation,
    bench_aquifer_router
);
criterion_main!(benches);
