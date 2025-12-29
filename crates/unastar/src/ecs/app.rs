//! ECS application wrapper.

use bevy_ecs::prelude::*;

use super::events::*;
use super::resources::*;
use super::schedules::*;

/// Main ECS application wrapper for Unastar.
///
/// Manages the ECS World and schedules for the game tick loop.
pub struct UnastarEcs {
    world: World,
    sim_schedule: Schedule,
    post_sim_schedule: Schedule,
}

impl UnastarEcs {
    /// Create a new ECS application with default resources.
    pub fn new() -> Self {
        let mut world = World::new();

        // Insert global resources
        world.insert_resource(TickCounter::default());
        world.insert_resource(EventBuffer::default());
        world.insert_resource(ActionQueue::default());

        // Build simulation schedule
        let mut sim_schedule = Schedule::default();
        sim_schedule.configure_sets(
            (
                PhysicsSet,
                EntityLogicSet,
                ChunkSet,
            )
                .chain(),
        );

        // Build post-simulation schedule (Network + Cleanup)
        let mut post_sim_schedule = Schedule::default();
        post_sim_schedule.configure_sets(
            (
                NetworkSendSet,
                CleanupSet,
            )
                .chain(),
        );

        // Add the tick increment system to cleanup
        post_sim_schedule.add_systems(increment_tick.in_set(CleanupSet));

        Self {
            world,
            sim_schedule,
            post_sim_schedule,
        }
    }

    /// Run the simulation phase of the tick.
    pub fn tick_simulation(&mut self) {
        self.sim_schedule.run(&mut self.world);
    }

    /// Run the post-simulation phase (network sending, cleanup).
    pub fn tick_post_simulation(&mut self) {
        self.post_sim_schedule.run(&mut self.world);
    }

    /// Run one full game tick (sim + post-sim).
    pub fn tick(&mut self) {
        self.tick_simulation();
        self.tick_post_simulation();
    }

    /// Get mutable access to the ECS world.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// Get read access to the ECS world.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Get the simulation schedule for adding systems.
    /// Note: Systems added here run BEFORE plugins.
    pub fn schedule_mut(&mut self) -> &mut Schedule {
        &mut self.sim_schedule
    }

    /// Get the post-simulation schedule for adding systems.
    /// Note: Systems added here run AFTER plugins.
    pub fn post_schedule_mut(&mut self) -> &mut Schedule {
        &mut self.post_sim_schedule
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
