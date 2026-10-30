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
    /// `before_native_task` or `native_task_future_completed`. Absent in
    /// captures that recorded task starts only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    invocation: Option<u64>,
    status: String,
    chunk_x: i32,
    chunk_z: i32,
    thread_id: u64,
    thread_name: String,
}

/// One native FEATURES task: start event, completion event (when recorded) and origin.
struct FeaturesTask {
    start: usize,
    end: Option<usize>,
    origin: ChunkPos,
}

/// FEATURES writes within radius one of its origin and reads its radius-one
/// region, so two origins further apart than this touch disjoint chunks.
const FEATURES_CONFLICT_DISTANCE: i32 = 2;

/// FEATURES tasks of a batch in native start order. With completion events,
/// two tasks that overlapped in time and are within conflict distance are
/// rejected: their interleaving is a native race no sequential replay
/// describes. Overlapping tasks further apart touch disjoint chunks and
/// commute, so start order replays them exactly.
fn features_tasks(schedule: &[NativeTask]) -> Result<Vec<FeaturesTask>, String> {
    let events = schedule
        .iter()
        .filter(|task| task.status == "minecraft:features");
    let mut tasks: Vec<FeaturesTask> = Vec::new();
    if schedule.iter().any(|task| task.phase.is_some()) {
        let mut by_invocation =
            std::collections::BTreeMap::<u64, (Option<usize>, Option<usize>, ChunkPos)>::new();
        for event in events {
            let invocation = event
                .invocation
                .ok_or("native task event has no invocation identity")?;
            let origin = ChunkPos::new(event.chunk_x, event.chunk_z);
            let entry = by_invocation
                .entry(invocation)
                .or_insert((None, None, origin));
            if entry.2 != origin {
                return Err(format!("native task invocation {invocation} changed chunk"));
            }
            let slot = match event.phase.as_deref() {
                Some("before_native_task") => &mut entry.0,
                Some("native_task_future_completed") => &mut entry.1,
                other => return Err(format!("unknown native task phase {other:?}")),
            };
            if slot.replace(event.event_ordinal).is_some() {
                return Err(format!(
                    "native task invocation {invocation} repeats a phase"
                ));
            }
        }
        for (invocation, (start, end, origin)) in by_invocation {
            let (Some(start), Some(end)) = (start, end) else {
                return Err(format!(
                    "native FEATURES invocation {invocation} lacks a start or completion event"
                ));
            };
            tasks.push(FeaturesTask {
                start,
                end: Some(end),
                origin,
            });
        }
    } else {
        // Start-only schedules (the 2026-10-28 restart capture); its full
        // trace showed every FEATURES task completing before the next began.
        tasks.extend(events.map(|event| FeaturesTask {
            start: event.event_ordinal,
            end: None,
            origin: ChunkPos::new(event.chunk_x, event.chunk_z),
        }));
    }
    tasks.sort_by_key(|task| task.start);
    for (index, earlier) in tasks.iter().enumerate() {
        let Some(end) = earlier.end else { continue };
        for later in tasks[index + 1..]
            .iter()
            .take_while(|later| later.start < end)
        {
            let distance = (earlier.origin.x - later.origin.x)
                .abs()
                .max((earlier.origin.z - later.origin.z).abs());
            if distance <= FEATURES_CONFLICT_DISTANCE {
                return Err(format!(
                    "native FEATURES tasks at {:?} and {:?} ran concurrently within conflict distance; no sequential replay describes that race",
                    earlier.origin, later.origin,
                ));
            }
        }
    }
    Ok(tasks)
}

fn position(text: &str) -> Result<ChunkPos, String> {
    let (x, z) = text
        .split_once(':')
        .ok_or_else(|| format!("invalid native chunk request {text}"))?;
    Ok(ChunkPos::new(
        x.parse()
            .map_err(|_| format!("invalid chunk x in {text}"))?,
        z.parse()
            .map_err(|_| format!("invalid chunk z in {text}"))?,
    ))
}

