//! Resource lifetime and native identity boundaries, independent of guest-language wrappers.

use super::super::{
    create_store,
    wit_bindings::unastar::plugin::types::{HostPlayer, HostPlayerHandle, HostTask, HostWorldTx},
    with_world,
};
use super::*;
use crate::entity::components::*;
use crate::plugin::manifest::{PluginCapability, PluginId, PluginLimits};
use wasmtime::{Engine, Store};
use wasmtime_wasi::WasiCtx;

/// Minimal complete actor with an owned transport and an actual world identity.
fn world() -> (
    World,
    Entity,
    tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
) {
    let mut world = World::new();
    world.insert_resource(crate::world::ChunkManager::new(
        crate::world::WorldConfig::void_spawn_platform(1),
    ));
    world.insert_resource(crate::ecs::TickCounter { current: 42 });
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    let id = uuid::Uuid::from_u128(42);
    let player = world
        .spawn((
            Player,
            PlayerUuid(id),
            RuntimeEntityId(42),
            PlayerName("Reader".into()),
            PlayerSession::new(
                7,
                "Reader".into(),
                Some("42".into()),
                Some(id.to_string()),
                tx,
            ),
            Position(glam::DVec3::new(1.0, 65.62, 3.0)),
            Rotation::default(),
            Health::default(),
            PlayerState::default(),
            GameMode::Creative,
        ))
        .id();
    (world, player, rx)
}
/// Use the real bounded host context rather than a permissive fake player implementation.
fn store(limits: PluginLimits) -> Store<HostContext> {
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true).epoch_interruption(true);
    let engine = Engine::new(&config).unwrap();
    let mut store = create_store(
        &engine,
        WasiCtx::builder().build(),
        PluginId("example.reader".into()),
        &limits,
    )
    .unwrap();
    store.data_mut().capabilities = [
        PluginCapability::ReadPlayers,
        PluginCapability::SendMessages,
        PluginCapability::ScheduleTasks,
    ]
    .into();
    store
}

#[test]
fn owned_live_player_expires_but_owned_reference_resolves_in_a_new_transaction() {
    let (mut world, player, _rx) = world();
    let mut store = store(PluginLimits::default());
    let (live, handle) = with_world(&mut store, &mut world, |store| {
        let ctx = store.data_mut();
        let borrowed = ctx.borrow_player(player).unwrap();
        let handle = HostPlayer::to_handle(ctx, Resource::new_borrow(borrowed.rep())).unwrap();
        let tx = ctx.borrow_transaction().unwrap();
        let live = HostWorldTx::player(
            ctx,
            Resource::new_borrow(tx.rep()),
            Resource::new_borrow(handle.rep()),
        )
        .unwrap();
        assert_eq!(
            HostPlayer::health(ctx, Resource::new_borrow(live.rep())).unwrap(),
            20.0
        );
        (live.rep(), handle.rep())
    });
    with_world(&mut store, &mut world, |store| {
        let ctx = store.data_mut();
        assert_eq!(
            HostPlayer::health(ctx, Resource::new_borrow(live)),
            Err(wit::AccessError::ExpiredScope)
        );
        let tx = ctx.borrow_transaction().unwrap();
        let next = HostWorldTx::player(
            ctx,
            Resource::new_borrow(tx.rep()),
            Resource::new_borrow(handle),
        )
        .unwrap();
        assert_ne!(next.rep(), live);
        assert_eq!(
            HostPlayer::game_mode(ctx, Resource::new_borrow(next.rep())).unwrap(),
            wit::GameMode::Creative
        );
    });
    assert!(store.data().world_ptr.is_null());
}

