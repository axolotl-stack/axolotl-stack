//! Actual cold initialization uses new authenticated actors and current canonical receipts.
use super::*;

/// A real old-target loss can recover through a fresh authenticated held actor, never historical recreation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_cold_rebind_receipt_initializes_fresh_connection_and_reserves_complete_publication()
 {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(1),
    });
    let mut old = Harness::new(30, provider.clone(), tempfile::tempdir().unwrap(), true);
    old.prepare_subject();
    old.retain().await;
    old.until("source:live-ack").await;
    let source = old.subject.unwrap();
    assert!(
        crate::entity::player_life::hurt_player(
            old.world_mut(),
            source,
            &Hit {
                damage: 2.0,
                cause: ActorDamageCause::Override,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT
            }
        ) > 0.0
    );
    old.world_mut()
        .get_mut::<crate::entity::components::Effects>(source)
        .unwrap()
        .add(crate::entity::components::EffectType::SPEED, 1, 400);
    let prior = old.stored().await;
    old.load(40);
    provider
        .written
        .store(false, std::sync::atomic::Ordering::Release);
    let (_lost_target, mut lost_packets) = old.spawn(2, Some(prior));
    old.until("rebind:staged").await;
    for _ in 0..300 {
        old.router.tick().await.unwrap();
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(provider.written.load(std::sync::atomic::Ordering::Acquire));
    assert!(lost_packets.try_recv().is_err());
    let acknowledged = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(acknowledged.health, 7.0);
    let old_epoch = acknowledged.inventory_revision;
    let old_receipts = acknowledged.inventory_effects.clone();
    let Harness {
        router,
        manager,
        archives,
        ..
    } = old;
    drop(router);
    drop(manager);
    provider.release.add_permits(1);
    for _ in 0..300 {
        if Arc::strong_count(&provider) == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert_eq!(Arc::strong_count(&provider), 1);
    drop(provider);
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(0),
    });
    let saved = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    let mut cold = Harness::new(60, provider.clone(), archives, false);
    let (target, mut packets) = cold.spawn(3, Some(saved));
    assert_eq!(cold.world().get::<Health>(target).unwrap().current, 7.0);
    let loaded = player_state::canonical_initial_record(cold.world(), target).unwrap();
    assert_eq!(loaded.health, 7.0);
    assert_eq!(loaded.inventory[0].count, 7);
    let (replacement, _unused) = tokio::sync::mpsc::channel(1);
    let original = std::mem::replace(
        &mut cold
            .world_mut()
            .get_mut::<PlayerSession>(target)
            .unwrap()
            .outbound_tx,
        replacement,
    );
    assert!(player_state::canonical_initial_record(cold.world(), target).is_none());
    cold.world_mut()
        .get_mut::<PlayerSession>(target)
        .unwrap()
        .outbound_tx = original;
    assert!(player_events::initial_state_pending(cold.world(), target));
    assert!(packets.try_recv().is_err());
    let base_count = cold
        .router
        .world(0)
        .unwrap()
        .initial_packet_batch(target)
        .unwrap()
        .packets
        .len();
    cold.until("cold:state-staged").await;
    let sender = cold
        .world()
        .get::<PlayerSession>(target)
        .unwrap()
        .outbound_tx
        .clone();
    let pressure = jolyne::valentine::McpePacket::from(crate::world::clock::clock_registry_packet(
        *cold.world().resource::<crate::world::rules::WorldTime>(),
    ));
    while sender.try_send(pressure.clone()).is_ok() {}
    provider.release.add_permits(1);
    cold.until("cold:accepted").await;
    for _ in 0..base_count {
        packets.try_recv().unwrap();
    }
    cold.router.tick().await.unwrap();
    assert!(player_events::initial_state_pending(cold.world(), target));
    assert!(!cold.logs().iter().any(|line| line == "cold:admitted"));
    while packets.try_recv().is_ok() {}
    cold.until("cold:admitted").await;
    assert!(!player_events::initial_state_pending(cold.world(), target));
    assert!(player_state::canonical_initial_record(cold.world(), target).is_none());
    assert_eq!(
        cold.world_mut()
            .query::<&Player>()
            .iter(cold.world())
            .count(),
        1
    );
    assert_eq!(cold.world().get::<Health>(target).unwrap().current, 7.0);
    assert_eq!(
        cold.world().get::<MainInventory>(target).unwrap().0.slots()[0].count,
        7
    );
    let (mut health, mut motion, mut position, mut effects, mut inventory) =
        (false, false, false, false, false);
    while let Ok(packet) = packets.try_recv() {
        let mut bytes = bytes::BytesMut::new();
        packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
        jolyne::valentine::BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap();
        match packet.data {
            jolyne::valentine::McpePacketData::UpdateAttributesPacket(value) => {
                health |= value
                    .attribute_list
                    .iter()
                    .any(|a| a.name == "minecraft:health" && a.current_value == 7.0)
            }
            jolyne::valentine::McpePacketData::MovePlayerPacket(_) => position = true,
            jolyne::valentine::McpePacketData::SetActorMotionPacket(_) => motion = true,
            jolyne::valentine::McpePacketData::MobEffectPacket(_) => effects = true,
            jolyne::valentine::McpePacketData::InventoryContentPacket(_) => inventory = true,
            _ => (),
        }
    }
    assert!(health && motion && position && effects && inventory);
    let current = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert!(current.inventory_revision > old_epoch);
    for (key, value) in old_receipts {
        assert_eq!(current.inventory_effects.get(&key), Some(&value));
    }
}

