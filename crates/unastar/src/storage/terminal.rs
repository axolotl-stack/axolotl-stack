//! Engine-owned evidence that one actual native death reached its durable post-drop state.

use super::{InventoryEffectRecord, PlayerData, StorageError, StorageResult};
use serde::{Deserialize, Serialize};

/// This record is native persistence evidence, not module ownership or a player capability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeTerminalRecord {
    pub(crate) version: u16,
    pub(crate) death: String,
    pub(crate) world: String,
    pub(crate) lifetime: String,
    pub(crate) dimension: i32,
    pub(crate) lineage: String,
    pub(crate) name: String,
    pub(crate) xuid: Option<String>,
    pub(crate) id: String,
    pub(crate) receipt: InventoryEffectRecord,
}
impl NativeTerminalRecord {
    /// Authority remains tied to the exact canonical checkpoint and receipt, never a history row.
    pub(crate) fn validate(&self, data: &PlayerData) -> StorageResult<()> {
        let token = |value: &str| {
            uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
        };
        let checkpoint = data
            .restoration_checkpoint
            .as_ref()
            .ok_or_else(|| StorageError::Deserialization("terminal checkpoint missing".into()))?;
        if self.version != 1
            || !token(&self.death)
            || !token(&self.world)
            || !token(&self.lifetime)
            || !token(&self.lineage)
            || self.dimension != data.dimension
            || data.health != 0.0
            || self.name.is_empty()
            || self.name.len() > 64
            || self.name.chars().any(char::is_control)
            || self.xuid.as_ref().is_some_and(|id| {
                id.is_empty() || id.len() > 128 || id.chars().any(char::is_control)
            })
            || self.xuid != checkpoint.supplement.xuid
            || self.id != format!("native-terminal:{}:{}", self.lifetime, self.death)
            || data.inventory_effects.get(&self.id) != Some(&self.receipt)
            || checkpoint.id != self.id
            || checkpoint.revision != self.receipt.revision
            || checkpoint.request_hash != self.receipt.request_hash
            || !checkpoint.supplement.dead
            || data.death_occurrence.as_ref().is_some_and(|event| {
                event.id != self.death
                    || event.runtime.as_ref().is_some_and(|runtime| {
                        runtime.lineage != self.lineage || runtime.world_lifetime != self.lifetime
                    })
                    || event
                        .post_drop
                        .as_ref()
                        .is_none_or(|proof| proof.id != self.id || proof.receipt != self.receipt)
            })
        {
            return Err(StorageError::Deserialization(
                "inconsistent native terminal proof".into(),
            ));
        }
        Ok(())
    }
}
