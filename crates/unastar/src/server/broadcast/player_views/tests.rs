//! Actual queued packet regressions for player audiences and displayed equipment.

use super::*;
use crate::item::ItemStack;
use crate::item::test_support::stack;
use glam::DVec3;
use jolyne::valentine::types::EnumsAnimatePacketPayloadAction;
use jolyne::valentine::{AnimatePacket, McpePacketData};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Minimal world with the normal item registry, spawn messages and spatial lookup.
fn world() -> World {
    let mut world = World::new();
    let mut items = crate::registry::ItemRegistry::new();
    items.load_vanilla();
    world.insert_resource(ItemRegistryResource(Arc::new(items)));
    world.init_resource::<Messages<PlayerSpawnedEvent>>();
    world.init_resource::<Messages<PlayerDespawnedEvent>>();
    world.init_resource::<super::super::EntityGrid>();
    world
}
/// Spawn a fully identified player and announce that its initial state is ready.
fn player(world: &mut World, id: i64) -> (Entity, mpsc::Receiver<McpePacket>) {
    let (tx, rx) = mpsc::channel(256);
    let entity = world
        .spawn((
            Player,
            PlayerName(format!("Actor{id}")),
            PlayerUuid(uuid::Uuid::from_u128(id as u128)),
            RuntimeEntityId(id),
            Position(DVec3::new(id as f64, 65.62, 1.0)),
            Rotation::default(),
            GameMode::Survival,
            PlayerSession::new(id as u64, format!("Actor{id}"), None, None, tx),
            MainInventory::default(),
            ArmourInventory::default(),
            OffhandSlot::default(),
            HeldSlot(0),
            PlayerState::default(),
            PlayerInput::default(),
            Health::default(),
        ))
        .id();
    world
        .resource_mut::<super::super::EntityGrid>()
        .insert((0, 0), entity);
    world.write_message(PlayerSpawnedEvent {
        entity,
        position: world.get::<Position>(entity).unwrap().0,
        runtime_id: id,
    });
    (entity, rx)
}
/// Drain every queued packet so subsequent assertions isolate one transition.
fn drain(rx: &mut mpsc::Receiver<McpePacket>) -> Vec<McpePacketData> {
    std::iter::from_fn(|| rx.try_recv().ok().map(|packet| packet.data)).collect()
}
/// Authoritative slots with distinct visible descriptors.
fn equip(world: &mut World, actor: Entity) {
    world
        .get_mut::<MainInventory>(actor)
        .unwrap()
        .0
        .set_item(0, stack("minecraft:diamond_sword", 1))
        .unwrap();
    world.get_mut::<OffhandSlot>(actor).unwrap().0 = stack("minecraft:totem_of_undying", 1);
    world
        .get_mut::<ArmourInventory>(actor)
        .unwrap()
        .set_helmet(stack("minecraft:diamond_helmet", 1))
        .unwrap();
}
/// Read the flag word carried by either a spawn or a metadata update.
fn flags(data: &[DataItemEntry]) -> i64 {
    match &data
        .iter()
        .find(|entry| entry.id == ids::RESERVED_0)
        .unwrap()
        .payload
    {
        DataItemEntryPayload::DataItemInt64Payload(value) => value.value,
        _ => panic!("flag payload"),
    }
}

#[test]
fn one_packet_channel_retries_spawn_hands_armour_and_remove_without_starvation() {
    let mut world = world();
    let (actor, _) = player(&mut world, 1);
    let (viewer, _) = player(&mut world, 2);
    let (tx, mut rx) = mpsc::channel(1);
    world.get_mut::<PlayerSession>(viewer).unwrap().outbound_tx = tx.clone();
    equip(&mut world, actor);
    tx.try_send(McpePacket::from(super::super::build_remove_actor_packet(
        999,
    )))
    .unwrap();
    sync_player_views(&mut world);
    assert!(
        !can_receive_actor(&world, actor, viewer),
        "failed spawn is not client-known"
    );
    drain(&mut rx);
    let mut packets = Vec::new();
    for _ in 0..5 {
        sync_player_views(&mut world);
        packets.extend(drain(&mut rx));
    }
    assert!(matches!(
        packets.first(),
        Some(McpePacketData::AddPlayerPacket(_))
    ));
    assert_eq!(
        packets
            .iter()
            .filter(|packet| matches!(packet, McpePacketData::MobEquipmentPacket(_)))
            .count(),
        2
    );
    assert_eq!(
        packets
            .iter()
            .filter(|packet| matches!(packet, McpePacketData::MobArmorEquipmentPacket(_)))
            .count(),
        1
    );
    assert!(can_receive_actor(&world, actor, viewer));
    tx.try_send(McpePacket::from(super::super::build_remove_actor_packet(
        999,
    )))
    .unwrap();
    world.entity_mut(actor).insert(ActorAudience::only([]));
    sync_player_views(&mut world);
    drain(&mut rx);
    sync_player_views(&mut world);
    assert!(
        matches!(drain(&mut rx).as_slice(), [McpePacketData::RemoveActorPacket(remove)] if remove.target_actor_id.actor_unique_id == 1)
    );
    sync_player_views(&mut world);
    assert!(drain(&mut rx).is_empty());
}

