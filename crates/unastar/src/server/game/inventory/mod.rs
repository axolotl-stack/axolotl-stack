//! Inventory domain system.
//!
//! Processes ItemStackRequest (through [`engine`]), ContainerClose,
//! MobEquipment, Interact and InventoryTransaction packets.

pub mod containers;
pub mod engine;

/// The BDS capture the inventory tests replay.
#[cfg(test)]
pub(crate) mod capture {
    use bytes::Bytes;
    use jolyne::valentine::{McpePacketArgs, McpePacketData};

    /// `testdata/bds-inventory-moves.jsonl`: the inventory packets of the
    /// client-diff scenario `inventory-moves` against BDS 1.26.51 (Endstone
    /// 0.11.12, gophertunnel client, survival, 2026-10-25): one JSON object
    /// per packet with `seq`, `dir`, `name` and the packet `hex`.
    pub const BDS_INVENTORY_MOVES: &str = include_str!("testdata/bds-inventory-moves.jsonl");

    /// Decode one captured packet (header included, no batch length).
    pub fn decode(hex: &str) -> McpePacketData {
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let mut framed = Vec::with_capacity(bytes.len() + 5);
        let mut len = bytes.len() as u32;
        while len >= 0x80 {
            framed.push(len as u8 | 0x80);
            len >>= 7;
        }
        framed.push(len as u8);
        framed.extend_from_slice(&bytes);
        McpePacketData::decode_inner_bytes(&mut Bytes::from(framed), McpePacketArgs)
            .unwrap()
            .1
    }
}

/// Real frozen vanilla advertisement/executor shared by inventory test worlds.
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Arc, LazyLock};

    use crate::crafting::{CraftingCatalog, execution::CraftingExecutor};
    use crate::item::test_support::VANILLA;
    use crate::registry::BlockRegistry;

    pub static CRAFTING: LazyLock<(CraftingCatalog, Arc<CraftingExecutor>)> = LazyLock::new(|| {
        let recipes = unastar_data::recipes::RecipeRegistry::vanilla().unwrap();
        let catalog = CraftingCatalog::new(&recipes, &VANILLA, &BlockRegistry::vanilla()).unwrap();
        let executor = Arc::new(CraftingExecutor::new(&catalog, &VANILLA).unwrap());
        (catalog, executor)
    });

    pub fn insert_executor(world: &mut bevy_ecs::world::World) {
        world.insert_resource(super::super::types::CraftingExecutorResource(
            CRAFTING.1.clone(),
        ));
    }

    pub fn recipe_id(identifier: &str) -> u32 {
        let packet = CRAFTING.0.packet();
        packet
            .shaped_recipes
            .iter()
            .find(|recipe| &*recipe.recipe_id == identifier)
            .map(|recipe| recipe.net_id.raw_id)
            .or_else(|| {
                packet
                    .shapeless_recipes
                    .iter()
                    .find(|recipe| &*recipe.recipe_id == identifier)
                    .map(|recipe| recipe.net_id.raw_id)
            })
            .expect("vanilla recipe is advertised")
    }
}

use std::time::Instant;

use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use bevy_ecs::system::SystemState;
use tracing::{debug, info, trace};

pub use engine::RequestIdAssignments;

use super::packet_queues::{InventoryAction, InventoryEvents, InventoryPacketQueue};
use crate::ecs::events::ServerEvent;
use crate::entity::components::{
    ArmourInventory, ContainerWindows, GameMode, HeldSlot, Inventory, MainInventory, OffhandSlot,
    PlayerSession, PlayerUi, Position, ScreenKind,
};
use crate::item::ItemStack;
use crate::registry::ItemRegistry;
use jolyne::valentine::{
    ActorUniqueId, BlockPos, ContainerClosePacket, ContainerOpenPacket, EnumsContainerEnumName,
    EnumsInteractPacketPayloadAction, EnumsInventorySourceType,
    EnumsItemUseInventoryTransactionActionType, InteractPacket,
    InventoryTransactionPacketTransaction, McpePacket,
};

/// The player state the inventory packets act on.
#[derive(QueryData)]
#[query_data(mutable)]
pub struct PlayerInventory {
    main: &'static mut MainInventory,
    armor: &'static mut ArmourInventory,
    offhand: &'static mut OffhandSlot,
    ui: &'static mut PlayerUi,
    held_slot: &'static mut HeldSlot,
    windows: &'static mut ContainerWindows,
    request_ids: &'static mut RequestIdAssignments,
    session: &'static PlayerSession,
    mode: &'static GameMode,
    position: &'static Position,
}

/// Once a fatal request has been diagnosed, ignore queued inventory input
/// while the network layer flushes its disconnect and closes the transport.
#[derive(Component)]
struct FatalInventoryRequest;

/// Cached query/resource access for the exclusive inventory commit boundary.
pub type InventorySystemState = SystemState<(
    Res<'static, super::types::ItemRegistryResource>,
    Res<'static, super::types::CraftingExecutorResource>,
    Res<'static, super::types::ServerWorldTemplate>,
    Query<'static, 'static, PlayerInventory>,
    ResMut<'static, InventoryEvents>,
)>;

/// Exclusive PacketApplySet system. A complete request validates before its
/// inventory changes and world throws become visible; no deferred spawn may
/// fail after source removal.
pub fn apply_inventory(world: &mut World, state: &mut InventorySystemState) {
    let actions = std::mem::take(&mut world.resource_mut::<InventoryPacketQueue>().0);
    let _span = tracing::info_span!("apply_inventory", count = actions.len()).entered();
    for (entity, action) in actions {
        if world.get::<FatalInventoryRequest>(entity).is_some() {
            continue;
        }
        let can_drop = world
            .get::<crate::entity::components::Rotation>(entity)
            .is_some()
            && crate::entity::item_drop::drop_thrower_id(world, entity).is_some();
        let position = world.get::<Position>(entity).map(|position| position.0);
        let mut drops = Vec::new();
        let mut fatal = false;
        {
            let (items, crafting, world_template, mut players, mut events) = state.get_mut(world);
            match action {
                InventoryAction::ItemStackRequest(pk) => {
                    if let Ok(mut player) = players.get_mut(entity) {
                        (drops, fatal) = handle_item_stack_request(
                            &pk,
                            &items.0,
                            &crafting.0,
                            &world_template,
                            &mut player,
                            can_drop,
                        );
                    }
                }
                InventoryAction::ContainerClose(pk) => {
                    if let Ok(mut player) = players.get_mut(entity) {
                        handle_container_close(entity, &pk, &mut player);
                    }
                }
                InventoryAction::MobEquipment(pk) => {
                    handle_mob_equipment(entity, &pk, &mut players, &mut events);
                }
                InventoryAction::Interact(pk) => {
                    if let Ok(mut player) = players.get_mut(entity) {
                        handle_interact(entity, &pk, &mut player);
                    }
                }
                InventoryAction::Transaction(pk) => {
                    let mode = players
                        .get(entity)
                        .map_or_else(|_| GameMode::default(), |p| *p.mode);
                    handle_inventory_transaction(
                        entity,
                        &pk,
                        &items,
                        mode,
                        &mut players,
                        &mut events,
                    );
                }
            }
        }
        for stack in drops {
            let spawned = crate::entity::item_drop::spawn_item_drop(
                world,
                position.expect("validated player has a position"),
                stack,
                Some(entity),
            );
            assert!(
                spawned.is_some(),
                "exclusive drop commit preserves validated thrower components"
            );
        }
        if fatal {
            world.entity_mut(entity).insert(FatalInventoryRequest);
        }
    }
}

