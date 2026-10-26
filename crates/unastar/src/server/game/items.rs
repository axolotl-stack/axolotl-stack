//! Item entity gameplay: wire descriptors and player pickup.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::{
    ActorRuntimeId, CerealizerNetworkItemStackDescriptorSerializedData as WireItem,
    EnumsContainerEnumName,
};
use jolyne::valentine::{FullContainerName, InventorySlotPacket, McpePacket, TakeItemActorPacket};

use super::types::{BlockRegistryResource, ItemRegistryResource};
use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::{
    ActorViewers, DroppedItem, GameMode, Health, ItemEntityStack, MainInventory, PhysicsBody,
    PickupDelay, Player, PlayerSession, Position, RuntimeEntityId, RuntimeId,
};
use crate::item::ItemStack;
use crate::registry::collision::Aabb;
use crate::registry::{BlockRegistry, ItemRegistry};

/// Width and height of a player's collision box.
const PLAYER_WIDTH: f64 = 0.6;
const PLAYER_HEIGHT: f64 = 1.8;

/// How far around the player's box items are touched (1 block horizontally,
/// half a block vertically, as in dragonfly's player item collection).
const PICKUP_REACH: DVec3 = DVec3::new(1.0, 0.5, 1.0);

/// Network descriptor of a server stack. `None` if the item is not in the
/// registry or its fields do not fit the wire types.
pub(crate) fn item_descriptor(stack: &ItemStack, items: &ItemRegistry) -> Option<WireItem> {
    if stack.is_empty() {
        return Some(WireItem::default());
    }
    Some(WireItem {
        id: i16::try_from(items.get_by_name(&stack.item_id)?.network_id).ok()?,
        stacksize: u16::from(stack.count),
        auxvalue: u32::try_from(stack.damage).ok()?,
        net_id_variant: None,
        block_runtime_id: u32::try_from(stack.runtime_id).ok()?,
        user_data_buffer: crate::item::user_data::network_user_data(
            &stack.item_id,
            stack.nbt.as_deref(),
        ),
    })
}

/// A registry-backed stack of `count` x `item_id`: carries the registry
/// stack limit and, for block items, the block's default runtime state.
pub(crate) fn registry_stack(
    item_id: &str,
    count: u8,
    items: &ItemRegistry,
    blocks: &BlockRegistry,
) -> Option<ItemStack> {
    let entry = items.get_by_name(item_id)?;
    let runtime_id = blocks
        .default_state_id_by_name(item_id)
        .map_or(Ok(0), i32::try_from)
        .ok()?;
    Some(
        ItemStack::new(item_id, count)
            .with_max_stack_size(entry.stack_size)
            .with_runtime_id(runtime_id),
    )
}

/// Feet position of a player whose `Position` is the eye position.
fn player_box(eye: DVec3) -> Aabb {
    Aabb::from_feet(
        eye - DVec3::new(0.0, PLAYER_EYE_HEIGHT, 0.0),
        PLAYER_WIDTH,
        PLAYER_HEIGHT,
    )
}

fn slot_packet(slot: usize, item: WireItem) -> InventorySlotPacket {
    InventorySlotPacket {
        container_id: 0,
        slot: slot as u32,
        full_container_name: Some(FullContainerName {
            container_name: EnumsContainerEnumName::Combinedhotbarandinventorycontainer,
            dynamic_id: None,
        }),
        storage_item: Some(item.clone()),
        item,
    }
}

/// Send the contents of main-inventory `slot` of `player` to its client.
pub(crate) fn send_slot(world: &World, player: Entity, slot: usize, stack: &ItemStack) {
    let (Some(session), Some(items)) = (
        world.get::<PlayerSession>(player),
        world.get_resource::<ItemRegistryResource>(),
    ) else {
        return;
    };
    if let Some(wire) = item_descriptor(stack, &items.0) {
        let _ = session.send(McpePacket::from(slot_packet(slot, wire)));
    }
}