/// Produce real engine post-drop state without any module source ownership.
async fn native_terminal_for_cold(directory: &std::path::Path) -> (PlayerData, tempfile::TempDir) {
    let mut old = Harness::new(
        56,
        Arc::new(LevelDBPlayerProvider::open(directory).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    old.prepare_subject();
    old.retain().await;
    let source = old.subject.unwrap();
    assert!(
        crate::entity::player_life::hurt_player(
            old.world_mut(),
            source,
            &Hit {
                damage: 100.0,
                cause: ActorDamageCause::Override,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT
            }
        ) > 0.0
    );
    old.until("source:death").await;
    let saved = old.stored().await;
    assert!(saved.native_terminal.is_some());
    assert!(saved.source_actor.is_none());
    let Harness {
        router,
        manager,
        archives,
        ..
    } = old;
    drop(router);
    drop(manager);
    (saved, archives)
}

/// Stock cold hydration is silent behind rejection; explicit new-life initialization is separately durable.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_cold_native_terminal_hydrates_behind_rejection_then_owning_state_ack_initializes()
 {
    let directory = tempfile::tempdir().unwrap();
    let (saved, archives) = native_terminal_for_cold(directory.path()).await;
    let mut cold = Harness::new(
        61,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    let (rejected, mut packets) = cold.spawn(3, Some(saved.clone()));
    // The actual admission callback asserted raw zero health before rejecting;
    // ordinary native teardown then removes the unpublished replacement.
    assert!(cold.logs().iter().any(|line| line == "cold:rejected"));
    assert!(cold.world().get_entity(rejected).is_err());
    for _ in 0..3 {
        cold.router.tick().await.unwrap();
    }
    while let Ok(packet) = packets.try_recv() {
        assert!(!matches!(
            packet.data,
            jolyne::valentine::McpePacketData::UpdateAttributesPacket(_)
                | jolyne::valentine::McpePacketData::InventoryContentPacket(_)
                | jolyne::valentine::McpePacketData::MobEffectPacket(_)
                | jolyne::valentine::McpePacketData::SetActorMotionPacket(_)
                | jolyne::valentine::McpePacketData::MovePlayerPacket(_)
                | jolyne::valentine::McpePacketData::RespawnPacket(_)
        ));
    }
    let Harness {
        router,
        manager,
        archives,
        ..
    } = cold;
    drop(router);
    drop(manager);
    let mut cold = Harness::new(
        62,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    let (target, mut packets) = cold.spawn(3, Some(saved));
    assert!(packets.try_recv().is_err());
    assert_eq!(cold.world().get::<Health>(target).unwrap().current, 0.0);
    let loaded = player_state::canonical_initial_record(cold.world(), target).unwrap();
    assert_eq!(loaded.health, 0.0);
    assert!(loaded.native_terminal.is_some());
    cold.until("cold:admitted").await;
    assert!(player_state::canonical_initial_record(cold.world(), target).is_none());
    assert_eq!(cold.world().get::<Health>(target).unwrap().current, 5.0);
    assert_eq!(loaded.health, 0.0);
    assert!(
        cold.world()
            .get::<crate::entity::player_life::Dead>(target)
            .is_none()
    );
    assert!(
        cold.world()
            .get::<crate::entity::player_life::death::NativeDeathId>(target)
            .is_none()
    );
    assert!(
        cold.world()
            .get::<MainInventory>(target)
            .unwrap()
            .0
            .slots()
            .iter()
            .all(|item| item.is_empty())
    );
    let current = cold.stored().await;
    assert!(current.native_terminal.is_none());
    assert_eq!(current.health, 5.0);
    assert!(current.inventory.iter().all(|item| item.count == 0));
    let mut actual = false;
    while let Ok(packet) = packets.try_recv() {
        if let jolyne::valentine::McpePacketData::UpdateAttributesPacket(value) = packet.data {
            actual |= value
                .attribute_list
                .iter()
                .any(|a| a.name == "minecraft:health" && a.current_value == 5.0);
        }
    }
    assert!(actual);
}

/// Keeping a real dead state opens native respawn UI without silently creating a life.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_cold_dead_initialization_publishes_native_respawn_ui_without_reviving() {
    let directory = tempfile::tempdir().unwrap();
    let (saved, archives) = native_terminal_for_cold(directory.path()).await;
    let mut cold = Harness::new(
        63,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    let (target, mut packets) = cold.spawn(3, Some(saved));
    assert!(packets.try_recv().is_err());
    assert_eq!(cold.world().get::<Health>(target).unwrap().current, 0.0);
    cold.until("cold:admitted").await;
    assert!(
        cold.world()
            .get::<crate::entity::player_life::Dead>(target)
            .is_some()
    );
    assert_eq!(cold.world().get::<Health>(target).unwrap().current, 0.0);
    assert!(
        cold.world()
            .get::<crate::entity::player_life::death::NativeDeathId>(target)
            .is_none()
    );
    let mut respawn = Vec::new();
    let mut zero = false;
    while let Ok(packet) = packets.try_recv() {
        let mut bytes = bytes::BytesMut::new();
        packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
        jolyne::valentine::BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap();
        match packet.data {
            jolyne::valentine::McpePacketData::RespawnPacket(value) => respawn.push(value.state),
            jolyne::valentine::McpePacketData::UpdateAttributesPacket(value) => {
                zero |= value
                    .attribute_list
                    .iter()
                    .any(|a| a.name == "minecraft:health" && a.current_value == 0.0)
            }
            _ => (),
        }
    }
    assert!(zero);
    assert_eq!(
        respawn,
        [
            jolyne::valentine::EnumsPlayerRespawnState::SearchingForSpawn,
            jolyne::valentine::EnumsPlayerRespawnState::ReadyToSpawn
        ]
    );
    let current = cold.stored().await;
    assert_eq!(current.health, 0.0);
    assert!(
        current
            .restoration_checkpoint
            .as_ref()
            .unwrap()
            .supplement
            .dead
    );
}

/// Explicit retirement preserves current recovery values for a genuine fresh held connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_cold_retired_state_handoff_preserves_latest_native_values_and_receipts() {
    let directory = tempfile::tempdir().unwrap();
    let mut old = Harness::new(
        30,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    old.prepare_subject();
    old.retain().await;
    old.until("source:live-ack").await;
    let source = old.subject.unwrap();
    assert!(
        crate::entity::player_life::hurt_player(
            old.world_mut(),
            source,
            &Hit {
                damage: 2.0,
                cause: ActorDamageCause::Override,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT
            }
        ) > 0.0
    );
    let prior = old.stored().await;
    old.load(45);
    let (_target, mut abandoned) = old.spawn(2, Some(prior));
    old.until("rebind:retired").await;
    assert!(old.world().get_entity(source).is_err());
    while let Ok(packet) = abandoned.try_recv() {
        assert!(!matches!(
            packet.data,
            jolyne::valentine::McpePacketData::UpdateAttributesPacket(_)
                | jolyne::valentine::McpePacketData::InventoryContentPacket(_)
                | jolyne::valentine::McpePacketData::MobEffectPacket(_)
                | jolyne::valentine::McpePacketData::SetActorMotionPacket(_)
                | jolyne::valentine::McpePacketData::MovePlayerPacket(_)
        ));
    }
    let saved = old.stored().await;
    assert_eq!(saved.health, 7.0);
    assert_eq!(
        saved.source_actor.as_ref().unwrap().phase,
        crate::storage::source::SourcePhase::Retired
    );
    let receipts = saved.inventory_effects.clone();
    let Harness {
        router,
        manager,
        archives,
        ..
    } = old;
    drop(router);
    drop(manager);
    let mut cold = Harness::new(
        64,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    let (target, mut packets) = cold.spawn(3, Some(saved));
    assert_eq!(cold.world().get::<Health>(target).unwrap().current, 7.0);
    assert!(player_state::source_actors::admission_blocked(
        cold.world(),
        target
    ));
    assert!(packets.try_recv().is_err());
    cold.until("cold:admitted").await;
    assert!(!player_state::source_actors::admission_blocked(
        cold.world(),
        target
    ));
    assert_eq!(cold.world().get::<Health>(target).unwrap().current, 7.0);
    assert_eq!(
        cold.world().get::<MainInventory>(target).unwrap().0.slots()[0].count,
        7
    );
    assert_eq!(
        cold.world()
            .get::<crate::entity::components::EnderChestInventory>(target)
            .unwrap()
            .0
            .slots()[2]
            .count,
        7
    );
    assert_eq!(
        cold.world_mut()
            .query::<&Player>()
            .iter(cold.world())
            .count(),
        1
    );
    let current = cold.stored().await;
    assert_eq!(
        current.source_actor.as_ref().unwrap().phase,
        crate::storage::source::SourcePhase::Transferred
    );
    for (key, receipt) in receipts {
        assert_eq!(current.inventory_effects.get(&key), Some(&receipt));
    }
}
