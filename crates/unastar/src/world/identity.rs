//! Durable identity of an ordinary configured world.

use bevy_ecs::prelude::Resource;
use uuid::Uuid;

/// A world's validated storage manifest owns its name and durable UUID.
/// Runtime handles have their own scope and cannot survive a server restart.
#[derive(Clone, Debug, PartialEq, Eq, Resource)]
pub struct WorldIdentity {
    pub id: Uuid,
    pub name: String,
    pub dimension: i32,
}
