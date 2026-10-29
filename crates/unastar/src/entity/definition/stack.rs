//! Startup-resolved component composition, independent of ECS component storage.
//!
//! Groups are destructive patches, not layers: adding a group overwrites its kinds,
//! and removing a group clears all its kinds, regardless of their current source.
//! Base bindings and earlier groups are never restored. Group membership is not
//! tracked, so removing a group that was never added still clears its listed kinds.
//!
//! Lifecycle policy: an identical repeated addition is idempotent. A real identity
//! transition followed by a return to the applied prototype in the same batch
//! (including remove + add) requests replacement via `previous == next`. This lets
//! the ECS consumer reset that component without replaying intermediate prototypes.
//! This reset policy is a local implementation choice, not a claim about measured
//! native runtime-state behavior.

/// One component prototype, with both IDs resolved during definition compilation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ComponentBinding {
    pub kind: usize,
    pub prototype: usize,
}

/// Immutable composition shared by instances of one compiled actor definition.
#[derive(Clone, Debug)]
pub(crate) struct DefinitionComposition {
    base: Vec<ComponentBinding>,
    groups: Vec<Vec<ComponentBinding>>,
    kind_count: usize,
}

impl DefinitionComposition {
    /// Validates kind IDs once at startup. Group IDs are indices into `groups`.
    ///
    /// Prototype IDs belong to the caller's prototype registry. Duplicate kinds
    /// are normalized once at startup, retaining only their last binding.
    pub(crate) fn new(
        kind_count: usize,
        mut base: Vec<ComponentBinding>,
        mut groups: Vec<Vec<ComponentBinding>>,
    ) -> Self {
        let mut seen = vec![false; kind_count];
        for bindings in std::iter::once(&mut base).chain(groups.iter_mut()) {
            for binding in bindings.iter() {
                assert!(
                    binding.kind < kind_count,
                    "component kind must be within the compiled kind registry"
                );
            }
            seen.fill(false);
            bindings.reverse();
            bindings.retain(|binding| {
                let keep = !seen[binding.kind];
                seen[binding.kind] = true;
                keep
            });
            bindings.reverse();
        }
        Self {
            base,
            groups,
            kind_count,
        }
    }
}

/// One final per-kind operation for the ECS consumer.
///
/// Remove all `previous` components in a batch before inserting any `next`
/// prototypes. Equal, nonempty IDs deliberately request replacement/reset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CompositionChange {
    pub kind: usize,
    pub previous: Option<usize>,
    pub next: Option<usize>,
}

/// Per-instance selected and last-applied prototypes, indexed by component kind.
///
/// Always use the same compiled composition with an instance. Mutations perform
/// no allocation after construction; dirty kinds retain first-touch order and
/// occur at most once in a batch. Storage is retained across collections.
#[derive(Clone, Debug)]
pub(crate) struct DefinitionStack {
    selected: Vec<Option<usize>>,
    applied: Vec<Option<usize>>,
    dirty: Vec<usize>,
    is_dirty: Vec<bool>,
}

impl DefinitionStack {
    /// Selects the base bindings; the first collection emits their insertions.
    pub(crate) fn new(composition: &DefinitionComposition) -> Self {
        let mut stack = Self {
            selected: vec![None; composition.kind_count],
            applied: vec![None; composition.kind_count],
            dirty: Vec::with_capacity(composition.kind_count),
            is_dirty: vec![false; composition.kind_count],
        };
        for binding in &composition.base {
            stack.select(binding.kind, Some(binding.prototype));
        }
        stack
    }

    /// Applies the group's bindings in order, overwriting current selections.
    pub(crate) fn add_group(&mut self, composition: &DefinitionComposition, group: usize) {
        for binding in &composition.groups[group] {
            self.select(binding.kind, Some(binding.prototype));
        }
    }

    /// Clears every kind listed by the group, without restoring earlier bindings.
    pub(crate) fn remove_group(&mut self, composition: &DefinitionComposition, group: usize) {
        for binding in &composition.groups[group] {
            self.select(binding.kind, None);
        }
    }

    /// Returns the currently selected prototype, including uncollected changes.
    /// Kind IDs must belong to this instance's compiled registry.
    pub(crate) fn component(&self, kind: usize) -> Option<usize> {
        self.selected[kind]
    }

