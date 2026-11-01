//! Full native block plans bridged into scoped component authorization and owned completion.

use super::lifecycle::invoke;
use super::{HostContext, PluginManager, make_player_resource, make_world_tx, wit_types as wit};
use crate::ecs::events::EventKind;
use crate::registry::block::StateValue;
use crate::server::game::block_interaction::{
    BlockClick, BlockInteraction, BlockInteractionHandler, BlockInteractionHooks,
    BlockInteractionKind, HeldItemSnapshot, PlayerBlockCommitted, ScopedBlockDispatcher,
};
use bevy_ecs::prelude::*;
use std::sync::{Arc, Mutex};
use wasmtime::component::Resource;

/// Native hooks outside manager-owned queued commits borrow the shared manager once.
struct Bridge(Arc<Mutex<PluginManager>>);
impl BlockInteractionHandler for Bridge {
    fn on_interaction(&self, world: &mut World, event: &mut BlockInteraction) {
        if event.is_cancelled() || event.inline() {
            return;
        }
        match self.0.try_lock() {
            Ok(mut manager) => manager.block_proposal(world, event),
            Err(_) => event.cancel(),
        }
    }
    fn on_committed(&self, world: &mut World, event: &PlayerBlockCommitted) {
        if event.inline() || event.operation_id() == 0 {
            return;
        }
        match self.0.try_lock() {
            Ok(mut manager) => manager.block_committed(world, event),
            Err(_) => super::readiness::fence_world(world),
        }
    }
}

/// Install one native semantic bridge rather than interpreting client JSON or packet IDs.
pub(super) fn install(manager: Arc<Mutex<PluginManager>>, world: &mut World) {
    world.init_resource::<BlockInteractionHooks>();
    world
        .resource_mut::<BlockInteractionHooks>()
        .register(Bridge(manager));
}

