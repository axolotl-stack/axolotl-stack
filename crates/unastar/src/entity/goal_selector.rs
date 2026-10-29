//! Numeric goal arbitration; actual goal state and behaviour belong to the caller.
//!
//! The entity roadmap (2026-10-24 research, §3 AI goals) chooses independent
//! target/normal selectors, target selection first, lower priorities first, and
//! interruptible-only pre-emption. Exact native scheduling has not been measured.
//! Here each kind performs continuation checks, activation, then running ticks;
//! the entire target phase precedes the normal phase. Ties follow the supplied
//! composition slice, while an already-running equal-priority goal keeps its
//! controls. These callback/random-draw timings are an explicit deterministic
//! policy, not a claim of measured BDS timing.
//!
//! Specs are sorted only at construction/composition updates. A tick does not
//! allocate, sort, look up names, or own concrete goal state. Small goal sets use
//! direct scans instead of maps or per-tick work lists.

use std::ops::BitOr;

/// Caller-assigned identity, unique across both kinds in one selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GoalId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum GoalKind {
    Target,
    Normal,
}

/// Exclusive controls within a kind; target and normal kinds do not conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GoalControls(u8);

impl GoalControls {
    pub const NONE: Self = Self(0);
    pub const MOVE: Self = Self(1);
    pub const LOOK: Self = Self(2);
    pub const JUMP: Self = Self(4);
    /// Held by target goals (hurt-by/nearest-attackable target): one target
    /// goal at a time chooses the target.
    pub const TARGET: Self = Self(8);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for GoalControls {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GoalSpec {
    pub id: GoalId,
    pub kind: GoalKind,
    /// Lower numbers win. Equal-priority running goals cannot be pre-empted.
    pub priority: i32,
    pub controls: GoalControls,
}

/// Dispatch these numeric IDs to caller-owned typed goal state and context.
///
/// `can_start` may prepare activation (including random draws), but must leave
/// existing running goals valid on failure. It is never called for a blocked
/// candidate. Perform all fallible preparation there: `start` is the infallible
/// commit after conflicting goals have stopped. `interruptible` is queried only
/// for lower-priority running conflicts, before `can_start`, in selector order.
/// `stop` must release concrete navigation/look/target state owned by that goal.
/// Newly started goals receive `tick` in the same selector tick.
pub(crate) trait GoalCallbacks {
    fn can_start(&mut self, id: GoalId) -> bool;
    fn can_continue(&mut self, id: GoalId) -> bool;
    fn interruptible(&mut self, id: GoalId) -> bool;
    fn start(&mut self, id: GoalId);
    fn stop(&mut self, id: GoalId);
    fn tick(&mut self, id: GoalId);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DuplicateGoalId(pub GoalId);

#[derive(Debug)]
struct Entry {
    spec: GoalSpec,
    order: usize,
    running: bool,
}

/// Lifecycle owner for arbitration only, not for the caller's goal state.
///
/// Call `stop_all` before discarding a live selector if its goal state survives.
/// No `Clone` implementation is provided: running lifecycles must not be copied.
#[derive(Debug, Default)]
pub(crate) struct GoalSelector {
    entries: Vec<Entry>,
    /// Reused for transactional composition replacement, never touched by tick.
    staging: Vec<Entry>,
    target_count: usize,
}

impl GoalSelector {
    #[cfg(test)]
    pub fn new(specs: &[GoalSpec]) -> Result<Self, DuplicateGoalId> {
        Self::validate(specs)?;
        let mut selector = Self::default();
        selector
            .entries
            .extend(specs.iter().enumerate().map(|(order, &spec)| Entry {
                spec,
                order,
                running: false,
            }));
        selector.sort();
        Ok(selector)
    }

    /// Replace the composition, retaining running state only for identical specs.
    ///
    /// Removed or changed specs receive `stop` in their OLD selector order before
    /// replacement. Keep their typed state accessible until this method returns.
    /// An identical spec retains its lifecycle even when its slice position moves.
    /// A changed priority/kind/control set stops and may restart on the next tick.
    /// Duplicate IDs reject the whole update with no callbacks or state changes.
    /// Both storage buffers retain capacity across successful updates.
    pub fn update(
        &mut self,
        specs: &[GoalSpec],
        goals: &mut impl GoalCallbacks,
    ) -> Result<(), DuplicateGoalId> {
        Self::validate(specs)?;
        self.staging.clear();
        for (order, &spec) in specs.iter().enumerate() {
            let running = self
                .entries
                .iter()
                .any(|entry| entry.running && entry.spec == spec);
            self.staging.push(Entry {
                spec,
                order,
                running,
            });
        }
        for entry in &mut self.entries {
            if entry.running && !self.staging.iter().any(|next| next.spec == entry.spec) {
                entry.running = false;
                goals.stop(entry.spec.id);
            }
        }
        std::mem::swap(&mut self.entries, &mut self.staging);
        self.staging.clear();
        self.sort();
        Ok(())
    }

    #[cfg(test)]
    pub fn is_running(&self, id: GoalId) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.spec.id == id && entry.running)
    }

    /// Explicitly stop a goal without removing its spec; it may start next tick.
    /// Returns whether a running lifecycle was stopped.
    #[cfg(test)]
    pub fn stop(&mut self, id: GoalId, goals: &mut impl GoalCallbacks) -> bool {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.spec.id == id && entry.running)
        {
            entry.running = false;
            goals.stop(id);
            true
        } else {
            false
        }
    }

