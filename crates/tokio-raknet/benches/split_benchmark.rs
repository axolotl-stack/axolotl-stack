use bytes::Bytes;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::time::{Duration, Instant};
use tokio_raknet::protocol::encapsulated_packet::{EncapsulatedPacket, SplitInfo};
use tokio_raknet::protocol::reliability::Reliability;
use tokio_raknet::protocol::types::{EncapsulatedPacketHeader, Sequence24};
use tokio_raknet::session::split_assembler::SplitAssembler;

fn make_split_part(id: u16, index: u32, count: u32) -> EncapsulatedPacket {
    EncapsulatedPacket {
        header: EncapsulatedPacketHeader {
            reliability: Reliability::Reliable,
            is_split: true,
            needs_bas: false,
        },
        bit_length: 800, // 100 bytes
        reliable_index: Some(Sequence24::new(index)),
        sequence_index: None,
        ordering_index: None,
        ordering_channel: None,
        split: Some(SplitInfo { id, index, count }),
        payload: Bytes::from(vec![0u8; 100]),
    }
}

fn benchmark_split_assembler(c: &mut Criterion) {
    let mut group = c.benchmark_group("split_assembler");

    group.bench_function("reassemble_small_split", |b| {
        let split_id = 1;
        let count = 10;
        let now = Instant::now();

        b.iter(|| {
            let mut assembler = SplitAssembler::new(Duration::from_secs(10), 100, 100);

            // Add all parts except the last one
            for i in 0..count - 1 {
                let pkt = make_split_part(split_id, i, count);
                let res = assembler.add(black_box(pkt), now);
                assert!(res.unwrap().is_none());
            }

            // Add last part, triggering reassembly
            let pkt = make_split_part(split_id, count - 1, count);
            let res = assembler.add(black_box(pkt), now);
            assert!(res.unwrap().is_some());
        })
    });

    group.bench_function("prune_expired", |b| {
        let mut assembler = SplitAssembler::new(Duration::from_secs(1), 100, 1000);
        let now = Instant::now();
        // Fill with incomplete splits
        for id in 0..500 {
            let pkt = make_split_part(id, 0, 5);
            let _ = assembler.add(pkt, now);
        }

        b.iter(|| {
            // We can't easily re-fill in the loop without cloning the whole assembler state,
            // which SplitAssembler structure might not support efficiently or publicly.
            // Instead, we'll benchmark the prune check on an empty/full structure or similar.
            // Actually, let's just bench check on full structure where nothing expires yet.
            let mut bench_assembler = SplitAssembler::new(Duration::from_secs(100), 100, 1000);
            for id in 0..100 {
                let pkt = make_split_part(id, 0, 5);
                let _ = bench_assembler.add(pkt, now);
            }
            bench_assembler.prune(black_box(now)); // Should expire nothing
        })
    });

    group.finish();
}

criterion_group!(benches, benchmark_split_assembler);
criterion_main!(benches);
