//! Exact, bounded live changes complete at a successful callback boundary.

use super::*;
use crate::entity::components::{ArmourInventory, HeldSlot, MainInventory, OffhandSlot};
use crate::item::ItemStack;
use crate::plugin::manifest::PluginCapability;
use std::sync::Mutex;

const MAX_CALLBACK_REQUESTS: usize = 64;

/// A status contains no live authority and remains readable after the source disappears.
pub(in crate::plugin::manager) struct Operation(Mutex<wit::InventoryMechanicsStatus>);
impl Operation {
    /// Complete once after applying or discarding the original callback's intent.
    fn finish(&self, status: wit::InventoryMechanicsStatus) {
        if let Ok(mut current) = self.0.lock() {
            *current = status;
        }
    }
}

/// Retained slot values keep their byte reservation until the callback finishes.
pub(in crate::plugin::manager) struct Request {
    scope: Scope,
    reference: PlayerLineage,
    player: Entity,
    held: u8,
    changes: Arc<values::Value<Vec<Change>>>,
    operation: Arc<Operation>,
}

struct Change {
    area: wit::InventoryArea,
    index: usize,
    expected: ItemStack,
    replacement: ItemStack,
}

/// Resolve only physical player areas; cursor/grid and private storage are outside this API.
fn slot(
    world: &World,
    player: Entity,
    area: wit::InventoryArea,
    index: usize,
) -> Option<&ItemStack> {
    match area {
        wit::InventoryArea::Main => world.get::<MainInventory>(player)?.0.item(index),
        wit::InventoryArea::Armour => world.get::<ArmourInventory>(player)?.slots().get(index),
        wit::InventoryArea::Offhand if index == 0 => Some(&world.get::<OffhandSlot>(player)?.0),
        _ => None,
    }
}

/// Network identity is part of an exact source even though item-value equality omits it.
fn same_source(left: &ItemStack, right: &ItemStack) -> bool {
    left == right && left.net_id == right.net_id
}

/// Complete retained actors may be changed, while dead, initializing or frozen actors may not.
fn available(world: &World, player: Entity, reference: &PlayerLineage) -> bool {
    matches!(
        availability(world, player),
        wit::PlayerAvailability::Ready | wit::PlayerAvailability::Retained
    ) && snapshot(world, player, reference).is_ok()
        && inventory::borrowed_bytes(world, player).is_ok()
        && super::super::readiness::admission_allowed(world)
}

/// Publish without invoking policy hooks or recursively entering the component manager.
/// This boundary precedes a native melee caller's original-stack weapon-wear check.
pub(in crate::plugin::manager) fn finish(
    context: &mut HostContext,
    world: &mut World,
    accepted: bool,
) {
    for request in std::mem::take(&mut context.pending_inventory_mechanics) {
        let status = if !accepted {
            wit::InventoryMechanicsStatus::Cancelled
        } else if !request.scope.world.matches(world)
            || !request.reference.matches(world, request.player)
            || !available(world, request.player, &request.reference)
            || (crate::server::game::inventory_effects::grant_pending(world, request.player)
                && request.changes.iter().any(|change| {
                    !change.replacement.is_empty()
                        && change.replacement.net_id != change.expected.net_id
                }))
            || world.get::<HeldSlot>(request.player).map(|slot| slot.0) != Some(request.held)
            || !request.changes.iter().all(|change| {
                slot(world, request.player, change.area, change.index)
                    .is_some_and(|current| same_source(current, &change.expected))
            })
        {
            wit::InventoryMechanicsStatus::Rejected(wit::AccessError::Unavailable)
        } else {
            for change in request.changes.iter() {
                match change.area {
                    wit::InventoryArea::Main => {
                        world
                            .get_mut::<MainInventory>(request.player)
                            .expect("checked main inventory")
                            .0
                            .set_item(change.index, change.replacement.clone())
                            .expect("checked main slot");
                    }
                    wit::InventoryArea::Armour => {
                        world
                            .get_mut::<ArmourInventory>(request.player)
                            .expect("checked armour inventory")
                            .inventory_mut()
                            .set_item(change.index, change.replacement.clone())
                            .expect("checked armour slot");
                    }
                    wit::InventoryArea::Offhand => {
                        world
                            .get_mut::<OffhandSlot>(request.player)
                            .expect("checked offhand inventory")
                            .0 = change.replacement.clone().into_slot();
                    }
                    _ => unreachable!("only physical areas may be staged"),
                }
            }
            world
                .entity_mut(request.player)
                .insert(crate::server::game::RequestIdAssignments::default());
            crate::server::game::send_inventory_contents(world, request.player);
            wit::InventoryMechanicsStatus::Applied
        };
        request.operation.finish(status);
    }
}