#[test]
fn exact_channel_replacement_never_inherits_a_retained_reference() {
    let (mut world, player, _rx) = world();
    let mut store = store(PluginLimits::default());
    let handle = with_world(&mut store, &mut world, |store| {
        let ctx = store.data_mut();
        let borrowed = ctx.borrow_player(player).unwrap();
        HostPlayer::to_handle(ctx, Resource::new_borrow(borrowed.rep()))
            .unwrap()
            .rep()
    });
    let (tx, _other_rx) = tokio::sync::mpsc::channel(8);
    world.entity_mut(player).insert(PlayerSession::new(
        7,
        "Reader".into(),
        Some("42".into()),
        Some(uuid::Uuid::from_u128(42).to_string()),
        tx,
    ));
    with_world(&mut store, &mut world, |store| {
        let ctx = store.data_mut();
        let tx = ctx.borrow_transaction().unwrap();
        assert!(matches!(
            HostWorldTx::player(
                ctx,
                Resource::new_borrow(tx.rep()),
                Resource::new_borrow(handle)
            ),
            Err(wit::AccessError::StalePlayer)
        ));
        assert!(matches!(
            HostPlayerHandle::snapshot(ctx, Resource::new_borrow(handle)),
            Err(wit::AccessError::StalePlayer)
        ));
    });
}

#[test]
fn snapshots_report_actual_feet_modes_and_missing_state_without_defaults() {
    let (mut world, player, _rx) = world();
    let mut store = store(PluginLimits::default());
    with_world(&mut store, &mut world, |store| {
        let ctx = store.data_mut();
        let borrowed = ctx.borrow_player(player).unwrap();
        let snapshot = HostPlayer::snapshot(ctx, Resource::new_borrow(borrowed.rep())).unwrap();
        assert_eq!(snapshot.identity.authenticated_xuid, Some(42));
        assert!((snapshot.position.y - 64.0).abs() < 0.000001);
        assert_eq!(snapshot.game_mode, wit::GameMode::Creative);
        assert_eq!(snapshot.effective_game_mode, wit::GameMode::Creative);
        assert_eq!(snapshot.flags.on_ground, None);
        assert!(!snapshot.configured_admin);
    });
    world.entity_mut(player).remove::<Health>();
    with_world(&mut store, &mut world, |store| {
        let ctx = store.data_mut();
        let borrowed = ctx.borrow_player(player).unwrap();
        assert_eq!(
            HostPlayer::availability(ctx, Resource::new_borrow(borrowed.rep())).unwrap(),
            wit::PlayerAvailability::Incomplete
        );
        assert!(matches!(
            HostPlayer::health(ctx, Resource::new_borrow(borrowed.rep())),
            Err(wit::AccessError::Unavailable)
        ));
    });
}

#[test]
fn callback_failure_rolls_back_messages_and_tasks_and_exact_binding_is_revalidated() {
    let (mut world, player, mut rx) = world();
    let mut store = store(PluginLimits::default());
    let task = with_world(&mut store, &mut world, |store| {
        let ctx = store.data_mut();
        let borrowed = ctx.borrow_player(player).unwrap();
        HostPlayer::message(ctx, Resource::new_borrow(borrowed.rep()), "queued".into()).unwrap();
        let handle = HostPlayer::to_handle(ctx, Resource::new_borrow(borrowed.rep())).unwrap();
        HostPlayerHandle::schedule(ctx, Resource::new_borrow(handle.rep()), 9)
            .unwrap()
            .rep()
    });
    finish_invocation(store.data_mut(), &mut world, false);
    assert!(rx.try_recv().is_err());
    assert!(store.data().task_requests.is_empty());
    assert!(matches!(
        HostTask::status(store.data_mut(), Resource::new_borrow(task)).unwrap(),
        wit::TaskStatus::Failed(wit::AccessError::Cancelled)
    ));
    with_world(&mut store, &mut world, |store| {
        let ctx = store.data_mut();
        let borrowed = ctx.borrow_player(player).unwrap();
        HostPlayer::message(
            ctx,
            Resource::new_borrow(borrowed.rep()),
            "committed".into(),
        )
        .unwrap();
    });
    finish_invocation(store.data_mut(), &mut world, true);
    assert!(rx.try_recv().is_ok());
    assert!(rx.try_recv().is_err());
    with_world(&mut store, &mut world, |store| {
        let ctx = store.data_mut();
        let borrowed = ctx.borrow_player(player).unwrap();
        HostPlayer::message(ctx, Resource::new_borrow(borrowed.rep()), "stale".into()).unwrap();
    });
    let (tx, mut replacement) = tokio::sync::mpsc::channel(8);
    world.entity_mut(player).insert(PlayerSession::new(
        7,
        "Reader".into(),
        Some("42".into()),
        Some(uuid::Uuid::from_u128(42).to_string()),
        tx,
    ));
    finish_invocation(store.data_mut(), &mut world, true);
    assert!(rx.try_recv().is_err());
    assert!(replacement.try_recv().is_err());
}