    /// Stop all running goals, keeping composition and allocated storage.
    pub fn stop_all(&mut self, goals: &mut impl GoalCallbacks) {
        for entry in &mut self.entries {
            if entry.running {
                entry.running = false;
                goals.stop(entry.spec.id);
            }
        }
    }

    pub fn tick(&mut self, goals: &mut impl GoalCallbacks) {
        self.tick_kind(0, self.target_count, goals);
        self.tick_kind(self.target_count, self.entries.len(), goals);
    }

    fn tick_kind(&mut self, begin: usize, end: usize, goals: &mut impl GoalCallbacks) {
        for entry in &mut self.entries[begin..end] {
            if entry.running && !goals.can_continue(entry.spec.id) {
                entry.running = false;
                goals.stop(entry.spec.id);
            }
        }
        for candidate in begin..end {
            if self.entries[candidate].running {
                continue;
            }
            let spec = self.entries[candidate].spec;
            let blocked = self.entries[begin..end].iter().any(|entry| {
                entry.running
                    && spec.controls.intersects(entry.spec.controls)
                    && (spec.priority >= entry.spec.priority || !goals.interruptible(entry.spec.id))
            });
            if blocked || !goals.can_start(spec.id) {
                continue;
            }
            // Do not stop anything until activation has succeeded. Eligibility
            // already established that all these conflicts are interruptible.
            for entry in &mut self.entries[begin..end] {
                if entry.running && spec.controls.intersects(entry.spec.controls) {
                    entry.running = false;
                    goals.stop(entry.spec.id);
                }
            }
            self.entries[candidate].running = true;
            goals.start(spec.id);
        }
        for entry in &self.entries[begin..end] {
            if entry.running {
                goals.tick(entry.spec.id);
            }
        }
    }

    fn validate(specs: &[GoalSpec]) -> Result<(), DuplicateGoalId> {
        for (index, spec) in specs.iter().enumerate() {
            if specs[..index].iter().any(|previous| previous.id == spec.id) {
                return Err(DuplicateGoalId(spec.id));
            }
        }
        Ok(())
    }

