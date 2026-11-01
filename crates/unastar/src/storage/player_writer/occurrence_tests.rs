//! Canonical event metadata follows real terminal writes rather than restored raw state.

use super::*;
use crate::entity::damage::owned::{Attribution, Cause};
use crate::entity::player_life::occurrence::{NativeDeathOccurrence, RuntimeIdentity};

/// Build a real terminal writer request with separate engine event and receipt identities.
async fn terminal(
    writer: &SerializedPlayerProvider,
    uuid: Uuid,
    mut data: PlayerData,
    event: u128,
    time: Option<u64>,
) -> PlayerData {
    let lifetime = Uuid::from_u128(901).to_string();
    let lineage = Uuid::from_u128(902).to_string();
    let death = Uuid::from_u128(event).to_string();
    data.health = 0.0;
    data.native_terminal = None;
    data.death_occurrence = Some(NativeDeathOccurrence {
        id: death.clone(),
        unix_time_millis: time,
        runtime: Some(RuntimeIdentity {
            lineage: lineage.clone(),
            world_lifetime: lifetime.clone(),
        }),
        cause: Cause::Override,
        attribution: Attribution {
            source_dimension: Some(0),
            source_world_lifetime: Some(lifetime.clone()),
            owner: None,
        },
        fall_distance: None,
        keep_inventory: false,
        post_drop: None,
    });
    let mut raw = super::source_tests::supplement();
    raw.dead = true;
    writer
        .commit_native_terminal(
            uuid,
            data,
            raw.clone(),
            super::super::terminal::NativeTerminalRecord {
                version: 1,
                death: death.clone(),
                world: Uuid::from_u128(903).to_string(),
                lifetime: lifetime.clone(),
                dimension: 0,
                lineage,
                name: "Example".into(),
                xuid: raw.xuid,
                id: format!("native-terminal:{lifetime}:{death}"),
                receipt: InventoryEffectRecord {
                    revision: 0,
                    request_hash: [0; 32],
                },
            },
        )
        .await
        .unwrap()
        .data
}

#[tokio::test]
async fn original_occurrence_and_receipt_survive_reopen_respawn_and_stale_state_install() {
    let directory = tempfile::tempdir().unwrap();
    let uuid = Uuid::from_u128(900);
    let writer = super::source_tests::writer(directory.path());
    let before = PlayerData {
        uuid: uuid.to_string(),
        game_mode: 0,
        ..Default::default()
    };
    let first = terminal(&writer, uuid, before, 904, Some(1_000)).await;
    let first_event = first.death_occurrence.clone().unwrap();
    let proof = first_event.post_drop.as_ref().unwrap();
    assert_eq!(first.inventory_effects.get(&proof.id), Some(&proof.receipt));
    let second = terminal(&writer, uuid, first.clone(), 905, None).await;
    let second_event = second.death_occurrence.clone().unwrap();
    assert_eq!(second_event.unix_time_millis, None);
    assert_ne!(first_event.id, second_event.id);
    let mut stale_raw = second.clone();
    stale_raw.native_terminal = None;
    stale_raw.death_occurrence = Some(first_event);
    stale_raw.health = 20.0;
    writer
        .commit_state_effect(
            uuid,
            "example:raw-install",
            [5; 32],
            stale_raw,
            super::source_tests::supplement(),
            StateOrigin::default(),
        )
        .await
        .unwrap();
    let installed = writer.load(uuid).await.unwrap().unwrap();
    assert_eq!(installed.death_occurrence, Some(second_event.clone()));
    let mut ordinary = installed.clone();
    ordinary.death_occurrence = None;
    ordinary.restoration_checkpoint = None;
    writer.save(uuid, &ordinary).await.unwrap();
    let mut gift = writer.load(uuid).await.unwrap().unwrap();
    gift.death_occurrence = None;
    writer
        .commit_effect(uuid, "example:later-gift", [6; 32], gift)
        .await
        .unwrap();
    drop(writer);
    let reopened = super::source_tests::writer(directory.path())
        .load(uuid)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.health, 20.0);
    assert_eq!(reopened.death_occurrence, Some(second_event));
    reopened.validate(uuid).unwrap();
}

#[tokio::test]
async fn ordinary_save_cannot_mint_an_occurrence_and_corrupt_receipts_fail_validation() {
    let directory = tempfile::tempdir().unwrap();
    let uuid = Uuid::from_u128(910);
    let writer = super::source_tests::writer(directory.path());
    let data = PlayerData {
        uuid: uuid.to_string(),
        game_mode: 0,
        ..Default::default()
    };
    let terminal = terminal(&writer, uuid, data, 911, Some(2_000)).await;
    let mut corrupt = terminal.clone();
    corrupt
        .death_occurrence
        .as_mut()
        .unwrap()
        .post_drop
        .as_mut()
        .unwrap()
        .receipt
        .request_hash = [9; 32];
    assert!(corrupt.validate(uuid).is_err());
    let other = Uuid::from_u128(912);
    let mut fabricated = PlayerData {
        uuid: other.to_string(),
        game_mode: 0,
        ..Default::default()
    };
    let mut event = terminal.death_occurrence.unwrap();
    event.post_drop = None;
    fabricated.death_occurrence = Some(event);
    writer.save(other, &fabricated).await.unwrap();
    assert!(
        writer
            .load(other)
            .await
            .unwrap()
            .unwrap()
            .death_occurrence
            .is_none()
    );
}
