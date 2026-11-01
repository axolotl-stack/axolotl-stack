//! Canonical ownership of a sessionless native source, independent of archive handles.

use super::{PlayerData, RestorationSupplement, StorageError, StorageResult};
use serde::{Deserialize, Serialize};

/// A durable source has an explicit physical disposition; absence is never a consumed proof.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourcePhase {
    Frozen,
    Active,
    Retired,
    Transferred,
    Terminal,
}
impl SourcePhase {
    /// Only these states may still own an old body or block normal account publication.
    pub(crate) fn unresolved(self) -> bool {
        matches!(self, Self::Frozen | Self::Active)
    }
}

/// One canonical generation binds real account storage to one module's actual source lineage.
/// Raw inventory stays in the same player row; this supplement is deliberately nonrecursive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceActorRecord {
    pub version: u16,
    pub module: String,
    pub nonce: String,
    pub generation: u64,
    pub sample: u64,
    pub name: String,
    pub world: String,
    pub dimension: i32,
    pub phase: SourcePhase,
    pub supplement: Option<RestorationSupplement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff: Option<SourceHandoff>,
}

/// A closed source and its destination payload were acknowledged in the same canonical row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceHandoff {
    pub id: String,
    pub request_hash: [u8; 32],
}
impl SourceActorRecord {
    /// Reject malformed authority before startup, a provider operation or any actor creation.
    pub(crate) fn validate(&self, data: &PlayerData) -> StorageResult<()> {
        self.validate_mode(data, data.game_mode)
    }

    /// Validate immutable archive stamps against the snapshot's separately captured durable base.
    pub(crate) fn validate_mode(&self, data: &PlayerData, canonical_mode: u8) -> StorageResult<()> {
        let token = |value: &str| {
            uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
        };
        if self.version != 1
            || self.module.is_empty()
            || self.module.len() > 128
            || !self
                .module
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            || !token(&self.nonce)
            || !token(&self.world)
            || self.generation == 0
            || self.name.is_empty()
            || self.name.len() > 64
            || self.name.chars().any(char::is_control)
            || (self.phase.unresolved() && self.dimension != data.dimension)
            || (self.phase.unresolved() && self.supplement.is_none())
            || (self.phase == SourcePhase::Terminal
                && (data.health != 0.0
                    || self
                        .supplement
                        .as_ref()
                        .is_none_or(|supplement| !supplement.dead)))
            || self.handoff.as_ref().is_some_and(|handoff| {
                !matches!(self.phase, SourcePhase::Transferred | SourcePhase::Retired)
                    || data
                        .inventory_effects
                        .get(&handoff.id)
                        .is_none_or(|receipt| receipt.request_hash != handoff.request_hash)
            })
        {
            return Err(StorageError::Deserialization(
                "invalid canonical source ownership".into(),
            ));
        }
        if let Some(supplement) = &self.supplement {
            supplement.validate_mode(data, canonical_mode)?;
        }
        Ok(())
    }

    /// Exact ownership includes generation; a historical nonce cannot select a replacement body.
    pub(crate) fn matches(&self, proof: &SourceProof) -> bool {
        self.module == proof.module
            && self.nonce == proof.nonce
            && self.generation == proof.generation
    }
}

/// Engine-created evidence for one already acknowledged canonical source generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceProof {
    pub module: String,
    pub nonce: String,
    pub generation: u64,
}
impl From<&SourceActorRecord> for SourceProof {
    /// Never bind a source using only its account or a historical actor/session number.
    fn from(record: &SourceActorRecord) -> Self {
        Self {
            module: record.module.clone(),
            nonce: record.nonce.clone(),
            generation: record.generation,
        }
    }
}