impl HostContext {
    /// Inspect owned results in host regressions after an optional failed guest is disabled.
    #[cfg(test)]
    pub(in crate::plugin::manager) fn test_inventory_mechanics_statuses(
        &self,
    ) -> Vec<wit::InventoryMechanicsStatus> {
        self.resources
            .entries
            .values()
            .filter_map(|entry| match entry {
                Entry::InventoryMechanicsOperation(operation) => Some(*operation.0.lock().unwrap()),
                _ => None,
            })
            .collect()
    }
    /// Copy the genuine physical stack rather than reconstructing a stored value with a new ID.
    pub(super) fn inventory_mechanics_stack(
        &mut self,
        rep: u32,
        area: wit::InventoryArea,
        index: u32,
    ) -> Result<Resource<wit::Stack>, wit::AccessError> {
        let (reference, player) = self.live_player(rep)?;
        self.require(PluginCapability::ReadInventory)?;
        self.require(PluginCapability::ItemValues)?;
        let world = unsafe { self.world() };
        if !available(world, player, &reference) {
            return Err(wit::AccessError::Unavailable);
        }
        let item =
            slot(world, player, area, index as usize).ok_or(wit::AccessError::InvalidArgument)?;
        self.own_item(item.clone())
    }
    /// Check authority and lifecycle before copying any touched slot's native data.
    fn inventory_mechanics_source(
        &mut self,
        rep: u32,
    ) -> Result<(PlayerLineage, Entity, u8), wit::AccessError> {
        let (reference, player) = self.live_player(rep)?;
        self.require(PluginCapability::InventoryMechanics)?;
        if self.pending_inventory_mechanics.len() >= MAX_CALLBACK_REQUESTS {
            return Err(wit::AccessError::LimitExceeded);
        }
        let world = unsafe { self.world() };
        if !available(world, player, &reference) {
            return Err(wit::AccessError::Unavailable);
        }
        let held = world
            .get::<HeldSlot>(player)
            .ok_or(wit::AccessError::Unavailable)?
            .0;
        if held >= 9 {
            return Err(wit::AccessError::Unavailable);
        }
        Ok((reference, player, held))
    }

    /// Resource capacity and retained bytes are acquired before publishing an intent.
    fn stage_inventory_mechanics(
        &mut self,
        source: (PlayerLineage, Entity, u8),
        changes: Arc<values::Value<Vec<Change>>>,
    ) -> Result<Resource<wit::InventoryMechanicsOperation>, wit::AccessError> {
        let scope = self.resources.scope.ok_or(wit::AccessError::ExpiredScope)?;
        let operation = Arc::new(Operation(Mutex::new(wit::InventoryMechanicsStatus::Staged)));
        let rep = self.resources.allocate(
            Entry::InventoryMechanicsOperation(operation.clone()),
            self.api_limits.max_owned_resources,
        )?;
        self.pending_inventory_mechanics.push(Request {
            scope,
            reference: source.0,
            player: source.1,
            held: source.2,
            changes,
            operation,
        });
        Ok(Resource::new_own(rep))
    }

    /// A bijection can move existing stacks but can never create, merge or duplicate them.
    pub(super) fn stage_hotbar(
        &mut self,
        rep: u32,
        order: Vec<u8>,
    ) -> Result<Resource<wit::InventoryMechanicsOperation>, wit::AccessError> {
        let source = self.inventory_mechanics_source(rep)?;
        if crate::server::game::inventory_effects::grant_pending(unsafe { self.world() }, source.1)
        {
            return Err(wit::AccessError::Unavailable);
        }
        if order.len() != 9
            || order.iter().any(|index| *index >= 9)
            || (0..9).any(|index| order.iter().filter(|value| **value == index).count() != 1)
        {
            return Err(wit::AccessError::InvalidArgument);
        }
        let world = unsafe { self.world() };
        let main = &world
            .get::<MainInventory>(source.1)
            .ok_or(wit::AccessError::Unavailable)?
            .0;
        let original = main.slots().get(..9).ok_or(wit::AccessError::Unavailable)?;
        let bytes = original
            .iter()
            .map(inventory::native_stack_bytes)
            .sum::<usize>()
            * 2
            + source.0.retained_bytes()
            + 256;
        let reservation = self.resources.value_budget.reserve(bytes)?;
        let changes = order
            .into_iter()
            .enumerate()
            .map(|(index, from)| Change {
                area: wit::InventoryArea::Main,
                index,
                expected: original[index].clone(),
                replacement: original[usize::from(from)].clone(),
            })
            .collect();
        self.stage_inventory_mechanics(source, reservation.bind(changes))
    }

