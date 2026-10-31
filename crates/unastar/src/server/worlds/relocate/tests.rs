//! Actual native relocation, policy, transport and durable failure boundaries.

use super::*;
use crate::entity::components::{GameMode, MainInventory};
use crate::entity::projectile::hooks::{PlayerTeleport, ProjectileHandler, ProjectileHooks};
use crate::server::game::{PlayerSpawnData, ServerConfig, types};
use crate::storage::{PlayerData, PlayerProvider};
use crate::world::WorldConfig;
use jolyne::valentine::{McpePacket, McpePacketData};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::mpsc;

#[derive(Default)]
struct Provider {
    record: Mutex<Option<PlayerData>>,
    writes: AtomicUsize,
    ambiguous: bool,
    pressure_probe: Mutex<Option<mpsc::Sender<McpePacket>>>,
    reservation_held: AtomicBool,
}
#[async_trait::async_trait]
impl PlayerProvider for Provider {
    fn atomic_durable_records(&self) -> bool {
        true
    }
    async fn load(&self, _: uuid::Uuid) -> crate::storage::StorageResult<Option<PlayerData>> {
        Ok(self.record.lock().unwrap().clone())
    }
    async fn save(&self, _: uuid::Uuid, data: &PlayerData) -> crate::storage::StorageResult<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        *self.record.lock().unwrap() = Some(data.clone());
        if let Some(sender) = self.pressure_probe.lock().unwrap().as_ref() {
            self.reservation_held.store(
                matches!(
                    sender.try_send(game::types::system_text("concurrent traffic").into()),
                    Err(mpsc::error::TrySendError::Full(_))
                ),
                Ordering::SeqCst,
            );
        }
        if self.ambiguous {
            Err(crate::storage::StorageError::Database(
                "write acknowledgement unavailable".into(),
            ))
        } else {
            Ok(())
        }
    }
    async fn delete(&self, _: uuid::Uuid) -> crate::storage::StorageResult<()> {
        Ok(())
    }
    async fn close(&self) -> crate::storage::StorageResult<()> {
        Ok(())
    }
}

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<(Vec<PlayerRelocationCommitted>, Vec<PlayerRelocationFailed>)>>);
impl RelocationHandler for Trace {
    fn on_committed(&self, world: &mut World, event: &PlayerRelocationCommitted) {
        assert_eq!(
            world.get::<Position>(event.player).unwrap().0,
            event.feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT
        );
        self.0.lock().unwrap().0.push(event.clone());
    }
    fn on_failed(&self, _: &mut World, event: &PlayerRelocationFailed) {
        self.0.lock().unwrap().1.push(event.clone());
    }
}

