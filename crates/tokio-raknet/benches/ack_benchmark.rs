use criterion::{Criterion, criterion_group, criterion_main};
use tokio_raknet::protocol::{ack::SequenceRange, types::Sequence24};
use tokio_raknet::session::ack_queue::AckQueue;

fn benchmark_ack_queue(c: &mut Criterion) {
    let mut group = c.benchmark_group("ack_queue");

    group.bench_function("push_sparse", |b| {
        b.iter(|| {
            let mut q = AckQueue::new(1024);
            // Push disjoint ranges (worst case for merging)
            for i in 0..100 {
                q.push(SequenceRange {
                    start: Sequence24::new(i * 2),
                    end: Sequence24::new(i * 2),
                });
            }
            q
        })
    });

    group.bench_function("push_contiguous", |b| {
        b.iter(|| {
            let mut q = AckQueue::new(1024);
            // Push contiguous ranges (best case, should merge into one)
            for i in 0..100 {
                q.push(SequenceRange {
                    start: Sequence24::new(i),
                    end: Sequence24::new(i),
                });
            }
            q
        })
    });

    group.bench_function("pop_for_mtu", |b| {
        // Setup a queue with many small ranges
        let mut q = AckQueue::new(2048);
        for i in 0..500 {
            q.push(SequenceRange {
                start: Sequence24::new(i * 2),
                end: Sequence24::new(i * 2),
            });
        }

        b.iter(|| {
            let mut bench_q = q.clone();
            let mut out = Vec::new();
            bench_q.pop_for_mtu(1400, 20, &mut out);
        })
    });

    group.finish();
}

criterion_group!(benches, benchmark_ack_queue);
criterion_main!(benches);