#[test]
fn default_audience_spawns_actual_hands_and_armour_once_for_each_new_viewer() {
    let mut world = world();
    let (actor, mut own) = player(&mut world, 1);
    let (_, mut viewer) = player(&mut world, 2);
    equip(&mut world, actor);
    sync_player_views(&mut world);
    let packets = drain(&mut viewer);
    let McpePacketData::AddPlayerPacket(add) = &packets[0] else {
        panic!("spawn must precede equipment")
    };
    assert_ne!(add.carried_item.id, 0);
    assert!(packets.iter().any(|packet| matches!(packet,McpePacketData::MobArmorEquipmentPacket(armour) if armour.head.id!=0)));
    assert_eq!(
        packets
            .iter()
            .filter(|packet| matches!(packet, McpePacketData::MobEquipmentPacket(_)))
            .count(),
        2
    );
    drain(&mut own);
    sync_player_views(&mut world);
    assert!(drain(&mut viewer).is_empty());
}

#[test]
fn hidden_actor_is_removed_and_revealed_with_current_metadata_and_equipment() {
    let mut world = world();
    let (actor, _) = player(&mut world, 1);
    let (viewer, mut rx) = player(&mut world, 2);
    equip(&mut world, actor);
    sync_player_views(&mut world);
    drain(&mut rx);
    world.entity_mut(actor).insert(ActorAudience::only([]));
    sync_player_views(&mut world);
    assert!(matches!(
        drain(&mut rx).as_slice(),
        [McpePacketData::RemoveActorPacket(_)]
    ));
    world.get_mut::<Position>(actor).unwrap().0.x += 2.0;
    world
        .get_mut::<ArmourInventory>(actor)
        .unwrap()
        .set_helmet(stack("minecraft:golden_helmet", 1))
        .unwrap();
    let mut effects = Effects::default();
    effects.add(EffectType::INVISIBILITY, 0, 200);
    world.entity_mut(actor).insert(effects);
    sync_player_views(&mut world);
    assert!(drain(&mut rx).is_empty());
    world
        .entity_mut(actor)
        .insert(ActorAudience::only([viewer]));
    sync_player_views(&mut world);
    let packets = drain(&mut rx);
    let McpePacketData::AddPlayerPacket(add) = &packets[0] else {
        panic!("fresh reveal snapshot")
    };
    assert_eq!(add.position.x, 3.0);
    assert_ne!(
        flags(&add.entity_data.data) & (1 << actor_flags::INVISIBLE),
        0
    );
    let gold = stack("minecraft:golden_helmet", 1)
        .item()
        .unwrap()
        .network_id() as i16;
    assert!(packets.iter().any(|packet| matches!(packet,McpePacketData::MobArmorEquipmentPacket(armour) if armour.head.id==gold)));
}

#[test]
fn projection_changes_display_without_mutating_inventory_and_restores_current_slots() {
    let mut world = world();
    let (actor, _) = player(&mut world, 1);
    let (_, mut rx) = player(&mut world, 2);
    equip(&mut world, actor);
    sync_player_views(&mut world);
    drain(&mut rx);
    world.entity_mut(actor).insert(PlayerEquipmentProjection {
        main_hand: Some(ItemStack::empty()),
        off_hand: Some(ItemStack::empty()),
        armour: Some(std::array::from_fn(|_| ItemStack::empty())),
    });
    sync_player_views(&mut world);
    let packets = drain(&mut rx);
    assert_eq!(packets.len(), 3);
    assert!(packets.iter().all(|packet| match packet {
        McpePacketData::MobEquipmentPacket(e) => e.item.id == 0,
        McpePacketData::MobArmorEquipmentPacket(e) =>
            e.head.id == 0 && e.torso.id == 0 && e.legs.id == 0 && e.feet.id == 0,
        _ => false,
    }));
    assert_eq!(
        world
            .get::<ArmourInventory>(actor)
            .unwrap()
            .helmet()
            .item_id(),
        "minecraft:diamond_helmet"
    );
    world
        .get_mut::<ArmourInventory>(actor)
        .unwrap()
        .set_helmet(stack("minecraft:golden_helmet", 1))
        .unwrap();
    sync_player_views(&mut world);
    assert!(drain(&mut rx).is_empty());
    world
        .entity_mut(actor)
        .remove::<PlayerEquipmentProjection>();
    sync_player_views(&mut world);
    assert!(drain(&mut rx).iter().any(|packet| matches!(packet,McpePacketData::MobArmorEquipmentPacket(armour) if armour.head.id!=0)));
}

