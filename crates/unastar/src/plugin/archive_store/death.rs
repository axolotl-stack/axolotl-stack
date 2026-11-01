//! Strict owned death facts accompany complete native archives without live actor identifiers.

use super::{ArchiveError, Record};
use crate::entity::damage::owned::valid_uuid;
pub(crate) use crate::entity::damage::owned::{Attribution, Cause};
use serde::{Deserialize, Serialize};

/// Complete immutable cause/credit facts from the same committed native death.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Phase {
    CommittedBeforeDrops,
}

/// Snapshot phase is fixed by native capture and never chosen by a component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Facts {
    pub capture_id: String,
    pub phase: Phase,
    pub cause: Cause,
    pub attribution: Attribution,
    pub fall_distance: Option<f64>,
    pub keep_inventory: bool,
}

impl Facts {
    /// Reject corrupt or forged native facts before exposing or persisting an archive.
    pub(crate) fn validate(&self) -> Result<(), ArchiveError> {
        if !valid_uuid(&self.capture_id)
            || self
                .fall_distance
                .is_some_and(|value| !value.is_finite() || value < 0.0)
            || !self.attribution.valid()
        {
            return Err(ArchiveError::InvalidSnapshot);
        }
        Ok(())
    }
}

impl Record {
    /// Bind owned death facts only to genuine raw dead state, never a restored live image.
    pub(crate) fn with_death(mut self, facts: Facts) -> Result<Self, ArchiveError> {
        facts.validate()?;
        if !self.state.dead || self.state.data.health != 0.0 || self.source_fence.is_some() {
            return Err(ArchiveError::InvalidSnapshot);
        }
        self.death = Some(facts);
        Ok(self)
    }
}
