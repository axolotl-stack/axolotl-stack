use crate::ecs::events::{EventBuffer, ServerEvent};
use crate::ecs::resources::TickCounter;
use bevy_ecs::prelude::*;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// A pending timer to be fired at a future tick.
#[derive(Debug, Eq, PartialEq)]
struct PendingTimer {
    fire_tick: u64,
    id: u64,
    plugin_id: String, // Plugin identifier
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
    pub fn set_timer(&mut self, current_tick: u64, after_ticks: u64, id: u64, plugin_id: String) {
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
            event_buffer.push(ServerEvent::Timer { id: fired.id });
        } else {
            break; // Heap is ordered, so no more timers can fire this tick
        }
    }
}