/// One response packet per incoming packet, in request order. The returned
/// drops belong only to committed frames and are spawned by the exclusive
/// caller before the next input packet can mutate the thrower.
fn handle_item_stack_request(
    pk: &jolyne::valentine::ItemStackRequestPacket,
    items: &ItemRegistry,
    crafting: &crate::crafting::execution::CraftingExecutor,
    world_template: &super::types::ServerWorldTemplate,
    player: &mut PlayerInventoryItem<'_, '_>,
    can_drop: bool,
) -> (Vec<ItemStack>, bool) {
    let creative_item = |net_id: u32| creative_stack(items, world_template, net_id);
    let ctx = engine::Context {
        screen: player.windows.screen(),
        creative: *player.mode == GameMode::Creative,
        can_drop,
        now: Instant::now(),
        creative_item: &creative_item,
        crafting,
    };
    let mut responses = Vec::with_capacity(pk.requests.len());
    let mut drops = Vec::new();
    let mut fatal = false;
    for request in &pk.requests {
        let mut stores = engine::Stores {
            main: &mut player.main.0,
            armor: &mut player.armor,
            offhand: &mut player.offhand.0,
            ui: &mut player.ui.0,
        };
        let outcome = engine::process(request, &mut stores, &mut player.request_ids, &ctx);
        if outcome.fatal_invalid_id {
            fatal = true;
            break;
        }
        send_equipment_updates(
            &outcome.equipment,
            &player.armor,
            &player.offhand.0,
            player.session,
        );
        drops.extend(outcome.drops);
        responses.extend(outcome.response);
    }
    if !responses.is_empty() {
        let _ = player.session.send(McpePacket::from(
            jolyne::valentine::ItemStackResponsePacket { responses },
        ));
    }
    if fatal {
        let _ = player
            .session
            .send(McpePacket::from(invalid_request_warning()));
    }
    (drops, fatal)
}

/// BDS 1.26.51, entity-player-drop-20261026-v4 seq 468/471: positive ID 1
/// receives this terminating warning, then RakNet closes. There is no
/// Minecraft Disconnect packet. The network loop owns flushing the warning
/// and closing the transport when it sees Terminatingconnection.
fn invalid_request_warning() -> jolyne::valentine::PacketViolationWarningPacket {
    use jolyne::valentine::{
        EnumsPacketViolationSeverity, EnumsPacketViolationType, PacketViolationWarningPacket,
    };
    PacketViolationWarningPacket {
        violation_type: EnumsPacketViolationType::Packetmalformed,
        violation_severity: EnumsPacketViolationSeverity::Terminatingconnection,
        violation_packet_id: 147,
        violation_context:
            "expected a valid ItemStackRequestId\nreadNoHeader failed! packetId: 147".into(),
    }
}

/// BDS follows a committed armour change with `InventorySlot` for the armour
/// window (120) and an offhand change with the offhand window's (119)
/// `InventoryContent`, both before the response.
fn send_equipment_updates(
    changes: &engine::EquipmentChanges,
    armor: &ArmourInventory,
    offhand: &ItemStack,
    session: &PlayerSession,
) {
    for &slot in &changes.armor {
        let Some(item) = armor.slot(slot).and_then(wire_item) else {
            continue;
        };
        let _ = session.send(McpePacket::from(jolyne::valentine::InventorySlotPacket {
            container_id: ARMOR_WINDOW,
            slot: slot as u32,
            full_container_name: None,
            storage_item: None,
            item,
        }));
    }
    if changes.offhand
        && let Some(item) = wire_item(offhand)
    {
        let _ = session.send(McpePacket::from(
            jolyne::valentine::InventoryContentPacket {
                container_id: OFFHAND_WINDOW.into(),
                slots: vec![item],
                full_container_name: UNNAMED_CONTAINER,
                storage_item: WireItem::default(),
            },
        ));
    }
}

/// Fixed window IDs of the player's own containers.
pub(crate) const INVENTORY_WINDOW: u8 = 0;
pub(crate) const OFFHAND_WINDOW: u8 = 119;
pub(crate) const ARMOR_WINDOW: u8 = 120;
pub(crate) const UI_WINDOW: u8 = 124;

/// The container name BDS puts in `InventoryContent` for the player's own
/// windows: the zero value (`AnvilInputContainer`, no dynamic ID).
pub(crate) const UNNAMED_CONTAINER: jolyne::valentine::FullContainerName =
    jolyne::valentine::FullContainerName {
        container_name: EnumsContainerEnumName::Anvilinputcontainer,
        dynamic_id: None,
    };

/// The stack a `CraftCreative` action creates: the advertised creative item
/// with its aux value, block and user data.
fn creative_stack(
    items: &ItemRegistry,
    world_template: &super::types::ServerWorldTemplate,
    creative_net_id: u32,
) -> Option<ItemStack> {
    let source = &world_template
        .0
        .creative_content
        .entries
        .iter()
        .find(|entry| entry.creative_net_id.id == creative_net_id)?
        .item_instance;
    let item = items.item_type_by_network_id(source.id)?;
    let stack = ItemStack::new(item, 1)
        .with_runtime_id(source.block_runtime_id)
        .with_damage(i16::try_from(source.auxvalue).ok()?)
        .with_nbt(source.user_data_buffer.clone());
    Some(stack)
}

/// Signed `ContainerType` values (int8 on the wire, `u8` fields in Valentine).
///
/// BDS 1.26.51 opens the player's own inventory with `INVENTORY` (-1) and
/// answers the client's `ContainerClose` with `NONE` (-9); see
/// `crates/valentine/tests/wire_probe_conformance.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i8)]
enum WireContainerType {
    None = -9,
    Inventory = -1,
}

impl WireContainerType {
    const fn byte(self) -> u8 {
        self as i8 as u8
    }
}

/// BDS answers a client close with the ID of the window it had open (the
/// client names window 0 for its own inventory) and type `NONE`.
fn handle_container_close(
    entity: Entity,
    pk: &ContainerClosePacket,
    player: &mut PlayerInventoryItem<'_, '_>,
) {
    debug!(entity = ?entity, container_id = pk.container_id, "ContainerClose");
    let container_id = player.windows.close().unwrap_or(pk.container_id);
    let _ = player.session.send(McpePacket::from(ContainerClosePacket {
        container_id,
        container_type: WireContainerType::None.byte(),
        server_initiated_close: false,
    }));
}

