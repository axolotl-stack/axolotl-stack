//! Owned committed death observations and detached pre-drop item values.

use super::damage::attribution;
use super::lifecycle::{damage_cause, invoke};
use super::{HostContext, PluginManager, make_world_tx, wit_types as wit};
use crate::ecs::events::EventKind;
use crate::entity::player_life::hooks::{DeathObservation, PlayerDeath, PlayerDeathInventory};
use bevy_ecs::prelude::*;
use wasmtime::component::Resource;

/// Validate captured native identity without resolving a removed or replacement actor.
fn identity(observation: &DeathObservation) -> Result<wit::PlayerIdentity, wit::AccessError> {
    let uuid = observation.uuid.ok_or(wit::AccessError::Unavailable)?;
    let name = observation
        .name
        .as_ref()
        .ok_or(wit::AccessError::Unavailable)?;
    if uuid.is_nil() || name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
        return Err(wit::AccessError::InvalidIdentity);
    }
    let connection = observation.connection.as_ref();
    if connection.is_none() && !observation.retained {
        return Err(wit::AccessError::StalePlayer);
    }
    if connection.is_some_and(|connection| {
        connection.name() != name
            || connection
                .uuid()
                .is_some_and(|value| uuid::Uuid::parse_str(value).ok() != Some(uuid))
    }) {
        return Err(wit::AccessError::InvalidIdentity);
    }
    let authenticated_xuid = observation
        .account
        .as_ref()
        .map(|value| {
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(wit::AccessError::InvalidIdentity);
            }
            value
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or(wit::AccessError::InvalidIdentity)
        })
        .transpose()?;
    Ok(wit::PlayerIdentity {
        authenticated_xuid,
        uuid: uuid.to_string(),
        name: name.clone(),
        connection_id: connection.map(|connection| connection.session_id()),
    })
}

/// Allocate a complete snapshot or roll back its own partial immutable resource batch.
fn inventory(
    context: &mut HostContext,
    inventory: Option<&PlayerDeathInventory>,
) -> Result<(wit::DeathInventory, Vec<u32>), wit::AccessError> {
    let inventory = inventory.ok_or(wit::AccessError::Unavailable)?;
    if usize::from(inventory.held_slot) >= inventory.main.len() {
        return Err(wit::AccessError::InvalidArgument);
    }
    let values = context.capture_item_values(
        inventory
            .main
            .iter()
            .chain(inventory.armour.iter())
            .chain(std::iter::once(&inventory.offhand))
            .chain(inventory.ui.iter()),
    )?;
    let reps = values.iter().map(Resource::rep).collect();
    let mut values = values.into_iter();
    let main = values.by_ref().take(inventory.main.len()).collect();
    let armour = values.by_ref().take(inventory.armour.len()).collect();
    let offhand = values.next().expect("captured complete offhand");
    let ui = values.collect();
    Ok((
        wit::DeathInventory {
            main,
            armour,
            offhand,
            ui,
            held_slot: inventory.held_slot,
        },
        reps,
    ))
}

impl PluginManager {
    /// Deliver committed facts after drops, with fresh world access and no fabricated live player.
    pub(super) fn player_death(&mut self, world: &mut World, event: &PlayerDeath) {
        if !self.begin_tick(world) {
            return;
        }
        let identity = identity(&event.observation);
        for owner in self.subscribers(EventKind::PlayerDeath) {
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            let mut reps = Vec::new();
            let result = invoke(plugin, world, |store, bindings| {
                let tx = make_world_tx(store)?;
                let captured = inventory(store.data_mut(), event.inventory.as_ref());
                let inventory = captured.map(|(values, owned)| {
                    reps = owned;
                    values
                });
                let record = wit::DeathCommitted {
                    identity: identity.clone(),
                    dimension: event.observation.dimension,
                    feet: event.observation.feet.map(|feet| wit::Vec3 {
                        x: feet.x,
                        y: feet.y,
                        z: feet.z,
                    }),
                    cause: damage_cause(event.cause.cause),
                    attribution: attribution(&event.attribution),
                    fall_distance: event.cause.fall_distance,
                    keep_inventory: event.keep_inventory,
                    inventory,
                };
                bindings.call_on_player_death(store, tx, &record)
            });
            if let Err(error) = result {
                plugin.store.data_mut().discard_captured_items(&reps);
                self.callback_failed(world, &owner, &error.to_string());
                if self.readiness.check().is_err() {
                    break;
                }
            }
        }
    }
}
