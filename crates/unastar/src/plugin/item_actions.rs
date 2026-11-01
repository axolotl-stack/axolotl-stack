//! Scoped authorization of real native item actions and owned physical completions.

use super::lifecycle::{invoke, invoke_validated};
use super::{HostContext, PluginManager, make_player_resource, make_world_tx, wit_types as wit};
use crate::ecs::events::EventKind;
use crate::entity::components::Player;
use crate::server::game::item_actions::{
    self as native, ItemActionHandler, ItemActionHooks, PlayerItemAction, PlayerItemActionCommitted,
};
use bevy_ecs::prelude::*;
use std::sync::{Arc, Mutex};
use wasmtime::component::Resource;

/// Native simulation callbacks borrow the manager once; inline completions use their caller.
struct Bridge(Arc<Mutex<PluginManager>>);
impl ItemActionHandler for Bridge {
    fn on_item_action(&self, world: &mut World, event: &mut PlayerItemAction) {
        if event.is_cancelled() {
            return;
        }
        match self.0.try_lock() {
            Ok(mut manager) => manager.item_proposal(world, event),
            Err(_) => event.cancel(),
        }
    }
    fn on_item_committed(&self, world: &mut World, event: &PlayerItemActionCommitted) {
        if event.inline {
            return;
        }
        match self.0.try_lock() {
            Ok(mut manager) => manager.item_completed(world, event),
            Err(_) => super::readiness::fence_world(world),
        }
    }
}
/// Register semantic native boundaries; no packet or JSON policy lives in the guest bridge.
pub(super) fn install(manager: Arc<Mutex<PluginManager>>, world: &mut World) {
    world.init_resource::<ItemActionHooks>();
    world
        .resource_mut::<ItemActionHooks>()
        .register(Bridge(manager));
}
/// Translate stage tags independently of packet discriminants.
fn stage(kind: native::ItemActionKind) -> wit::ItemActionStage {
    match kind {
        native::ItemActionKind::Use => wit::ItemActionStage::Use,
        native::ItemActionKind::Release => wit::ItemActionStage::Release,
        native::ItemActionKind::Consume => wit::ItemActionStage::Consume,
        native::ItemActionKind::Charge => wit::ItemActionStage::Charge,
        native::ItemActionKind::Launch => wit::ItemActionStage::Launch,
    }
}
/// Snapshot account facts, rejecting malformed identities instead of inventing authentication.
fn identity(identity: &native::ItemActorIdentity) -> Result<wit::PlayerIdentity, wit::AccessError> {
    let authenticated_xuid = identity
        .xuid()
        .map(|xuid| {
            if !xuid.is_empty() && xuid.bytes().all(|b| b.is_ascii_digit()) {
                xuid.parse::<u64>()
                    .ok()
                    .filter(|id| *id > 0)
                    .ok_or(wit::AccessError::InvalidIdentity)
            } else {
                Err(wit::AccessError::InvalidIdentity)
            }
        })
        .transpose()?;
    Ok(wit::PlayerIdentity {
        uuid: identity
            .uuid()
            .ok_or(wit::AccessError::InvalidIdentity)?
            .to_string(),
        name: identity.name().ok_or(wit::AccessError::Unavailable)?.into(),
        authenticated_xuid,
        connection_id: identity.session_id(),
    })
}
/// Coordinates are native feet facts rather than eye positions or guessed destinations.
fn vec3(value: glam::DVec3) -> wit::Vec3 {
    wit::Vec3 {
        x: value.x,
        y: value.y,
        z: value.z,
    }
}
/// Preserve unavailable block state explicitly and never manufacture a target player.
fn target(world: &World, target: &native::ItemTarget) -> wit::ItemActionTarget {
    match target {
        native::ItemTarget::Air => wit::ItemActionTarget::Air,
        native::ItemTarget::Block(block) => wit::ItemActionTarget::Block(wit::ItemBlockTarget {
            click: super::blocks::click(Some(
                &crate::server::game::block_interaction::BlockClick {
                    item_action: None,
                    position: block.position,
                    face: Some(block.face),
                    offset: block.offset,
                },
            ))
            .expect("native click exists"),
            block: block
                .state
                .ok_or(wit::AccessError::Unavailable)
                .and_then(|state| super::blocks::value(world, state)),
        }),
        native::ItemTarget::Actor(actor) => wit::ItemActionTarget::Actor(wit::ItemActorTarget {
            identifier: actor.identifier.clone(),
            feet: vec3(actor.feet),
            player: actor.identity.as_ref().map(identity).transpose(),
        }),
    }
}
/// Release only resources supplied to a failed canonical lowering.
fn discard(context: &mut HostContext, reps: &[u32]) {
    for rep in reps {
        let _ = wit::HostStack::drop(context, Resource::new_own(*rep));
    }
}
/// Keep a guest correlation inside its originating component instance's native claim.
fn custom_action(context: &HostContext, correlation: u64) -> String {
    format!("component:{}:{correlation}", context.generation.simple())
}

