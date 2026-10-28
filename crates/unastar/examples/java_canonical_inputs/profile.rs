//! Request history is an input to native feature generation, not a seed-only
//! normalization. Only operation metadata is consumed; no reference voxel/actor
//! values are used to drive generation.

use serde::{Deserialize, Serialize};
use unastar::world::ChunkPos;
use unastar::world::generator::ChunkPipeline;

#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Profile {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    coordinate_request_order: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coordinate_request_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    requested_stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    features_halo: Option<Halo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parallel_batch: Option<ParallelBatch>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
struct Halo {
    profile: String,
    native_features_block_state_write_radius: i32,
    additional_native_features_request_order: Vec<String>,
}

/// Only native operations and worker identities, never native voxel/biome data.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
struct ParallelBatch {
    dimension: String,
    status: String,
    request_ordinals: Vec<usize>,
    coordinate_request_order: Vec<String>,
    pending_futures_before_native_pump: usize,
    generation_worker_count: usize,
    observation_boundary: String,
    native_task_schedule: Vec<NativeTask>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
struct NativeTask {
    event_ordinal: usize,
    status: String,
    chunk_x: i32,
    chunk_z: i32,
    thread_id: u64,
    thread_name: String,
}

fn position(text: &str) -> Result<ChunkPos, String> {
    let (x, z) = text.split_once(':').ok_or_else(|| format!("invalid native chunk request {text}"))?;
    Ok(ChunkPos::new(x.parse().map_err(|_| format!("invalid chunk x in {text}"))?,
        z.parse().map_err(|_| format!("invalid chunk z in {text}"))?))
}

impl Profile {
    pub(super) fn validate_runtime_history(&self) -> Result<(), String> {
        if self.parallel_batch.is_some() {
            return Err("parallel native task/worker histories cannot be treated as serial FEATURES or halo requests".into());
        }
        Ok(())
    }
    pub(super) fn observation_index(&self, column_index: Option<usize>) -> Option<usize> {
        column_index.or(self.coordinate_request_index)
    }

    /// Whether the capture recorded every neighboring FEATURES writer it ran.
    pub(super) fn closes_features_halo(&self) -> bool {
        self.features_halo.is_some()
    }

    pub(super) fn validate_observation(&self, target: ChunkPos, index: Option<usize>) -> Result<(), String> {
        if let Some(index) = self.observation_index(index) {
            let request = self.coordinate_request_order.get(index)
                .ok_or("native coordinate observation index is outside its request history")?;
            if position(request)? != target {
                return Err("native coordinate observation index does not identify its chunk".into());
            }
        }
        Ok(())
    }

    pub(super) fn decorate(
        &self, pipeline: &ChunkPipeline, dimension: &str, target: ChunkPos, observation: Option<usize>,
    ) -> Result<(), String> {
        if self.coordinate_request_order.is_empty() {
            if self.features_halo.is_some() {
                return Err("native halo profile has no preceding target request order".into());
            }
            // Legacy captures without operation metadata describe a closed
            // neighborhood, not an immediate native FEATURES observation.
            pipeline.generate_native(target);
            return Ok(());
        }
        let requests = self.coordinate_request_order.iter().map(|s| position(s))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(halo) = &self.features_halo {
            if halo.native_features_block_state_write_radius != 1 {
                return Err("native FEATURES write radius is not the supported three-by-three region".into());
            }
            for origin in requests {
                pipeline.decorate_origin(origin);
            }
            let prefix = match dimension {
                "minecraft:overworld" => "OVERWORLD:",
                "minecraft:the_nether" => "NETHER:",
                "minecraft:the_end" => "END:",
                other => return Err(format!("unsupported native request dimension {other}")),
            };
            for request in &halo.additional_native_features_request_order {
                if let Some(request) = request.strip_prefix(prefix) {
                    pipeline.decorate_origin(position(request)?);
                }
            }
            return Ok(());
        }
        let occurrence = if let Some(index) = self.observation_index(observation) {
            if requests.get(index) != Some(&target) {
                return Err("native coordinate observation index does not identify the captured target".into());
            }
            index
        } else {
            let mut matches = requests.iter().enumerate().filter(|(_, pos)| **pos == target);
            let index = matches.next().ok_or("native request order omits the captured target")?.0;
            if matches.next().is_some() {
                return Err("repeated native observations require an explicit coordinate_request_index; do not collapse their history".into());
            }
            index
        };
        for &origin in &requests[..=occurrence] {
            pipeline.decorate_origin(origin);
        }
        Ok(())
    }
}
