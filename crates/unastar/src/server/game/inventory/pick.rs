//! `BlockPickRequest` (I15): BDS 1.26.51 `selectItem` rules.
//!
//! - The packet's hotbar-slot field is ignored. Dead players, air and
//!   blocks farther than 14 blocks (creative) or 8.7 (otherwise) from the
//!   eye position to the block's integer corner are ignored silently.
//! - The picked item is the block's own item. Only items in the advertised
//!   creative content are pickable, so internal block items (wall signs,
//!   crops) are never handed out; picking those blocks does nothing until a
//!   measured pick-item table exists.
//! - A pick with block data of a block that has a block entity does nothing:
//!   the block-entity user data BDS copies needs I8 block entities.
//! - Found in the hotbar: select it. Found in the main inventory: swap it
//!   into the first empty hotbar slot, else into the held slot. Not found:
//!   survival does nothing; creative places the item in the first empty
//!   hotbar slot, else moves the held item to the first empty main slot (or
//!   overwrites it when the inventory is full) and places it there. A held
//!   item with `minecraft:item_lock` 1 is never displaced.
//! - On success the player gets the whole inventory and `PlayerHotbar`
//!   selecting the slot. `GuiDataPickItem` (the HUD name popup) is not sent:
//!   its name fields have not been captured from BDS yet.

use bevy_ecs::prelude::*;
use jolyne::valentine::{BlockPickRequestPacket, McpePacket, PlayerHotbarPacket};
use zuri_nbt::NBTTag;

use super::super::packet_queues::InventoryEvents;
use super::{INVENTORY_WINDOW, PlayerInventoryItem, send_inventory};
use crate::ecs::events::ServerEvent;
use crate::entity::components::{GameMode, Health, Position};
use crate::item::ItemStack;
use crate::world::chunk::blocks;
use crate::world::ecs::{ChunkData, ChunkManager, world_to_chunk_coords};

const HOTBAR: usize = 9;
const CREATIVE_PICK_DISTANCE: f64 = 14.0;
const PICK_DISTANCE: f64 = 8.7;

/// The item a valid pick request selects, or `None` when BDS ignores it.
pub(super) fn picked_item(
    world: &World,
    entity: Entity,
    pk: &BlockPickRequestPacket,
) -> Option<ItemStack> {
    if !world
        .get::<Health>(entity)
        .is_some_and(|health| health.current > 0.0)
    {
        return None;
    }
    let creative = world.get::<GameMode>(entity) == Some(&GameMode::Creative);
    let eye = world.get::<Position>(entity)?.0;
    let at = (pk.position.x, pk.position.y, pk.position.z);
    let corner = glam::DVec3::new(f64::from(at.0), f64::from(at.1), f64::from(at.2));
    let reach = if creative {
        CREATIVE_PICK_DISTANCE
    } else {
        PICK_DISTANCE
    };
    if !eye.is_finite() || eye.distance(corner) > reach {
        return None;
    }
    let runtime_id = super::super::blocks::block_at(world, at)?;
    if runtime_id == *blocks::AIR {
        return None;
    }
    if pk.with_data && has_block_entity(world, at) {
        return None;
    }
    let block = world
        .get_resource::<super::super::types::BlockRegistryResource>()?
        .0
        .get_by_runtime_id(runtime_id)?;
    let items = &world
        .get_resource::<super::super::types::ItemRegistryResource>()?
        .0;
    let network_id = items.get_by_name(&block.string_id)?.network_id();
    let template = world.get_resource::<super::super::types::ServerWorldTemplate>()?;
    let creative_net_id = template
        .0
        .creative_content
        .entries
        .iter()
        .find(|entry| entry.item_instance.id == network_id)?
        .creative_net_id
        .id;
    super::creative_stack(items, template, creative_net_id)
}

fn has_block_entity(world: &World, (x, y, z): (i32, i32, i32)) -> bool {
    let (cx, cz) = world_to_chunk_coords(x, z);
    world
        .get_resource::<ChunkManager>()
        .and_then(|chunks| chunks.get_by_coords(cx, cz))
        .and_then(|chunk| world.get::<ChunkData>(chunk))
        .is_some_and(|chunk| {
            chunk
                .inner
                .block_entities()
                .iter()
                .any(|entity| entity.position() == [x, y, z])
        })
}

/// Same item, aux value and user data (BDS `getSlotWithItem` with aux and
/// data checks).
fn same_item(slot: &ItemStack, item: &ItemStack) -> bool {
    !slot.is_empty()
        && slot.item_id() == item.item_id()
        && slot.damage == item.damage
        && slot.nbt == item.nbt
}

