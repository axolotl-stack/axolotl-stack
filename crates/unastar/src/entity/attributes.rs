//! Ordered, layout-scoped actor attributes and reusable AddActor wire snapshots.
//!
//! Callers supply both the ordered names and every initial numeric triple from
//! their effective definition/native facts. A captured current value is sampled
//! state, not an authored or universal default. This module does not clamp,
//! validate ranges, apply modifiers, or implement native attribute handlers.
//! AddActor's `SyncedAttribute` triples do not establish UpdateAttributes default
//! or modifier semantics; those require independent measurements.

use std::collections::BTreeSet;
use std::sync::Arc;

use bevy_ecs::prelude::Component;
use jolyne::valentine::types::SyncedAttribute;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AttributeError {
    #[error("duplicate actor attribute name: {0}")]
    DuplicateName(Arc<str>),
    #[error("actor attribute layout requires {expected} values, received {actual}")]
    ValueCount { expected: usize, actual: usize },
    #[error("attribute handle belongs to another layout")]
    ForeignHandle,
    #[error("attribute snapshot belongs to another layout")]
    ForeignSnapshot,
}

/// A frozen, ordered set of names, shared by actors with the same wire layout.
/// Equal names in separately constructed layouts do not make them interchangeable.
#[derive(Debug)]
pub struct AttributeLayout {
    names: Box<[Arc<str>]>,
}

impl AttributeLayout {
    /// Preserve caller order exactly and reject duplicate names.
    pub fn new(names: impl IntoIterator<Item = Arc<str>>) -> Result<Self, AttributeError> {
        let mut seen = BTreeSet::new();
        let mut ordered = Vec::new();
        for name in names {
            if !seen.insert(Arc::clone(&name)) {
                return Err(AttributeError::DuplicateName(name));
            }
            ordered.push(name);
        }
        Ok(Self {
            names: ordered.into_boxed_slice(),
        })
    }

    pub fn names(&self) -> &[Arc<str>] {
        &self.names
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Resolve at startup/command compilation, not in a per-tick actor loop.
    /// The returned handle retains the exact layout, preventing stale identity reuse.
    pub fn resolve(self: &Arc<Self>, name: &str) -> Option<AttributeId> {
        let index = self
            .names
            .iter()
            .position(|candidate| candidate.as_ref() == name)?;
        Some(AttributeId {
            owner: Arc::clone(self),
            index,
        })
    }
}

/// An opaque numeric slot scoped to the exact `Arc<AttributeLayout>` resolved.
#[derive(Debug, Clone)]
pub struct AttributeId {
    owner: Arc<AttributeLayout>,
    index: usize,
}

/// A caller-supplied wire triple, with no implicit clamp/default/modifier policy.
/// Equality compares the transmitted float bits, including signed zero and NaNs,
/// so a repeated identical wire state does not create a spurious revision.
#[derive(Debug, Clone, Copy)]
pub struct AttributeValue {
    pub min: f32,
    pub current: f32,
    pub max: f32,
}

impl PartialEq for AttributeValue {
    fn eq(&self, other: &Self) -> bool {
        self.min.to_bits() == other.min.to_bits()
            && self.current.to_bits() == other.current.to_bits()
            && self.max.to_bits() == other.max.to_bits()
    }
}

/// Independent actor state backed by a shared frozen layout.
/// Numeric access/update and snapshot refresh perform no allocation or name lookup.
#[derive(Component, Debug)]
pub struct ActorAttributes {
    layout: Arc<AttributeLayout>,
    values: Box<[AttributeValue]>,
    revision: u64,
    clean_revision: u64,
}

impl ActorAttributes {
    /// Initial triples must be explicitly supplied in the layout's order.
    pub fn new(
        layout: Arc<AttributeLayout>,
        values: impl Into<Box<[AttributeValue]>>,
    ) -> Result<Self, AttributeError> {
        let values = values.into();
        if values.len() != layout.len() {
            return Err(AttributeError::ValueCount {
                expected: layout.len(),
                actual: values.len(),
            });
        }
        Ok(Self {
            layout,
            values,
            revision: 0,
            clean_revision: 0,
        })
    }

    pub fn layout(&self) -> &Arc<AttributeLayout> {
        &self.layout
    }

