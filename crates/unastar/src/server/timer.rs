use crate::ecs::events::{EventBuffer, ServerEvent};
use crate::ecs::resources::TickCounter;
use crate::plugin::PluginId;
use bevy_ecs::prelude::*;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// A pending timer to be fired at a future tick.
#[derive(Debug, Eq, PartialEq)]
struct PendingTimer {
    fire_tick: u64,
    id: u64,
    /// The plugin that set the timer; only it receives `on-timer`.
    plugin_id: PluginId,
}

impl Ord for PendingTimer {
    fn cmp(&self, other: &Self) -> Ordering {
        // In a min-heap, we want the smallest fire_tick to have the highest priority.
        other.fire_tick.cmp(&self.fire_tick)
    }
}

impl PartialOrd for PendingTimer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Manages all timers set by plugins.
#[derive(Resource, Default)]
pub struct TimerManager {
    timers: BinaryHeap<PendingTimer>,
}

impl TimerManager {
    pub fn set_timer(&mut self, current_tick: u64, after_ticks: u64, id: u64, plugin_id: PluginId) {
        let fire_tick = current_tick.wrapping_add(after_ticks);
        self.timers.push(PendingTimer {
            fire_tick,
            id,
            plugin_id,
        });
    }
}

/// System that checks for and fires timers.
pub fn timer_system(
    mut timers: ResMut<TimerManager>,
    tick_counter: Res<TickCounter>,
    mut event_buffer: ResMut<EventBuffer>,
) {
    let current_tick = tick_counter.get();
    while let Some(timer) = timers.timers.peek() {
        if timer.fire_tick <= current_tick {
            let fired = timers.timers.pop().unwrap(); // We just peeked, so it's safe
            event_buffer.push(ServerEvent::Timer {
                id: fired.id,
                plugin_id: fired.plugin_id,
            });
        } else {
            break; // Heap is ordered, so no more timers can fire this tick
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::system::RunSystemOnce;

    #[test]
    fn fired_timer_event_names_the_plugin_that_set_it() {
        let mut world = World::new();
        world.insert_resource(TickCounter { current: 10 });
        world.insert_resource(EventBuffer::default());
        let mut timers = TimerManager::default();
        timers.set_timer(5, 5, 7, PluginId("owner".into()));
        timers.set_timer(5, 6, 8, PluginId("later".into()));
        world.insert_resource(timers);
        world.run_system_once(timer_system).unwrap();
        let events = world.resource_mut::<EventBuffer>().drain();
        assert!(matches!(
            events.as_slice(),
            [ServerEvent::Timer { id: 7, plugin_id }] if plugin_id.0 == "owner"
        ));
    }
}
