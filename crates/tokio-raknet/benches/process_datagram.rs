use criterion::{Criterion, black_box, criterion_group, criterion_main};

use tokio_raknet::protocol::types::Sequence24;
use tokio_raknet::session::Session;

fn benchmark_process_datagram(c: &mut Criterion) {
    c.bench_function("process_datagram_sequence", |b| {
        b.iter_batched(
            || Session::new(1400),
            |mut session| {
                let seq = Sequence24::new(123);
                black_box(session.process_datagram_sequence(seq));
            },
            criterion::BatchSize::SmallInput,
        )
    });

    c.bench_function("process_datagram_sequence_gap", |b| {
        b.iter_batched(
            || {
                let mut s = Session::new(1400);
                s.process_datagram_sequence(Sequence24::new(0));
                s
            },
            |mut session| {
                // Simulate a gap (0 -> 5)
                let seq = Sequence24::new(5);
                black_box(session.process_datagram_sequence(seq));
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, benchmark_process_datagram);
criterion_main!(benches);