#[test]
fn newly_joined_viewers_do_not_receive_hidden_actor_identity_or_equipment() {
    let mut world = world();
    let (actor, _) = player(&mut world, 1);
    world.entity_mut(actor).insert(ActorAudience::only([]));
    sync_player_views(&mut world);
    let (_, mut rx) = player(&mut world, 2);
    sync_player_views(&mut world);
    assert!(drain(&mut rx).is_empty());
}

#[test]
fn replacing_viewer_transport_replays_a_fresh_snapshot_and_retained_sources_stay_visible() {
    let mut world = world();
    let (actor, _) = player(&mut world, 1);
    let (viewer, mut old) = player(&mut world, 2);
    equip(&mut world, actor);
    sync_player_views(&mut world);
    drain(&mut old);
    world.entity_mut(actor).remove::<PlayerSession>();
    let (tx, mut replacement) = mpsc::channel(256);
    world.get_mut::<PlayerSession>(viewer).unwrap().outbound_tx = tx;
    sync_player_views(&mut world);
    assert!(drain(&mut old).is_empty());
    assert!(drain(&mut replacement).iter().any(|packet| matches!(packet,McpePacketData::AddPlayerPacket(add) if &add.player_name.as_bytes()[..]==b"Actor1")));
}

#[test]
fn players_without_a_ready_spawn_notification_do_not_exchange_actor_snapshots() {
    let mut world = world();
    let (_, mut first) = player(&mut world, 1);
    sync_player_views(&mut world);
    let (pending, mut second) = player(&mut world, 2);
    world.resource_mut::<Messages<PlayerSpawnedEvent>>().clear();
    sync_player_views(&mut world);
    assert!(drain(&mut first).is_empty());
    assert!(drain(&mut second).is_empty());
    world.write_message(PlayerSpawnedEvent {
        entity: pending,
        position: DVec3::ZERO,
        runtime_id: 2,
    });
    sync_player_views(&mut world);
    assert!(
        drain(&mut first)
            .iter()
            .any(|packet| matches!(packet, McpePacketData::AddPlayerPacket(_)))
    );
    assert!(
        drain(&mut second)
            .iter()
            .any(|packet| matches!(packet, McpePacketData::AddPlayerPacket(_)))
    );
}

#[test]
fn hidden_player_swing_using_hurt_and_mode_events_do_not_reveal_runtime_identity() {
    let mut world = world();
    let (actor, _) = player(&mut world, 1);
    let (_, mut rx) = player(&mut world, 2);
    sync_player_views(&mut world);
    drain(&mut rx);
    world.entity_mut(actor).insert(ActorAudience::only([]));
    sync_player_views(&mut world);
    drain(&mut rx);
    super::super::relay_player_swing(
        &mut world,
        actor,
        &AnimatePacket {
            action: EnumsAnimatePacketPayloadAction::Swing,
            target_actor_runtime_id: ActorRuntimeId {
                actor_runtime_id: 1,
            },
            ..Default::default()
        },
    );
    crate::server::game::item_use::start_using(&mut world, actor, 0, 1);
    crate::entity::player_life::send_to_player_and_viewers(
        &world,
        actor,
        &McpePacket::from(jolyne::valentine::ActorEventPacket::default()),
    );
    crate::command::exec::set_game_mode(&mut world, actor, GameMode::Creative);
    assert!(drain(&mut rx).is_empty());
}

#[test]
fn despawn_only_removes_known_actors_and_self_is_always_allowed() {
    let mut world = world();
    let (actor, _) = player(&mut world, 1);
    let (_, mut rx) = player(&mut world, 2);
    assert!(ActorAudience::only([]).allows(actor, actor));
    sync_player_views(&mut world);
    drain(&mut rx);
    world.despawn(actor);
    sync_player_views(&mut world);
    assert!(matches!(
        drain(&mut rx).as_slice(),
        [McpePacketData::RemoveActorPacket(_)]
    ));
}

#[test]
fn visible_effect_metadata_changes_without_movement_and_clears_when_effect_ends() {
    let mut world = world();
    let (actor, _) = player(&mut world, 1);
    let (_, mut rx) = player(&mut world, 2);
    sync_player_views(&mut world);
    drain(&mut rx);
    let mut effects = Effects::default();
    effects.add(EffectType::INVISIBILITY, 0, 200);
    world.entity_mut(actor).insert(effects);
    sync_player_views(&mut world);
    let packets = drain(&mut rx);
    let [McpePacketData::SetActorDataPacket(data)] = packets.as_slice() else {
        panic!("effect metadata update")
    };
    assert_ne!(
        flags(&data.actor_data.data) & (1 << actor_flags::INVISIBLE),
        0
    );
    world
        .get_mut::<Effects>(actor)
        .unwrap()
        .remove(EffectType::INVISIBILITY);
    sync_player_views(&mut world);
    let packets = drain(&mut rx);
    let [McpePacketData::SetActorDataPacket(data)] = packets.as_slice() else {
        panic!("effect metadata removal")
    };
    assert_eq!(
        flags(&data.actor_data.data) & (1 << actor_flags::INVISIBLE),
        0
    );
}
