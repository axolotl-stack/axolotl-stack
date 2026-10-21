//! Background load-before-generate worker.
//!
//! Storage reads and stack-heavy vanilla generation run outside the ECS tick.
//! Only an explicit storage miss permits generation; read errors never produce
//! replacement terrain.

use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

use crate::storage::{StorageResult, WorldProvider};
use crate::world::generator::VanillaGenerator;
use crate::world::{Chunk, ChunkPos};

/// Chunk data plus its storage provenance (`true` means loaded, not generated).
pub type ChunkLoadResult = StorageResult<(Chunk, bool)>;

const CHUNK_GENERATION_STACK_SIZE: usize = 16 * 1024 * 1024;

/// Request to generate a chunk at the given coordinates.
pub struct ChunkGenRequest {
    pub x: i32,
    pub z: i32,
    pub provider: Option<Arc<dyn WorldProvider>>,
    pub dimension: i32,
    pub response_tx: oneshot::Sender<ChunkLoadResult>,
}

/// Handle to the chunk generation worker.
///
/// Send requests via `generate()` and receive chunks through the returned receiver.
/// Each request loads storage before generating on a miss, on a dedicated
/// stack-sized thread. Storage failures are returned without generating.
#[derive(Clone)]
pub struct ChunkGenerationWorker {
    request_tx: mpsc::UnboundedSender<ChunkGenRequest>,
}

impl ChunkGenerationWorker {
    /// Spawn a new chunk generation worker with the given generator.
    ///
    /// Requests run on explicit 16 MiB stacks for Windows terrain generation.
    pub fn spawn(generator: Arc<VanillaGenerator>) -> Self {
        let (request_tx, request_rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            Self::run_worker(generator, request_rx).await;
        });

        Self { request_tx }
    }

    /// Queue a chunk for generation, returns a receiver for the result.
    ///
    /// The generation happens asynchronously on a blocking thread.
    /// Returns `None` if the worker has been shut down.
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
                x,
                z,
                provider,
                dimension,
                response_tx,
            })
            .ok()?;
        Some(response_rx)
    }

    /// Internal worker loop that processes generation requests.
    ///
    /// Starts each request without awaiting its load/generation thread.
    async fn run_worker(
        generator: Arc<VanillaGenerator>,
        mut request_rx: mpsc::UnboundedReceiver<ChunkGenRequest>,
    ) {
        while let Some(req) = request_rx.recv().await {
            let gen_clone = generator.clone();
            let x = req.x;
            let z = req.z;
            let runtime = tokio::runtime::Handle::current();

            // Vanilla generation is stack-heavy on Windows. Use an explicit
            // stack size instead of tokio's blocking-pool default so terrain
            // tests and real generation do not overflow worker stacks.
            if let Err(e) = std::thread::Builder::new()
                .name(format!("unastar-chunk-gen-{x}-{z}"))
                .stack_size(CHUNK_GENERATION_STACK_SIZE)
                .spawn(move || {
                    let result = (|| {
                        if let Some(provider) = req.provider
                            && let Some(column) = runtime.block_on(
                                provider.load_column(ChunkPos::new(x, z), req.dimension),
                            )?
                        {
                            tracing::debug!(chunk = ?(x, z), dimension = req.dimension, "Loaded chunk from storage");
                            return Ok((column.chunk, true));
                        }
                        tracing::debug!(chunk = ?(x, z), dimension = req.dimension, "Generating missing chunk");
                        Ok((gen_clone.generate_chunk(x, z), false))
                    })();
                    let _ = req.response_tx.send(result);
                })
            {
                tracing::warn!(
                    chunk = ?(x, z),
                    error = %e,
                    "Failed to spawn chunk generation thread"
                );
            }
        }

        tracing::debug!("Chunk generation worker shutting down");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require a tokio runtime
    #[tokio::test]
    async fn test_worker_generates_chunk() {
        let generator = Arc::new(VanillaGenerator::new(12345));
        let worker = ChunkGenerationWorker::spawn(generator);

        let receiver = worker
            .generate(0, 0, None, 0)
            .expect("Worker should accept request");
        let (chunk, _) = receiver.await.expect("Should receive chunk").unwrap();

        assert_eq!(chunk.x, 0);
        assert_eq!(chunk.z, 0);
    }

    #[tokio::test]
    async fn test_worker_multiple_chunks() {
        let generator = Arc::new(VanillaGenerator::new(12345));
        let worker = ChunkGenerationWorker::spawn(generator);

        // Request multiple chunks
        let r1 = worker.generate(0, 0, None, 0).unwrap();
        let r2 = worker.generate(1, 0, None, 0).unwrap();
        let r3 = worker.generate(0, 1, None, 0).unwrap();

        // All should complete
        let (c1, _) = r1.await.unwrap().unwrap();
        let (c2, _) = r2.await.unwrap().unwrap();
        let (c3, _) = r3.await.unwrap().unwrap();

        assert_eq!((c1.x, c1.z), (0, 0));
        assert_eq!((c2.x, c2.z), (1, 0));
        assert_eq!((c3.x, c3.z), (0, 1));
    }
}
