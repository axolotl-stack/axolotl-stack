//! Admission ownership against complete native joins and actual transport replacement.

use super::*;
use crate::entity::components::{Health, PlayerSession};
use crate::server::game::player_events::{PlayerEventHandler, PlayerEventHooks, PlayerJoined};
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};

struct Install(AdmissionBook);
impl PlayerEventHandler for Install {
    fn on_joined(&self, world: &mut World, event: &PlayerJoined) {
        self.0.begin(world, event.player()).unwrap();
    }
}
struct Harness {
    server: GameServer,
    player: Entity,
    id: AdmissionId,
    book: AdmissionBook,
    packets: tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
}
impl Harness {
    /// Real synchronous join dispatch runs before native initial publication.
    fn new() -> Self {
        let book = AdmissionBook::default();
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        server
            .ecs
            .world_mut()
            .resource_mut::<PlayerEventHooks>()
            .register(Install(book.clone()));
        let (tx, packets) = tokio::sync::mpsc::channel(65536);
        let player = server
            .spawn_player(PlayerSpawnData {
                session_id: 42,
                runtime_id: 7,
                display_name: "Player".into(),
                xuid: Some("42".into()),
                uuid: Some(uuid::Uuid::from_u128(42).to_string()),
                position: glam::DVec3::new(0.5, 65.62, 0.5),
                outbound_tx: tx,
                saved_data: None,
                chunk_radius: 0,
            })
            .unwrap();
        let id = server.ecs.world().get::<AdmissionGuard>(player).unwrap().id;
        Self {
            server,
            player,
            id,
            book,
            packets,
        }
    }
    /// Borrow current authoritative state only for one transaction assertion.
    fn world(&self) -> &World {
        self.server.ecs.world()
    }
}

#[tokio::test]
async fn all_component_holds_must_resolve_before_the_same_session_can_publish() {
    let mut harness = Harness::new();
    assert!(
        harness.packets.try_recv().is_err(),
        "held join sends no initial state"
    );
    assert!(blocks_publication(harness.world(), harness.player));
    let first = harness.book.hold(harness.id, "first").unwrap();
    let second = harness.book.hold(harness.id, "second").unwrap();
    harness
        .book
        .decide(harness.id, "first", Decision::Deferred)
        .unwrap();
    harness
        .book
        .decide(harness.id, "second", Decision::Deferred)
        .unwrap();
    harness.book.evaluated(harness.id).unwrap();
    assert_eq!(
        harness.book.status(harness.world(), harness.id),
        Status::Waiting
    );
    assert_eq!(
        harness
            .book
            .resolve(harness.world(), first, "second", Decision::Allow),
        Err(Error::WrongOwner)
    );
    harness
        .book
        .resolve(harness.world(), first, "first", Decision::Allow)
        .unwrap();
    assert!(blocks_publication(harness.world(), harness.player));
    harness
        .book
        .resolve(harness.world(), second, "second", Decision::Allow)
        .unwrap();
    assert_eq!(
        harness.book.status(harness.world(), harness.id),
        Status::Ready
    );
    assert!(!blocks_publication(harness.world(), harness.player));
    assert!(player_events::release_initial_state(
        harness.server.ecs.world_mut(),
        harness.player
    ));
    harness.server.tick();
    assert!(
        harness.packets.try_recv().is_ok(),
        "native initial batch follows real readiness"
    );
    assert!(!player_events::initial_state_pending(
        harness.world(),
        harness.player
    ));
    harness.book.remove(harness.id);
    assert_eq!(
        harness
            .book
            .resolve(harness.world(), first, "first", Decision::Allow),
        Err(Error::Stale)
    );
}

#[tokio::test]
async fn retained_token_cannot_admit_a_replaced_transport_even_with_reused_session_and_actor() {
    let mut harness = Harness::new();
    let token = harness.book.hold(harness.id, "owner").unwrap();
    harness
        .book
        .decide(harness.id, "owner", Decision::Deferred)
        .unwrap();
    harness.book.evaluated(harness.id).unwrap();
    let (tx, _packets) = tokio::sync::mpsc::channel(128);
    harness
        .server
        .ecs
        .world_mut()
        .entity_mut(harness.player)
        .insert(PlayerSession::new(
            42,
            "Player".into(),
            Some("42".into()),
            Some(uuid::Uuid::from_u128(42).to_string()),
            tx,
        ));
    assert_eq!(
        harness
            .book
            .resolve(harness.world(), token, "owner", Decision::Allow),
        Err(Error::Stale)
    );
    assert_eq!(
        harness.book.status(harness.world(), harness.id),
        Status::Stale
    );
    assert!(blocks_publication(harness.world(), harness.player));
    assert!(!player_events::release_initial_state(
        harness.server.ecs.world_mut(),
        harness.player
    ));
}

