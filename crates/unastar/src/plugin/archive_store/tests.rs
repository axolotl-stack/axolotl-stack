use super::*;
use crate::entity::components::{ENDER_CHEST_SIZE, PLAYER_UI_SIZE};
use crate::storage::{PlayerData, StoredItemStack};

/// A fully validated native snapshot with deliberately opaque item bytes.
fn record(plugin: &str) -> Record {
    let mut item = StoredItemStack {
        item_id: "minecraft:diamond".into(),
        runtime_id: 1,
        count: 3,
        damage: 0,
        nbt: Some(vec![255, 255, 1, 10, 0, 0, 0, 42, 19]),
    };
    let mut main = vec![StoredItemStack::default(); 36];
    main[4] = item.clone();
    item.count = 2;
    let mut ui = vec![StoredItemStack::default(); PLAYER_UI_SIZE];
    ui[0] = item.clone();
    let mut ender = vec![StoredItemStack::default(); ENDER_CHEST_SIZE];
    ender[7] = item;
    let state = PlayerSnapshot {
        version: 1,
        data: PlayerData {
            uuid: uuid::Uuid::from_u128(1).to_string(),
            position: [12.5, 66.62, -7.5],
            inventory: main,
            cursor: ui[0].clone(),
            ui: ui.clone(),
            ender_chest: ender,
            health: 7.0,
            food: 14,
            saturation: 3.5,
            exhaustion: 1.5,
            ..Default::default()
        },
        base_game_mode: Some(0),
        flight_override: Some(false),
        xuid: Some("12345".into()),
        ui,
        velocity: [0.25, -0.5, 0.75],
        head_yaw: 12.0,
        absorption: 2.0,
        air_current: 200,
        air_max: 300,
        on_fire_ticks: 11,
        burning_remaining: 15,
        burning_next: 2,
        effects: Vec::new(),
        dead: false,
        food_policy: None,
        food_ticks: 0,
        food_regeneration: 0,
    };
    Record::new(
        plugin,
        Owner {
            uuid: state.data.uuid.clone(),
            authenticated_xuid: Some(12345),
            name: "Example".into(),
            connection_id: Some(3),
        },
        SourceWorld {
            uuid: uuid::Uuid::from_u128(9).to_string(),
            name: "sample-overworld".into(),
            dimension: 0,
        },
        42,
        state,
    )
    .unwrap()
}

#[test]
fn complete_native_state_survives_reopen_with_original_identity_and_opaque_items() {
    let root = tempfile::tempdir().unwrap();
    let original = record("sample.policy");
    let mut registry = Registry::new(root.path().into(), storage::Limits::default());
    let namespace = registry.namespace("sample.policy").unwrap();
    let reference = namespace
        .lock()
        .unwrap()
        .capture("checkpoint-1", &original)
        .unwrap();
    assert_eq!(reference.revision, 1);
    assert_eq!(reference.fingerprint.len(), 64);
    let reload = registry.namespace("sample.policy").unwrap();
    assert!(Arc::ptr_eq(&namespace, &reload));
    let mut other_process = Registry::new(root.path().into(), storage::Limits::default());
    assert!(matches!(
        other_process.namespace("sample.policy"),
        Err(ArchiveError::IoError)
    ));
    drop(reload);
    drop(namespace);
    drop(registry);
    let mut reopened = Registry::new(root.path().into(), storage::Limits::default());
    let namespace = reopened.namespace("sample.policy").unwrap();
    let loaded = namespace.lock().unwrap().read(&reference).unwrap();
    assert!(loaded.record == original);
    assert_eq!(loaded.bytes, serde_json::to_vec(&original).unwrap().len());
    assert_eq!(
        loaded.record.state.data.inventory[4].nbt,
        original.state.data.inventory[4].nbt
    );
    assert_eq!(
        loaded.record.state.data.ender_chest[7].nbt,
        original.state.data.ender_chest[7].nbt
    );
    assert_eq!(loaded.record.state.ui[0].nbt, original.state.ui[0].nbt);
}