    /// A captured selected stack is never replaced by a later similarly named item.
    pub(super) fn stage_consumption(
        &mut self,
        rep: u32,
        expected_rep: u32,
        count: u8,
    ) -> Result<Resource<wit::InventoryMechanicsOperation>, wit::AccessError> {
        let source = self.inventory_mechanics_source(rep)?;
        let expected = self.item_value(expected_rep)?;
        if expected.is_empty() || count == 0 || count > expected.count {
            return Err(wit::AccessError::InvalidArgument);
        }
        let index = usize::from(source.2);
        let world = unsafe { self.world() };
        let current = slot(world, source.1, wit::InventoryArea::Main, index)
            .ok_or(wit::AccessError::Unavailable)?;
        if !same_source(current, &expected) {
            return Err(wit::AccessError::Unavailable);
        }
        let reservation = self.resources.value_budget.reserve(
            inventory::native_stack_bytes(&expected) * 2 + source.0.retained_bytes() + 256,
        )?;
        let replacement = if count == expected.count {
            ItemStack::empty()
        } else {
            expected.clone().with_count(expected.count - count)
        };
        self.stage_inventory_mechanics(
            source,
            reservation.bind(vec![Change {
                area: wit::InventoryArea::Main,
                index,
                expected,
                replacement,
            }]),
        )
    }

    /// Native wear is the only modified field; unknown NBT and user-data suffixes survive.
    pub(super) fn stage_repair(
        &mut self,
        rep: u32,
        area: wit::InventoryArea,
        index: u32,
        expected_rep: u32,
        points: u32,
    ) -> Result<Resource<wit::InventoryMechanicsOperation>, wit::AccessError> {
        let source = self.inventory_mechanics_source(rep)?;
        let expected = self.item_value(expected_rep)?;
        let maximum = expected
            .item()
            .map(crate::server::game::durability::max_durability)
            .unwrap_or(0);
        let damage = crate::item::enchant::durability_damage(&expected);
        if points == 0
            || points > i32::MAX as u32
            || maximum <= 0
            || damage < 0
            || damage >= maximum
            || !crate::item::enchant::valid_user_data(&expected)
            || !matches!(
                area,
                wit::InventoryArea::Main | wit::InventoryArea::Armour | wit::InventoryArea::Offhand
            )
        {
            return Err(wit::AccessError::InvalidArgument);
        }
        let index = usize::try_from(index).map_err(|_| wit::AccessError::InvalidArgument)?;
        let current = slot(unsafe { self.world() }, source.1, area, index)
            .ok_or(wit::AccessError::InvalidArgument)?;
        if !same_source(current, &expected) {
            return Err(wit::AccessError::Unavailable);
        }
        let reservation = self.resources.value_budget.reserve(
            inventory::native_stack_bytes(&expected) * 2 + source.0.retained_bytes() + 288,
        )?;
        let replacement = if damage == 0 {
            expected.clone()
        } else {
            crate::item::enchant::with_durability_damage(
                &expected,
                damage.saturating_sub(points as i32).max(0),
            )
        };
        self.stage_inventory_mechanics(
            source,
            reservation.bind(vec![Change {
                area,
                index,
                expected,
                replacement,
            }]),
        )
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostInventoryMechanicsOperation
    for HostContext
{
    fn status(
        &mut self,
        operation: Resource<wit::InventoryMechanicsOperation>,
    ) -> Result<wit::InventoryMechanicsStatus, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::InventoryMechanics)?;
        let Some(Entry::InventoryMechanicsOperation(operation)) =
            self.resources.entries.get(&operation.rep())
        else {
            return Err(wit::AccessError::InvalidArgument);
        };
        operation
            .0
            .lock()
            .map(|status| *status)
            .map_err(|_| wit::AccessError::Unavailable)
    }
    fn drop(
        &mut self,
        operation: Resource<wit::InventoryMechanicsOperation>,
    ) -> wasmtime::Result<()> {
        if matches!(
            self.resources.entries.get(&operation.rep()),
            Some(Entry::InventoryMechanicsOperation(_))
        ) {
            self.resources.entries.remove(&operation.rep());
        }
        Ok(())
    }
}
