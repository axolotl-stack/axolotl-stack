//! Exact-owner terminal fences against native captures, saves and worker completion.

use super::*;
use crate::entity::components::{Effects, Hunger, Velocity};
use crate::server::game::player_state;

#[tokio::test]
async fn terminal_fence_withholds_real_gameplay_components_captures_and_ordinary_saves() {
    let (mut server, player, provider, _rx) = harness();
    let original = player_state::capture(server.ecs.world(), player).unwrap();
    let source = RecoverySource::capture(server.ecs.world(), player).unwrap();
    let fence = fence_for_recovery(server.ecs.world_mut(), &source).unwrap();
    assert_eq!(fence.player(), player);
    assert!(fence.is_current(server.ecs.world()));
    assert!(is_quarantined(server.ecs.world(), player));
    assert!(is_recovery_fenced(server.ecs.world(), player));
    assert!(server.ecs.world().get::<MainInventory>(player).is_none());
    assert!(server.ecs.world().get::<ArmourInventory>(player).is_none());
    assert!(server.ecs.world().get::<OffhandSlot>(player).is_none());
    assert!(server.ecs.world().get::<PlayerUi>(player).is_none());
    assert!(
        server
            .ecs
            .world()
            .get::<EnderChestInventory>(player)
            .is_none()
    );
    assert!(server.ecs.world().get::<Health>(player).is_none());
    assert!(server.ecs.world().get::<Hunger>(player).is_none());
    assert!(server.ecs.world().get::<Position>(player).is_none());
    assert!(server.ecs.world().get::<Velocity>(player).is_none());
    assert!(server.ecs.world().get::<Effects>(player).is_none());
    assert!(server.ecs.world().get::<PlayerInput>(player).is_none());
    assert!(player_state::capture(server.ecs.world(), player).is_err());
    assert!(player_state::restore(server.ecs.world_mut(), player, &original).is_err());
    assert_eq!(
        crate::entity::player_life::hurt::hurt_player(
            server.ecs.world_mut(),
            player,
            &crate::entity::mob::combat::Hit {
                damage: 50.0,
                cause: unastar_data::actor_components::body::ActorDamageCause::Override,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
            }
        ),
        0.0,
        "native damage cannot mutate withheld vitals"
    );
    assert!(!server.save_player(7).await);
    assert_eq!(server.save_all_players().await, 0);
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    for _ in 0..40 {
        server.tick();
    }
    assert!(is_recovery_fenced(server.ecs.world(), player));
    assert!(server.ecs.world().get::<Health>(player).is_none());
    assert!(server.ecs.world().get::<Hunger>(player).is_none());
    assert!(server.ecs.world().get::<MainInventory>(player).is_none());
    assert_eq!(
        fence_for_recovery(server.ecs.world_mut(), &source),
        Ok(fence)
    );
}

#[tokio::test]
async fn terminal_fence_never_releases_for_reused_session_id_or_replacement_channel() {
    let (mut server, player, _provider, _rx) = harness();
    let original = player_state::capture(server.ecs.world(), player).unwrap();
    let source = RecoverySource::capture(server.ecs.world(), player).unwrap();
    let fence = fence_for_recovery(server.ecs.world_mut(), &source).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(4096);
    let current = server.ecs.world().get::<PlayerSession>(player).unwrap();
    let replacement = PlayerSession::new(
        current.session_id,
        current.display_name.clone(),
        current.xuid.clone(),
        current.uuid.clone(),
        tx,
    );
    server
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert(replacement);
    assert!(!fence.is_current(server.ecs.world()));
    assert_eq!(
        fence_for_recovery(server.ecs.world_mut(), &source),
        Err(EffectError::Conflict)
    );
    poll(server.ecs.world_mut());
    assert!(is_recovery_fenced(server.ecs.world(), player));
    assert!(!server.save_player(7).await);
    server.despawn_player(7);
    assert!(player_state::spawn_detached(server.ecs.world_mut(), "Alice", &original).is_err());
    assert!(recovery_account_fenced(
        server.ecs.world(),
        Uuid::from_u128(7),
        None
    ));
    assert!(recovery_account_fenced(
        server.ecs.world(),
        Uuid::from_u128(999),
        Some("123")
    ));
    assert!(!recovery_account_fenced(
        server.ecs.world(),
        Uuid::from_u128(999),
        None
    ));
    assert!(!recovery_account_fenced(
        server.ecs.world(),
        Uuid::from_u128(999),
        Some("")
    ));
    let (tx, _rx) = tokio::sync::mpsc::channel(4096);
    assert!(
        server
            .spawn_player(PlayerSpawnData {
                session_id: 8,
                display_name: "Alice".into(),
                xuid: Some("123".into()),
                uuid: Some("00000000-0000-0000-0000-000000000007".into()),
                runtime_id: 8,
                position: glam::DVec3::new(1.5, 17.62, 1.5),
                outbound_tx: tx,
                saved_data: None,
                chunk_radius: 2,
            })
            .is_err(),
        "entity removal never silently releases fenced account admission"
    );
}

