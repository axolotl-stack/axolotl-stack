//! Bounded load-before-generate pool.
//!
//! Storage reads and stack-heavy generation run outside the ECS tick on a fixed set of
//! worker threads (16 MiB stacks for Windows terrain generation). Only an explicit storage
//! miss permits generation; read errors never produce replacement terrain. Generation is a
//! pure function of `(seed, position)` (see `world::generator::pipeline`), so the number
//! of workers never changes the output. Positions outside `world.bounds` are generated as
//! all-air by [`generate_within_bounds`], the same helper the inline path uses.

use std::sync::{Arc, Mutex, mpsc};
use tokio::sync::oneshot;

use crate::storage::{ChunkColumn, StorageResult, WorldProvider};
use crate::world::generator::ChunkPipeline;
use crate::world::{Chunk, ChunkPos, WorldBounds};

/// The chunk column (with its stored actors) plus its storage provenance
/// (`true` means loaded, not generated).
pub type ChunkLoadResult = StorageResult<(ChunkColumn, bool)>;

const CHUNK_GENERATION_STACK_SIZE: usize = 16 * 1024 * 1024;

/// Most worker threads picked automatically.
const MAX_AUTO_THREADS: usize = 8;

/// Request to load or generate a chunk.
struct ChunkGenRequest {
    pos: ChunkPos,
    provider: Option<Arc<dyn WorldProvider>>,
    dimension: i32,
    response_tx: oneshot::Sender<ChunkLoadResult>,
}

/// Generate the chunk at `pos`, or an all-air chunk when `pos` lies outside `bounds`.
///
/// Every generation path (inline and the pool) goes through here, so the configured
/// bounds hold whichever path a generator uses. Out-of-bounds chunks still exist (as air)
/// so the client can mesh the border.
pub(crate) fn generate_within_bounds(
    pipeline: &ChunkPipeline,
    bounds: WorldBounds,
    pos: ChunkPos,
) -> std::io::Result<Chunk> {
    if !bounds.contains(pos) {
        tracing::debug!(chunk = ?(pos.x, pos.z), ?bounds, "Chunk outside world bounds - generating air");
        return Ok(Chunk::new(pos.x, pos.z));
    }
    pipeline.generate(pos)
}

/// Handle to the chunk generation pool. Dropping every handle stops the workers once
/// queued requests are done.
#[derive(Clone)]
pub struct ChunkGenerationWorker {
    request_tx: mpsc::Sender<ChunkGenRequest>,
}

/// Worker count for a configured value (0 = half the cores, clamped to 1..=8).
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
        let (request_tx, request_rx) = mpsc::channel::<ChunkGenRequest>();
        let request_rx = Arc::new(Mutex::new(request_rx));
        let runtime = tokio::runtime::Handle::current();
        for index in 0..threads {
            let pipeline = pipeline.clone();
            let request_rx = request_rx.clone();
            let runtime = runtime.clone();
            std::thread::Builder::new()
                .name(format!("unastar-chunk-gen-{index}"))
                .stack_size(CHUNK_GENERATION_STACK_SIZE)
                .spawn(move || Self::run_worker(&pipeline, bounds, &request_rx, &runtime))
                .expect("spawn chunk generation worker");
        }
        Self { request_tx }
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
        self.request_tx
            .send(ChunkGenRequest {
                pos: ChunkPos::new(x, z),
                provider,
                dimension,
                response_tx,
            })
            .ok()?;
        Some(response_rx)
    }

    fn run_worker(
        pipeline: &ChunkPipeline,
        bounds: WorldBounds,
        request_rx: &Mutex<mpsc::Receiver<ChunkGenRequest>>,
        runtime: &tokio::runtime::Handle,
    ) {
        loop {
            let request = {
                let rx = request_rx.lock().expect("generation queue poisoned");
                rx.recv()
            };
            let Ok(req) = request else {
                tracing::debug!("Chunk generation worker shutting down");
                return;
            };
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
                let chunk = generate_within_bounds(pipeline, bounds, pos).map_err(|error| {
                    crate::storage::StorageError::Serialization(format!(
                        "chunk {pos:?}: native Bedrock adaptation failed: {error}"
                    ))
                })?;
                Ok((ChunkColumn::new(chunk), false))
            })();
            let _ = req.response_tx.send(result);
        }
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
    fn auto_thread_count_is_bounded() {
        let n = generation_threads(0);
        assert!((1..=MAX_AUTO_THREADS).contains(&n));
        assert_eq!(generation_threads(3), 3);
    }
}
