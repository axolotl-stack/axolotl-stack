//! Owned complete death evidence never resolves a departed or replacement player.

use super::*;
use crate::entity::player_life::hooks::PlayerDeath;
use crate::plugin::{archive_store, manifest::PluginCapability};
use crate::server::game::player_state::death_capture;

/// Per-component retention stays charged independently of native event dispatch.
pub(super) struct Image {
    pub(super) capture: Arc<death_capture::Capture>,
    pub(super) owner: wit::PlayerIdentity,
    facts: archive_store::DeathFacts,
}

/// Copy descriptive archived launch evidence without granting any old actor capability.
pub(super) fn facts(facts: &archive_store::DeathFacts) -> wit::DeathFacts {
    wit::DeathFacts {
        capture_id: facts.capture_id.clone(),
        phase: match facts.phase {
            archive_store::DeathPhase::CommittedBeforeDrops => {
                wit::DeathCapturePhase::CommittedBeforeDrops
            }
        },
        cause: super::super::lifecycle::damage_cause(facts.cause.into()),
        attribution: super::occurrences::attribution(&facts.attribution),
        fall_distance: facts.fall_distance,
        keep_inventory: facts.keep_inventory,
    }
}

impl HostContext {
    /// Allocate from genuine pre-drop evidence with complete account and per-instance bounds.
    pub(in crate::plugin::manager) fn capture_death_archive(
        &mut self,
        event: &PlayerDeath,
    ) -> Result<Resource<wit::DeathState>, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::PlayerStateArchives)?;
        let capture = event
            .state
            .as_ref()
            .map_err(|error| match error {
                death_capture::Error::Unavailable => wit::AccessError::Unavailable,
                death_capture::Error::TooLarge => wit::AccessError::LimitExceeded,
                death_capture::Error::Invalid => wit::AccessError::InvalidArgument,
            })?
            .clone();
        let owner = super::super::deaths::identity(&event.observation)?;
        if owner.uuid != capture.snapshot.data.uuid
            || owner.authenticated_xuid
                != capture
                    .snapshot
                    .xuid
                    .as_deref()
                    .and_then(|xuid| xuid.parse::<u64>().ok())
        {
            return Err(wit::AccessError::InvalidIdentity);
        }
        let facts = archive_store::DeathFacts {
            capture_id: capture.nonce.to_string(),
            phase: archive_store::DeathPhase::CommittedBeforeDrops,
            cause: event.cause.cause.into(),
            attribution: archive_store::DeathAttribution::from(&event.attribution),
            fall_distance: event.cause.fall_distance,
            keep_inventory: event.keep_inventory,
        };
        facts
            .validate()
            .map_err(|_| wit::AccessError::InvalidArgument)?;
        let bytes = capture
            .bytes
            .checked_add(4096)
            .ok_or(wit::AccessError::LimitExceeded)?;
        let value = self.resources.value_budget.own(
            Image {
                capture,
                owner,
                facts,
            },
            bytes,
        )?;
        self.resources
            .allocate(
                Entry::DeathState(value),
                self.api_limits.max_owned_resources,
            )
            .map(Resource::new_own)
    }

    /// Callback failure releases only this captured resource, never a durable archive file.
    pub(in crate::plugin::manager) fn discard_death_archive(&mut self, rep: u32) {
        if matches!(self.resources.entries.get(&rep), Some(Entry::DeathState(_))) {
            self.resources.entries.remove(&rep);
        }
    }

    /// Historical ownership stays usable outside the actor's original world lifetime.
    fn death_archive(&mut self, rep: u32) -> Result<Arc<values::Value<Image>>, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::PlayerStateArchives)?;
        match self.resources.entries.get(&rep) {
            Some(Entry::DeathState(image)) => Ok(image.clone()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostDeathState for HostContext {
    fn death_occurrence(
        &mut self,
        state: Resource<wit::DeathState>,
    ) -> Result<Option<wit::DeathOccurrence>, wit::AccessError> {
        Ok(self
            .death_archive(state.rep())?
            .capture
            .snapshot
            .data
            .death_occurrence
            .as_ref()
            .map(super::occurrences::death))
    }
    fn facts(
        &mut self,
        state: Resource<wit::DeathState>,
    ) -> Result<wit::DeathFacts, wit::AccessError> {
        Ok(facts(&self.death_archive(state.rep())?.facts))
    }
    fn state_image(
        &mut self,
        state: Resource<wit::DeathState>,
    ) -> Result<Resource<wit::PlayerStateImage>, wit::AccessError> {
        let image = self.death_archive(state.rep())?;
        self.state_from_death(&image.capture, image.owner.clone())
    }
    fn archive(
        &mut self,
        state: Resource<wit::DeathState>,
        key: String,
    ) -> Result<wit::ArchiveReference, wit::ArchiveError> {
        let image = self
            .death_archive(state.rep())
            .map_err(wit::ArchiveError::Access)?;
        let _copy = self
            .resources
            .value_budget
            .reserve(image.capture.bytes)
            .map_err(wit::ArchiveError::Access)?;
        let capture = &image.capture;
        let record = archive_store::Record::new(
            &self.plugin_id.0,
            archive_store::Owner {
                uuid: image.owner.uuid.clone(),
                authenticated_xuid: image.owner.authenticated_xuid,
                name: image.owner.name.clone(),
                connection_id: image.owner.connection_id,
            },
            archive_store::SourceWorld {
                uuid: capture.world.id.to_string(),
                name: capture.world.name.clone(),
                dimension: capture.world.dimension,
            },
            capture.tick,
            (*capture.snapshot).clone(),
        )
        .and_then(|record| record.with_death(image.facts.clone()))
        .map_err(|_| wit::ArchiveError::InvalidSnapshot)?;
        let reference = self.archive_call(true, |namespace| namespace.capture(&key, &record))?;
        Ok(wit::ArchiveReference {
            key: reference.key,
            revision: reference.revision,
            fingerprint: reference.fingerprint,
            owner: super::archive::identity(&reference.owner),
        })
    }
    fn drop(&mut self, state: Resource<wit::DeathState>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&state.rep());
        Ok(())
    }
}