#[tokio::test]
async fn dropping_pending_acknowledgement_rejects_but_allow_needs_no_retained_token() {
    let harness = Harness::new();
    let hold = harness.book.hold(harness.id, "owner").unwrap();
    harness
        .book
        .decide(harness.id, "owner", Decision::Deferred)
        .unwrap();
    harness.book.evaluated(harness.id).unwrap();
    harness.book.abandoned(hold, "owner").unwrap();
    assert!(matches!(
        harness.book.status(harness.world(), harness.id),
        Status::Rejected(_)
    ));
    assert_eq!(
        harness
            .book
            .resolve(harness.world(), hold, "owner", Decision::Allow),
        Err(Error::Stale)
    );

    let harness = Harness::new();
    let unused = harness.book.hold(harness.id, "owner").unwrap();
    harness.book.abandoned(unused, "owner").unwrap();
    harness
        .book
        .decide(harness.id, "owner", Decision::Allow)
        .unwrap();
    harness.book.evaluated(harness.id).unwrap();
    assert_eq!(
        harness.book.status(harness.world(), harness.id),
        Status::Ready
    );
}

#[tokio::test]
async fn deferred_requires_an_owned_live_hold_and_rejection_reason_remains_exact() {
    let harness = Harness::new();
    assert_eq!(
        harness.book.decide(harness.id, "owner", Decision::Deferred),
        Err(Error::InvalidDecision)
    );
    assert_eq!(
        harness
            .book
            .decide(harness.id, "owner", Decision::Reject("".into())),
        Err(Error::InvalidDecision)
    );
    let hold = harness.book.hold(harness.id, "owner").unwrap();
    harness.book.abandoned(hold, "owner").unwrap();
    assert_eq!(
        harness.book.decide(harness.id, "owner", Decision::Deferred),
        Err(Error::InvalidDecision)
    );
    harness
        .book
        .decide(
            harness.id,
            "owner",
            Decision::Reject("Please retry later.".into()),
        )
        .unwrap();
    harness.book.evaluated(harness.id).unwrap();
    assert_eq!(
        harness.book.status(harness.world(), harness.id),
        Status::Rejected("Please retry later.".into())
    );
    assert_eq!(harness.book.hold(harness.id, "owner"), Err(Error::Stale));
}

#[tokio::test]
async fn successful_ack_cannot_publish_an_incomplete_native_initial_plan() {
    let mut harness = Harness::new();
    let hold = harness.book.hold(harness.id, "owner").unwrap();
    harness
        .book
        .decide(harness.id, "owner", Decision::Deferred)
        .unwrap();
    harness.book.evaluated(harness.id).unwrap();
    harness
        .book
        .resolve(harness.world(), hold, "owner", Decision::Allow)
        .unwrap();
    let health = harness
        .server
        .ecs
        .world_mut()
        .entity_mut(harness.player)
        .take::<Health>()
        .unwrap();
    assert!(!player_events::release_initial_state(
        harness.server.ecs.world_mut(),
        harness.player
    ));
    assert!(harness.packets.try_recv().is_err());
    harness
        .server
        .ecs
        .world_mut()
        .entity_mut(harness.player)
        .insert(health);
    assert!(player_events::release_initial_state(
        harness.server.ecs.world_mut(),
        harness.player
    ));
}

#[tokio::test]
async fn repeated_component_decisions_cannot_discard_an_existing_hold() {
    let harness = Harness::new();
    let _hold = harness.book.hold(harness.id, "owner").unwrap();
    harness
        .book
        .decide(harness.id, "owner", Decision::Deferred)
        .unwrap();
    assert_eq!(
        harness.book.decide(harness.id, "owner", Decision::Allow),
        Err(Error::InvalidDecision)
    );
    assert_eq!(harness.book.hold(harness.id, "owner"), Err(Error::Stale));
    harness.book.evaluated(harness.id).unwrap();
    assert_eq!(
        harness.book.status(harness.world(), harness.id),
        Status::Waiting
    );
}

#[tokio::test]
async fn component_failure_rejects_retained_holds_without_a_guest_drop_callback() {
    let harness = Harness::new();
    let _hold = harness.book.hold(harness.id, "owner").unwrap();
    harness
        .book
        .decide(harness.id, "owner", Decision::Deferred)
        .unwrap();
    harness.book.evaluated(harness.id).unwrap();
    harness.book.owner_failed("unrelated");
    assert_eq!(
        harness.book.status(harness.world(), harness.id),
        Status::Waiting
    );
    harness.book.owner_failed("owner");
    assert!(matches!(
        harness.book.status(harness.world(), harness.id),
        Status::Rejected(_)
    ));
}

#[tokio::test]
async fn sources_removed_by_late_observers_release_native_admission_bookkeeping() {
    let mut harness = Harness::new();
    let token = harness.book.hold(harness.id, "owner").unwrap();
    harness
        .book
        .decide(harness.id, "owner", Decision::Deferred)
        .unwrap();
    harness.book.evaluated(harness.id).unwrap();
    harness.server.ecs.world_mut().despawn(harness.player);
    harness.book.prune(harness.world());
    assert_eq!(
        harness.book.status(harness.world(), harness.id),
        Status::Stale
    );
    assert_eq!(
        harness
            .book
            .resolve(harness.world(), token, "owner", Decision::Allow),
        Err(Error::Stale)
    );
    assert!(harness.book.0.lock().unwrap().admissions.is_empty());
    assert!(harness.book.0.lock().unwrap().holds.is_empty());
}