/// Spawn one real identified player with a durable account provider and clear initial packets.
fn fixture(
    provider: Arc<dyn PlayerProvider>,
) -> (GameServer, Entity, mpsc::Receiver<McpePacket>, Trace) {
    let mut server = GameServer::with_config(ServerConfig {
        world: WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    server
        .ecs
        .world_mut()
        .insert_resource(types::PlayerProviderResource(provider));
    let (tx, mut rx) = mpsc::channel(128);
    let player = server
        .spawn_player(PlayerSpawnData {
            session_id: 7,
            display_name: "actor".into(),
            xuid: Some("123".into()),
            uuid: Some(uuid::Uuid::from_u128(7).to_string()),
            runtime_id: 77,
            position: DVec3::new(4.5, 17.62, 4.5),
            saved_data: None,
            outbound_tx: tx,
            chunk_radius: 0,
        })
        .unwrap();
    while rx.try_recv().is_ok() {}
    let trace = Trace::default();
    server.ecs.world_mut().init_resource::<RelocationHooks>();
    server
        .ecs
        .world_mut()
        .resource_mut::<RelocationHooks>()
        .register(trace.clone());
    (server, player, rx, trace)
}

/// Execute the queued native boundary directly, keeping the fixture independent of periodic simulation.
async fn run(server: &mut GameServer) {
    let request = server
        .ecs
        .world_mut()
        .resource_mut::<RelocationQueue>()
        .0
        .remove(0);
    let player = request.player;
    server
        .ecs
        .world_mut()
        .get_resource_or_init::<ActiveRelocations>()
        .0
        .insert(player);
    attempt(server, request).await;
    server
        .ecs
        .world_mut()
        .resource_mut::<ActiveRelocations>()
        .0
        .remove(&player);
}

/// Queue one safe position on the native platform.
fn request(server: &mut GameServer, player: Entity) {
    request_relocation_checked(
        server.ecs.world_mut(),
        player,
        DVec3::new(8.5, 16.0, 8.5),
        0,
        "move-1".into(),
        |_, _| true,
    )
    .unwrap();
}

struct Rewrite;
impl ProjectileHandler for Rewrite {
    fn on_teleport(&self, _: &mut World, event: &mut PlayerTeleport) {
        assert_eq!(event.request_id(), Some("move-1"));
        event.destination.x = 9.5;
    }
}

#[tokio::test]
async fn actual_relocation_saves_exact_rewrite_items_and_base_mode_before_same_actor_commit() {
    let provider = Arc::new(Provider::default());
    let (mut server, player, mut rx, trace) = fixture(provider.clone());
    let world = server.ecs.world_mut();
    let diamond = world
        .resource::<types::ItemRegistryResource>()
        .0
        .stack("minecraft:diamond", 3)
        .unwrap();
    world
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, diamond)
        .unwrap();
    assert!(crate::command::exec::set_mode_override(
        world,
        player,
        Some(GameMode::Creative)
    ));
    while rx.try_recv().is_ok() {}
    world.init_resource::<ProjectileHooks>();
    world.resource_mut::<ProjectileHooks>().register(Rewrite);
    request(&mut server, player);
    run(&mut server).await;
    let saved = provider.record.lock().unwrap().clone().unwrap();
    assert_eq!(saved.position, [9.5, 17.62, 8.5]);
    assert_eq!(saved.game_mode, 0);
    assert_eq!(saved.inventory[0].count, 3);
    assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
    let events = trace.0.lock().unwrap();
    assert_eq!(events.0.len(), 1);
    assert!(events.1.is_empty());
    assert_eq!(events.0[0].player, player);
    assert_eq!(events.0[0].identity().session_id(), 7);
    assert!(
        rx.try_recv()
            .is_ok_and(|packet| matches!(packet.data, McpePacketData::MovePlayerPacket(_)))
    );
}

struct Veto;
impl ProjectileHandler for Veto {
    fn on_teleport(&self, _: &mut World, event: &mut PlayerTeleport) {
        event.cancel();
    }
}
#[tokio::test]
async fn ordinary_native_teleport_veto_spends_no_record_and_notifies_only_failure() {
    let provider = Arc::new(Provider::default());
    let (mut server, player, mut rx, trace) = fixture(provider.clone());
    let before = server.ecs.world().get::<Position>(player).unwrap().0;
    server.ecs.world_mut().init_resource::<ProjectileHooks>();
    server
        .ecs
        .world_mut()
        .resource_mut::<ProjectileHooks>()
        .register(Veto);
    request(&mut server, player);
    run(&mut server).await;
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    assert_eq!(
        server.ecs.world().get::<Position>(player).unwrap().0,
        before
    );
    assert!(rx.try_recv().is_err());
    let events = trace.0.lock().unwrap();
    assert!(events.0.is_empty());
    assert_eq!(events.1[0].error, TransferError::Rejected);
}

struct AlterInventory;
impl ProjectileHandler for AlterInventory {
    fn on_teleport(&self, world: &mut World, event: &mut PlayerTeleport) {
        let diamond = world
            .resource::<types::ItemRegistryResource>()
            .0
            .stack("minecraft:diamond", 1)
            .unwrap();
        world
            .get_mut::<MainInventory>(event.player())
            .unwrap()
            .0
            .set_item(0, diamond)
            .unwrap();
    }
}
#[tokio::test]
async fn callback_inventory_change_is_preserved_and_rejects_the_stale_move() {
    let provider = Arc::new(Provider::default());
    let (mut server, player, _rx, trace) = fixture(provider.clone());
    server.ecs.world_mut().init_resource::<ProjectileHooks>();
    server
        .ecs
        .world_mut()
        .resource_mut::<ProjectileHooks>()
        .register(AlterInventory);
    request(&mut server, player);
    run(&mut server).await;
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    assert_eq!(
        server
            .ecs
            .world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        1
    );
    assert_eq!(
        trace.0.lock().unwrap().1[0].error,
        TransferError::StaleSource
    );
}

#[tokio::test]
async fn ambiguous_acknowledgement_fences_source_without_erasing_possible_destination_record() {
    let provider = Arc::new(Provider {
        ambiguous: true,
        ..Default::default()
    });
    let (mut server, player, mut rx, trace) = fixture(provider.clone());
    request(&mut server, player);
    run(&mut server).await;
    assert_eq!(
        provider.record.lock().unwrap().as_ref().unwrap().position,
        [8.5, 17.62, 8.5]
    );
    assert!(inventory_effects::is_recovery_fenced(
        server.ecs.world(),
        player
    ));
    assert!(!server.save_player(7).await);
    assert!(trace.0.lock().unwrap().0.is_empty());
    assert_eq!(
        trace.0.lock().unwrap().1[0].error,
        TransferError::RecoveryRequired
    );
    assert!(
        !std::iter::from_fn(|| rx.try_recv().ok())
            .any(|packet| matches!(packet.data, McpePacketData::MovePlayerPacket(_)))
    );
}

#[tokio::test]
async fn reserved_transport_slot_survives_storage_work_and_delivers_the_actual_move() {
    let provider = Arc::new(Provider::default());
    let (mut server, player, mut rx, trace) = fixture(provider.clone());
    let sender = server
        .ecs
        .world()
        .get::<PlayerSession>(player)
        .unwrap()
        .outbound_tx
        .clone();
    for _ in 0..127 {
        sender
            .try_send(game::types::system_text("queued").into())
            .unwrap();
    }
    *provider.pressure_probe.lock().unwrap() = Some(sender);
    request(&mut server, player);
    run(&mut server).await;
    assert!(provider.reservation_held.load(Ordering::SeqCst));
    assert_eq!(trace.0.lock().unwrap().0.len(), 1);
    assert_eq!(
        std::iter::from_fn(|| rx.try_recv().ok())
            .filter(|packet| matches!(packet.data, McpePacketData::MovePlayerPacket(_)))
            .count(),
        1
    );
}

#[tokio::test]
async fn real_leveldb_reopen_restores_the_accepted_same_world_destination() {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap());
    let (mut server, player, _rx, trace) = fixture(provider.clone());
    request(&mut server, player);
    run(&mut server).await;
    assert_eq!(trace.0.lock().unwrap().0.len(), 1);
    drop(server);
    drop(provider);
    let reopened = crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap();
    assert_eq!(
        reopened
            .load(uuid::Uuid::from_u128(7))
            .await
            .unwrap()
            .unwrap()
            .position,
        [8.5, 17.62, 8.5]
    );
}