    pub fn get(&self, id: &AttributeId) -> Result<AttributeValue, AttributeError> {
        if !Arc::ptr_eq(&self.layout, &id.owner) {
            return Err(AttributeError::ForeignHandle);
        }
        Ok(self.values[id.index])
    }

    /// Preserve the complete supplied triple. Returns whether its wire bits changed.
    /// A foreign handle is rejected before any state or revision is touched.
    pub fn update(
        &mut self,
        id: &AttributeId,
        value: AttributeValue,
    ) -> Result<bool, AttributeError> {
        if !Arc::ptr_eq(&self.layout, &id.owner) {
            return Err(AttributeError::ForeignHandle);
        }
        if self.values[id.index] == value {
            return Ok(false);
        }
        self.values[id.index] = value;
        self.revision = self.revision.wrapping_add(1);
        Ok(true)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether a value changed since construction or the caller's last `mark_clean`.
    /// This is local bookkeeping, not an assertion about network delivery.
    pub fn is_dirty(&self) -> bool {
        self.revision != self.clean_revision
    }

    pub fn mark_clean(&mut self) {
        self.clean_revision = self.revision;
    }

    /// Allocate names and wire entries once, seeded only from explicit actor state.
    pub fn snapshot(&self) -> AttributeSnapshot {
        let entries = self
            .layout
            .names
            .iter()
            .zip(self.values.iter())
            .map(|(name, value)| SyncedAttribute {
                attribute_name: name.as_ref().into(),
                min_value: value.min,
                current_value: value.current,
                max_value: value.max,
            })
            .collect();
        AttributeSnapshot {
            layout: Arc::clone(&self.layout),
            entries,
            revision: self.revision,
        }
    }

    /// Refresh only numeric fields in the existing wire buffer, without allocations
    /// or replacing names. A matching layout may be shared by independent actors.
    /// Reject a foreign snapshot before modifying any of its entries or revision.
    pub fn copy_into(&self, snapshot: &mut AttributeSnapshot) -> Result<(), AttributeError> {
        if !Arc::ptr_eq(&self.layout, &snapshot.layout) {
            return Err(AttributeError::ForeignSnapshot);
        }
        for (entry, value) in snapshot.entries.iter_mut().zip(self.values.iter()) {
            entry.min_value = value.min;
            entry.current_value = value.current;
            entry.max_value = value.max;
        }
        snapshot.revision = self.revision;
        Ok(())
    }
}

/// A reusable, layout-bound AddActor attribute buffer in source order.
/// Its private entries keep callers from altering names, length, or ordering.
#[derive(Debug)]
pub struct AttributeSnapshot {
    layout: Arc<AttributeLayout>,
    entries: Vec<SyncedAttribute>,
    revision: u64,
}

impl AttributeSnapshot {
    pub fn layout(&self) -> &Arc<AttributeLayout> {
        &self.layout
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn entries(&self) -> &[SyncedAttribute] {
        &self.entries
    }

    /// Transfer the buffer directly into `AddActorPacket::attributes_list`.
    /// Consuming the snapshot performs no allocation or numeric conversion.
    pub fn into_entries(self) -> Vec<SyncedAttribute> {
        self.entries
    }

    /// Clone owned entries for AddActor while retaining this reusable buffer.
    /// Unlike `copy_into`, this packet-ownership boundary may allocate.
    pub fn to_owned_entries(&self) -> Vec<SyncedAttribute> {
        self.entries.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{ActorAttributes, AttributeError, AttributeLayout, AttributeValue};
    use std::sync::Arc;

    fn layout(names: &[&str]) -> Arc<AttributeLayout> {
        Arc::new(AttributeLayout::new(names.iter().map(|name| Arc::<str>::from(*name))).unwrap())
    }

    fn value(current: f32) -> AttributeValue {
        AttributeValue {
            min: -4.0,
            current,
            max: 12.0,
        }
    }

    #[test]
    fn duplicate_names_and_incomplete_initial_state_are_rejected() {
        let layout = layout(&["second", "first"]);
        assert!(layout.resolve("missing").is_none());
        assert!(matches!(
            AttributeLayout::new([Arc::from("duplicate"), Arc::from("duplicate")]),
            Err(AttributeError::DuplicateName(name)) if name.as_ref() == "duplicate"
        ));
        assert!(matches!(
            ActorAttributes::new(Arc::clone(&layout), vec![value(1.0)]),
            Err(AttributeError::ValueCount {
                expected: 2,
                actual: 1
            })
        ));
    }

    #[test]
    fn foreign_handle_rejection_preserves_numeric_state_and_dirty_status() {
        let own = layout(&["amount"]);
        let foreign = layout(&["amount"]);
        let own_id = own.resolve("amount").unwrap();
        let foreign_id = foreign.resolve("amount").unwrap();
        let mut actor = ActorAttributes::new(own, vec![value(3.0)]).unwrap();
        assert_eq!(actor.get(&foreign_id), Err(AttributeError::ForeignHandle));
        assert_eq!(
            actor.update(&foreign_id, value(8.0)),
            Err(AttributeError::ForeignHandle)
        );
        assert_eq!(actor.get(&own_id).unwrap(), value(3.0));
        assert_eq!(actor.revision(), 0);
        assert!(!actor.is_dirty());
    }

    #[test]
    fn foreign_snapshot_rejection_preserves_all_entries_and_revision() {
        let first_layout = layout(&["first", "second"]);
        let second_layout = layout(&["first", "second"]);
        let first = ActorAttributes::new(first_layout, vec![value(1.0), value(2.0)]).unwrap();
        let second_id = second_layout.resolve("second").unwrap();
        let mut second = ActorAttributes::new(second_layout, vec![value(3.0), value(4.0)]).unwrap();
        second.update(&second_id, value(5.0)).unwrap();
        let mut snapshot = second.snapshot();
        let original = snapshot.to_owned_entries();
        let original_revision = snapshot.revision();
        assert_eq!(
            first.copy_into(&mut snapshot),
            Err(AttributeError::ForeignSnapshot)
        );
        assert_eq!(snapshot.entries(), original.as_slice());
        assert_eq!(snapshot.revision(), original_revision);
    }

    #[test]
    fn identical_wire_state_does_not_dirty_or_revise() {
        let layout = layout(&["amount"]);
        let id = layout.resolve("amount").unwrap();
        let initial = value(3.0);
        let mut actor = ActorAttributes::new(layout, vec![initial]).unwrap();
        assert!(!actor.update(&id, initial).unwrap());
        assert_eq!(actor.revision(), 0);
        assert!(!actor.is_dirty());
        assert!(actor.update(&id, value(4.0)).unwrap());
        assert_eq!(actor.revision(), 1);
        assert!(actor.is_dirty());
        actor.mark_clean();
        assert!(!actor.update(&id, value(4.0)).unwrap());
        assert_eq!(actor.revision(), 1);
        assert!(!actor.is_dirty());

        let exact = AttributeValue {
            min: -0.0,
            current: f32::from_bits(0x7fc0_0001),
            max: f32::INFINITY,
        };
        assert!(actor.update(&id, exact).unwrap());
        actor.mark_clean();
        assert!(!actor.update(&id, exact).unwrap());
        assert!(!actor.is_dirty());
        assert_eq!(actor.get(&id).unwrap(), exact);
    }

    #[test]
    fn shared_layout_actors_are_independent_and_snapshots_follow_each_actor() {
        let layout = layout(&["second", "first"]);
        let id = layout.resolve("first").unwrap();
        let mut first =
            ActorAttributes::new(Arc::clone(&layout), vec![value(2.0), value(1.0)]).unwrap();
        let second = ActorAttributes::new(layout, vec![value(6.0), value(5.0)]).unwrap();
        let mut snapshot = first.snapshot();
        let unconstrained = AttributeValue {
            min: 7.0,
            current: -9.0,
            max: -2.0,
        };
        first.update(&id, unconstrained).unwrap();
        assert_eq!(first.get(&id).unwrap(), unconstrained);
        assert_eq!(second.get(&id).unwrap(), value(5.0));
        assert_eq!(second.revision(), 0);
        first.copy_into(&mut snapshot).unwrap();
        assert_eq!(snapshot.revision(), first.revision());
        assert_eq!(snapshot.entries()[1].min_value, 7.0);
        assert_eq!(snapshot.entries()[1].current_value, -9.0);
        assert_eq!(snapshot.entries()[1].max_value, -2.0);
        second.copy_into(&mut snapshot).unwrap();
        assert_eq!(snapshot.entries()[1].current_value, 5.0);
    }
}