#[test]
fn resource_and_scope_overflow_fail_closed_without_aliasing() {
    let (world, _player, _rx) = world();
    let mut resources = PlayerResources {
        next: u32::MAX,
        ..Default::default()
    };
    resources.begin_scope(&world);
    let scope = resources.scope.unwrap();
    assert_eq!(
        resources.allocate(
            Entry::WorldTx {
                scope,
                transient: false
            },
            32
        ),
        Err(wit::AccessError::LimitExceeded)
    );
    resources.epoch = u64::MAX;
    resources.begin_scope(&world);
    assert!(resources.scope.is_none());
    assert_eq!(resources.scope(scope), Err(wit::AccessError::ExpiredScope));
}

#[test]
fn capability_and_shared_operation_limits_apply_to_retained_resources() {
    let (mut world, player, _rx) = world();
    let mut store = store(PluginLimits {
        max_host_operations_per_tick: 1,
        ..Default::default()
    });
    with_world(&mut store, &mut world, |store| {
        let ctx = store.data_mut();
        let borrowed = ctx.borrow_player(player).unwrap();
        ctx.capabilities.clear();
        assert!(matches!(
            HostPlayer::identity(ctx, Resource::new_borrow(borrowed.rep())),
            Err(wit::AccessError::PermissionDenied)
        ));
        ctx.capabilities.insert(PluginCapability::ReadPlayers);
        assert!(matches!(
            HostPlayer::identity(ctx, Resource::new_borrow(borrowed.rep())),
            Err(wit::AccessError::LimitExceeded)
        ));
    });
    begin_budget(store.data_mut());
    with_world(&mut store, &mut world, |store| {
        let ctx = store.data_mut();
        let borrowed = ctx.borrow_player(player).unwrap();
        assert!(HostPlayer::identity(ctx, Resource::new_borrow(borrowed.rep())).is_ok());
    });
}

#[test]
fn foreign_live_access_is_rejected_even_when_aggregate_snapshot_is_readable() {
    let (mut source, player, _rx) = world();
    let (mut foreign, _, _foreign_rx) = world();
    let mut store = store(PluginLimits::default());
    let handle = with_world(&mut store, &mut source, |store| {
        let ctx = store.data_mut();
        let borrowed = ctx.borrow_player(player).unwrap();
        HostPlayer::to_handle(ctx, Resource::new_borrow(borrowed.rep()))
            .unwrap()
            .rep()
    });
    let directory = ComponentDirectory {
        worlds: vec![
            WorldReference::of(&source).unwrap(),
            WorldReference::of(&foreign).unwrap(),
        ],
        actors: observe(&mut source),
    };
    foreign.insert_resource(directory);
    with_world(&mut store, &mut foreign, |store| {
        let ctx = store.data_mut();
        let tx = ctx.borrow_transaction().unwrap();
        assert!(matches!(
            HostWorldTx::player(
                ctx,
                Resource::new_borrow(tx.rep()),
                Resource::new_borrow(handle)
            ),
            Err(wit::AccessError::WrongWorld)
        ));
        assert_eq!(
            HostPlayerHandle::snapshot(ctx, Resource::new_borrow(handle))
                .unwrap()
                .captured_tick,
            42
        );
    });
}

#[test]
fn malformed_authenticated_accounts_are_rejected_instead_of_becoming_anonymous() {
    for account in ["0", "not-an-account", "18446744073709551616", ""] {
        let (mut world, player, _rx) = world();
        world.get_mut::<PlayerSession>(player).unwrap().xuid = Some(account.into());
        assert!(matches!(
            ensure_identity(&mut world, player),
            Err(wit::AccessError::InvalidIdentity)
        ));
        assert!(world.get::<PlayerLineage>(player).is_none());
    }
    let (mut world, player, _rx) = world();
    world.get_mut::<PlayerSession>(player).unwrap().xuid = None;
    assert_eq!(
        ensure_identity(&mut world, player)
            .unwrap()
            .identity
            .authenticated_xuid,
        None
    );
}