#[tokio::test]
async fn same_world_and_dimension_moves_cannot_queue_over_each_other() {
    let provider = Arc::new(Provider::default());
    let (mut server, player, _rx, _trace) = fixture(provider);
    request(&mut server, player);
    assert_eq!(
        super::super::request_transfer_checked(
            server.ecs.world_mut(),
            player,
            1,
            DVec3::new(4.5, 16.0, 4.5),
            0,
            "cross".into(),
            |_, _, _| true
        ),
        Err(TransferError::Pending)
    );
}

#[tokio::test]
async fn router_tick_executes_the_real_public_relocation_queue() {
    let provider = Arc::new(Provider::default());
    let (server, player, _rx, trace) = fixture(provider.clone());
    let mut router = super::super::DimensionRouter::new(server);
    request(router.world_mut(0).unwrap(), player);
    router.tick().await.unwrap();
    assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
    assert_eq!(trace.0.lock().unwrap().0.len(), 1);
    assert_eq!(router.locate_session(7), Some((0, player)));
}

#[tokio::test]
async fn transport_backpressure_retries_without_authorizing_or_writing() {
    let provider = Arc::new(Provider::default());
    let (mut server, player, mut rx, trace) = fixture(provider.clone());
    let sender = server
        .ecs
        .world()
        .get::<PlayerSession>(player)
        .unwrap()
        .outbound_tx
        .clone();
    for _ in 0..128 {
        sender
            .try_send(game::types::system_text("queued").into())
            .unwrap();
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let validator_calls = calls.clone();
    request_relocation_checked(
        server.ecs.world_mut(),
        player,
        DVec3::new(8.5, 16.0, 8.5),
        0,
        "busy".into(),
        move |_, _| {
            validator_calls.fetch_add(1, Ordering::SeqCst);
            true
        },
    )
    .unwrap();
    run(&mut server).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    assert!(trace.0.lock().unwrap().1.is_empty());
    while rx.try_recv().is_ok() {}
    run(&mut server).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
    assert_eq!(trace.0.lock().unwrap().0.len(), 1);
}

struct UnsafeRewrite;
impl ProjectileHandler for UnsafeRewrite {
    fn on_teleport(&self, _: &mut World, event: &mut PlayerTeleport) {
        event.destination.y = 15.0;
    }
}
#[tokio::test]
async fn rewritten_solid_geometry_is_rejected_before_application_authorization() {
    let provider = Arc::new(Provider::default());
    let (mut server, player, _rx, trace) = fixture(provider.clone());
    server.ecs.world_mut().init_resource::<ProjectileHooks>();
    server
        .ecs
        .world_mut()
        .resource_mut::<ProjectileHooks>()
        .register(UnsafeRewrite);
    let calls = Arc::new(AtomicUsize::new(0));
    let validator_calls = calls.clone();
    request_relocation_checked(
        server.ecs.world_mut(),
        player,
        DVec3::new(8.5, 16.0, 8.5),
        0,
        "unsafe".into(),
        move |_, _| {
            validator_calls.fetch_add(1, Ordering::SeqCst);
            true
        },
    )
    .unwrap();
    run(&mut server).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    assert_eq!(
        trace.0.lock().unwrap().1[0].error,
        TransferError::UnsafeDestination
    );
}

struct Context(Arc<Mutex<Vec<Option<String>>>>);
impl ProjectileHandler for Context {
    fn on_teleport(&self, world: &mut World, event: &mut PlayerTeleport) {
        self.0
            .lock()
            .unwrap()
            .push(event.request_id().map(str::to_owned));
        if event.request_id().is_some() {
            assert_eq!(
                request_relocation_checked(
                    world,
                    event.player(),
                    DVec3::new(9.5, 16.0, 9.5),
                    0,
                    "nested".into(),
                    |_, _| true
                ),
                Err(TransferError::Pending)
            );
        }
    }
}
#[tokio::test]
async fn queued_context_cannot_leak_to_an_ordinary_item_teleport_or_nested_request() {
    let provider = Arc::new(Provider::default());
    let (mut server, player, _rx, _trace) = fixture(provider);
    let contexts = Arc::new(Mutex::new(Vec::new()));
    server.ecs.world_mut().init_resource::<ProjectileHooks>();
    server
        .ecs
        .world_mut()
        .resource_mut::<ProjectileHooks>()
        .register(Context(contexts.clone()));
    request(&mut server, player);
    run(&mut server).await;
    assert!(item_use::teleport_from(
        server.ecs.world_mut(),
        player,
        None,
        DVec3::new(4.5, 16.0, 4.5),
        item_use::TeleportCause::Projectile
    ));
    assert_eq!(*contexts.lock().unwrap(), vec![Some("move-1".into()), None]);
}