/// System: players pick up items they touch.
///
/// Only items whose pickup delay ran out can be taken, by living
/// non-spectator players. As much of the stack as fits goes
/// into the inventory; creative players consume the whole stack. A fully
/// taken item is announced with `TakeItemActor` to the item's viewers and the
/// collector and removed; a partially taken one stays with the remainder.
/// Pickup reach and the partial-pickup rule are to be confirmed by a BDS
/// capture (see the entities research doc).
#[allow(clippy::type_complexity)]
pub fn pickup_items(
    mut commands: Commands,
    mut items: Query<
        (
            Entity,
            &Position,
            &mut ItemEntityStack,
            &PickupDelay,
            &RuntimeId,
            Option<&ActorViewers>,
        ),
        With<DroppedItem>,
    >,
    mut players: Query<
        (
            Entity,
            &Position,
            &GameMode,
            &mut MainInventory,
            &RuntimeEntityId,
            Option<&Health>,
        ),
        With<Player>,
    >,
    sessions: Query<&PlayerSession>,
    registry: Res<ItemRegistryResource>,
) {
    let mut taken: Vec<Entity> = Vec::new();
    for (player, eye, mode, mut inventory, player_rid, health) in players.iter_mut() {
        if *mode == GameMode::Spectator || health.is_some_and(|h| h.current <= 0.0) {
            continue;
        }
        let reach = player_box(eye.0).inflate(PICKUP_REACH);
        for (item, position, mut stack, delay, item_rid, viewers) in items.iter_mut() {
            if taken.contains(&item) || !delay.can_pickup() || stack.0.is_empty() {
                continue;
            }
            let item_box = Aabb::from_feet(
                position.0,
                PhysicsBody::ITEM.width,
                PhysicsBody::ITEM.height,
            );
            if !item_box.intersects(&reach) {
                continue;
            }
            let before = inventory.0.slots().to_vec();
            let (added, leftover) = inventory.0.add_item(stack.0.clone());
            let remaining = if *mode == GameMode::Creative {
                ItemStack::empty()
            } else {
                leftover
            };
            if added == 0 && !remaining.is_empty() {
                continue;
            }
            if let Ok(session) = sessions.get(player) {
                for (slot, (old, new)) in before.iter().zip(inventory.0.slots()).enumerate() {
                    if old != new
                        && let Some(wire) = item_descriptor(new, &registry.0)
                    {
                        let _ = session.send(McpePacket::from(slot_packet(slot, wire)));
                    }
                }
            }
            if remaining.is_empty() {
                let packet = TakeItemActorPacket {
                    item_runtime_id: ActorRuntimeId {
                        actor_runtime_id: item_rid.0 as u64,
                    },
                    actor_runtime_id: ActorRuntimeId {
                        actor_runtime_id: player_rid.0 as u64,
                    },
                };
                let mut receivers: Vec<Entity> =
                    viewers.map(|v| v.viewers.clone()).unwrap_or_default();
                if !receivers.contains(&player) {
                    receivers.push(player);
                }
                for receiver in receivers {
                    if let Ok(session) = sessions.get(receiver) {
                        let _ = session.send(McpePacket::from(packet.clone()));
                    }
                }
                stack.0 = ItemStack::empty();
                taken.push(item);
                commands.entity(item).despawn();
            } else {
                stack.0 = remaining;
            }
        }
    }
}