    /// Replaces `out` with this batch's final differences and marks them applied.
    ///
    /// The caller must apply the returned batch before collecting again. Reusing
    /// `out` retains its allocation; it may grow to hold up to `kind_count` entries.
    /// Transient additions to an initially absent kind that ends absent produce
    /// no operation. An applied kind that changes and returns still gets reset.
    pub(crate) fn collect_changes(&mut self, out: &mut Vec<CompositionChange>) {
        out.clear();
        for kind in self.dirty.drain(..) {
            let previous = self.applied[kind];
            let next = self.selected[kind];
            if previous != next || previous.is_some() {
                out.push(CompositionChange {
                    kind,
                    previous,
                    next,
                });
            }
            self.applied[kind] = next;
            self.is_dirty[kind] = false;
        }
    }

    fn select(&mut self, kind: usize, prototype: Option<usize>) {
        if self.selected[kind] == prototype {
            return;
        }
        self.selected[kind] = prototype;
        if !self.is_dirty[kind] {
            self.is_dirty[kind] = true;
            self.dirty.push(kind);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(kind: usize, prototype: usize) -> ComponentBinding {
        ComponentBinding { kind, prototype }
    }

    fn change(kind: usize, previous: Option<usize>, next: Option<usize>) -> CompositionChange {
        CompositionChange {
            kind,
            previous,
            next,
        }
    }

    #[test]
    fn shadowed_base_is_removed_instead_of_restored() {
        let composition =
            DefinitionComposition::new(1, vec![binding(0, 10)], vec![vec![binding(0, 20)]]);
        let mut stack = DefinitionStack::new(&composition);
        let mut changes = Vec::new();
        stack.collect_changes(&mut changes);
        assert_eq!(changes, vec![change(0, None, Some(10))]);

        stack.add_group(&composition, 0);
        stack.collect_changes(&mut changes);
        assert_eq!(changes, vec![change(0, Some(10), Some(20))]);

        stack.remove_group(&composition, 0);
        assert_eq!(stack.component(0), None);
        stack.collect_changes(&mut changes);
        assert_eq!(changes, vec![change(0, Some(20), None)]);
    }

    #[test]
    fn overlapping_groups_are_last_wins_but_removal_is_unconditional() {
        let composition = DefinitionComposition::new(
            3,
            vec![],
            vec![
                vec![binding(0, 10), binding(1, 11)],
                vec![binding(0, 20), binding(2, 22)],
            ],
        );
        let mut stack = DefinitionStack::new(&composition);
        let mut changes = Vec::new();
        stack.add_group(&composition, 0);
        stack.add_group(&composition, 1);
        stack.collect_changes(&mut changes);
        assert_eq!(stack.component(0), Some(20));
        assert_eq!(
            changes,
            vec![
                change(0, None, Some(20)),
                change(1, None, Some(11)),
                change(2, None, Some(22)),
            ]
        );

        // Removing the earlier group also deletes the later group's shared kind.
        stack.remove_group(&composition, 0);
        stack.collect_changes(&mut changes);
        assert_eq!(stack.component(0), None);
        assert_eq!(stack.component(1), None);
        assert_eq!(stack.component(2), Some(22));
        assert_eq!(
            changes,
            vec![change(0, Some(20), None), change(1, Some(11), None)]
        );
    }

    #[test]
    fn independent_base_and_group_kinds_survive_removal() {
        let composition = DefinitionComposition::new(
            4,
            vec![binding(0, 10), binding(1, 11)],
            vec![vec![binding(1, 21), binding(2, 22)], vec![binding(3, 33)]],
        );
        let mut stack = DefinitionStack::new(&composition);
        let mut changes = Vec::new();
        stack.add_group(&composition, 0);
        stack.add_group(&composition, 1);
        stack.collect_changes(&mut changes);
        stack.remove_group(&composition, 0);
        stack.collect_changes(&mut changes);
        assert_eq!(stack.component(0), Some(10));
        assert_eq!(stack.component(1), None);
        assert_eq!(stack.component(2), None);
        assert_eq!(stack.component(3), Some(33));
        assert_eq!(
            changes,
            vec![change(1, Some(21), None), change(2, Some(22), None)]
        );
    }

    #[test]
    fn removing_never_added_group_still_deletes_its_base_kind() {
        let composition =
            DefinitionComposition::new(1, vec![binding(0, 10)], vec![vec![binding(0, 20)]]);
        let mut stack = DefinitionStack::new(&composition);
        let mut changes = Vec::new();
        stack.collect_changes(&mut changes);
        stack.remove_group(&composition, 0);
        stack.collect_changes(&mut changes);
        assert_eq!(changes, vec![change(0, Some(10), None)]);
    }

    #[test]
    fn batching_preserves_operation_order_and_emits_only_final_prototypes() {
        let composition = DefinitionComposition::new(
            3,
            vec![binding(0, 10)],
            vec![
                vec![binding(1, 20), binding(0, 11)],
                vec![binding(0, 12), binding(2, 30)],
            ],
        );
        let mut stack = DefinitionStack::new(&composition);
        let mut changes = Vec::new();
        stack.collect_changes(&mut changes);
        stack.add_group(&composition, 0);
        stack.remove_group(&composition, 0);
        stack.add_group(&composition, 1);
        stack.collect_changes(&mut changes);
        assert_eq!(
            changes,
            vec![change(0, Some(10), Some(12)), change(2, None, Some(30))]
        );

        stack.remove_group(&composition, 1);
        stack.add_group(&composition, 0);
        stack.remove_group(&composition, 0);
        stack.collect_changes(&mut changes);
        assert_eq!(
            changes,
            vec![change(0, Some(12), None), change(2, Some(30), None)]
        );
    }

    #[test]
    fn identical_repeated_add_is_idempotent_but_remove_add_requests_reset() {
        let composition = DefinitionComposition::new(1, vec![], vec![vec![binding(0, 10)]]);
        let mut stack = DefinitionStack::new(&composition);
        let mut changes = Vec::new();
        stack.add_group(&composition, 0);
        stack.add_group(&composition, 0);
        stack.collect_changes(&mut changes);
        assert_eq!(changes, vec![change(0, None, Some(10))]);

        stack.add_group(&composition, 0);
        stack.collect_changes(&mut changes);
        assert!(changes.is_empty());

        stack.remove_group(&composition, 0);
        stack.add_group(&composition, 0);
        stack.collect_changes(&mut changes);
        assert_eq!(changes, vec![change(0, Some(10), Some(10))]);

        stack.collect_changes(&mut changes);
        assert!(changes.is_empty());
    }

    #[test]
    fn returning_to_applied_prototype_after_overwrite_requests_reset() {
        let composition = DefinitionComposition::new(
            1,
            vec![binding(0, 10)],
            vec![vec![binding(0, 20)], vec![binding(0, 10)]],
        );
        let mut stack = DefinitionStack::new(&composition);
        let mut changes = Vec::new();
        stack.collect_changes(&mut changes);
        stack.add_group(&composition, 0);
        stack.add_group(&composition, 1);
        stack.collect_changes(&mut changes);
        assert_eq!(changes, vec![change(0, Some(10), Some(10))]);
    }

    #[test]
    fn canceled_changes_drain_cleanly_and_kind_can_be_dirtied_again() {
        let composition = DefinitionComposition::new(1, vec![], vec![vec![binding(0, 10)]]);
        let mut stack = DefinitionStack::new(&composition);
        let mut changes = vec![change(0, Some(99), None)];
        stack.add_group(&composition, 0);
        stack.remove_group(&composition, 0);
        stack.collect_changes(&mut changes);
        assert!(changes.is_empty());
        stack.collect_changes(&mut changes);
        assert!(changes.is_empty());

        stack.add_group(&composition, 0);
        stack.collect_changes(&mut changes);
        assert_eq!(changes, vec![change(0, None, Some(10))]);
        stack.collect_changes(&mut changes);
        assert!(changes.is_empty());
    }

    #[test]
    fn duplicate_kind_bindings_choose_last_prototype_and_remove_once() {
        let composition = DefinitionComposition::new(
            1,
            vec![binding(0, 1), binding(0, 2)],
            vec![vec![binding(0, 3), binding(0, 4)]],
        );
        let mut stack = DefinitionStack::new(&composition);
        let mut changes = Vec::new();
        stack.collect_changes(&mut changes);
        assert_eq!(changes, vec![change(0, None, Some(2))]);
        stack.add_group(&composition, 0);
        stack.collect_changes(&mut changes);
        assert_eq!(changes, vec![change(0, Some(2), Some(4))]);
        stack.add_group(&composition, 0);
        stack.collect_changes(&mut changes);
        assert!(changes.is_empty());
        stack.remove_group(&composition, 0);
        stack.collect_changes(&mut changes);
        assert_eq!(changes, vec![change(0, Some(4), None)]);
    }
}