/// Copy a loaded registry value into typed properties without exposing internal runtime IDs.
pub(super) fn value(world: &World, runtime: u32) -> Result<wit::BlockValue, wit::AccessError> {
    let registry = world
        .get_resource::<crate::server::game::types::BlockRegistryResource>()
        .ok_or(wit::AccessError::Unavailable)?;
    let block = registry
        .0
        .get_by_runtime_id(runtime)
        .ok_or(wit::AccessError::Unavailable)?;
    let mut properties = Vec::new();
    for property in block.data.properties {
        let state = block
            .state_value(runtime, property.name)
            .ok_or(wit::AccessError::Unavailable)?;
        properties.push(wit::BlockProperty {
            name: property.name.into(),
            value: match state {
                StateValue::Byte(value) => wit::BlockPropertyValue::Byte(value),
                StateValue::Int(value) => wit::BlockPropertyValue::Integer(value),
                StateValue::Str(value) => wit::BlockPropertyValue::Text(value.into()),
            },
        });
    }
    properties.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(wit::BlockValue {
        identifier: block.string_id.clone(),
        properties,
    })
}
/// Semantic operation tags do not reuse numeric packet or ECS discriminants.
fn kind(kind: BlockInteractionKind) -> wit::BlockOperationKind {
    match kind {
        BlockInteractionKind::Break => wit::BlockOperationKind::Break,
        BlockInteractionKind::Place => wit::BlockOperationKind::Place,
        BlockInteractionKind::Use => wit::BlockOperationKind::Use,
    }
}
/// Every coordinate belongs to the current real native world.
fn position(pos: glam::IVec3) -> wit::BlockPos {
    wit::BlockPos {
        x: pos.x,
        y: pos.y,
        z: pos.z,
    }
}
/// Preserve only present native click facts, including unknown faces as none.
pub(super) fn click(source: Option<&BlockClick>) -> Option<wit::BlockClick> {
    source.map(|source| wit::BlockClick {
        position: position(source.position),
        face: source.face.and_then(|face| match face {
            0 => Some(wit::BlockFace::Down),
            1 => Some(wit::BlockFace::Up),
            2 => Some(wit::BlockFace::North),
            3 => Some(wit::BlockFace::South),
            4 => Some(wit::BlockFace::West),
            5 => Some(wit::BlockFace::East),
            _ => None,
        }),
        offset: source
            .offset
            .filter(|offset| offset.is_finite())
            .map(|offset| wit::Vec3 {
                x: offset.x,
                y: offset.y,
                z: offset.z,
            }),
    })
}
/// Obtain the actual current dimension; no default dimension is invented.
fn dimension(world: &World) -> Result<i32, wit::AccessError> {
    world
        .get_resource::<crate::world::ChunkManager>()
        .map(|manager| manager.dimension())
        .ok_or(wit::AccessError::Unavailable)
}
/// Release a supplied batch after failed canonical lowering while preserving ordinary item accounting.
fn discard(context: &mut HostContext, reps: &[u32]) {
    for rep in reps {
        let _ = wit::HostStack::drop(context, Resource::new_own(*rep));
    }
}
/// Capture both held cells as one bounded batch; no partial values survive failure.
fn held_values(
    context: &mut HostContext,
    before: Option<&HeldItemSnapshot>,
    after: Option<&HeldItemSnapshot>,
) -> Result<(wit::BlockHeldItems, Vec<u32>), wit::AccessError> {
    let mut reps = Vec::new();
    let result = (|| {
        let mut capture = |held: Option<&HeldItemSnapshot>| -> Result<_, wit::AccessError> {
            held.map(|held| {
                let resource = context.own_item(held.stack.clone())?;
                reps.push(resource.rep());
                Ok(resource)
            })
            .transpose()
        };
        Ok(wit::BlockHeldItems {
            slot: before.or(after).map(|held| held.slot),
            before: capture(before)?,
            after: capture(after)?,
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

impl ScopedBlockDispatcher for PluginManager {
    fn item_committed(
        &mut self,
        world: &mut World,
        event: &crate::server::game::item_actions::PlayerItemActionCommitted,
    ) {
        self.item_completed(world, event);
    }
    fn proposed(&mut self, world: &mut World, event: &mut BlockInteraction) {
        self.block_proposal(world, event);
    }
    fn committed(&mut self, world: &mut World, event: &PlayerBlockCommitted) {
        self.block_committed(world, event);
    }
}

impl PluginManager {
    /// Authorize the full immutable batch using the original shared cycle budget.
    fn block_proposal(&mut self, world: &mut World, event: &mut BlockInteraction) {
        if event.is_cancelled() {
            return;
        }
        if !self.begin_tick(world) {
            event.cancel();
            return;
        }
        let owners = self.subscribers(EventKind::BlockTransaction);
        if owners.is_empty() {
            return;
        }
        let Ok(dimension) = dimension(world) else {
            event.cancel();
            return;
        };
        let cells: Result<Vec<_>, _> = event
            .cells()
            .iter()
            .map(|cell| {
                Ok(wit::PlannedBlockCell {
                    position: position(cell.position),
                    before: value(world, cell.before.runtime_id)?,
                    proposed: value(world, cell.proposed_runtime_id)?,
                })
            })
            .collect::<Result<_, wit::AccessError>>();
        let Ok(cells) = cells else {
            event.cancel();
            return;
        };
        for owner in owners {
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            if cells.len() > plugin.manifest.limits.max_block_cells as usize {
                event.cancel();
                break;
            }
            let player = event.player();
            let result = invoke(plugin, world, |store, bindings| {
                let tx = make_world_tx(store)?;
                let player = make_player_resource(store, player)?;
                let (items, reps) = held_values(store.data_mut(), event.held(), None)
                    .map_err(|error| anyhow::anyhow!("block item capture failed: {error:?}"))?;
                let proposal = wit::BlockProposal {
                    operation_id: event.operation_id(),
                    dimension,
                    kind: kind(event.kind()),
                    cells: cells.clone(),
                    click: click(event.click()),
                    held_slot: items.slot,
                    held: items.before,
                };
                let result = bindings.call_on_block_transaction(&mut *store, tx, player, &proposal);
                if result.is_err() {
                    discard(store.data_mut(), &reps);
                }
                result
            });
            match result {
                Ok(true) => (),
                Ok(false) => {
                    event.cancel();
                    break;
                }
                Err(error) => {
                    self.callback_failed(world, &owner, &error.to_string());
                    event.cancel();
                    break;
                }
            }
        }
    }

    /// Deliver an owned actual completion even if a post-mutation callback removed the source.
    fn block_committed(&mut self, world: &mut World, event: &PlayerBlockCommitted) {
        if event.operation_id() == 0 || !self.begin_tick(world) {
            return;
        }
        let owners = self.subscribers(EventKind::BlockCommitted);
        if owners.is_empty() {
            return;
        }
        let Ok(dimension) = dimension(world) else {
            return;
        };
        let Some(uuid) = event.actor_uuid() else {
            return;
        };
        let authenticated_xuid = match event
            .identity()
            .xuid()
            .map(|xuid| {
                if xuid.is_empty() || !xuid.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(());
                }
                xuid.parse::<u64>().map_err(|_| ())
            })
            .transpose()
        {
            Ok(value) if value.is_none_or(|value| value > 0) => value,
            _ => return,
        };
        let identity = wit::PlayerIdentity {
            uuid: uuid.to_string(),
            name: event.identity().name().into(),
            authenticated_xuid,
            connection_id: Some(event.identity().session_id()),
        };
        let cells: Result<Vec<_>, _> = event
            .changes()
            .iter()
            .map(|cell| {
                Ok(wit::CommittedBlockCell {
                    position: position(cell.position),
                    before: value(world, cell.before.runtime_id)?,
                    after: cell
                        .after
                        .as_ref()
                        .map(|after| value(world, after.runtime_id))
                        .transpose()?,
                })
            })
            .collect::<Result<_, wit::AccessError>>();
        for owner in owners {
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            let cells = if cells
                .as_ref()
                .is_ok_and(|cells| cells.len() > plugin.manifest.limits.max_block_cells as usize)
            {
                Err(wit::AccessError::LimitExceeded)
            } else {
                cells.clone()
            };
            let result = invoke(plugin, world, |store, bindings| {
                let tx = make_world_tx(store)?;
                let captured =
                    held_values(store.data_mut(), event.held_before(), event.held_after());
                let (held_items, reps) = match captured {
                    Ok((items, reps)) => (Ok(items), reps),
                    Err(error) => (Err(error), Vec::new()),
                };
                let record = wit::BlockCommitted {
                    operation_id: event.operation_id(),
                    dimension,
                    player: identity.clone(),
                    kind: kind(event.kind()),
                    cells: cells.clone(),
                    click: click(event.click()),
                    held_items,
                };
                let result = bindings.call_on_block_committed(&mut *store, tx, &record);
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