/// Resolve the stack a broken block or container should drop.
pub(crate) fn world_stack(world: &World, item_id: &str, count: u8) -> Option<ItemStack> {
    registry_stack(
        item_id,
        count,
        &world.get_resource::<ItemRegistryResource>()?.0,
        &world.get_resource::<BlockRegistryResource>()?.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::components::{Rotation, Velocity};
    use crate::entity::{EntityRandom, spawn_item_drop};
    use jolyne::valentine::McpePacketData;
    use std::sync::Arc;

    struct Harness {
        world: World,
        schedule: Schedule,
        player: Entity,
        rx: tokio::sync::mpsc::Receiver<McpePacket>,
    }

    fn harness(mode: GameMode) -> Harness {
        let mut world = World::new();
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        world.insert_resource(ItemRegistryResource(Arc::new(items)));
        world.insert_resource(BlockRegistryResource(Arc::new(
            BlockRegistry::vanilla().clone(),
        )));
        world.insert_resource(EntityRandom::seeded(3));
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let player = world
            .spawn((
                Player,
                Position(DVec3::new(0.5, 64.0 + PLAYER_EYE_HEIGHT, 0.5)),
                Rotation::default(),
                mode,
                MainInventory::default(),
                RuntimeEntityId(1),
                PlayerSession::new(1, "test".into(), None, None, tx),
            ))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(pickup_items);
        Harness {
            world,
            schedule,
            player,
            rx,
        }
    }

    fn drop_at(h: &mut Harness, position: DVec3, id: &str, count: u8) -> Entity {
        let stack = world_stack(&h.world, id, count).unwrap();
        let item = spawn_item_drop(&mut h.world, position, stack, None).unwrap();
        h.world.get_mut::<Velocity>(item).unwrap().0 = DVec3::ZERO;
        item
    }

    fn inventory_count(h: &Harness, id: &str) -> usize {
        h.world
            .get::<MainInventory>(h.player)
            .unwrap()
            .0
            .slots()
            .iter()
            .filter(|s| s.item_id == id)
            .map(|s| usize::from(s.count))
            .sum()
    }

    #[test]
    fn items_wait_for_their_pickup_delay() {
        let mut h = harness(GameMode::Survival);
        let item = drop_at(
            &mut h,
            DVec3::new(1.0, 64.0, 0.5),
            "minecraft:cobblestone",
            3,
        );
        h.schedule.run(&mut h.world);
        assert!(h.world.get_entity(item).is_ok());
        assert_eq!(inventory_count(&h, "minecraft:cobblestone"), 0);

        h.world.get_mut::<PickupDelay>(item).unwrap().0 = 0;
        h.schedule.run(&mut h.world);
        assert!(h.world.get_entity(item).is_err(), "taken items are removed");
        assert_eq!(inventory_count(&h, "minecraft:cobblestone"), 3);
        let mut saw_slot = false;
        let mut saw_take = false;
        while let Ok(packet) = h.rx.try_recv() {
            match packet.data {
                McpePacketData::InventorySlotPacket(slot) => {
                    saw_slot = true;
                    assert_eq!(slot.item.stacksize, 3);
                }
                McpePacketData::TakeItemActorPacket(take) => {
                    saw_take = true;
                    assert_eq!(take.actor_runtime_id.actor_runtime_id, 1);
                }
                _ => {}
            }
        }
        assert!(saw_slot && saw_take);
    }

    #[test]
    fn items_out_of_reach_stay() {
        let mut h = harness(GameMode::Survival);
        let item = drop_at(&mut h, DVec3::new(3.0, 64.0, 0.5), "minecraft:dirt", 1);
        h.world.get_mut::<PickupDelay>(item).unwrap().0 = 0;
        h.schedule.run(&mut h.world);
        assert!(h.world.get_entity(item).is_ok());
    }

    #[test]
    fn full_inventory_takes_what_fits_and_leaves_the_rest() {
        let mut h = harness(GameMode::Survival);
        let filler = world_stack(&h.world, "minecraft:dirt", 64).unwrap();
        let partial = world_stack(&h.world, "minecraft:stone", 60).unwrap();
        {
            let mut inventory = h.world.get_mut::<MainInventory>(h.player).unwrap();
            for slot in 0..35 {
                inventory.0.set_item(slot, filler.clone()).unwrap();
            }
            inventory.0.set_item(35, partial).unwrap();
        }
        let item = drop_at(&mut h, DVec3::new(0.5, 64.0, 0.5), "minecraft:stone", 10);
        h.world.get_mut::<PickupDelay>(item).unwrap().0 = 0;
        h.schedule.run(&mut h.world);
        assert_eq!(inventory_count(&h, "minecraft:stone"), 64);
        assert_eq!(h.world.get::<ItemEntityStack>(item).unwrap().0.count, 6);
        while let Ok(packet) = h.rx.try_recv() {
            assert!(
                !matches!(packet.data, McpePacketData::TakeItemActorPacket(_)),
                "partial pickups do not send TakeItemActor"
            );
        }

        let dirt = drop_at(&mut h, DVec3::new(0.5, 64.0, 0.5), "minecraft:dirt", 1);
        h.world.get_mut::<PickupDelay>(dirt).unwrap().0 = 0;
        h.schedule.run(&mut h.world);
        assert!(h.world.get_entity(dirt).is_ok(), "nothing fits");
    }

    #[test]
    fn spectators_and_dead_players_do_not_pick_up() {
        let mut h = harness(GameMode::Spectator);
        let item = drop_at(&mut h, DVec3::new(0.5, 64.0, 0.5), "minecraft:dirt", 1);
        h.world.get_mut::<PickupDelay>(item).unwrap().0 = 0;
        h.schedule.run(&mut h.world);
        assert!(h.world.get_entity(item).is_ok());

        let mut h = harness(GameMode::Survival);
        h.world.entity_mut(h.player).insert(Health {
            current: 0.0,
            ..Health::default()
        });
        let item = drop_at(&mut h, DVec3::new(0.5, 64.0, 0.5), "minecraft:dirt", 1);
        h.world.get_mut::<PickupDelay>(item).unwrap().0 = 0;
        h.schedule.run(&mut h.world);
        assert!(h.world.get_entity(item).is_ok());
    }

    #[test]
    fn registry_stacks_carry_limits_and_block_states() {
        let h = harness(GameMode::Survival);
        let stone = world_stack(&h.world, "minecraft:stone", 1).unwrap();
        assert_eq!(stone.max_stack_size(), 64);
        assert_eq!(
            u32::try_from(stone.runtime_id).unwrap(),
            BlockRegistry::vanilla()
                .default_state_id_by_name("minecraft:stone")
                .unwrap()
        );
        let pearl = world_stack(&h.world, "minecraft:ender_pearl", 1).unwrap();
        assert_eq!(pearl.max_stack_size(), 16);
        assert_eq!(pearl.runtime_id, 0);
        assert!(world_stack(&h.world, "minecraft:not_an_item", 1).is_none());
    }
}