#[test]
fn configured_admin_reads_immutable_configuration_instead_of_mutable_operator_state() {
    let (mut world, player, _rx) = world();
    world
        .entity_mut(player)
        .insert(crate::command::context::Operator);
    let lineage = ensure_identity(&mut world, player).unwrap();
    assert!(!snapshot(&world, player, &lineage).unwrap().configured_admin);
    let config = crate::server::game::ServerConfig {
        operators: vec!["42".into()],
        ..Default::default()
    };
    world.insert_resource(crate::server::game::types::ServerConfigResource(config));
    assert!(snapshot(&world, player, &lineage).unwrap().configured_admin);
}

/// Exercise the actual native retention boundary rather than synthesizing a detached marker.
struct Retain;
impl crate::server::game::player_events::PlayerEventHandler for Retain {
    fn on_quit(&self, _: &mut World, event: &mut crate::server::game::player_events::PlayerQuit) {
        event.retain_entity();
    }
}

#[tokio::test]
async fn retained_sessionless_actor_keeps_the_original_handle_without_binding_a_reconnect() {
    use crate::server::game::{GameServer, PlayerSpawnData, player_events::PlayerEventHooks};
    let mut server = GameServer::with_config(crate::ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    server
        .ecs
        .world_mut()
        .resource_mut::<PlayerEventHooks>()
        .register(Retain);
    let (tx, _rx) = tokio::sync::mpsc::channel(8192);
    let player = server
        .spawn_player(PlayerSpawnData {
            session_id: 7,
            runtime_id: 42,
            display_name: "Reader".into(),
            xuid: Some("42".into()),
            uuid: Some(uuid::Uuid::from_u128(42).to_string()),
            position: glam::DVec3::new(1.0, 65.62, 3.0),
            outbound_tx: tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    let mut store = store(PluginLimits::default());
    let handle = with_world(&mut store, server.ecs.world_mut(), |store| {
        let ctx = store.data_mut();
        let player = ctx.borrow_player(player).unwrap();
        HostPlayer::to_handle(ctx, Resource::new_borrow(player.rep()))
            .unwrap()
            .rep()
    });
    server.despawn_player(7);
    assert!(server.ecs.world().get::<PlayerSession>(player).is_none());
    with_world(&mut store, server.ecs.world_mut(), |store| {
        let ctx = store.data_mut();
        let tx = ctx.borrow_transaction().unwrap();
        let player = HostWorldTx::player(
            ctx,
            Resource::new_borrow(tx.rep()),
            Resource::new_borrow(handle),
        )
        .unwrap();
        assert_eq!(
            HostPlayer::availability(ctx, Resource::new_borrow(player.rep())).unwrap(),
            wit::PlayerAvailability::Retained
        );
        assert_eq!(
            HostPlayer::identity(ctx, Resource::new_borrow(player.rep()))
                .unwrap()
                .authenticated_xuid,
            Some(42)
        );
        assert_eq!(
            HostPlayer::message(
                ctx,
                Resource::new_borrow(player.rep()),
                "unavailable".into()
            ),
            Err(wit::AccessError::Unavailable)
        );
    });
    let (replacement, _replacement_rx) = tokio::sync::mpsc::channel(8);
    server
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert(PlayerSession::new(
            7,
            "Reader".into(),
            None,
            Some(uuid::Uuid::from_u128(42).to_string()),
            replacement,
        ));
    assert_eq!(
        crate::server::game::player_state::account_xuid(server.ecs.world(), player),
        None
    );
    with_world(&mut store, server.ecs.world_mut(), |store| {
        let ctx = store.data_mut();
        let tx = ctx.borrow_transaction().unwrap();
        assert!(matches!(
            HostWorldTx::player(
                ctx,
                Resource::new_borrow(tx.rep()),
                Resource::new_borrow(handle)
            ),
            Err(wit::AccessError::StalePlayer)
        ));
    });
}
