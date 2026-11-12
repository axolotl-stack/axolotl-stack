//! Bounded load-before-generate pool.
//!
//! Storage reads and stack-heavy generation run outside the ECS tick on a fixed set of
//! worker threads (16 MiB stacks for Windows terrain generation). Only an explicit storage
//! miss permits generation; read errors never produce replacement terrain. Generation is a
//! pure function of `(seed, position)` (see `world::generator::pipeline`), so the number
//! of workers and the order they take requests in never change the output. Positions
//! outside `world.bounds` are generated as all-air by [`ChunkPipeline::generate_within`],
//! the same helper the inline path and the spawn search use.
//!
//! Requests are served in queue order, except that a worker skips ahead (a bounded
//! distance) past requests close to ones already in flight: neighbouring chunks share
//! the source chunks they decorate, and workers on them mostly wait for each other.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use tokio::sync::oneshot;

use crate::storage::{ChunkColumn, StorageResult, WorldProvider};
use crate::world::generator::ChunkPipeline;
use crate::world::{ChunkPos, WorldBounds};

/// The chunk column (with its stored actors) plus its storage provenance
/// (`true` means loaded, not generated).
pub type ChunkLoadResult = StorageResult<(ChunkColumn, bool)>;

const CHUNK_GENERATION_STACK_SIZE: usize = 16 * 1024 * 1024;

/// Most worker threads picked automatically. A radius-12 join scales from
/// about 160 chunks/s on 8 workers to about 250 on 16 (load_bench).
const MAX_AUTO_THREADS: usize = 16;

/// Chebyshev distance at which two requests touch disjoint source chunks: a
/// finished chunk decorates the 3x3 around it, and each of those decorations
/// locks its own 3x3, so a request uses the 5x5 around its position.
const INDEPENDENT_DISTANCE: i32 = 5;

/// How far past the queue front a worker may look, per worker thread, for a
/// request independent of those in flight.
const LOOKAHEAD_PER_THREAD: usize = 4;

/// Request to load or generate a chunk.
struct ChunkGenRequest {
    pos: ChunkPos,
    provider: Option<Arc<dyn WorldProvider>>,
    dimension: i32,
    response_tx: oneshot::Sender<ChunkLoadResult>,
}

struct Queue {
    pending: VecDeque<ChunkGenRequest>,
    in_flight: Vec<ChunkPos>,
    /// Every handle is gone: workers exit once `pending` drains.
    closed: bool,
    /// Workers still running; none left means requests can never complete.
    workers: usize,
}

struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
    lookahead: usize,
}

impl Shared {
    /// The queue is consistent whenever its lock is released, so a worker
    /// that panicked elsewhere does not stop the others.
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Closes the queue when the last handle drops.
struct Closer(Arc<Shared>);

impl Drop for Closer {
    fn drop(&mut self) {
        self.0.lock().closed = true;
        self.0.ready.notify_all();
    }
}

/// Counts a worker out even if it unwinds.
struct WorkerExit<'a>(&'a Shared);

impl Drop for WorkerExit<'_> {
    fn drop(&mut self) {
        self.0.lock().workers -= 1;
    }
}

/// Index of the request to serve: the first of the next `lookahead` positions
/// independent of every in-flight position, else the front.
fn pick(
    pending: impl IntoIterator<Item = ChunkPos>,
    in_flight: &[ChunkPos],
    lookahead: usize,
) -> usize {
    pending
        .into_iter()
        .take(lookahead)
        .position(|pos| {
            in_flight.iter().all(|busy| {
                (busy.x - pos.x).abs().max((busy.z - pos.z).abs()) >= INDEPENDENT_DISTANCE
            })
        })
        .unwrap_or(0)
}

/// Marks a position in flight until the worker is done with it (or unwinds).
struct InFlight<'a> {
    shared: &'a Shared,
    pos: ChunkPos,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let mut queue = self.shared.lock();
        if let Some(index) = queue.in_flight.iter().position(|&pos| pos == self.pos) {
            queue.in_flight.swap_remove(index);
        }
    }
}

/// Handle to the chunk generation pool. Dropping every handle stops the workers once
/// queued requests are done.
#[derive(Clone)]
pub struct ChunkGenerationWorker {
    shared: Arc<Shared>,
    _closer: Arc<Closer>,
    threads: usize,
}

/// Worker count for a configured value (0 = half the cores, clamped to 1..=16).
pub fn generation_threads(configured: usize) -> usize {
    if configured > 0 {
        return configured;
    }
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    (cores / 2).clamp(1, MAX_AUTO_THREADS)
}