fn item_locked(stack: &ItemStack) -> bool {
    crate::item::enchant::item_tag(stack).is_some_and(
        |tag| matches!(tag.0.get("minecraft:item_lock"), Some(NBTTag::Byte(value)) if value.0 == 1),
    )
}

/// Apply `selectItem` for a picked `item`; nothing is sent when it fails.
pub(super) fn select_item(
    entity: Entity,
    item: ItemStack,
    player: &mut PlayerInventoryItem<'_, '_>,
    events: &mut ResMut<InventoryEvents>,
) {
    let main = &mut player.main.0;
    let held = usize::from(player.held_slot.0);
    let found = (0..main.size()).find(|&slot| same_item(main.item(slot).unwrap(), &item));
    let empty = main.first_empty();
    let empty_hotbar = empty.filter(|&slot| slot < HOTBAR);
    let held_locked = || item_locked(main.item(held).unwrap());
    let target = match found {
        Some(slot) if slot < HOTBAR => slot,
        Some(slot) => {
            let target = match empty_hotbar {
                Some(target) => target,
                None if held_locked() => return,
                None => held,
            };
            main.swap(slot, target).expect("inventory slots");
            target
        }
        None if *player.mode != GameMode::Creative => return,
        None => {
            let target = match empty_hotbar {
                Some(target) => target,
                None if held_locked() => return,
                None => {
                    if let Some(empty) = empty {
                        main.swap(held, empty).expect("inventory slots");
                    }
                    held
                }
            };
            main.set_item(target, item.into_slot())
                .expect("hotbar slot");
            target
        }
    };
    let old_slot = player.held_slot.0;
    let new_slot = target as u8;
    if old_slot != new_slot {
        player.held_slot.set(new_slot);
        events.0.push(ServerEvent::PlayerHeldSlotChange {
            entity,
            old_slot,
            new_slot,
        });
    }
    send_inventory(main, player.session);
    let _ = player.session.send(McpePacket::from(PlayerHotbarPacket {
        selected_slot: u32::from(new_slot),
        container_id: INVENTORY_WINDOW,
        shouldselectslot: true,
    }));
}

#[cfg(test)]
mod tests {
    use bevy_ecs::system::SystemState;

    use super::*;
    use crate::entity::components::{
        ArmourInventory, ContainerWindows, HeldSlot, MainInventory, OffhandSlot, PlayerSession,
        PlayerUi,
    };
    use crate::item::test_support::stack;
    use crate::item::user_data::stored_user_data;
    use jolyne::valentine::McpePacketData;

    use super::super::{PlayerInventory, RequestIdAssignments};

    struct Picker {
        world: World,
        entity: Entity,
        rx: tokio::sync::mpsc::Receiver<McpePacket>,
    }

    impl Picker {
        fn new(mode: GameMode, held: u8, slots: &[(usize, ItemStack)]) -> Self {
            let mut world = World::new();
            world.insert_resource(InventoryEvents::default());
            let (tx, rx) = tokio::sync::mpsc::channel(8);
            let mut main = MainInventory::default();
            for (slot, item) in slots {
                main.0.set_item(*slot, item.clone().into_slot()).unwrap();
            }
            let entity = world
                .spawn((
                    mode,
                    main,
                    ArmourInventory::default(),
                    OffhandSlot::default(),
                    PlayerUi::default(),
                    HeldSlot(held),
                    ContainerWindows::default(),
                    RequestIdAssignments::default(),
                    PlayerSession::new(1, "test".into(), None, None, tx),
                    Position::new(0.5, 65.62, 0.5),
                ))
                .id();
            Self { world, entity, rx }
        }

        fn pick(&mut self, item: ItemStack) {
            let mut state: SystemState<(Query<PlayerInventory>, ResMut<InventoryEvents>)> =
                SystemState::new(&mut self.world);
            let (mut players, mut events) = state.get_mut(&mut self.world);
            let mut player = players.get_mut(self.entity).unwrap();
            select_item(self.entity, item, &mut player, &mut events);
        }

        fn slot(&self, slot: usize) -> &ItemStack {
            self.world
                .get::<MainInventory>(self.entity)
                .unwrap()
                .0
                .item(slot)
                .unwrap()
        }

        fn held(&self) -> u8 {
            self.world.get::<HeldSlot>(self.entity).unwrap().0
        }

