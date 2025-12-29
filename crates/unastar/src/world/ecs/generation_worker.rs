//! Async chunk generation worker.
//!
//! Offloads chunk generation to `tokio::spawn_blocking` to prevent
//! blocking the main ECS tick loop during vanilla terrain generation.

use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

use crate::world::Chunk;
use crate::world::generator::VanillaGenerator;

/// Request to generate a chunk at the given coordinates.
pub struct ChunkGenRequest {
    pub x: i32,
    pub z: i32,
    pub response_tx: oneshot::Sender<Chunk>,
}

/// Handle to the chunk generation worker.
///
/// Send requests via `generate()` and receive chunks through the returned receiver.
/// The worker runs chunk generation on tokio's blocking thread pool to avoid
/// blocking the async runtime or ECS systems.
#[derive(Clone)]
pub struct ChunkGenerationWorker {
    request_tx: mpsc::UnboundedSender<ChunkGenRequest>,
}

impl ChunkGenerationWorker {
    /// Spawn a new chunk generation worker with the given generator.
    ///
    /// The worker will process generation requests asynchronously using
    /// `tokio::spawn_blocking` for CPU-intensive terrain generation.
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
    pub fn generate(&self, x: i32, z: i32) -> Option<oneshot::Receiver<Chunk>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.request_tx
            .send(ChunkGenRequest { x, z, response_tx })
            .ok()?;
        Some(response_rx)
    }

    /// Internal worker loop that processes generation requests.
    async fn run_worker(
        generator: Arc<VanillaGenerator>,
        mut request_rx: mpsc::UnboundedReceiver<ChunkGenRequest>,
    ) {
        while let Some(req) = request_rx.recv().await {
            let gen_clone = generator.clone();
            let x = req.x;
            let z = req.z;

            // Use spawn_blocking for CPU-intensive generation
            let result = tokio::task::spawn_blocking(move || gen_clone.generate_chunk(x, z)).await;

            if let Ok(chunk) = result {
                // Send result back; ignore error if receiver was dropped
                let _ = req.response_tx.send(chunk);
            } else {
                tracing::warn!(
                    chunk = ?(x, z),
                    "Chunk generation task panicked or was cancelled"
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

        let receiver = worker.generate(0, 0).expect("Worker should accept request");
        let chunk = receiver.await.expect("Should receive chunk");

        assert_eq!(chunk.x, 0);
        assert_eq!(chunk.z, 0);
    }

    #[tokio::test]
    async fn test_worker_multiple_chunks() {
        let generator = Arc::new(VanillaGenerator::new(12345));
        let worker = ChunkGenerationWorker::spawn(generator);

        // Request multiple chunks
        let r1 = worker.generate(0, 0).unwrap();
        let r2 = worker.generate(1, 0).unwrap();
        let r3 = worker.generate(0, 1).unwrap();

        // All should complete
        let c1 = r1.await.unwrap();
        let c2 = r2.await.unwrap();
        let c3 = r3.await.unwrap();

        assert_eq!((c1.x, c1.z), (0, 0));
        assert_eq!((c2.x, c2.z), (1, 0));
        assert_eq!((c3.x, c3.z), (0, 1));
    }
}