impl Profile {
    pub(super) fn validate_runtime_history(&self) -> Result<(), String> {
        if let Some(batch) = &self.parallel_batch {
            if self.features_halo.is_some() {
                return Err("a parallel batch observation cannot also be a halo closure".into());
            }
            if batch.coordinate_request_order != self.coordinate_request_order {
                return Err(
                    "parallel batch targets differ from the capture's request order".into(),
                );
            }
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

    pub(super) fn validate_observation(
        &self,
        target: ChunkPos,
        index: Option<usize>,
    ) -> Result<(), String> {
        if let Some(index) = self.observation_index(index) {
            let request = self
                .coordinate_request_order
                .get(index)
                .ok_or("native coordinate observation index is outside its request history")?;
            if position(request)? != target {
                return Err(
                    "native coordinate observation index does not identify its chunk".into(),
                );
            }
        }
        Ok(())
    }

    pub(super) fn decorate(
        &self,
        pipeline: &ChunkPipeline,
        dimension: &str,
        target: ChunkPos,
        observation: Option<usize>,
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
        let requests = self
            .coordinate_request_order
            .iter()
            .map(|s| position(s))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(batch) = &self.parallel_batch {
            self.validate_observation(target, observation)?;
            // Native snapshots every target only after the whole batch's futures
            // complete, so an observation includes later batch origins' neighbor
            // writes. Origins replay in the recorded native task order, never a
            // normalized coordinate order; `features_tasks` proves the order is
            // well-defined.
            let tasks = features_tasks(&batch.native_task_schedule)?;
            for request in &requests {
                if !tasks.iter().any(|task| task.origin == *request) {
                    return Err(format!(
                        "parallel batch has no native FEATURES task for target {request:?}"
                    ));
                }
            }
            for task in tasks {
                pipeline.decorate_origin(task.origin);
            }
            return Ok(());
        }
        if let Some(halo) = &self.features_halo {
            if halo.native_features_block_state_write_radius != 1 {
                return Err(
                    "native FEATURES write radius is not the supported three-by-three region"
                        .into(),
                );
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
                return Err(
                    "native coordinate observation index does not identify the captured target"
                        .into(),
                );
            }
            index
        } else {
            let mut matches = requests
                .iter()
                .enumerate()
                .filter(|(_, pos)| **pos == target);
            let index = matches
                .next()
                .ok_or("native request order omits the captured target")?
                .0;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn event(ordinal: usize, phase: &str, invocation: u64, x: i32) -> NativeTask {
        NativeTask {
            event_ordinal: ordinal,
            phase: Some(phase.into()),
            invocation: Some(invocation),
            status: "minecraft:features".into(),
            chunk_x: x,
            chunk_z: 0,
            thread_id: invocation,
            thread_name: String::new(),
        }
    }

    #[test]
    fn concurrent_features_tasks_replay_only_when_their_regions_are_disjoint() {
        // Sequential neighbours replay in start order.
        let sequential = [
            event(0, "before_native_task", 1, 1),
            event(1, "native_task_future_completed", 1, 1),
            event(2, "before_native_task", 2, 0),
            event(3, "native_task_future_completed", 2, 0),
        ];
        let order: Vec<_> = features_tasks(&sequential)
            .unwrap()
            .iter()
            .map(|task| task.origin.x)
            .collect();
        assert_eq!(order, [1, 0]);
        // Overlapping tasks three chunks apart touch disjoint chunks and commute.
        let disjoint = [
            event(0, "before_native_task", 1, 0),
            event(1, "before_native_task", 2, 3),
            event(2, "native_task_future_completed", 2, 3),
            event(3, "native_task_future_completed", 1, 0),
        ];
        assert_eq!(features_tasks(&disjoint).unwrap().len(), 2);
        // Overlapping tasks two chunks apart share a written chunk: a native race.
        let racing = [
            event(0, "before_native_task", 1, 0),
            event(1, "before_native_task", 2, 2),
            event(2, "native_task_future_completed", 1, 0),
            event(3, "native_task_future_completed", 2, 2),
        ];
        assert!(features_tasks(&racing).is_err());
        // A start without a completion cannot be ordered.
        assert!(features_tasks(&racing[..2]).is_err());
    }
}