        /// The packets sent for the last pick: inventory content then the
        /// hotbar selection, or nothing.
        fn sent(&mut self) -> Option<u32> {
            let first = self.rx.try_recv().ok()?;
            let McpePacketData::InventoryContentPacket(content) = first.data else {
                panic!("whole inventory first");
            };
            assert_eq!((content.container_id, content.slots.len()), (0, 36));
            let McpePacketData::PlayerHotbarPacket(hotbar) = self.rx.try_recv().unwrap().data
            else {
                panic!("hotbar selection");
            };
            assert_eq!(hotbar.container_id, 0);
            assert!(hotbar.shouldselectslot);
            assert!(self.rx.try_recv().is_err());
            Some(hotbar.selected_slot)
        }
    }

    fn fill_hotbar() -> Vec<(usize, ItemStack)> {
        (0..HOTBAR)
            .map(|slot| (slot, stack("minecraft:dirt", 1)))
            .collect()
    }

    #[test]
    fn survival_selects_hotbar_matches_and_swaps_main_matches_into_the_hotbar() {
        let mut picker = Picker::new(
            GameMode::Survival,
            0,
            &[
                (0, stack("minecraft:dirt", 1)),
                (4, stack("minecraft:stone", 3)),
            ],
        );
        picker.pick(stack("minecraft:stone", 1));
        assert_eq!((picker.held(), picker.sent()), (4, Some(4)));

        // First empty hotbar slot (1) takes the main-inventory stack.
        let mut picker = Picker::new(
            GameMode::Survival,
            0,
            &[
                (0, stack("minecraft:dirt", 1)),
                (20, stack("minecraft:stone", 3)),
            ],
        );
        let id = picker.slot(20).net_id;
        picker.pick(stack("minecraft:stone", 1));
        assert_eq!((picker.held(), picker.sent()), (1, Some(1)));
        assert_eq!((picker.slot(1).count, picker.slot(1).net_id), (3, id));
        assert!(picker.slot(20).is_empty());

        // Full hotbar: swap with the held slot.
        let mut slots = fill_hotbar();
        slots.push((20, stack("minecraft:stone", 3)));
        let mut picker = Picker::new(GameMode::Survival, 6, &slots);
        picker.pick(stack("minecraft:stone", 1));
        assert_eq!((picker.held(), picker.sent()), (6, Some(6)));
        assert_eq!(picker.slot(6).item_id(), "minecraft:stone");
        assert_eq!(picker.slot(20).item_id(), "minecraft:dirt");
    }