#[test]
fn immutable_keys_reject_overwrite_and_every_forged_reference_field() {
    let root = tempfile::tempdir().unwrap();
    let mut registry = Registry::new(root.path().into(), storage::Limits::default());
    let namespace = registry.namespace("sample.policy").unwrap();
    let mut namespace = namespace.lock().unwrap();
    let mut original = record("sample.policy");
    let reference = namespace.capture("saved", &original).unwrap();
    original.state.data.health = 1.0;
    assert_eq!(
        namespace.capture("saved", &original),
        Err(ArchiveError::Conflict)
    );
    assert_eq!(namespace.lookup("saved").unwrap(), Some(reference.clone()));
    assert_eq!(
        namespace.read(&reference).unwrap().record.state.data.health,
        7.0
    );
    for variant in 0..5 {
        let mut invalid = reference.clone();
        match variant {
            0 => invalid.revision += 1,
            1 => invalid.fingerprint.replace_range(..1, "z"),
            2 => invalid.owner.uuid = uuid::Uuid::from_u128(2).to_string(),
            3 => invalid.owner.authenticated_xuid = Some(23456),
            _ => invalid.owner.connection_id = Some(4),
        }
        assert!(matches!(
            namespace.read(&invalid),
            Err(ArchiveError::InvalidReference)
        ));
    }
    let same = namespace.read(&reference).unwrap().record;
    let other = namespace.capture("same-data", &same).unwrap();
    assert_ne!(reference.fingerprint, other.fingerprint);
    let mut renamed = reference.clone();
    renamed.key = other.key;
    assert!(matches!(
        namespace.read(&renamed),
        Err(ArchiveError::InvalidReference)
    ));
    let mut missing = reference;
    missing.key = "missing".into();
    assert!(matches!(
        namespace.read(&missing),
        Err(ArchiveError::NotFound)
    ));
}

#[test]
fn native_archive_reopen_preserves_fractional_position_bits() {
    let root = tempfile::tempdir().unwrap();
    let mut registry = Registry::new(root.path().into(), storage::Limits::default());
    let namespace = registry.namespace("sample.policy").unwrap();
    let mut saved = Vec::new();
    for index in 1..=32 {
        let fraction = f64::from(index as f32 / 10.0);
        let mut original = record("sample.policy");
        original.state.data.position = [fraction, 64.0 + fraction, -fraction];
        let reference = namespace
            .lock()
            .unwrap()
            .capture(&format!("fraction-{index}"), &original)
            .unwrap();
        saved.push((reference, original));
    }
    let mut zero = record("sample.policy");
    zero.state.data.position = [-0.0, 64.0, 0.0];
    let reference = namespace
        .lock()
        .unwrap()
        .capture("signed-zero", &zero)
        .unwrap();
    saved.push((reference, zero));
    drop(namespace);
    drop(registry);

    let mut reopened = Registry::new(root.path().into(), storage::Limits::default());
    let namespace = reopened.namespace("sample.policy").unwrap();
    for (reference, original) in saved {
        let loaded = namespace.lock().unwrap().read(&reference).unwrap();
        assert_eq!(
            loaded.record.state.data.position.map(f64::to_bits),
            original.state.data.position.map(f64::to_bits),
            "native coordinates must retain their captured bits through durable storage"
        );
        assert!(loaded.record == original);
    }
}

#[test]
fn module_namespaces_cannot_read_or_import_each_others_native_payload() {
    let root = tempfile::tempdir().unwrap();
    let mut registry = Registry::new(root.path().into(), storage::Limits::default());
    let first = registry.namespace("sample.first").unwrap();
    let reference = first
        .lock()
        .unwrap()
        .capture("saved", &record("sample.first"))
        .unwrap();
    let second = registry.namespace("sample.second").unwrap();
    assert!(matches!(
        second.lock().unwrap().read(&reference),
        Err(ArchiveError::NotFound)
    ));
    let bytes = serde_json::to_vec(&record("sample.first")).unwrap();
    second
        .lock()
        .unwrap()
        .backend
        .lock()
        .unwrap()
        .compare_exchange("forged", None, &bytes)
        .unwrap();
    assert!(matches!(
        registry.namespace("sample.second"),
        Err(ArchiveError::InvalidSnapshot)
    ));
}

