//! ECS system schedules.

use bevy_ecs::prelude::*;

/// System set for physics and movement.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PhysicsSet;

/// System set for entity logic (AI, effects, etc).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntityLogicSet;

/// System set for chunk management.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkSet;

/// System set for network sending.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NetworkSendSet;

/// System set for cleanup and tick increment.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CleanupSet;