    #[test]
    fn survival_without_the_item_and_different_aux_or_data_change_nothing() {
        let mut picker = Picker::new(
            GameMode::Survival,
            2,
            &[(5, stack("minecraft:stone", 1).with_damage(1))],
        );
        picker.pick(stack("minecraft:stone", 1));
        let named = stack("minecraft:stone", 1).with_nbt(
            stored_user_data(
                "minecraft:stone",
                &[0xff, 0xff, 1, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            )
            .unwrap(),
        );
        picker.pick(named);
        assert_eq!((picker.held(), picker.sent()), (2, None));
        assert_eq!(picker.slot(5).damage, 1);
    }

    #[test]
    fn creative_places_new_items_in_an_empty_hotbar_slot_or_moves_the_held_item_aside() {
        let mut picker = Picker::new(GameMode::Creative, 0, &[(0, stack("minecraft:dirt", 1))]);
        picker.pick(stack("minecraft:stone", 1));
        assert_eq!((picker.held(), picker.sent()), (1, Some(1)));
        assert_eq!(
            (picker.slot(1).item_id(), picker.slot(1).count),
            ("minecraft:stone", 1)
        );
        assert!(picker.slot(1).net_id > 0);

        let mut picker = Picker::new(GameMode::Creative, 3, &fill_hotbar());
        let held_id = picker.slot(3).net_id;
        picker.pick(stack("minecraft:stone", 1));
        assert_eq!((picker.held(), picker.sent()), (3, Some(3)));
        assert_eq!(picker.slot(3).item_id(), "minecraft:stone");
        assert_eq!(
            (picker.slot(9).item_id(), picker.slot(9).net_id),
            ("minecraft:dirt", held_id)
        );

        // A full inventory overwrites the held item.
        let full: Vec<_> = (0..36)
            .map(|slot| (slot, stack("minecraft:dirt", 1)))
            .collect();
        let mut picker = Picker::new(GameMode::Creative, 3, &full);
        picker.pick(stack("minecraft:stone", 1));
        assert_eq!(picker.sent(), Some(3));
        assert_eq!(picker.slot(3).item_id(), "minecraft:stone");
    }

    #[test]
    fn a_locked_held_item_is_never_displaced() {
        let locked = crate::item::enchant::with_item_tag(
            &stack("minecraft:dirt", 1),
            zuri_nbt::tag::Compound(
                [(
                    "minecraft:item_lock".to_owned(),
                    NBTTag::Byte(zuri_nbt::tag::Byte(1)),
                )]
                .into_iter()
                .collect(),
            ),
        );
        let mut slots: Vec<_> = (0..HOTBAR).map(|slot| (slot, locked.clone())).collect();
        slots.push((20, stack("minecraft:stone", 3)));
        for mode in [GameMode::Survival, GameMode::Creative] {
            let mut picker = Picker::new(mode, 4, &slots);
            picker.pick(stack("minecraft:stone", 1));
            assert_eq!(picker.sent(), None);
            assert_eq!(picker.slot(4).item_id(), "minecraft:dirt");
            assert_eq!(picker.slot(20).item_id(), "minecraft:stone");
        }
        // A free hotbar slot does not displace the held item, so it is allowed.
        let mut picker = Picker::new(GameMode::Creative, 0, &[(0, locked)]);
        picker.pick(stack("minecraft:stone", 1));
        assert_eq!(picker.sent(), Some(1));
    }

    /// The block side of a pick against the real server resources (item
    /// registry, advertised creative content) and a loaded chunk.
    #[test]
    fn picked_item_is_the_creative_block_item_within_reach_of_a_living_player() {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let _guard = runtime.enter();
                let mut config = crate::world::WorldConfig::void_spawn_platform(1);
                config.generation_threads = 1;
                let mut server = crate::GameServer::with_config(crate::ServerConfig {
                    default_gamemode: GameMode::Creative,
                    world: config,
                    ..Default::default()
                });
                let (tx, _rx) = tokio::sync::mpsc::channel(64);
                let player = server
                    .spawn_player(crate::PlayerSpawnData {
                        session_id: 12,
                        display_name: "pick".into(),
                        xuid: None,
                        uuid: Some(uuid::Uuid::from_u128(12).to_string()),
                        runtime_id: 42,
                        position: glam::DVec3::new(1.5, 66.62, 1.5),
                        saved_data: None,
                        outbound_tx: tx,
                        chunk_radius: 0,
                    })
                    .unwrap();
                let request = |x, y, z, with_data| BlockPickRequestPacket {
                    position: jolyne::valentine::BlockPos { x, y, z },
                    with_data,
                    max_slots: 9,
                };
                let world = server.ecs.world_mut();
                let sign = crate::registry::BlockRegistry::vanilla()
                    .default_state_id_by_name("minecraft:standing_sign")
                    .unwrap();
                let mut chunk = crate::world::chunk::Chunk::new(0, 0);
                chunk.set_block(1, 64, 1, *blocks::STONE);
                chunk.set_block(2, 64, 1, sign);
                let chunk = world.spawn(ChunkData::new(chunk)).id();
                world
                    .resource_mut::<ChunkManager>()
                    .insert(crate::world::ecs::ChunkPosition::new(0, 0), chunk);

                let stone = picked_item(world, player, &request(1, 64, 1, false)).unwrap();
                assert_eq!(
                    (stone.item_id(), stone.count, stone.damage),
                    ("minecraft:stone", 1, 0)
                );
                assert!(
                    picked_item(world, player, &request(1, 65, 1, false)).is_none(),
                    "air"
                );
                assert!(
                    picked_item(world, player, &request(2, 64, 1, false)).is_none(),
                    "the internal standing_sign item is not handed out"
                );

                // 8.7 blocks outside creative, 14 in creative, to the corner.
                world.get_mut::<Position>(player).unwrap().0 = glam::DVec3::new(1.0, 64.0, 14.9);
                assert!(picked_item(world, player, &request(1, 64, 1, true)).is_some());
                world.get_mut::<Position>(player).unwrap().0 = glam::DVec3::new(1.0, 64.0, 15.1);
                assert!(picked_item(world, player, &request(1, 64, 1, false)).is_none());
                *world.get_mut::<GameMode>(player).unwrap() = GameMode::Survival;
                world.get_mut::<Position>(player).unwrap().0 = glam::DVec3::new(1.0, 64.0, 9.6);
                assert!(picked_item(world, player, &request(1, 64, 1, false)).is_some());
                world.get_mut::<Position>(player).unwrap().0 = glam::DVec3::new(1.0, 64.0, 9.8);
                assert!(picked_item(world, player, &request(1, 64, 1, false)).is_none());

                world.get_mut::<Position>(player).unwrap().0 = glam::DVec3::new(1.5, 66.62, 1.5);
                world.get_mut::<Health>(player).unwrap().current = 0.0;
                assert!(
                    picked_item(world, player, &request(1, 64, 1, false)).is_none(),
                    "dead"
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
