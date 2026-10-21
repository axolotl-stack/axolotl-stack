use bench_compare::codec;
use bytes::BytesMut;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn codec_benches(criterion: &mut Criterion) {
    for case in codec::load().expect("validated corpus") {
        let mut group = criterion.benchmark_group(&case.name);
        group.throughput(Throughput::Bytes(case.payload_bytes as u64));
        group.bench_function(
            BenchmarkId::new(case.decode_operation(), "rust-owned"),
            |b| b.iter(|| case.decode_owned()),
        );
        if case.borrowed_supported {
            group.bench_function(
                BenchmarkId::new(case.decode_operation(), "rust-borrowed"),
                |b| b.iter(|| case.decode_borrowed()),
            );
        }
        let mut out = BytesMut::with_capacity(case.batch_entries.len());
        group.bench_function(
            BenchmarkId::new(case.encode_operation(), "rust-owned"),
            |b| b.iter(|| case.encode(&mut out)),
        );
        group.finish();
    }
}
criterion_group!(benches, codec_benches);
criterion_main!(benches);
