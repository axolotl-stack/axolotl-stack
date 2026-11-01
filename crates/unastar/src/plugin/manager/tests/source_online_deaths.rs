//! Genuine online deaths share the canonical post-drop barrier without module ownership.
use super::*;

/// Read actual native drops rather than an archive's prospective carried contents.
fn drops(h: &mut Harness) -> usize {
    h.world_mut()
        .query::<&crate::entity::components::DroppedItem>()
        .iter(h.world())
        .count()
}

/// Drain only this real connection's owned outbound channel.
fn packets(h: &mut Harness) -> Vec<jolyne::valentine::McpePacket> {
    let rx = h.subject_outbound.as_mut().unwrap();
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

/// Observe the physical fatal transition while provider acknowledgement remains withheld.
async fn fatal_pending(h: &mut Harness, provider: &SourceAckGate) -> uuid::Uuid {
    let actor = h.subject.unwrap();
    // This control exercises an admitted live connection, not death during bootstrap.
    for _ in 0..100 {
        if h.world()
            .get::<player_events::InitialStatePublished>(actor)
            .is_some()
        {
            break;
        }
        h.router.tick().await.unwrap();
    }
    assert!(
        h.world()
            .get::<player_events::InitialStatePublished>(actor)
            .is_some()
    );
    assert!(!crate::plugin::manager::sessions::blocks_publication(
        h.world(),
        actor
    ));
    let _ = packets(h);
    assert!(
        crate::entity::player_life::hurt_player(
            h.world_mut(),
            actor,
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
    let id = h
        .world()
        .get::<crate::entity::player_life::death::NativeDeathId>(actor)
        .unwrap()
        .0;
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        assert!(!h.logs().iter().any(|line| line == "source:death"));
        assert_eq!(drops(h), 0);
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(provider.written.load(std::sync::atomic::Ordering::Acquire));
    assert!(h.world().get::<PlayerSession>(actor).is_some());
    assert!(
        h.world()
            .get::<crate::server::game::player_events::DetachedPlayer>(actor)
            .is_none()
    );
    assert!(player_state::quarantine::is_held(h.world(), actor));
    crate::entity::player_life::respawn::respawn(h.world_mut(), actor);
    assert!(
        h.world()
            .get::<crate::entity::player_life::Dead>(actor)
            .is_some()
    );
    assert!(!h.router.world_mut(0).unwrap().save_player(1).await);
    for packet in packets(h) {
        match packet.data {
            jolyne::valentine::McpePacketData::DeathInfoPacket(_)
            | jolyne::valentine::McpePacketData::RespawnPacket(_) => {
                panic!("death publication preceded durable acknowledgement")
            }
            jolyne::valentine::McpePacketData::ActorEventPacket(value) => {
                assert_ne!(value.event_id, jolyne::valentine::EnumsActorEvent::Death)
            }
            _ => (),
        }
    }
    id
}

/// Both native respawn modes must clear a witness only after its acknowledged death.
async fn online_respawn(immediate: bool) {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(0),
    });
    let mut h = Harness::new(56, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    h.world_mut()
        .resource_mut::<crate::world::rules::GameRules>()
        .do_immediate_respawn = immediate;
    let actor = h.subject.unwrap();
    let death = fatal_pending(&mut h, &provider).await;
    let before = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        before.native_terminal.as_ref().unwrap().death,
        death.to_string()
    );
    assert!(before.source_actor.is_none());
    assert_eq!(before.health, 0.0);
    assert!(before.inventory.iter().all(|stack| stack.count == 0));
    assert_eq!(before.ender_chest[2].count, 7);
    // Changes during I/O cannot rewrite the original drop disposition.
    h.world_mut()
        .resource_mut::<crate::world::rules::GameRules>()
        .keep_inventory = true;
    provider.release.add_permits(1);
    h.until("source:death").await;
    assert_eq!(drops(&mut h), 1);
    assert!(
        h.world()
            .get::<player_state::source_actors::ManagedSource>(actor)
            .is_none()
    );
    assert!(!player_state::quarantine::is_held(h.world(), actor));
    let output = packets(&mut h);
    assert!(output.iter().any(|packet| matches!(
        packet.data,
        jolyne::valentine::McpePacketData::DeathInfoPacket(_)
    )));
    for packet in output {
        let mut bytes = bytes::BytesMut::new();
        packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
        jolyne::valentine::BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap();
    }
    if !immediate {
        assert_eq!(h.world().get::<Health>(actor).unwrap().current, 0.0);
        assert!(!h.router.world_mut(0).unwrap().save_player(1).await);
        let still = provider
            .inner
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .unwrap();
        assert!(still == before);
        crate::entity::player_life::respawn::respawn(h.world_mut(), actor);
    }
    assert!(
        h.world()
            .get::<crate::entity::player_life::Dead>(actor)
            .is_none()
    );
    assert!(
        h.world()
            .get::<crate::entity::player_life::death::NativeDeathId>(actor)
            .is_none()
    );
    assert!(
        h.world()
            .get::<player_state::source_actors::unowned_terminal::Witness>(actor)
            .is_none()
    );
    assert!(h.world().get::<Health>(actor).unwrap().current > 0.0);
    provider.release.add_permits(1);
    assert!(h.router.world_mut(0).unwrap().save_player(1).await);
    let current = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert!(current.native_terminal.is_none());
    assert!(current.restoration_checkpoint.is_none());
    assert_eq!(current.inventory_effects, before.inventory_effects);
    assert!(current.health > 0.0);
    assert!(current.inventory.iter().all(|stack| stack.count == 0));
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    drop(provider);
    let mut cold = Harness::new(
        56,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    let (fresh, _) = cold.spawn(3, Some(current));
    assert!(cold.world().get::<Health>(fresh).unwrap().current > 0.0);
    assert!(
        cold.world()
            .get::<crate::entity::player_life::Dead>(fresh)
            .is_none()
    );
    assert_eq!(
        cold.world_mut()
            .query::<&Player>()
            .iter(cold.world())
            .count(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_online_death_waits_for_canonical_ack_then_native_manual_respawn_and_cold_save()
 {
    online_respawn(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_online_death_waits_for_canonical_ack_then_native_immediate_respawn_and_cold_save()
 {
    online_respawn(true).await;
}

/// Losing the live world after a real write cannot turn a historical live value into recovery state.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_online_death_world_closure_preserves_real_postdrop_canonical_proof_without_notification()
 {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(0),
    });
    let mut h = Harness::new(56, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    let death = fatal_pending(&mut h, &provider).await;
    let Harness {
        router, manager, ..
    } = h;
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
    let reopened = LevelDBPlayerProvider::open(directory.path()).unwrap();
    let current = reopened
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        current.native_terminal.as_ref().unwrap().death,
        death.to_string()
    );
    assert!(current.source_actor.is_none());
    assert_eq!(current.health, 0.0);
    assert!(current.inventory.iter().all(|stack| stack.count == 0));
    assert_eq!(current.ender_chest[2].count, 7);
    assert!(current.restoration_checkpoint.unwrap().supplement.dead);
}

/// A provider failure cannot acknowledge death, release the corpse or publish prospective drops.
struct RejectTerminalWrites {
    inner: Arc<LevelDBPlayerProvider>,
    attempted: std::sync::atomic::AtomicBool,
}
#[async_trait::async_trait]
impl PlayerProvider for RejectTerminalWrites {
    fn atomic_durable_records(&self) -> bool {
        true
    }
    async fn load(&self, uuid: uuid::Uuid) -> crate::storage::StorageResult<Option<PlayerData>> {
        self.inner.load(uuid).await
    }
    async fn save(
        &self,
        _uuid: uuid::Uuid,
        _data: &PlayerData,
    ) -> crate::storage::StorageResult<()> {
        self.attempted
            .store(true, std::sync::atomic::Ordering::Release);
        Err(crate::storage::StorageError::Database(
            "controlled terminal storage failure".into(),
        ))
    }
    async fn delete(&self, uuid: uuid::Uuid) -> crate::storage::StorageResult<()> {
        self.inner.delete(uuid).await
    }
    async fn close(&self) -> crate::storage::StorageResult<()> {
        self.inner.close().await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_online_death_storage_failure_keeps_corpse_and_account_withheld_without_drops()
 {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(RejectTerminalWrites {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        attempted: std::sync::atomic::AtomicBool::new(false),
    });
    let mut h = Harness::new(56, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    let actor = h.subject.unwrap();
    let _ = packets(&mut h);
    assert!(
        crate::entity::player_life::hurt_player(
            h.world_mut(),
            actor,
            &Hit {
                damage: 100.0,
                cause: ActorDamageCause::Override,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
            }
        ) > 0.0
    );
    for _ in 0..100 {
        h.router.tick().await.unwrap();
        assert!(!h.logs().iter().any(|line| line == "source:death"));
        if provider
            .attempted
            .load(std::sync::atomic::Ordering::Acquire)
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(
        provider
            .attempted
            .load(std::sync::atomic::Ordering::Acquire)
    );
    for _ in 0..3 {
        h.router.tick().await.unwrap();
    }
    assert!(player_state::quarantine::is_held(h.world(), actor));
    assert!(h.world().get::<PlayerSession>(actor).is_some());
    assert!(
        h.world()
            .get::<crate::entity::player_life::Dead>(actor)
            .is_some()
    );
    assert!(
        h.world()
            .get::<player_state::source_actors::unowned_terminal::Witness>(actor)
            .is_none()
    );
    assert_eq!(drops(&mut h), 0);
    assert!(
        provider
            .inner
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .is_none()
    );
    assert!(!h.router.world_mut(0).unwrap().save_player(1).await);
    crate::entity::player_life::respawn::respawn(h.world_mut(), actor);
    assert!(
        h.world()
            .get::<crate::entity::player_life::Dead>(actor)
            .is_some()
    );
    let (outbound_tx, _rx) = tokio::sync::mpsc::channel(32);
    let result =
        h.router
            .world_mut(0)
            .unwrap()
            .spawn_player(crate::server::game::PlayerSpawnData {
                session_id: 2,
                runtime_id: 2,
                display_name: "Player2".into(),
                xuid: Some("1".into()),
                uuid: Some(uuid::Uuid::from_u128(1).to_string()),
                position: glam::DVec3::new(0.5, 17.62, 0.5),
                outbound_tx,
                saved_data: None,
                chunk_radius: 0,
            });
    assert!(result.is_err());
    assert_eq!(h.world_mut().query::<&Player>().iter(h.world()).count(), 1);
    for packet in packets(&mut h) {
        assert!(!matches!(
            packet.data,
            jolyne::valentine::McpePacketData::DeathInfoPacket(_)
                | jolyne::valentine::McpePacketData::RespawnPacket(_)
        ));
    }
}
