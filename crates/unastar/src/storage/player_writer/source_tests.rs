use super::*;
use crate::storage::{LevelDBPlayerProvider, RestorationSupplement};

/// Native raw extras are stored beside the canonical slots without a recursive player row.
fn supplement() -> RestorationSupplement {
    RestorationSupplement {
        version: 1,
        xuid: Some("73".into()),
        effective_mode: 0,
        base_mode: None,
        flight_override: None,
        velocity: [0.25, 0.5, -0.25],
        head_yaw: 30.0,
        absorption: 2.0,
        air_current: 17,
        air_max: 300,
        on_fire_ticks: 40,
        burning_remaining: 0,
        burning_next: 0,
        effects: vec![],
        dead: false,
        food_policy: None,
        food_ticks: 0,
        food_regeneration: 0,
    }
}

/// A fresh source seal owns a genuine complete row rather than an archive's descriptive nonce.
fn fresh() -> (Uuid, PlayerData, SourceActorRecord) {
    let uuid = Uuid::from_u128(73);
    let data = PlayerData {
        uuid: uuid.to_string(),
        game_mode: 0,
        food: 9,
        inventory: vec![crate::storage::StoredItemStack {
            item_id: "minecraft:stone".into(),
            count: 7,
            nbt: Some(vec![1, 2, 3]),
            ..Default::default()
        }],
        ..Default::default()
    };
    let source = SourceActorRecord {
        version: 1,
        module: "example.sources".into(),
        nonce: Uuid::from_u128(19).to_string(),
        generation: 1,
        sample: 0,
        name: "Example".into(),
        world: Uuid::from_u128(3).to_string(),
        dimension: 0,
        phase: SourcePhase::Frozen,
        supplement: Some(supplement()),
        handoff: None,
    };
    (uuid, data, source)
}

/// Open the actual durable provider through the same account serializer used by ordinary saves.
fn writer(path: &std::path::Path) -> SerializedPlayerProvider {
    SerializedPlayerProvider::new(Arc::new(LevelDBPlayerProvider::open(path).unwrap()))
}

#[tokio::test]
async fn source_seal_cold_reopens_exact_raw_state_and_rejects_ordinary_saves() {
    let directory = tempfile::tempdir().unwrap();
    let (uuid, before, source) = fresh();
    {
        let writer = writer(directory.path());
        let sealed = writer
            .commit_source(
                uuid,
                "source:seal",
                [1; 32],
                before.clone(),
                None,
                source.clone(),
            )
            .await
            .unwrap();
        assert!(!sealed.effect.already_committed);
        assert_eq!(sealed.data.source_actor, Some(source.clone()));
        let mut ordinary = sealed.data.clone();
        ordinary.source_actor = None;
        assert!(matches!(
            writer.save(uuid, &ordinary).await,
            Err(StorageError::PlayerRevisionConflict)
        ));
        assert!(matches!(
            writer.save(uuid, &before).await,
            Err(StorageError::PlayerRevisionConflict)
        ));
    }
    let record = writer(directory.path()).load(uuid).await.unwrap().unwrap();
    assert_eq!(record.source_actor, Some(source));
    assert_eq!(record.inventory, before.inventory);
    assert_eq!(record.inventory_revision, 1);
    assert_eq!(
        record.inventory_effects["source:seal"].request_hash,
        [1; 32]
    );
}

#[tokio::test]
async fn active_source_ordinary_samples_preserve_latest_ack_and_fence_stale_captures() {
    let directory = tempfile::tempdir().unwrap();
    let (uuid, before, source) = fresh();
    {
        let writer = writer(directory.path());
        let sealed = writer
            .commit_source(uuid, "source:seal", [1; 32], before, None, source.clone())
            .await
            .unwrap();
        let mut active = source.clone();
        active.generation = 2;
        active.phase = SourcePhase::Active;
        let acknowledged = writer
            .commit_source(
                uuid,
                "source:claim",
                [2; 32],
                sealed.data,
                Some((&source).into()),
                active,
            )
            .await
            .unwrap();
        let stale = acknowledged.data.clone();
        let mut current = stale.clone();
        current.food = 5;
        current.source_actor.as_mut().unwrap().sample = 1;
        current
            .source_actor
            .as_mut()
            .unwrap()
            .supplement
            .as_mut()
            .unwrap()
            .on_fire_ticks = 19;
        writer.save(uuid, &current).await.unwrap();
        assert!(matches!(
            writer.save(uuid, &stale).await,
            Err(StorageError::PlayerRevisionConflict)
        ));
    }
    let record = writer(directory.path()).load(uuid).await.unwrap().unwrap();
    assert_eq!(record.food, 5);
    let source = record.source_actor.unwrap();
    assert_eq!(source.generation, 2);
    assert_eq!(source.sample, 1);
    assert_eq!(source.supplement.unwrap().on_fire_ticks, 19);
}