/// Only the original component instance can observe its own opaque correlation echo.
fn custom_correlation(context: &HostContext, action: &str) -> Option<u64> {
    let prefix = format!("component:{}:", context.generation.simple());
    action.strip_prefix(&prefix)?.parse().ok()
}

/// Capture a bounded complete known cost batch; partial quota allocations never survive failure.
fn items(
    context: &mut HostContext,
    event: &PlayerItemActionCommitted,
) -> Result<(wit::ItemActionItems, Vec<u32>), wit::AccessError> {
    let mut reps = Vec::new();
    let result = (|| {
        let mut capture = |stack: &crate::item::ItemStack| {
            let item = context.own_item(stack.clone())?;
            reps.push(item.rep());
            Ok::<_, wit::AccessError>(item)
        };
        let before = capture(&event.stack)?;
        let after = event.after.as_ref().map(&mut capture).transpose()?;
        let ammunition = event
            .ammo
            .as_ref()
            .map(|ammo| {
                Ok::<_, wit::AccessError>(wit::ItemAmmunitionChange {
                    main_slot: ammo.slot.map(|slot| slot as u32),
                    before: capture(&ammo.before)?,
                    after: event.ammo_after.as_ref().map(&mut capture).transpose()?,
                })
            })
            .transpose()?;
        Ok(wit::ItemActionItems {
            selected_before: before,
            selected_after: after,
            ammunition,
        })
    })();
    match result {
        Ok(items) => Ok((items, reps)),
        Err(error) => {
            discard(context, &reps);
            Err(error)
        }
    }
}
impl PluginManager {
    /// Run a typed decision before any native cost or effect, sharing the normal cycle budget.
    fn item_proposal(&mut self, world: &mut World, event: &mut PlayerItemAction) {
        if !self.begin_tick(world) {
            event.cancel();
            return;
        }
        let owners = self.subscribers(EventKind::ItemAction);
        if owners.is_empty() {
            return;
        }
        let Some(dimension) = world
            .get_resource::<crate::world::ChunkManager>()
            .map(|m| m.dimension())
        else {
            event.cancel();
            return;
        };
        let target_facts = target(world, event.target());
        let target_player = match event.target() {
            native::ItemTarget::Actor(actor) if world.get::<Player>(actor.entity).is_some() => {
                Some(actor.entity)
            }
            _ => None,
        };
        for owner in owners {
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            let owner_lease = Arc::downgrade(&plugin.store.data().command_owner);
            let available = event.stack().count;
            let result = invoke_validated(
                plugin,
                world,
                |store, bindings| {
                    let tx = make_world_tx(store)?;
                    let player = make_player_resource(store, event.player())?;
                    let target_player = target_player
                        .map(|target| make_player_resource(store, target))
                        .transpose()?;
                    let selected = store
                        .data_mut()
                        .own_item(event.stack().clone())
                        .map_err(|error| anyhow::anyhow!("item capture failed: {error:?}"))?;
                    let rep = selected.rep();
                    let proposal = wit::ItemActionProposal {
                        action_id: event.action_id(),
                        dimension,
                        stage: stage(event.kind()),
                        slot: event.slot(),
                        selected,
                        target: target_facts.clone(),
                    };
                    let result = bindings.call_on_item_action(
                        &mut *store,
                        tx,
                        player,
                        target_player,
                        &proposal,
                    );
                    if result.is_err() {
                        discard(store.data_mut(), &[rep]);
                    }
                    result
                },
                |decision| match decision {
                    wit::ItemActionDecision::Pass | wit::ItemActionDecision::Deny => true,
                    wit::ItemActionDecision::Handled(action) => {
                        action.consume > 0 && action.consume <= available
                    }
                },
            );
            match result {
                Ok(wit::ItemActionDecision::Pass) => event.require_owner(owner_lease),
                Ok(wit::ItemActionDecision::Deny) => {
                    event.cancel();
                    break;
                }
                Ok(wit::ItemActionDecision::Handled(action)) => {
                    event.require_owner(owner_lease);
                    event.handle(native::HandledItemAction {
                        action: custom_action(plugin.store.data(), action.correlation),
                        consume: action.consume,
                    });
                }
                Err(error) => {
                    self.callback_failed(world, &owner, &error.to_string());
                    event.cancel();
                    break;
                }
            }
        }
    }
    /// Notify actual physical outcomes without depending on a surviving live source actor.
    pub(super) fn item_completed(&mut self, world: &mut World, event: &PlayerItemActionCommitted) {
        if !self.begin_tick(world) {
            return;
        }
        let (Some(dimension), Ok(player)) = (event.dimension, identity(event.identity())) else {
            return;
        };
        let target = target(world, &event.target);
        let outcome = match &event.outcome {
            native::ItemActionOutcome::Started => wit::ItemActionOutcome::Started,
            native::ItemActionOutcome::Consumed { count } => {
                wit::ItemActionOutcome::Consumed(*count)
            }
            native::ItemActionOutcome::Charged => wit::ItemActionOutcome::Charged,
            native::ItemActionOutcome::Interacted => wit::ItemActionOutcome::Interacted,
            native::ItemActionOutcome::Block { operation_id } => {
                wit::ItemActionOutcome::Block(*operation_id)
            }
            native::ItemActionOutcome::Launched { .. } => wit::ItemActionOutcome::Launched(
                event
                    .projectile
                    .as_ref()
                    .map(|(id, feet)| wit::LaunchedItemActor {
                        identifier: id.clone(),
                        feet: vec3(*feet),
                    })
                    .ok_or(wit::AccessError::Unavailable),
            ),
            native::ItemActionOutcome::Custom { count, .. } => {
                wit::ItemActionOutcome::Custom(wit::CustomItemOutcome {
                    correlation: None,
                    consumed: *count,
                })
            }
        };
        for owner in self.subscribers(EventKind::ItemActionCommitted) {
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            let mut outcome = outcome.clone();
            if let (
                wit::ItemActionOutcome::Custom(custom),
                native::ItemActionOutcome::Custom { action, .. },
            ) = (&mut outcome, &event.outcome)
            {
                custom.correlation = custom_correlation(plugin.store.data(), action);
            }
            let result = invoke(plugin, world, |store, bindings| {
                let tx = make_world_tx(store)?;
                let (items, reps) = match items(store.data_mut(), event) {
                    Ok((items, reps)) => (Ok(items), reps),
                    Err(error) => (Err(error), Vec::new()),
                };
                let event = wit::ItemActionCommitted {
                    action_id: event.action_id,
                    dimension,
                    player: player.clone(),
                    stage: stage(event.kind),
                    slot: event.slot,
                    target: target.clone(),
                    items,
                    outcome: outcome.clone(),
                };
                let result = bindings.call_on_item_action_committed(&mut *store, tx, &event);
                if result.is_err() {
                    discard(store.data_mut(), &reps);
                }
                result
            });
            if let Err(error) = result {
                self.callback_failed(world, &owner, &error.to_string());
                if self.readiness.check().is_err() {
                    break;
                }
            }
        }
    }
}