    fn sort(&mut self) {
        self.entries
            .sort_unstable_by_key(|entry| (entry.spec.kind, entry.spec.priority, entry.order));
        self.target_count = self
            .entries
            .partition_point(|entry| entry.spec.kind == GoalKind::Target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Call {
        CanStart(u16),
        CanContinue(u16),
        Interruptible(u16),
        Start(u16),
        Stop(u16),
        Tick(u16),
    }

    struct Goals {
        can_start: [bool; 8],
        can_continue: [bool; 8],
        interruptible: [bool; 8],
        active: [bool; 8],
        calls: Vec<Call>,
    }

    impl Goals {
        fn new() -> Self {
            Self {
                can_start: [true; 8],
                can_continue: [true; 8],
                interruptible: [true; 8],
                active: [false; 8],
                calls: Vec::new(),
            }
        }
    }

    impl GoalCallbacks for Goals {
        fn can_start(&mut self, id: GoalId) -> bool {
            self.calls.push(Call::CanStart(id.0));
            self.can_start[id.0 as usize]
        }

        fn can_continue(&mut self, id: GoalId) -> bool {
            assert!(self.active[id.0 as usize]);
            self.calls.push(Call::CanContinue(id.0));
            self.can_continue[id.0 as usize]
        }

        fn interruptible(&mut self, id: GoalId) -> bool {
            assert!(self.active[id.0 as usize]);
            self.calls.push(Call::Interruptible(id.0));
            self.interruptible[id.0 as usize]
        }

        fn start(&mut self, id: GoalId) {
            assert!(!self.active[id.0 as usize]);
            self.active[id.0 as usize] = true;
            self.calls.push(Call::Start(id.0));
        }

        fn stop(&mut self, id: GoalId) {
            assert!(self.active[id.0 as usize]);
            self.active[id.0 as usize] = false;
            self.calls.push(Call::Stop(id.0));
        }

        fn tick(&mut self, id: GoalId) {
            assert!(self.active[id.0 as usize]);
            self.calls.push(Call::Tick(id.0));
        }
    }

    fn normal(id: u16, priority: i32, controls: GoalControls) -> GoalSpec {
        GoalSpec {
            id: GoalId(id),
            kind: GoalKind::Normal,
            priority,
            controls,
        }
    }

    #[test]
    fn disjoint_controls_run_together_in_composition_order() {
        let specs = [
            normal(3, 2, GoalControls::MOVE),
            normal(1, 2, GoalControls::LOOK),
            normal(2, 2, GoalControls::JUMP),
            normal(0, 2, GoalControls::TARGET),
        ];
        let mut selector = GoalSelector::new(&specs).unwrap();
        let mut goals = Goals::new();
        selector.tick(&mut goals);
        assert_eq!(
            goals.calls,
            [
                Call::CanStart(3),
                Call::Start(3),
                Call::CanStart(1),
                Call::Start(1),
                Call::CanStart(2),
                Call::Start(2),
                Call::CanStart(0),
                Call::Start(0),
                Call::Tick(3),
                Call::Tick(1),
                Call::Tick(2),
                Call::Tick(0),
            ]
        );
    }

    #[test]
    fn equal_priority_running_goal_keeps_controls_after_reordering() {
        let first = normal(1, 2, GoalControls::MOVE);
        let peer = normal(0, 2, GoalControls::MOVE | GoalControls::LOOK);
        let mut selector = GoalSelector::new(&[first, peer]).unwrap();
        let mut goals = Goals::new();
        selector.tick(&mut goals);
        goals.calls.clear();
        selector.update(&[peer, first], &mut goals).unwrap();
        selector.tick(&mut goals);
        assert_eq!(goals.calls, [Call::CanContinue(1), Call::Tick(1)]);
        assert!(selector.is_running(GoalId(1)));
        assert!(!selector.is_running(GoalId(0)));
    }

    #[test]
    fn higher_priority_stops_every_conflict_before_starting() {
        let mut selector = GoalSelector::new(&[
            normal(0, 1, GoalControls::MOVE | GoalControls::LOOK),
            normal(1, 2, GoalControls::MOVE),
            normal(2, 3, GoalControls::LOOK),
            normal(3, 4, GoalControls::JUMP),
        ])
        .unwrap();
        let mut goals = Goals::new();
        goals.can_start[0] = false;
        selector.tick(&mut goals);
        goals.can_start[0] = true;
        goals.calls.clear();
        selector.tick(&mut goals);
        assert_eq!(
            goals.calls,
            [
                Call::CanContinue(1),
                Call::CanContinue(2),
                Call::CanContinue(3),
                Call::Interruptible(1),
                Call::Interruptible(2),
                Call::CanStart(0),
                Call::Stop(1),
                Call::Stop(2),
                Call::Start(0),
                Call::Tick(0),
                Call::Tick(3),
            ]
        );
        assert!(goals.active[0] && goals.active[3]);
        assert!(!goals.active[1] && !goals.active[2]);
    }

    #[test]
    fn noninterruptible_conflict_blocks_activation_without_partial_preemption() {
        let mut selector = GoalSelector::new(&[
            normal(0, 1, GoalControls::MOVE | GoalControls::LOOK),
            normal(1, 2, GoalControls::MOVE),
            normal(2, 3, GoalControls::LOOK),
        ])
        .unwrap();
        let mut goals = Goals::new();
        goals.can_start[0] = false;
        selector.tick(&mut goals);
        goals.can_start[0] = true;
        goals.interruptible[2] = false;
        goals.calls.clear();
        selector.tick(&mut goals);
        assert_eq!(
            goals.calls,
            [
                Call::CanContinue(1),
                Call::CanContinue(2),
                Call::Interruptible(1),
                Call::Interruptible(2),
                Call::Tick(1),
                Call::Tick(2),
            ]
        );
    }

    #[test]
    fn failed_candidate_preserves_running_goal_and_its_tick() {
        let mut selector = GoalSelector::new(&[
            normal(0, 0, GoalControls::MOVE),
            normal(1, 1, GoalControls::MOVE),
        ])
        .unwrap();
        let mut goals = Goals::new();
        goals.can_start[0] = false;
        selector.tick(&mut goals);
        goals.calls.clear();
        selector.tick(&mut goals);
        assert_eq!(
            goals.calls,
            [
                Call::CanContinue(1),
                Call::Interruptible(1),
                Call::CanStart(0),
                Call::Tick(1),
            ]
        );
        assert!(selector.is_running(GoalId(1)));
    }

    #[test]
    fn failed_continuation_releases_controls_before_other_activation() {
        let mut selector = GoalSelector::new(&[
            normal(0, 0, GoalControls::MOVE),
            normal(1, 1, GoalControls::MOVE),
        ])
        .unwrap();
        let mut goals = Goals::new();
        selector.tick(&mut goals);
        goals.can_continue[0] = false;
        goals.can_start[0] = false;
        goals.calls.clear();
        selector.tick(&mut goals);
        assert_eq!(
            goals.calls,
            [
                Call::CanContinue(0),
                Call::Stop(0),
                Call::CanStart(0),
                Call::CanStart(1),
                Call::Start(1),
                Call::Tick(1),
            ]
        );
    }

    #[test]
    fn explicit_stop_removal_and_stop_all_release_lifecycles_once() {
        let movement = normal(0, 0, GoalControls::MOVE);
        let look = normal(1, 0, GoalControls::LOOK);
        let fallback = normal(2, 1, GoalControls::MOVE | GoalControls::LOOK);
        let mut selector = GoalSelector::new(&[movement, look, fallback]).unwrap();
        let mut goals = Goals::new();
        selector.tick(&mut goals);
        goals.calls.clear();
        assert!(selector.stop(GoalId(0), &mut goals));
        assert!(!selector.stop(GoalId(0), &mut goals));
        selector.update(&[fallback], &mut goals).unwrap();
        selector.tick(&mut goals);
        selector.stop_all(&mut goals);
        selector.stop_all(&mut goals);
        assert_eq!(
            goals.calls,
            [
                Call::Stop(0),
                Call::Stop(1),
                Call::CanStart(2),
                Call::Start(2),
                Call::Tick(2),
                Call::Stop(2),
            ]
        );
        assert!(!goals.active.iter().any(|&active| active));
    }

    #[test]
    fn changed_spec_stops_old_lifecycle_before_restarting() {
        let mut selector = GoalSelector::new(&[normal(0, 0, GoalControls::MOVE)]).unwrap();
        let mut goals = Goals::new();
        selector.tick(&mut goals);
        goals.calls.clear();
        selector
            .update(
                &[
                    normal(0, 1, GoalControls::LOOK),
                    normal(1, 0, GoalControls::MOVE),
                ],
                &mut goals,
            )
            .unwrap();
        assert!(!selector.is_running(GoalId(0)));
        selector.tick(&mut goals);
        assert_eq!(
            goals.calls,
            [
                Call::Stop(0),
                Call::CanStart(1),
                Call::Start(1),
                Call::CanStart(0),
                Call::Start(0),
                Call::Tick(1),
                Call::Tick(0),
            ]
        );
    }

    #[test]
    fn invalid_composition_preserves_running_lifecycles_without_callbacks() {
        let spec = normal(0, 0, GoalControls::MOVE);
        let mut selector = GoalSelector::new(&[spec]).unwrap();
        let mut goals = Goals::new();
        selector.tick(&mut goals);
        goals.calls.clear();
        assert_eq!(
            selector.update(&[spec, spec], &mut goals),
            Err(DuplicateGoalId(GoalId(0)))
        );
        assert!(goals.calls.is_empty());
        selector.tick(&mut goals);
        assert_eq!(goals.calls, [Call::CanContinue(0), Call::Tick(0)]);
    }

    #[test]
    fn target_phase_finishes_before_normal_even_with_same_controls() {
        let normal = normal(0, -10, GoalControls::TARGET | GoalControls::MOVE);
        let target = GoalSpec {
            id: GoalId(1),
            kind: GoalKind::Target,
            priority: 10,
            controls: GoalControls::TARGET | GoalControls::MOVE,
        };
        let mut selector = GoalSelector::new(&[normal, target]).unwrap();
        let mut goals = Goals::new();
        selector.tick(&mut goals);
        assert_eq!(
            goals.calls,
            [
                Call::CanStart(1),
                Call::Start(1),
                Call::Tick(1),
                Call::CanStart(0),
                Call::Start(0),
                Call::Tick(0),
            ]
        );
        goals.calls.clear();
        selector.tick(&mut goals);
        assert_eq!(
            goals.calls,
            [
                Call::CanContinue(1),
                Call::Tick(1),
                Call::CanContinue(0),
                Call::Tick(0),
            ]
        );
    }
}