fn handle_mob_equipment(
    entity: Entity,
    pk: &jolyne::valentine::MobEquipmentPacket,
    players: &mut Query<PlayerInventory>,
    events: &mut ResMut<InventoryEvents>,
) {
    if let Ok(mut player) = players.get_mut(entity) {
        let old_slot = player.held_slot.0;
        // Describe the committed (clamped) selection so a repeated
        // out-of-range request is not reported as another change.
        let mut committed = *player.held_slot;
        committed.set(pk.selected_slot);
        let new_slot = committed.0;
        if old_slot != new_slot {
            *player.held_slot = committed;
            trace!(entity = ?entity, slot = new_slot, "Player changed held slot");
            events.0.push(ServerEvent::PlayerHeldSlotChange {
                entity,
                old_slot,
                new_slot,
            });
        }
    }
}

fn handle_interact(entity: Entity, pk: &InteractPacket, player: &mut PlayerInventoryItem<'_, '_>) {
    debug!(entity = ?entity, action = ?pk.action, "Interact packet");

    match pk.action {
        EnumsInteractPacketPayloadAction::Openinventory => {
            // A second ContainerOpen for an open window crashes the client.
            if player.windows.open_id().is_some() {
                debug!(entity = ?entity, "Window already open, skipping");
                return;
            }
            let container_id = player.windows.open(ScreenKind::Inventory);
            // BDS sends the block position of the player's (eye) position.
            let at = player.position.0.floor();
            let _ = player.session.send(McpePacket::from(ContainerOpenPacket {
                container_id,
                container_type: WireContainerType::Inventory.byte(),
                position: BlockPos {
                    x: at.x as i32,
                    y: at.y as i32,
                    z: at.z as i32,
                },
                target_actor_id: ActorUniqueId {
                    actor_unique_id: -1,
                },
            }));
        }
        EnumsInteractPacketPayloadAction::Interactupdate => {}
        _ => {
            debug!(entity = ?entity, action = ?pk.action, "Unhandled Interact action");
        }
    }
}

fn handle_inventory_transaction(
    entity: Entity,
    pk: &jolyne::valentine::InventoryTransactionPacket,
    items: &Res<super::types::ItemRegistryResource>,
    mode: GameMode,
    players: &mut Query<PlayerInventory>,
    events: &mut ResMut<InventoryEvents>,
) {
    let transaction = &pk.transaction;

    info!(transaction = ?transaction, "InventoryTransaction");

    match transaction {
        InventoryTransactionPacketTransaction::NormalTransactionData(normal) => {
            handle_normal_transaction(entity, &normal.actions.actions, items, mode, players);
        }
        InventoryTransactionPacketTransaction::InventoryMismatchData(_) => {
            if let Ok(player) = players.get(entity) {
                send_inventory(&player.main.0, player.session);
            }
        }
        InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(use_item) => {
            match use_item.action_type {
                EnumsItemUseInventoryTransactionActionType::Use => {
                    events.0.push(ServerEvent::PlayerItemUse { entity });
                }
                _ => {
                    debug!(action = ?use_item.action_type, "Unhandled ItemUse action in inventory system");
                }
            }
        }
        InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(_) => {
            debug!("ItemUseOnEntity transaction");
        }
        InventoryTransactionPacketTransaction::ItemReleaseInventoryTransaction(_) => {
            debug!("ItemRelease transaction");
        }
    }
}

fn handle_normal_transaction(
    entity: Entity,
    actions: &[jolyne::valentine::InventoryAction],
    items: &Res<super::types::ItemRegistryResource>,
    mode: GameMode,
    players: &mut Query<PlayerInventory>,
) {
    if let Ok(mut player) = players.get_mut(entity)
        && !apply_normal_actions(&mut player.main.0, actions, &items.0, mode)
    {
        send_inventory(&player.main.0, player.session);
    }
}

type WireItem = jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData;

/// A stack as BDS sends it in inventory packets, stack network ID included.
pub(crate) fn wire_item(stack: &ItemStack) -> Option<WireItem> {
    if stack.is_empty() {
        return Some(WireItem::default());
    }
    Some(WireItem {
        id: i16::try_from(stack.item()?.network_id()).ok()?,
        stacksize: u16::from(stack.count),
        auxvalue: u32::try_from(stack.damage).ok()?,
        net_id_variant: Some(stack.net_id),
        block_runtime_id: u32::try_from(stack.runtime_id).ok()?,
        user_data_buffer: stack.user_data(),
    })
}

fn validated_item(wire: &WireItem, items: &ItemRegistry) -> Option<ItemStack> {
    if wire.id == 0 {
        return (wire.stacksize == 0
            && wire.auxvalue == 0
            && wire.block_runtime_id == 0
            && wire.user_data_buffer.is_empty())
        .then(ItemStack::empty);
    }
    let item = items.item_type_by_network_id(i32::from(wire.id))?;
    if wire.stacksize == 0 || wire.stacksize > u16::from(item.max_stack_size()) {
        return None;
    }
    let stack = ItemStack::new(item, u8::try_from(wire.stacksize).ok()?)
        .with_runtime_id(i32::try_from(wire.block_runtime_id).ok()?)
        .with_damage(i16::try_from(wire.auxvalue).ok()?)
        .with_nbt(wire.user_data_buffer.clone());
    Some(stack)
}

/// Validate against the original inventory, then commit all slots together.
/// Duplicate slots and non-main containers are unsupported, not implicit grants.
fn apply_normal_actions(
    inv: &mut Inventory,
    actions: &[jolyne::valentine::InventoryAction],
    items: &ItemRegistry,
    mode: GameMode,
) -> bool {
    let mut changes: Vec<(usize, ItemStack)> = Vec::with_capacity(actions.len().min(inv.size()));
    if actions.len() > inv.size() {
        return false;
    }
    for action in actions {
        if action.source.source_type != EnumsInventorySourceType::Containerinventory
            || action.source.container_id != Some(0)
            || action.source.bit_flags.is_some()
        {
            return false;
        }
        let Ok(slot) = usize::try_from(action.slot) else {
            return false;
        };
        let Some(actual) = inv.item(slot) else {
            return false;
        };
        if changes.iter().any(|(previous, _)| *previous == slot) {
            return false;
        }
        let Some(expected) = wire_item(actual) else {
            return false;
        };
        // Stack prediction IDs are transport bookkeeping, not item identity;
        // user data compares decoded (an omitted buffer equals the default).
        let expected = WireItem {
            net_id_variant: action.from_item.net_id_variant,
            user_data_buffer: action.from_item.user_data_buffer.clone(),
            ..expected
        };
        let Some(from) = validated_item(&action.from_item, items) else {
            return false;
        };
        if expected != action.from_item || from.nbt != actual.nbt {
            return false;
        }
        let Some(item) = validated_item(&action.to_item, items) else {
            return false;
        };
        changes.push((slot, item));
    }
    if mode != GameMode::Creative {
        // Compare every affected item identity (including damage and NBT).
        // The bounded main inventory keeps this allocation-free tally small.
        for identity in changes
            .iter()
            .flat_map(|(slot, item)| [inv.item(*slot).unwrap(), item])
        {
            if identity.is_empty() {
                continue;
            }
            let before: u32 = changes
                .iter()
                .map(|(slot, _)| inv.item(*slot).unwrap())
                .filter(|item| item.comparable(identity) && item.runtime_id == identity.runtime_id)
                .map(|item| u32::from(item.count))
                .sum();
            let after: u32 = changes
                .iter()
                .map(|(_, item)| item)
                .filter(|item| item.comparable(identity) && item.runtime_id == identity.runtime_id)
                .map(|item| u32::from(item.count))
                .sum();
            if before != after {
                return false;
            }
        }
    }
    for (slot, item) in changes {
        inv.set_item(slot, item).expect("validated inventory slot");
    }
    true
}