impl ChunkGenerationWorker {
    /// Spawn `threads` workers driving `pipeline` within `bounds`. Storage reads run on
    /// the current tokio runtime (this must be called inside one).
    pub fn spawn(pipeline: Arc<ChunkPipeline>, bounds: WorldBounds, threads: usize) -> Self {
        assert!(threads > 0, "generation pool needs at least one thread");
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                pending: VecDeque::new(),
                in_flight: Vec::with_capacity(threads),
                closed: false,
                workers: threads,
            }),
            ready: Condvar::new(),
            lookahead: threads * LOOKAHEAD_PER_THREAD,
        });
        let runtime = tokio::runtime::Handle::current();
        for index in 0..threads {
            let pipeline = pipeline.clone();
            let shared = shared.clone();
            let runtime = runtime.clone();
            std::thread::Builder::new()
                .name(format!("unastar-chunk-gen-{index}"))
                .stack_size(CHUNK_GENERATION_STACK_SIZE)
                .spawn(move || Self::run_worker(&pipeline, bounds, &shared, &runtime))
                .expect("spawn chunk generation worker");
        }
        Self {
            _closer: Arc::new(Closer(shared.clone())),
            shared,
            threads,
        }
    }

    /// Number of worker threads.
    pub fn threads(&self) -> usize {
        self.threads
    }

    /// Queue a chunk; the receiver yields the loaded or generated chunk.
    /// Returns `None` if the pool has shut down.
    pub fn generate(
        &self,
        x: i32,
        z: i32,
        provider: Option<Arc<dyn WorldProvider>>,
        dimension: i32,
    ) -> Option<oneshot::Receiver<ChunkLoadResult>> {
        let (response_tx, response_rx) = oneshot::channel();
        {
            let mut queue = self.shared.lock();
            if queue.workers == 0 {
                return None;
            }
            queue.pending.push_back(ChunkGenRequest {
                pos: ChunkPos::new(x, z),
                provider,
                dimension,
                response_tx,
            });
        }
        self.shared.ready.notify_one();
        Some(response_rx)
    }

    /// The next request to serve, marked in flight; `None` once closed and drained.
    fn next_request(shared: &Shared) -> Option<(ChunkGenRequest, InFlight<'_>)> {
        let mut queue = shared.lock();
        loop {
            if !queue.pending.is_empty() {
                let index = pick(
                    queue.pending.iter().map(|request| request.pos),
                    &queue.in_flight,
                    shared.lookahead,
                );
                let request = queue
                    .pending
                    .remove(index)
                    .expect("picked a queued request");
                queue.in_flight.push(request.pos);
                let pos = request.pos;
                return Some((request, InFlight { shared, pos }));
            }
            if queue.closed {
                return None;
            }
            queue = shared
                .ready
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn run_worker(
        pipeline: &ChunkPipeline,
        bounds: WorldBounds,
        shared: &Shared,
        runtime: &tokio::runtime::Handle,
    ) {
        let _exit = WorkerExit(shared);
        while let Some((req, _in_flight)) = Self::next_request(shared) {
            let pos = req.pos;
            let result = (|| {
                if let Some(provider) = req.provider
                    && let Some(column) =
                        runtime.block_on(provider.load_column(pos, req.dimension))?
                {
                    tracing::debug!(chunk = ?(pos.x, pos.z), dimension = req.dimension, "Loaded chunk from storage");
                    return Ok((column, true));
                }
                tracing::debug!(chunk = ?(pos.x, pos.z), dimension = req.dimension, "Generating missing chunk");
                let chunk = pipeline.generate_within(bounds, pos).map_err(|error| {
                    crate::storage::StorageError::Serialization(format!(
                        "chunk {pos:?}: native Bedrock adaptation failed: {error}"
                    ))
                })?;
                Ok((ChunkColumn::new(chunk), false))
            })();
            let _ = req.response_tx.send(result);
        }
        tracing::debug!("Chunk generation worker shutting down");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generator::FlatGenerator;

    #[tokio::test]
    async fn pool_generates_requested_chunks() {
        let pipeline = Arc::new(ChunkPipeline::new(Arc::new(FlatGenerator)));
        let worker = ChunkGenerationWorker::spawn(pipeline, WorldBounds::Infinite, 2);

        let receivers: Vec<_> = [(0, 0), (1, 0), (0, 1), (-3, 7)]
            .into_iter()
            .map(|(x, z)| (x, z, worker.generate(x, z, None, 0).unwrap()))
            .collect();
        for (x, z, receiver) in receivers {
            let (column, loaded) = receiver.await.unwrap().unwrap();
            assert_eq!((column.chunk.x, column.chunk.z), (x, z));
            assert!(!loaded);
        }
    }

    /// The pool honours `world.bounds` like the inline path: chunks outside are air,
    /// chunks inside are generated terrain.
    #[tokio::test]
    async fn pool_generates_air_outside_world_bounds() {
        let pipeline = Arc::new(ChunkPipeline::new(Arc::new(FlatGenerator)));
        let bounds = WorldBounds::Radius { radius_chunks: 1 };
        let worker = ChunkGenerationWorker::spawn(pipeline, bounds, 2);

        for (x, z, inside) in [(1, -1, true), (2, 0, false), (0, -5, false)] {
            let (column, loaded) = worker
                .generate(x, z, None, 0)
                .unwrap()
                .await
                .unwrap()
                .unwrap();
            assert!(!loaded);
            assert_eq!(
                column.chunk.highest_subchunk() == 0,
                !inside,
                "chunk ({x}, {z}) inside bounds: {inside}"
            );
        }
    }

    #[test]
    fn pick_skips_requests_sharing_source_chunks_with_in_flight_work() {
        let queue = [(1, 0), (4, 4), (5, 0), (0, 9)].map(|(x, z)| ChunkPos::new(x, z));
        let busy = ChunkPos::new(0, 0);
        // (1, 0) and (4, 4) overlap the 5x5 areas of `busy`; (5, 0) does not.
        assert_eq!(pick(queue, &[busy], 8), 2);
        // Nothing independent within the lookahead: serve the front.
        assert_eq!(pick(queue, &[busy], 2), 0);
        assert_eq!(pick(queue, &[busy, ChunkPos::new(6, 0)], 8), 3);
        // An idle pool keeps queue order.
        assert_eq!(pick(queue, &[], 8), 0);
    }

    #[test]
    fn auto_thread_count_is_bounded() {
        let n = generation_threads(0);
        assert!((1..=MAX_AUTO_THREADS).contains(&n));
        assert_eq!(generation_threads(3), 3);
    }
}
