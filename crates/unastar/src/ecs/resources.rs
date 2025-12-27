//! Global ECS resources.

use bevy_ecs::prelude::*;

/// Current game tick counter.
#[derive(Resource, Default)]
pub struct TickCounter {
    pub current: u64,
}

impl TickCounter {
    pub fn increment(&mut self) {
        self.current = self.current.wrapping_add(1);
    }

    pub fn get(&self) -> u64 {
        self.current
    }
}