#[tokio::test]
async fn stale_failure_source_cannot_fence_a_replacement_and_other_accounts_remain_available() {
    let (mut server, player, provider, _rx) = harness();
    let old = RecoverySource::capture(server.ecs.world(), player).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(4096);
    let current = server.ecs.world().get::<PlayerSession>(player).unwrap();
    let replacement = PlayerSession::new(
        current.session_id,
        current.display_name.clone(),
        current.xuid.clone(),
        current.uuid.clone(),
        tx,
    );
    server
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert(replacement);
    assert_eq!(
        fence_for_recovery(server.ecs.world_mut(), &old),
        Err(EffectError::Conflict)
    );
    assert!(server.ecs.world().get::<Health>(player).is_some());
    assert!(server.ecs.world().get::<MainInventory>(player).is_some());
    assert!(!is_quarantined(server.ecs.world(), player));
    let source = RecoverySource::capture(server.ecs.world(), player).unwrap();
    fence_for_recovery(server.ecs.world_mut(), &source).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(4096);
    let other = server
        .spawn_player(PlayerSpawnData {
            session_id: 8,
            display_name: "Bob".into(),
            xuid: Some("124".into()),
            uuid: Some("00000000-0000-0000-0000-000000000008".into()),
            runtime_id: 8,
            position: glam::DVec3::new(2.5, 17.62, 2.5),
            outbound_tx: tx,
            saved_data: None,
            chunk_radius: 2,
        })
        .unwrap();
    assert!(!is_quarantined(server.ecs.world(), other));
    assert!(player_state::capture(server.ecs.world(), other).is_ok());
    provider.write_gate.add_permits(1);
    assert!(server.save_player(8).await);
    assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
    assert!(!server.save_player(7).await);
}

#[tokio::test]
async fn terminal_fence_survives_an_already_running_inventory_write_completion() {
    let (mut server, player, provider, _rx) = harness();
    let effect = effect(server.ecs.world(), player, "pending-then-terminal");
    submit(server.ecs.world_mut(), player, effect).unwrap();
    provider.started.acquire().await.unwrap().forget();
    let source = RecoverySource::capture(server.ecs.world(), player).unwrap();
    let fence = fence_for_recovery(server.ecs.world_mut(), &source).unwrap();
    assert!(fence.is_current(server.ecs.world()));
    provider.write_gate.add_permits(1);
    for _ in 0..200 {
        tokio::task::yield_now().await;
        poll(server.ecs.world_mut());
        let observed = server.ecs.world().resource::<Observed>();
        if !observed.committed.is_empty() || !observed.failed.is_empty() {
            break;
        }
    }
    let observed = server.ecs.world().resource::<Observed>();
    assert!(
        !observed.committed.is_empty() || !observed.failed.is_empty(),
        "controlled writer completed"
    );
    assert!(observed.committed.iter().all(|event| !event.live_applied));
    assert!(is_quarantined(server.ecs.world(), player));
    assert!(is_recovery_fenced(server.ecs.world(), player));
    assert!(server.ecs.world().get::<MainInventory>(player).is_none());
    assert!(server.ecs.world().get::<Health>(player).is_none());
    assert!(!server.save_player(7).await);
}

#[test]
fn invalid_terminal_fence_sources_do_not_remove_or_invent_actor_state() {
    let mut world = World::new();
    let actor = world.spawn_empty().id();
    assert!(matches!(
        RecoverySource::capture(&world, actor),
        Err(EffectError::MissingPlayer)
    ));
    assert!(!is_recovery_fenced(&world, actor));
    assert!(!is_quarantined(&world, actor));
}
