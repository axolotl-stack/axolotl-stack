//! Numeric actor flag state. Catalogue values are bit indices, not masks.
//!
//! Storage includes the third word needed by the pinned catalogue's highest
//! member. This type does not choose constructor defaults or bind words to
//! actor-data fields; those are separate, target-version wire decisions.

use bevy_ecs::prelude::Component;
use jolyne::valentine::actor_flags;

const WORD_BITS: u32 = u64::BITS;
const WORDS: usize = (actor_flags::NAMEPLATE_DEPTH_TESTED / WORD_BITS + 1) as usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("actor flag index {0} is outside the catalogue-sized storage")]
pub struct FlagIndexOutOfRange(pub u32);

/// Per-actor flags, addressed by startup-resolved catalogue indices.
///
/// A zero value is empty storage, not a vanilla actor's constructor state.
/// Bounds validate storage capacity; the caller resolves registered flag names.
#[derive(Component, Debug, Clone, Default)]
pub struct ActorFlags {
    words: [u64; WORDS],
    revision: u64,
    clean_revision: u64,
}

impl ActorFlags {
    /// Explicitly supplied state, without assuming it is a constructor default.
    pub fn from_words(words: [u64; WORDS]) -> Self {
        Self {
            words,
            revision: 0,
            clean_revision: 0,
        }
    }

    pub fn get(&self, index: u32) -> Result<bool, FlagIndexOutOfRange> {
        let word = self
            .words
            .get((index / WORD_BITS) as usize)
            .ok_or(FlagIndexOutOfRange(index))?;
        Ok(word & (1u64 << (index % WORD_BITS)) != 0)
    }

    /// Returns whether the transmitted bits changed. Invalid indices leave all
    /// words and revision bookkeeping untouched.
    pub fn update(&mut self, index: u32, enabled: bool) -> Result<bool, FlagIndexOutOfRange> {
        let word = self
            .words
            .get_mut((index / WORD_BITS) as usize)
            .ok_or(FlagIndexOutOfRange(index))?;
        let mask = 1u64 << (index % WORD_BITS);
        let next = if enabled { *word | mask } else { *word & !mask };
        if next == *word {
            return Ok(false);
        }
        *word = next;
        self.revision = self.revision.wrapping_add(1);
        Ok(true)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.clean_revision
    }

    /// Local bookkeeping only; this is not an acknowledgment of viewer delivery.
    pub fn mark_clean(&mut self) {
        self.clean_revision = self.revision;
    }

    /// Fixed-size, independent state for a network/composition barrier.
    pub fn snapshot(&self) -> ActorFlagSnapshot {
        ActorFlagSnapshot {
            words: self.words,
            revision: self.revision,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorFlagSnapshot {
    words: [u64; WORDS],
    revision: u64,
}

impl ActorFlagSnapshot {
    pub fn words(&self) -> &[u64; WORDS] {
        &self.words
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

/// Register the marker-to-flag effects used by the living actor lane before
/// freezing component factories. No definition names are looked up on apply.
/// Constructor/class flags, if any, must be supplied independently.
pub fn register_component_kinds(
    builder: &mut super::definition::ComponentKindsBuilder,
) -> Result<(), String> {
    use unastar_data::actor_components::body;
    register_flag::<body::IsBaby, { actor_flags::BABY }>(builder)?;
    register_flag::<body::CanClimb, { actor_flags::CANCLIMB }>(builder)?;
    register_flag::<body::IsHiddenWhenInvisible, { actor_flags::HIDDEN_WHEN_INVISIBLE }>(builder)?;
    register_flag::<body::UsesLegacyFriction, { actor_flags::USES_LEGACY_FRICTION }>(builder)?;
    register_flag::<body::IsSaddled, { actor_flags::SADDLED }>(builder)?;
    register_flag::<body::UsesUniformAirDrag, { actor_flags::USES_UNIFORM_AIR_DRAG }>(builder)?;
    Ok(())
}

pub(crate) fn register_flag<B: unastar_data::actor_components::ComponentBody, const INDEX: u32>(
    builder: &mut super::definition::ComponentKindsBuilder,
) -> Result<(), String> {
    builder.register(|_: B| Ok(()), apply_flag::<INDEX>, remove_flag::<INDEX>)
}

fn apply_flag<const INDEX: u32>(_: &(), entity: &mut bevy_ecs::world::EntityWorldMut<'_>) {
    if !entity.contains::<ActorFlags>() {
        entity.insert(ActorFlags::default());
    }
    entity
        .get_mut::<ActorFlags>()
        .expect("flag state inserted")
        .update(INDEX, true)
        .expect("generated flag index fits catalogue storage");
}

fn remove_flag<const INDEX: u32>(entity: &mut bevy_ecs::world::EntityWorldMut<'_>) {
    if let Some(mut flags) = entity.get_mut::<ActorFlags>() {
        flags
            .update(INDEX, false)
            .expect("generated flag index fits catalogue storage");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_crossing_word_boundaries_remain_independent() {
        let mut flags = ActorFlags::default();
        for index in [
            actor_flags::EATING,
            actor_flags::LAYING_DOWN,
            actor_flags::USES_LEGACY_FRICTION,
            actor_flags::USES_UNIFORM_AIR_DRAG,
            actor_flags::NAMEPLATE_DEPTH_TESTED,
        ] {
            flags.update(index, true).unwrap();
        }
        assert_eq!(flags.snapshot().words(), &[1u64 << 63, 1 | (1u64 << 63), 3]);
        flags
            .update(actor_flags::USES_LEGACY_FRICTION, false)
            .unwrap();
        assert!(!flags.get(actor_flags::USES_LEGACY_FRICTION).unwrap());
        for index in [
            actor_flags::EATING,
            actor_flags::LAYING_DOWN,
            actor_flags::USES_UNIFORM_AIR_DRAG,
            actor_flags::NAMEPLATE_DEPTH_TESTED,
        ] {
            assert!(flags.get(index).unwrap());
        }
    }

    #[test]
    fn snapshots_and_revisions_survive_unchanged_and_invalid_updates() {
        let mut flags = ActorFlags::default();
        flags
            .update(actor_flags::USES_UNIFORM_AIR_DRAG, true)
            .unwrap();
        let snapshot = flags.snapshot();
        flags.mark_clean();
        assert!(!flags.is_dirty());
        assert!(
            !flags
                .update(actor_flags::USES_UNIFORM_AIR_DRAG, true)
                .unwrap()
        );
        assert_eq!(flags.snapshot(), snapshot);
        assert_eq!(
            flags.update(u32::MAX, true),
            Err(FlagIndexOutOfRange(u32::MAX))
        );
        assert_eq!(flags.get(u32::MAX), Err(FlagIndexOutOfRange(u32::MAX)));
        assert_eq!(flags.snapshot(), snapshot);
        assert!(!flags.is_dirty());
        flags
            .update(actor_flags::USES_UNIFORM_AIR_DRAG, false)
            .unwrap();
        assert!(flags.is_dirty());
        assert_ne!(flags.revision(), snapshot.revision());
        assert_eq!(snapshot.words()[2], 1);
        assert_eq!(flags.snapshot().words()[2], 0);
    }
}