#[test]
fn semantic_corruption_and_future_native_versions_reject_namespace_open_instead_of_resetting() {
    for variant in 0..5 {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::new(root.path().into(), storage::Limits::default());
        let namespace = registry.namespace("sample.policy").unwrap();
        let mut value = record("sample.policy");
        let bytes = match variant {
            0 => b"{}".to_vec(),
            1 => {
                value.state.version += 1;
                serde_json::to_vec(&value).unwrap()
            }
            2 => {
                value.owner.authenticated_xuid = Some(2);
                serde_json::to_vec(&value).unwrap()
            }
            3 => {
                value.version += 1;
                serde_json::to_vec(&value).unwrap()
            }
            _ => {
                let mut json = serde_json::to_value(&value).unwrap();
                json["state"]["data"]["unknown-native-field"] = serde_json::Value::Bool(true);
                serde_json::to_vec(&json).unwrap()
            }
        };
        namespace
            .lock()
            .unwrap()
            .backend
            .lock()
            .unwrap()
            .compare_exchange("broken", None, &bytes)
            .unwrap();
        assert_eq!(
            namespace.lock().unwrap().lookup("broken"),
            Err(ArchiveError::InvalidSnapshot),
            "semantic read variant {variant}"
        );
        drop(namespace);
        drop(registry);
        let mut reopened = Registry::new(root.path().into(), storage::Limits::default());
        assert!(
            reopened.namespace("sample.policy").is_err(),
            "corrupt variant {variant} unexpectedly opened"
        );
    }
}

#[test]
fn malformed_owned_death_facts_reject_lookup_and_fresh_namespace_open() {
    for variant in 0..4 {
        let root = tempfile::tempdir().unwrap();
        let mut original = record("sample.policy");
        original.state.dead = true;
        original.state.data.health = 0.0;
        let original = original
            .with_death(DeathFacts {
                capture_id: uuid::Uuid::from_u128(3).to_string(),
                phase: DeathPhase::CommittedBeforeDrops,
                cause: unastar_data::actor_components::body::ActorDamageCause::Override.into(),
                attribution: DeathAttribution {
                    source_dimension: Some(0),
                    source_world_lifetime: None,
                    owner: None,
                },
                fall_distance: None,
                keep_inventory: false,
            })
            .unwrap();
        let mut json = serde_json::to_value(original).unwrap();
        match variant {
            0 => json["death"]["phase"] = serde_json::json!("after-drops"),
            1 => json["death"]["cause"] = serde_json::json!("unknown-cause"),
            2 => {
                json["death"]["attribution"]["source_world_lifetime"] =
                    serde_json::json!("not-a-world-lifetime")
            }
            _ => json["state"]["dead"] = serde_json::json!(false),
        }
        let mut registry = Registry::new(root.path().into(), storage::Limits::default());
        let namespace = registry.namespace("sample.policy").unwrap();
        namespace
            .lock()
            .unwrap()
            .backend
            .lock()
            .unwrap()
            .compare_exchange("broken", None, &serde_json::to_vec(&json).unwrap())
            .unwrap();
        assert_eq!(
            namespace.lock().unwrap().lookup("broken"),
            Err(ArchiveError::InvalidSnapshot)
        );
        drop(namespace);
        drop(registry);
        let mut reopened = Registry::new(root.path().into(), storage::Limits::default());
        assert!(reopened.namespace("sample.policy").is_err());
    }
}

