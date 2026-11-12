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

/// When each stage of the current tick began, so a slow tick can say which
/// stage was slow. Marker systems between the tick's system sets (and between
/// the chunk systems) record a stage's name as it starts.
#[derive(Resource, Default, Debug)]
pub struct TickStages {
    marks: Vec<(&'static str, std::time::Instant)>,
}

impl TickStages {
    /// Forget the previous tick's marks.
    pub fn begin(&mut self) {
        self.marks.clear();
    }

    /// Stage `name` starts now.
    pub fn mark(&mut self, name: &'static str) {
        self.marks.push((name, std::time::Instant::now()));
    }

    /// Each stage's duration, in tick order: from its mark to the next one,
    /// the last stage ending at `end`.
    pub fn durations(&self, end: std::time::Instant) -> Vec<(&'static str, std::time::Duration)> {
        self.marks
            .iter()
            .enumerate()
            .map(|(index, &(name, start))| {
                let until = self.marks.get(index + 1).map_or(end, |&(_, next)| next);
                (name, until.saturating_duration_since(start))
            })
            .collect()
    }

    /// The `count` slowest stages, slowest first, as `name=ms` text.
    pub fn slowest(&self, end: std::time::Instant, count: usize) -> String {
        let mut stages = self.durations(end);
        stages.sort_by_key(|&(_, duration)| std::cmp::Reverse(duration));
        stages
            .iter()
            .take(count)
            .map(|(name, duration)| format!("{name}={:.1}", duration.as_secs_f64() * 1e3))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A system that marks the start of stage `name` in [`TickStages`].
pub fn stage_marker(name: &'static str) -> impl FnMut(ResMut<TickStages>) {
    move |mut stages: ResMut<TickStages>| stages.mark(name)
}
