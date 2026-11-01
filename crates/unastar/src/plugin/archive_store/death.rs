//! Strict owned death facts accompany complete native archives without live actor identifiers.

use super::{ArchiveError, Record};
use crate::entity::damage::DamageAttribution;
use serde::{Deserialize, Serialize};
use unastar_data::actor_components::body::ActorDamageCause;

macro_rules! causes {
    ($($variant:ident),* $(,)?)=>{
        #[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize,Deserialize)]
        #[serde(rename_all="kebab-case")]
        pub(crate) enum Cause {$($variant,)*}
        impl From<ActorDamageCause> for Cause {
            fn from(value:ActorDamageCause)->Self {match value {$(ActorDamageCause::$variant=>Self::$variant,)*}}
        }
        impl From<Cause> for ActorDamageCause {
            fn from(value:Cause)->Self {match value {$(Cause::$variant=>Self::$variant,)*}}
        }
    }
}
causes! {None,Override,Contact,EntityAttack,Projectile,Suffocation,Fall,Fire,FireTick,Lava,Drowning,
BlockExplosion,EntityExplosion,Void,SelfDestruct,Magic,Wither,Starve,Anvil,Thorns,FallingBlock,
Piston,FlyIntoWall,Magma,Fireworks,Lightning,Charging,Temperature,Freezing,Stalactite,Stalagmite,
RamAttack,SonicBoom,Campfire,SoulCampfire,MaceSmash,All}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Account {
    pub uuid: String,
    pub xuid: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Owner {
    pub is_player: bool,
    pub uuid: Option<String>,
    pub authenticated: Option<Account>,
    pub connection_id: Option<u64>,
    pub dimension: Option<i32>,
    pub world_lifetime: Option<String>,
    pub feet: Option<[f64; 3]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Attribution {
    pub source_dimension: Option<i32>,
    pub source_world_lifetime: Option<String>,
    pub owner: Option<Owner>,
}
impl From<&DamageAttribution> for Attribution {
    /// Copy only immutable native launch evidence, never resolve an absent source actor.
    fn from(value: &DamageAttribution) -> Self {
        Self {
            source_dimension: value.dimension(),
            source_world_lifetime: value.world_lifetime().map(|id| id.to_string()),
            owner: value.owner().map(|owner| Owner {
                is_player: owner.is_player(),
                uuid: owner.player_uuid().map(|id| id.to_string()),
                authenticated: owner.authenticated().map(|account| Account {
                    uuid: account.uuid().to_string(),
                    xuid: account.xuid(),
                }),
                connection_id: owner.session_id(),
                dimension: owner.dimension(),
                world_lifetime: owner.world_lifetime().map(|id| id.to_string()),
                feet: owner.feet_position().map(|feet| feet.to_array()),
            }),
        }
    }
}

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

/// Canonical opaque lifetime strings remain descriptive and cannot authorize live access.
fn valid_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}
impl Facts {
    /// Reject corrupt or forged native facts before exposing or persisting an archive.
    pub(crate) fn validate(&self) -> Result<(), ArchiveError> {
        if !valid_uuid(&self.capture_id)
            || self
                .fall_distance
                .is_some_and(|value| !value.is_finite() || value < 0.0)
            || self
                .attribution
                .source_world_lifetime
                .as_deref()
                .is_some_and(|value| !valid_uuid(value))
        {
            return Err(ArchiveError::InvalidSnapshot);
        }
        if let Some(owner) = &self.attribution.owner
            && (owner
                .uuid
                .as_deref()
                .is_some_and(|value| !valid_uuid(value))
                || owner
                    .world_lifetime
                    .as_deref()
                    .is_some_and(|value| !valid_uuid(value))
                || owner.connection_id == Some(0)
                || owner.feet.is_some_and(|feet| {
                    feet.iter()
                        .any(|value| !value.is_finite() || !(*value as f32).is_finite())
                })
                || owner.authenticated.as_ref().is_some_and(|account| {
                    !owner.is_player
                        || account.xuid == 0
                        || !valid_uuid(&account.uuid)
                        || owner.uuid.as_deref() != Some(account.uuid.as_str())
                }))
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