#[test]
fn quota_rejection_and_failed_installation_do_not_create_successful_references() {
    let root = tempfile::tempdir().unwrap();
    let limits = storage::Limits {
        keys: 1,
        ..Default::default()
    };
    let mut registry = Registry::new(root.path().into(), limits);
    let namespace = registry.namespace("sample.policy").unwrap();
    let mut namespace = namespace.lock().unwrap();
    let record = record("sample.policy");
    namespace
        .backend
        .lock()
        .unwrap()
        .inject_fault(storage::Fault::BeforeRename);
    assert_eq!(
        namespace.capture("saved", &record),
        Err(ArchiveError::IoError)
    );
    assert_eq!(namespace.lookup("saved").unwrap(), None);
    namespace.capture("saved", &record).unwrap();
    assert_eq!(
        namespace.capture("other", &record),
        Err(ArchiveError::QuotaExceeded)
    );
    assert_eq!(namespace.lookup("other").unwrap(), None);
    for invalid in ["", "..", "../escape", "a/b"] {
        assert_eq!(
            namespace.capture(invalid, &record),
            Err(ArchiveError::InvalidKey)
        );
    }
}

#[test]
fn uncertain_installation_requires_lookup_and_reopens_as_the_same_immutable_capture() {
    let root = tempfile::tempdir().unwrap();
    let mut registry = Registry::new(root.path().into(), storage::Limits::default());
    let namespace = registry.namespace("sample.policy").unwrap();
    let original = record("sample.policy");
    let reference = {
        let mut namespace = namespace.lock().unwrap();
        namespace
            .backend
            .lock()
            .unwrap()
            .inject_fault(storage::Fault::AfterRename);
        assert_eq!(
            namespace.capture("saved", &original),
            Err(ArchiveError::Uncertain)
        );
        assert_eq!(
            namespace.capture("different", &original),
            Err(ArchiveError::Uncertain)
        );
        namespace.lookup("saved").unwrap().unwrap()
    };
    drop(namespace);
    drop(registry);
    let mut reopened = Registry::new(root.path().into(), storage::Limits::default());
    let namespace = reopened.namespace("sample.policy").unwrap();
    let loaded = namespace.lock().unwrap().read(&reference).unwrap();
    assert!(loaded.record == original);
    assert_eq!(
        namespace.lock().unwrap().capture("saved", &original),
        Err(ArchiveError::Conflict)
    );
}

#[test]
fn archive_roots_cannot_enter_a_component_filesystem_preopen() {
    let root = tempfile::tempdir().unwrap();
    let plugin = root.path().join("plugin");
    std::fs::create_dir(&plugin).unwrap();
    for path in [plugin.clone(), plugin.join("native-state")] {
        let registry = Registry::new(path, storage::Limits::default());
        assert_eq!(
            registry.validate_directory(&plugin),
            Err(ArchiveError::IoError)
        );
    }
}

#[test]
fn native_record_value_and_encoded_namespace_limits_are_both_enforced_before_installation() {
    let original = record("sample.policy");
    let bytes = serde_json::to_vec(&original).unwrap().len();
    for limits in [
        storage::Limits {
            value_bytes: bytes - 1,
            ..Default::default()
        },
        storage::Limits {
            namespace_bytes: bytes as u64 - 1,
            ..Default::default()
        },
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::new(root.path().into(), limits);
        let namespace = registry.namespace("sample.policy").unwrap();
        let mut namespace = namespace.lock().unwrap();
        assert_eq!(
            namespace.capture("saved", &original),
            Err(ArchiveError::QuotaExceeded)
        );
        assert_eq!(namespace.lookup("saved").unwrap(), None);
    }
}

#[test]
fn raw_guest_storage_cannot_alias_native_archive_files_even_through_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let private = root.path().join("private");
    std::fs::create_dir(&private).unwrap();
    let archive = Registry::new(private.clone(), storage::Limits::default());
    for directory in [
        private.clone(),
        private.join("nested"),
        root.path().to_path_buf(),
    ] {
        let raw = storage::Registry::new(directory, storage::Limits::default());
        assert_eq!(archive.validate_storage(&raw), Err(ArchiveError::IoError));
    }
    let raw = storage::Registry::new(root.path().join("separate"), storage::Limits::default());
    archive.validate_storage(&raw).unwrap();
    #[cfg(unix)]
    {
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&private, &alias).unwrap();
        let raw = storage::Registry::new(alias, storage::Limits::default());
        assert_eq!(archive.validate_storage(&raw), Err(ArchiveError::IoError));
    }
}