fn send_inventory(inv: &Inventory, session: &PlayerSession) {
    let Some(slots) = (0..inv.size())
        .map(|slot| wire_item(inv.item(slot).unwrap()))
        .collect::<Option<Vec<_>>>()
    else {
        tracing::warn!("Cannot resync inventory containing an unrepresentable server item");
        return;
    };
    let _ = session.send(McpePacket::from(
        jolyne::valentine::InventoryContentPacket {
            container_id: INVENTORY_WINDOW.into(),
            slots,
            full_container_name: UNNAMED_CONTAINER,
            storage_item: WireItem::default(),
        },
    ));
}

/// Correct client prediction after a rejected gameplay inventory mutation.
pub(super) fn resync_inventory(world: &World, entity: Entity) {
    if let (Some(inv), Some(session)) = (
        world.get::<MainInventory>(entity),
        world.get::<PlayerSession>(entity),
    ) {
        send_inventory(&inv.0, session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::components::Inventory;
    use jolyne::valentine::EnumsItemStackNetResult;

    fn registry() -> ItemRegistry {
        crate::registry::item::test_support::registry(&[("minecraft:honey_bottle", 77, 16)])
    }

    fn descriptor(
        count: u16,
    ) -> jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData {
        jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData {
            id: if count == 0 { 0 } else { 77 },
            stacksize: count,
            ..Default::default()
        }
    }

    fn action(slot: u32, from: u16, to: u16) -> jolyne::valentine::InventoryAction {
        jolyne::valentine::InventoryAction {
            source: jolyne::valentine::InventorySource {
                source_type: EnumsInventorySourceType::Containerinventory,
                container_id: Some(0),
                ..Default::default()
            },
            slot,
            from_item: descriptor(from),
            to_item: descriptor(to),
        }
    }

    #[test]
    fn forged_stacks_are_rejected_without_truncation() {
        let items = registry();
        for count in [17, 256, 272, u16::MAX] {
            let mut inv = Inventory::new(2);
            assert!(!apply_normal_actions(
                &mut inv,
                &[action(0, 0, count)],
                &items,
                GameMode::Creative
            ));
            assert!(inv.item(0).unwrap().is_empty());
        }
        let mut inv = Inventory::new(2);
        let mut forged = action(0, 0, 1);
        forged.to_item.id = -12;
        assert!(!apply_normal_actions(
            &mut inv,
            &[forged],
            &items,
            GameMode::Creative
        ));
    }

    #[test]
    fn stale_from_item_rejects_entire_transaction() {
        let items = registry();
        let mut inv = Inventory::new(2);
        inv.set_item(0, items.stack("minecraft:honey_bottle", 8).unwrap())
            .unwrap();
        let actions = [action(0, 8, 0), action(1, 1, 8)];
        assert!(!apply_normal_actions(
            &mut inv,
            &actions,
            &items,
            GameMode::Survival
        ));
        assert_eq!(inv.item(0).unwrap().count, 8);
        assert!(inv.item(1).unwrap().is_empty());
    }

    #[test]
    fn survival_moves_conserve_items_and_clear_source() {
        let items = registry();
        let mut inv = Inventory::new(2);
        inv.set_item(0, items.stack("minecraft:honey_bottle", 8).unwrap())
            .unwrap();
        assert!(!apply_normal_actions(
            &mut inv,
            &[action(1, 0, 8)],
            &items,
            GameMode::Survival
        ));
        assert!(apply_normal_actions(
            &mut inv,
            &[action(0, 8, 0), action(1, 0, 8)],
            &items,
            GameMode::Survival
        ));
        assert!(inv.item(0).unwrap().is_empty());
        assert_eq!(inv.item(1).unwrap().count, 8);
        assert_eq!(inv.item(1).unwrap().max_stack_size(), 16);
    }

    #[test]
    fn unsupported_sources_containers_and_repeated_slots_are_atomic() {
        let items = registry();
        for bad in [
            {
                let mut a = action(1, 0, 1);
                a.source.container_id = Some(119);
                a
            },
            {
                let mut a = action(1, 0, 1);
                a.source.source_type = EnumsInventorySourceType::Worldinteraction;
                a
            },
            action(99, 0, 1),
            action(0, 0, 1),
        ] {
            let mut inv = Inventory::new(2);
            assert!(!apply_normal_actions(
                &mut inv,
                &[action(0, 0, 1), bad],
                &items,
                GameMode::Creative
            ));
            assert!(inv.item(0).unwrap().is_empty());
        }
    }

    #[test]
    fn creative_creation_requires_server_creative_mode() {
        let items = registry();
        for mode in [GameMode::Survival, GameMode::Adventure, GameMode::Spectator] {
            let mut inv = Inventory::new(1);
            assert!(!apply_normal_actions(
                &mut inv,
                &[action(0, 0, 16)],
                &items,
                mode
            ));
        }
    }

    #[test]
    fn survival_cannot_change_item_metadata_or_destroy_items() {
        let items = registry();
        let mut inv = Inventory::new(1);
        inv.set_item(0, items.stack("minecraft:honey_bottle", 8).unwrap())
            .unwrap();
        for forged in [
            {
                let mut a = action(0, 8, 8);
                a.to_item.auxvalue = 1;
                a
            },
            {
                let mut a = action(0, 8, 8);
                a.to_item.block_runtime_id = 1;
                a
            },
            {
                let mut a = action(0, 8, 8);
                a.to_item.user_data_buffer = vec![1].into();
                a
            },
            action(0, 8, 0),
        ] {
            assert!(!apply_normal_actions(
                &mut inv,
                &[forged],
                &items,
                GameMode::Survival
            ));
            assert_eq!(inv.item(0).unwrap().count, 8);
        }
    }

    /// The components `apply_inventory` needs, inventory screen open.
    fn player(mode: GameMode, tx: tokio::sync::mpsc::Sender<McpePacket>) -> impl Bundle {
        let mut windows = ContainerWindows::default();
        windows.open(ScreenKind::Inventory);
        (
            mode,
            MainInventory::default(),
            ArmourInventory::default(),
            OffhandSlot::default(),
            PlayerUi::default(),
            HeldSlot::default(),
            windows,
            RequestIdAssignments::default(),
            PlayerSession::new(1, "test".into(), None, None, tx),
            Position::new(0.5, 65.62, 0.5),
            crate::entity::components::Rotation::default(),
            crate::entity::components::RuntimeId(1),
        )
    }

    fn run_creative_packet(
        mode: GameMode,
        amount: u8,
    ) -> (World, Entity, tokio::sync::mpsc::Receiver<McpePacket>) {
        use super::super::types::{ItemRegistryResource, ServerWorldTemplate};
        use jolyne::valentine::*;
        use std::sync::Arc;
        let mut world = World::new();
        world.insert_resource(ItemRegistryResource(Arc::new(registry())));
        test_support::insert_executor(&mut world);
        let mut template = jolyne::WorldTemplate::default();
        let mut content = CreativeContentPacket::default();
        content.entries.push(CreativeItemEntryPayload {
            creative_net_id: TypedServerNetIdstructCreativeItemNetIdTag { id: 1 },
            item_instance: CerealizerNetworkItemInstanceDescriptorSerializedData {
                id: 77,
                stacksize: 1,
                auxvalue: 7,
                block_runtime_id: 33,
                user_data_buffer: vec![1, 2].into(),
            },
            ..Default::default()
        });
        template.creative_content = Arc::new(content);
        world.insert_resource(ServerWorldTemplate(Arc::new(template)));
        world.insert_resource(InventoryEvents::default());
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let entity = world.spawn(player(mode, tx)).id();
        let actions = vec![
            ItemStackRequestPacketDataRequestDataActionsItem::CraftCreativeActionData(
                ItemStackRequestCerealCraftCreativeActionData {
                    creative_item_net_id: 1,
                    numberofrequestedcrafts: 1,
                    ..Default::default()
                },
            ),
            ItemStackRequestPacketDataRequestDataActionsItem::PlaceActionData(
                ItemStackRequestCerealPlaceActionData {
                    amount,
                    // The client names created output by its request ID.
                    source: ItemStackRequestCerealSlotInfoData {
                        fullcontainername: FullContainerName {
                            container_name: EnumsContainerEnumName::Createdoutputcontainer,
                            dynamic_id: None,
                        },
                        slot: 50,
                        net_id_variant: -1,
                    },
                    destination: ItemStackRequestCerealSlotInfoData {
                        fullcontainername: FullContainerName {
                            container_name:
                                EnumsContainerEnumName::Combinedhotbarandinventorycontainer,
                            dynamic_id: None,
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ),
        ];
        world.insert_resource(InventoryPacketQueue(vec![(
            entity,
            super::InventoryAction::ItemStackRequest(ItemStackRequestPacket {
                requests: vec![ItemStackRequestPacketDataRequestData {
                    client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 {
                        id: -1,
                    },
                    actions,
                    ..Default::default()
                }],
            }),
        )]));
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_inventory);
        schedule.run(&mut world);
        (world, entity, rx)
    }

    #[test]
    fn creative_acquisition_preserves_advertised_metadata_for_later_moves() {
        let (mut world, entity, mut rx) = run_creative_packet(GameMode::Creative, 16);
        let jolyne::valentine::McpePacketData::ItemStackResponsePacket(response) =
            rx.try_recv().unwrap().data
        else {
            panic!("stack response")
        };
        assert_eq!(
            response.responses[0].result,
            EnumsItemStackNetResult::Success
        );
        let stack = world
            .get::<MainInventory>(entity)
            .unwrap()
            .0
            .item(0)
            .unwrap();
        assert_eq!(
            (stack.runtime_id, stack.damage, stack.nbt.as_deref()),
            (33, 7, Some([1, 2].as_slice()))
        );
        let mut from = descriptor(16);
        from.block_runtime_id = 33;
        from.auxvalue = 7;
        from.user_data_buffer = vec![1, 2].into();
        let mut remove = action(0, 16, 0);
        remove.from_item = from.clone();
        let mut insert = action(1, 0, 16);
        insert.to_item = from;
        let items = registry();
        let mut inventory = world.get_mut::<MainInventory>(entity).unwrap();
        assert!(apply_normal_actions(
            &mut inventory.0,
            &[remove, insert],
            &items,
            GameMode::Survival
        ));
        assert!(inventory.0.item(0).unwrap().is_empty());
        assert_eq!(inventory.0.item(1).unwrap().damage, 7);
    }

    /// BDS answers a failed request with its status only: no containers and
    /// no `InventoryContent` resync.
    #[test]
    fn rejected_creative_requests_change_nothing_and_send_only_the_status() {
        for (mode, amount, status) in [
            (
                GameMode::Creative,
                0,
                EnumsItemStackNetResult::Invalidadjustedamount,
            ),
            (GameMode::Survival, 16, EnumsItemStackNetResult::Error),
        ] {
            let (world, entity, mut rx) = run_creative_packet(mode, amount);
            assert!(
                world
                    .get::<MainInventory>(entity)
                    .unwrap()
                    .0
                    .slots()
                    .iter()
                    .all(ItemStack::is_empty)
            );
            let jolyne::valentine::McpePacketData::ItemStackResponsePacket(response) =
                rx.try_recv().unwrap().data
            else {
                panic!("stack response")
            };
            assert_eq!(response.responses[0].result, status);
            assert!(response.responses[0].containers.is_none());
            assert!(rx.try_recv().is_err());
        }
    }

    /// Same exchanges as the BDS 1.26.51 captures (`wire_probe_conformance.rs`
    /// and scenario `inventory-moves`): the inventory opens as `INVENTORY`
    /// (-1) under window 2 at the player's block position, the close is
    /// answered with window 2 and `NONE` (-9), and the next open uses 3.
    #[test]
    fn inventory_windows_count_like_bds() {
        use super::super::types::{ItemRegistryResource, ServerWorldTemplate};
        use jolyne::valentine::*;
        use std::sync::Arc;
        let mut world = World::new();
        world.insert_resource(ItemRegistryResource(Arc::new(registry())));
        test_support::insert_executor(&mut world);
        world.insert_resource(ServerWorldTemplate(Arc::new(
            jolyne::WorldTemplate::default(),
        )));
        world.insert_resource(InventoryEvents::default());
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let entity = world.spawn(player(GameMode::Creative, tx)).id();
        *world.get_mut::<ContainerWindows>(entity).unwrap() = ContainerWindows::default();
        let open = || {
            super::InventoryAction::Interact(InteractPacket {
                action: EnumsInteractPacketPayloadAction::Openinventory,
                ..Default::default()
            })
        };
        let close = || {
            super::InventoryAction::ContainerClose(ContainerClosePacket {
                container_id: 0,
                container_type: 0xff,
                server_initiated_close: false,
            })
        };
        world.insert_resource(InventoryPacketQueue(vec![
            (entity, open()),
            (entity, open()),
            (entity, close()),
            (entity, open()),
        ]));
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_inventory);
        schedule.run(&mut world);

        let McpePacketData::ContainerOpenPacket(first) = rx.try_recv().unwrap().data else {
            panic!("container open");
        };
        assert_eq!((first.container_id, first.container_type as i8), (2, -1));
        assert_eq!(
            (first.position.x, first.position.y, first.position.z),
            (0, 65, 0)
        );
        assert_eq!(first.target_actor_id.actor_unique_id, -1);
        let McpePacketData::ContainerClosePacket(close) = rx.try_recv().unwrap().data else {
            panic!("the second open of an open window is ignored; container close");
        };
        assert_eq!((close.container_id, close.container_type as i8), (2, -9));
        assert!(!close.server_initiated_close);
        let McpePacketData::ContainerOpenPacket(second) = rx.try_recv().unwrap().data else {
            panic!("container open");
        };
        assert_eq!(second.container_id, 3);
        assert_eq!(
            world.get::<ContainerWindows>(entity).unwrap().screen(),
            ScreenKind::Inventory
        );
    }

    #[test]
    fn held_slot_events_describe_committed_selection_and_do_not_repeat_after_clamping() {
        #[derive(Resource)]
        struct Selection {
            entity: Entity,
            packet: jolyne::valentine::MobEquipmentPacket,
        }
        fn apply_selection(
            mut players: Query<PlayerInventory>,
            mut events: ResMut<InventoryEvents>,
            selection: Res<Selection>,
        ) {
            handle_mob_equipment(selection.entity, &selection.packet, &mut players, &mut events);
        }
        let mut world = World::new();
        world.insert_resource(InventoryEvents::default());
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let entity = world.spawn(player(GameMode::Survival, tx)).id();
        world.get_mut::<HeldSlot>(entity).unwrap().0 = 0;
        world.insert_resource(Selection {
            entity,
            packet: jolyne::valentine::MobEquipmentPacket { selected_slot: 9, ..Default::default() },
        });
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_selection);
        schedule.run(&mut world);
        let selected = world.get::<HeldSlot>(entity).unwrap().0;
        assert_eq!(selected, 8);
        let [ServerEvent::PlayerHeldSlotChange { entity: changed, old_slot, new_slot }] =
            world.resource::<InventoryEvents>().0.as_slice() else {
                panic!("one committed selection change");
            };
        assert_eq!((*changed, *old_slot, *new_slot), (entity, 0, selected));
        world.resource_mut::<InventoryEvents>().0.clear();
        schedule.run(&mut world);
        assert!(world.resource::<InventoryEvents>().0.is_empty(),
            "repeating a selection that normalizes to the current slot is not another change");
    }
}

#[cfg(test)]
mod drop_tests {
    use super::InventoryAction;
    use super::*;
    use crate::entity::components::{ItemEntityStack, Rotation, RuntimeId};
    use jolyne::valentine::*;
    use std::sync::Arc;

    fn run_drop_frame(
        fail_later: bool,
        thrower_ready: bool,
    ) -> (World, Entity, tokio::sync::mpsc::Receiver<McpePacket>) {
        let items = Arc::new(crate::item::test_support::VANILLA.clone());
        let mut world = World::new();
        world.insert_resource(super::super::types::ItemRegistryResource(items.clone()));
        test_support::insert_executor(&mut world);
        world.insert_resource(super::super::types::ServerWorldTemplate(Arc::new(
            jolyne::WorldTemplate::default(),
        )));
        world.insert_resource(InventoryEvents::default());
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        let mut windows = ContainerWindows::default();
        windows.open(ScreenKind::Inventory);
        let mut main = MainInventory::default();
        main.0
            .set_item(0, items.stack("minecraft:stone", 8).unwrap())
            .unwrap();
        let id = main.0.item(0).unwrap().net_id;
        let entity = world
            .spawn((
                GameMode::Survival,
                main,
                ArmourInventory::default(),
                OffhandSlot::default(),
                PlayerUi::default(),
                HeldSlot::default(),
                windows,
                RequestIdAssignments::default(),
                PlayerSession::new(1, "drop-test".into(), None, None, tx),
                Position::new(0.5, 65.62, 0.5),
                RuntimeId(1),
            ))
            .id();
        if thrower_ready {
            world.entity_mut(entity).insert(Rotation::default());
        }
        let source = ItemStackRequestCerealSlotInfoData {
            fullcontainername: FullContainerName {
                container_name: EnumsContainerEnumName::Combinedhotbarandinventorycontainer,
                dynamic_id: None,
            },
            slot: 0,
            net_id_variant: id,
        };
        let mut actions = vec![
            ItemStackRequestPacketDataRequestDataActionsItem::DropActionData(
                ItemStackRequestCerealDropActionData {
                    amount: 3,
                    source: source.clone(),
                    ..Default::default()
                },
            ),
        ];
        if fail_later {
            actions.push(
                ItemStackRequestPacketDataRequestDataActionsItem::TakeActionData(
                    ItemStackRequestCerealTakeActionData {
                        amount: 0,
                        source: ItemStackRequestCerealSlotInfoData {
                            net_id_variant: -1,
                            ..source.clone()
                        },
                        destination: ItemStackRequestCerealSlotInfoData {
                            slot: 1,
                            net_id_variant: 0,
                            ..source
                        },
                        ..Default::default()
                    },
                ),
            );
        }
        world.insert_resource(InventoryPacketQueue(vec![(
            entity,
            InventoryAction::ItemStackRequest(ItemStackRequestPacket {
                requests: vec![ItemStackRequestPacketDataRequestData {
                    client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 {
                        id: -1,
                    },
                    actions,
                    ..Default::default()
                }],
            }),
        )]));
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_inventory);
        schedule.run(&mut world);
        (world, entity, rx)
    }

    #[test]
    fn accepted_drop_spawns_existing_item_drop_once_without_double_removal() {
        let (mut world, player, mut rx) = run_drop_frame(false, true);
        assert_eq!(
            world
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            5
        );
        let drops: Vec<_> = world
            .query::<(&ItemEntityStack, &Position)>()
            .iter(&world)
            .collect();
        assert_eq!(drops.len(), 1);
        assert_eq!(drops[0].0.0.count, 3);
        assert_eq!(drops[0].1.0, glam::DVec3::new(0.5, 65.12, 0.5));
        let McpePacketData::ItemStackResponsePacket(packet) = rx.try_recv().unwrap().data else {
            panic!("response")
        };
        assert_eq!(packet.responses[0].result, EnumsItemStackNetResult::Success);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn failed_drop_frame_has_no_entities_rng_or_actor_id_side_effects() {
        let (mut world, player, mut rx) = run_drop_frame(true, true);
        assert_eq!(
            world
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            8
        );
        assert_eq!(world.query::<&ItemEntityStack>().iter(&world).count(), 0);
        assert!(!world.contains_resource::<crate::entity::EntityRandom>());
        assert!(!world.contains_resource::<super::super::types::ItemEntityIdCounter>());
        let McpePacketData::ItemStackResponsePacket(packet) = rx.try_recv().unwrap().data else {
            panic!("response")
        };
        assert_eq!(
            packet.responses[0].result,
            EnumsItemStackNetResult::Invalidadjustedamount
        );
        assert!(packet.responses[0].containers.is_none());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn missing_thrower_rotation_rejects_before_removing_source() {
        let (mut world, player, mut rx) = run_drop_frame(false, false);
        assert_eq!(
            world
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            8
        );
        assert_eq!(world.query::<&ItemEntityStack>().iter(&world).count(), 0);
        let McpePacketData::ItemStackResponsePacket(packet) = rx.try_recv().unwrap().data else {
            panic!("response")
        };
        assert_eq!(
            packet.responses[0].result,
            EnumsItemStackNetResult::Cannotdropitem
        );
    }

    #[test]
    fn fatal_request_ids_warn_and_stop_queued_inventory_actions_until_transport_closes() {
        for id in [0, 1, 2, -2] {
            let (mut world, player, mut rx) = run_drop_frame(false, true);
            rx.try_recv().unwrap();
            world.insert_resource(InventoryPacketQueue(vec![
                (
                    player,
                    InventoryAction::ItemStackRequest(ItemStackRequestPacket {
                        requests: vec![ItemStackRequestPacketDataRequestData {
                            client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 {
                                id,
                            },
                            ..Default::default()
                        }],
                    }),
                ),
                (
                    player,
                    InventoryAction::MobEquipment(MobEquipmentPacket {
                        selected_slot: 7,
                        ..Default::default()
                    }),
                ),
            ]));
            let mut schedule = Schedule::default();
            schedule.add_systems(apply_inventory);
            schedule.run(&mut world);
            assert!(matches!(
                rx.try_recv().unwrap().data,
                McpePacketData::PacketViolationWarningPacket(_)
            ));
            assert!(rx.try_recv().is_err());
            assert!(world.get::<FatalInventoryRequest>(player).is_some());
            assert_eq!(world.get::<HeldSlot>(player).unwrap().0, 0);
            assert_eq!(
                world
                    .get::<MainInventory>(player)
                    .unwrap()
                    .0
                    .item(0)
                    .unwrap()
                    .count,
                5
            );
            assert_eq!(world.query::<&ItemEntityStack>().iter(&world).count(), 1);
        }
    }

    #[test]
    fn terminating_warning_matches_retained_bds_positive_id_capture() {
        // Two original wire rows from the native v4 session; +1 was measured.
        // The negative-odd parser contract is covered separately, not claimed
        // as a measured negative-even control by this fixture.
        let packets: Vec<_> = include_str!("testdata/bds-invalid-request-id.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .map(|entry| capture::decode(entry["hex"].as_str().unwrap()))
            .collect();
        let McpePacketData::ItemStackRequestPacket(request) = &packets[0] else {
            panic!("captured request")
        };
        assert_eq!(request.requests[0].client_request_id.id, 1);
        let McpePacketData::PacketViolationWarningPacket(warning) = &packets[1] else {
            panic!("captured terminating warning")
        };
        assert_eq!(&invalid_request_warning(), warning);
    }
}

#[cfg(test)]
mod real_player_drop_test {
    use super::InventoryAction;
    use super::*;
    use crate::entity::components::{ItemEntityStack, ItemOwner, RuntimeEntityId, RuntimeId};
    use jolyne::valentine::*;

    #[test]
    fn real_game_server_player_drop_preserves_owner_and_consumes_once() {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let _guard = runtime.enter();
                let mut world_config = crate::world::WorldConfig::void_spawn_platform(1);
                world_config.generation_threads = 1;
                let mut server = crate::GameServer::with_config(crate::ServerConfig {
                    default_gamemode: GameMode::Survival,
                    world: world_config,
                    ..Default::default()
                });
                let (tx, mut rx) = tokio::sync::mpsc::channel(64);
                let player = server
                    .spawn_player(crate::PlayerSpawnData {
                        session_id: 11,
                        display_name: "real-drop".into(),
                        xuid: None,
                        uuid: Some(uuid::Uuid::from_u128(11).to_string()),
                        runtime_id: 41,
                        position: glam::DVec3::new(0.5, 17.0, 0.5),
                        saved_data: None,
                        outbound_tx: tx,
                        chunk_radius: 0,
                    })
                    .unwrap();
                while rx.try_recv().is_ok() {}
                assert_eq!(
                    server.ecs.world().get::<RuntimeEntityId>(player).unwrap().0,
                    41
                );
                assert!(server.ecs.world().get::<RuntimeId>(player).is_none());
                let stack = {
                    let world = server.ecs.world();
                    world
                        .resource::<super::super::types::ItemRegistryResource>()
                        .0
                        .stack_with_block(
                            "minecraft:stone",
                            8,
                            &world
                                .resource::<super::super::types::BlockRegistryResource>()
                                .0,
                        )
                        .unwrap()
                };
                let world = server.ecs.world_mut();
                world
                    .get_mut::<MainInventory>(player)
                    .unwrap()
                    .0
                    .set_item(0, stack)
                    .unwrap();
                let source_id = world
                    .get::<MainInventory>(player)
                    .unwrap()
                    .0
                    .item(0)
                    .unwrap()
                    .net_id;
                world.resource_mut::<InventoryPacketQueue>().0.push((
                    player,
                    InventoryAction::ItemStackRequest(ItemStackRequestPacket {
                        requests: vec![ItemStackRequestPacketDataRequestData {
                            client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 {
                                id: -1,
                            },
                            actions: vec![
                                ItemStackRequestPacketDataRequestDataActionsItem::DropActionData(
                                    ItemStackRequestCerealDropActionData {
                                        amount: 3,
                                        source: ItemStackRequestCerealSlotInfoData {
                                            fullcontainername: FullContainerName {
                                                container_name:
                                                    EnumsContainerEnumName::Hotbarcontainer,
                                                dynamic_id: None,
                                            },
                                            slot: 0,
                                            net_id_variant: source_id,
                                        },
                                        ..Default::default()
                                    },
                                ),
                            ],
                            ..Default::default()
                        }],
                    }),
                ));
                server.tick();
                let world = server.ecs.world_mut();
                let drops: Vec<_> = world
                    .query::<(&ItemEntityStack, &ItemOwner)>()
                    .iter(world)
                    .map(|(stack, owner)| (stack.0.count, owner.0))
                    .collect();
                assert_eq!(drops, [(3, 41)]);
                assert_eq!(
                    world
                        .get::<MainInventory>(player)
                        .unwrap()
                        .0
                        .item(0)
                        .unwrap()
                        .count,
                    5
                );
                let responses: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
                    .filter_map(|packet| {
                        if let McpePacketData::ItemStackResponsePacket(packet) = packet.data {
                            Some(packet)
                        } else {
                            None
                        }
                    })
                    .collect();
                assert_eq!(responses.len(), 1);
                assert_eq!(
                    responses[0].responses[0].result,
                    EnumsItemStackNetResult::Success
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

#[cfg(test)]
mod grid_transaction_tests {
    use super::*;
    use crate::entity::components::{ItemEntityStack, Rotation, RuntimeId};
    use crate::item::test_support::{VANILLA, stack};
    use jolyne::valentine::*;
    use std::sync::Arc;

    fn slot(name: EnumsContainerEnumName, slot: u8, id: i32) -> ItemStackRequestCerealSlotInfoData {
        ItemStackRequestCerealSlotInfoData {
            fullcontainername: FullContainerName { container_name: name, dynamic_id: None },
            slot,
            net_id_variant: id,
        }
    }

    fn take(amount: u8, source: ItemStackRequestCerealSlotInfoData, destination: ItemStackRequestCerealSlotInfoData)
        -> ItemStackRequestPacketDataRequestDataActionsItem
    {
        ItemStackRequestPacketDataRequestDataActionsItem::TakeActionData(ItemStackRequestCerealTakeActionData {
            amount, source, destination, ..Default::default()
        })
    }

    fn run(late_failure: bool) -> (World, Entity, tokio::sync::mpsc::Receiver<McpePacket>) {
        use EnumsContainerEnumName as Name;
        use ItemStackRequestPacketDataRequestDataActionsItem as Action;
        let mut world = World::new();
        world.insert_resource(super::super::types::ItemRegistryResource(Arc::new(VANILLA.clone())));
        test_support::insert_executor(&mut world);
        world.insert_resource(super::super::types::ServerWorldTemplate(Arc::new(jolyne::WorldTemplate::default())));
        world.insert_resource(InventoryEvents::default());
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        let mut windows = ContainerWindows::default();
        windows.open(ScreenKind::Inventory);
        let mut main = MainInventory::default();
        main.0.set_item(0, stack("minecraft:stone", 5)).unwrap();
        let drop_id = main.0.item(0).unwrap().net_id;
        let mut ui = PlayerUi::default();
        ui.0.set_item(28, stack("minecraft:oak_log", 2)).unwrap();
        let input_id = ui.0.item(28).unwrap().net_id;
        let player = world.spawn((
            GameMode::Survival, main, ArmourInventory::default(), OffhandSlot::default(), ui,
            HeldSlot::default(), windows, RequestIdAssignments::default(),
            PlayerSession::new(1, "grid-test".into(), None, None, tx),
            Position::new(0.5, 65.62, 0.5), RuntimeId(1), Rotation::default(),
        )).id();
        let mut actions = Vec::new();
        if late_failure {
            actions.push(Action::DropActionData(ItemStackRequestCerealDropActionData {
                amount: 2,
                source: slot(Name::Combinedhotbarandinventorycontainer, 0, drop_id),
                ..Default::default()
            }));
        }
        actions.extend([
            Action::CraftRecipeActionData(ItemStackRequestCerealCraftRecipeActionData {
                recipe_net_id: TypedServerNetIdstructRecipeNetIdTag {
                    raw_id: test_support::recipe_id("minecraft:oak_planks"),
                },
                numberofrequestedcrafts: 1,
                ..Default::default()
            }),
            Action::ConsumeActionData(ItemStackRequestCerealConsumeActionData {
                amount: 1,
                source: slot(Name::Craftinginputcontainer, 28, input_id),
                ..Default::default()
            }),
            take(4, slot(Name::Createdoutputcontainer, 50, -1),
                slot(Name::Combinedhotbarandinventorycontainer, 1, 0)),
        ]);
        if late_failure {
            actions.push(take(0, slot(Name::Combinedhotbarandinventorycontainer, 1, -1),
                slot(Name::Combinedhotbarandinventorycontainer, 2, 0)));
        }
        let mut requests = vec![ItemStackRequestPacketDataRequestData {
            client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id: -1 },
            actions,
            ..Default::default()
        }];
        if !late_failure {
            requests.push(ItemStackRequestPacketDataRequestData {
                client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id: -3 },
                actions: vec![take(4, slot(Name::Combinedhotbarandinventorycontainer, 1, -1),
                    slot(Name::Combinedhotbarandinventorycontainer, 2, 0))],
                ..Default::default()
            });
        }
        world.insert_resource(InventoryPacketQueue(vec![(
            player, super::InventoryAction::ItemStackRequest(ItemStackRequestPacket { requests }),
        )]));
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_inventory);
        schedule.run(&mut world);
        (world, player, rx)
    }

    #[test]
    fn grid_craft_and_same_packet_followup_share_one_ordered_response() {
        let (world, player, mut rx) = run(false);
        let main = &world.get::<MainInventory>(player).unwrap().0;
        assert!(main.item(1).unwrap().is_empty());
        assert_eq!((main.item(2).unwrap().item_id(), main.item(2).unwrap().count),
            ("minecraft:oak_planks", 4));
        assert_eq!(world.get::<PlayerUi>(player).unwrap().0.item(28).unwrap().count, 1);
        let McpePacketData::ItemStackResponsePacket(packet) = rx.try_recv().unwrap().data else {
            panic!("one grouped response")
        };
        assert_eq!(packet.responses.len(), 2);
        assert_eq!(packet.responses.iter().map(|response| response.client_request_id.id).collect::<Vec<_>>(), [-1, -3]);
        assert!(packet.responses.iter().all(|response| response.result == EnumsItemStackNetResult::Success));
        let first_id = packet.responses[0].containers.as_ref().unwrap()[1].slots[0].item_stack_net_id.as_ref().unwrap().id;
        let second_id = packet.responses[1].containers.as_ref().unwrap()[0].slots[1].item_stack_net_id.as_ref().unwrap().id;
        assert_eq!(first_id, second_id);
        assert_eq!(second_id, main.item(2).unwrap().net_id);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn failed_grid_frame_rolls_back_input_output_and_all_world_drop_side_effects() {
        let (mut world, player, mut rx) = run(true);
        let main = &world.get::<MainInventory>(player).unwrap().0;
        assert_eq!(main.item(0).unwrap().count, 5);
        assert!(main.item(1).unwrap().is_empty());
        assert!(main.item(2).unwrap().is_empty());
        assert_eq!(world.get::<PlayerUi>(player).unwrap().0.item(28).unwrap().count, 2);
        assert_eq!(world.query::<&ItemEntityStack>().iter(&world).count(), 0);
        assert!(!world.contains_resource::<crate::entity::EntityRandom>());
        assert!(!world.contains_resource::<super::super::types::ItemEntityIdCounter>());
        let McpePacketData::ItemStackResponsePacket(packet) = rx.try_recv().unwrap().data else {
            panic!("one rejected response")
        };
        assert_eq!(packet.responses.len(), 1);
        assert_eq!(packet.responses[0].result, EnumsItemStackNetResult::Invalidadjustedamount);
        assert!(packet.responses[0].containers.is_none());
        assert!(rx.try_recv().is_err());
    }
}
