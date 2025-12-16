use bytes::BytesMut;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use tokio_raknet::protocol::packet::RaknetEncodable;

fn benchmark_varint(c: &mut Criterion) {
    let mut group = c.benchmark_group("varint");

    group.bench_function("u32_small", |b| {
        b.iter(|| {
            let val: u32 = 127; // 1 byte
            let mut buf = BytesMut::with_capacity(5);
            black_box(val).encode_raknet(&mut buf).unwrap();
            buf
        })
    });

    group.bench_function("u32_large", |b| {
        b.iter(|| {
            let val: u32 = u32::MAX; // 5 bytes
            let mut buf = BytesMut::with_capacity(5);
            black_box(val).encode_raknet(&mut buf).unwrap();
            buf
        })
    });

    group.bench_function("u64_small", |b| {
        b.iter(|| {
            let val: u64 = 127; // 1 byte
            let mut buf = BytesMut::with_capacity(10);
            black_box(val).encode_raknet(&mut buf).unwrap();
            buf
        })
    });

    group.bench_function("u64_large", |b| {
        b.iter(|| {
            let val: u64 = u64::MAX; // 10 bytes
            let mut buf = BytesMut::with_capacity(10);
            black_box(val).encode_raknet(&mut buf).unwrap();
            buf
        })
    });

    let mut buf_u32_large = BytesMut::new();
    u32::MAX.encode_raknet(&mut buf_u32_large).unwrap();
    let bytes_u32_large = buf_u32_large.freeze();

    group.bench_function("decode_u32_large", |b| {
        b.iter(|| {
            let mut src = bytes_u32_large.clone();
            u32::decode_raknet(&mut src).unwrap()
        })
    });

    group.finish();
}

criterion_group!(benches, benchmark_varint);
criterion_main!(benches);
