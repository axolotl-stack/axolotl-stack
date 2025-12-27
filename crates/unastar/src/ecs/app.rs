//! ECS application wrapper.

use bevy_ecs::prelude::*;

use super::resources::*;
use super::schedules::*;

/// Main ECS application wrapper for Unastar.
///
/// Manages the ECS World and schedules for the game tick loop.
pub struct UnastarEcs {
    world: World,
    tick_schedule: Schedule,
}

impl UnastarEcs {
    /// Create a new ECS application with default resources.
    pub fn new() -> Self {
        let mut world = World::new();

        // Insert global resources
        world.insert_resource(TickCounter::default());

        // Build tick schedule with ordered system sets
        let mut tick_schedule = Schedule::default();

        // Configure system set ordering
        tick_schedule.configure_sets(
            (
                PhysicsSet,
                EntityLogicSet,
                ChunkSet,
                NetworkSendSet,
                CleanupSet,
            )
                .chain(),
        );

        // Add the tick increment system to cleanup
        tick_schedule.add_systems(increment_tick.in_set(CleanupSet));

        Self {
            world,
            tick_schedule,
        }
    }

    /// Run one game tick.
    pub fn tick(&mut self) {
        self.tick_schedule.run(&mut self.world);
    }

    /// Get mutable access to the ECS world.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// Get read access to the ECS world.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Get the tick schedule for adding systems.
    pub fn schedule_mut(&mut self) -> &mut Schedule {
        &mut self.tick_schedule
    }
}

impl Default for UnastarEcs {
    fn default() -> Self {
        Self::new()
    }
}

/// System to increment the tick counter each tick.
fn increment_tick(mut tick: ResMut<TickCounter>) {
    tick.increment();
}