#[tokio::test]
async fn target_receipt_and_source_tombstone_are_one_atomic_canonical_commit() {
    let directory = tempfile::tempdir().unwrap();
    let (uuid, before, source) = fresh();
    {
        let writer = writer(directory.path());
        let sealed = writer
            .commit_source(uuid, "source:seal", [1; 32], before, None, source.clone())
            .await
            .unwrap();
        let mut target = sealed.data;
        target.source_actor = None;
        target.food = 4;
        let mut raw = supplement();
        raw.velocity = [0.0; 3];
        let effect = writer
            .commit_state_effect(
                uuid,
                "target:restore",
                [2; 32],
                target,
                raw,
                StateOrigin {
                    source: Some((&source).into()),
                    baseline: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(effect.record.revision, 2);
    }
    let writer = writer(directory.path());
    let canonical = writer.load(uuid).await.unwrap().unwrap();
    assert_eq!(canonical.food, 4);
    assert_eq!(
        canonical.source_actor.as_ref().unwrap().phase,
        SourcePhase::Transferred
    );
    assert_eq!(canonical.inventory_effects["target:restore"].revision, 2);
    let mut attempted = source.clone();
    attempted.phase = SourcePhase::Active;
    attempted.generation = 3;
    let failure = writer
        .commit_source(
            uuid,
            "old:claim",
            [3; 32],
            canonical,
            Some((&source).into()),
            attempted,
        )
        .await
        .err()
        .unwrap();
    assert_eq!(failure.kind, WriteFailureKind::NotApplied);
    assert!(!failure.resume_source);
}

#[tokio::test]
async fn retirement_is_idempotent_and_old_generation_cannot_reopen_the_body() {
    let directory = tempfile::tempdir().unwrap();
    let (uuid, before, source) = fresh();
    let writer = writer(directory.path());
    let sealed = writer
        .commit_source(uuid, "source:seal", [1; 32], before, None, source.clone())
        .await
        .unwrap();
    let mut retired = source.clone();
    retired.generation = 2;
    retired.phase = SourcePhase::Retired;
    retired.supplement = None;
    let first = writer
        .commit_source(
            uuid,
            "source:retire",
            [2; 32],
            sealed.data.clone(),
            Some((&source).into()),
            retired.clone(),
        )
        .await
        .unwrap();
    let replay = writer
        .commit_source(
            uuid,
            "source:retire",
            [2; 32],
            sealed.data,
            Some((&source).into()),
            retired,
        )
        .await
        .unwrap();
    assert!(replay.effect.already_committed);
    assert_eq!(first.data, replay.data);
    let mut old = source.clone();
    old.generation = 3;
    old.phase = SourcePhase::Active;
    assert!(
        writer
            .commit_source(
                uuid,
                "source:reopen",
                [3; 32],
                first.data,
                Some((&source).into()),
                old
            )
            .await
            .is_err()
    );
}

#[test]
fn corrupt_source_authority_and_unknown_schema_fields_fail_validation() {
    let (uuid, mut data, source) = fresh();
    data.source_actor = Some(source.clone());
    data.validate(uuid).unwrap();
    for mutate in [0, 1, 2, 3] {
        let mut invalid = data.clone();
        let record = invalid.source_actor.as_mut().unwrap();
        match mutate {
            0 => record.generation = 0,
            1 => record.nonce = "not-an-identity".into(),
            2 => record.supplement = None,
            _ => record.phase = SourcePhase::Terminal,
        }
        assert!(invalid.validate(uuid).is_err());
    }
    let mut encoded = serde_json::to_value(source).unwrap();
    encoded["future_authority"] = true.into();
    assert!(serde_json::from_value::<SourceActorRecord>(encoded).is_err());
}

#[tokio::test]
async fn initialization_canonical_baseline_cannot_replace_newer_same_epoch_physical_state() {
    let directory = tempfile::tempdir().unwrap();
    let (uuid, before, source) = fresh();
    let writer = writer(directory.path());
    let sealed = writer
        .commit_source(uuid, "source:seal", [1; 32], before, None, source.clone())
        .await
        .unwrap();
    let mut active = source.clone();
    active.phase = SourcePhase::Active;
    active.generation = 2;
    let acknowledged = writer
        .commit_source(
            uuid,
            "source:active",
            [2; 32],
            sealed.data,
            Some((&source).into()),
            active.clone(),
        )
        .await
        .unwrap();
    let stale = Arc::new(acknowledged.data.clone());
    let mut latest = acknowledged.data;
    latest.health = 3.0;
    latest.source_actor.as_mut().unwrap().sample = 1;
    writer.save(uuid, &latest).await.unwrap();
    let mut candidate = (*stale).clone();
    candidate.source_actor = None;
    let result = writer
        .commit_state_effect(
            uuid,
            "target:stale",
            [3; 32],
            candidate,
            supplement(),
            StateOrigin {
                source: Some((&active).into()),
                baseline: Some(stale),
            },
        )
        .await
        .err()
        .unwrap();
    assert_eq!(result.kind, WriteFailureKind::NotApplied);
    assert!(!result.resume_source);
    assert_eq!(writer.load(uuid).await.unwrap().unwrap().health, 3.0);
}
