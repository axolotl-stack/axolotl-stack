//! Genuine Rust components capture, reopen and read immutable native player state.

use super::*;
use crate::entity::components::*;
use crate::plugin::archive_store;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig, player_state};
use std::{
    path::Path,
    process::Command,
    sync::{Arc, Mutex, OnceLock},
};

const ID: &str = "sample.archive";

/// Build independently of the outer native Cargo artifact lock.
fn bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let manifest =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/player-archive/Cargo.toml");
        let target = tempfile::tempdir().unwrap();
        let mut command = Command::new(env!("CARGO"));
        command
            .args([
                "build",
                "--locked",
                "--target",
                "wasm32-wasip2",
                "--manifest-path",
            ])
            .arg(manifest)
            .env("CARGO_TARGET_DIR", target.path())
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS");
        if let Some(toolchain) = option_env!("RUSTUP_TOOLCHAIN") {
            command.env("RUSTUP_TOOLCHAIN", toolchain);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "archive fixture build failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read(
            target
                .path()
                .join("wasm32-wasip2/debug/player_archive_fixture.wasm"),
        )
        .unwrap()
    })
}

/// Real native ownership, complete stores and a real component loader without listeners.
struct Fixture {
    server: GameServer,
    manager: Arc<Mutex<PluginManager>>,
    player: Option<Entity>,
    _receiver: Option<tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>>,
}
impl Fixture {
    /// Reopening uses the same immutable archive directory and a fresh component/world instance.
    fn open(
        root: &Path,
        mode: &str,
        spawn: bool,
        keys: usize,
        resources: u32,
    ) -> anyhow::Result<Self> {
        let directory = root.join("plugins/sample");
        std::fs::create_dir_all(&directory)?;
        let archive_cap = if mode == "denied" {
            ""
        } else {
            ", 'player-state-archives'"
        };
        std::fs::write(
            directory.join("plugin.toml"),
            format!(
                r#"
id = 'sample.archive'
name = 'Sample archive'
version = '1'
api_version = '0.4.0'
required = true
config = 'mode.txt'
capabilities = ['read-players', 'read-inventory', 'item-values'{archive_cap}]
subscriptions = ['player_command']
[limits]
max_owned_resources = {resources}
"#
            ),
        )?;
        std::fs::write(directory.join("mode.txt"), mode)?;
        std::fs::write(directory.join("plugin.wasm"), bytes())?;
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        server
            .ecs
            .world_mut()
            .insert_resource(crate::world::WorldIdentity {
                id: uuid::Uuid::from_u128(9),
                name: "sample-overworld".into(),
                dimension: 0,
            });
        let (player, receiver) = if spawn {
            let (player, receiver) = spawn_player(&mut server, 7, 1, "12345", "Example");
            populate(&mut server, player);
            (Some(player), Some(receiver))
        } else {
            (None, None)
        };
        let mut manager = PluginManager {
            engine: engine(),
            plugins: Default::default(),
            pending: Vec::new(),
            tick_prepared: false,
            tick_budget_valid: true,
            readiness: Default::default(),
            storage_registry: Default::default(),
            archive_registry: Default::default(),
            command_roots: Default::default(),
            ui_registry: Default::default(),
            view_registry: Default::default(),
        };
        manager.configure_storage(&crate::config::PluginStorageConfig {
            directory: root.join("guest-store"),
            ..Default::default()
        })?;
        manager.configure_archives(&crate::config::PluginArchiveConfig {
            directory: root.join("archives"),
            max_keys: keys,
            ..Default::default()
        })?;
        manager.load_plugins(&root.join("plugins"))?;
        let manager = Arc::new(Mutex::new(manager));
        PluginManager::attach_to_world(manager.clone(), server.ecs.world_mut());
        manager
            .lock()
            .unwrap()
            .initialize_startup(server.ecs.world_mut())
            .map_err(anyhow::Error::msg)?;
        Ok(Self {
            server,
            manager,
            player,
            _receiver: receiver,
        })
    }
    /// Use the exact production native command boundary, never slash-chat or a synthetic player.
    fn command(&mut self, line: &str) {
        assert!(commands::dispatch(
            self.server.ecs.world_mut(),
            self.player.unwrap(),
            line,
            None
        ));
    }
    /// Inspect only bounded fixture diagnostics.
    fn logged(&self, expected: &str) -> bool {
        self.manager
            .lock()
            .unwrap()
            .plugins
            .get(&PluginId(ID.into()))
            .unwrap()
            .store
            .data()
            .test_logs
            .iter()
            .any(|log| log == expected)
    }
    /// Re-read the exact acknowledged host record without changing it.
    fn record(&self, key: &str) -> Option<archive_store::Loaded> {
        let manager = self.manager.lock().unwrap();
        let namespace = manager
            .archive_registry
            .lock()
            .unwrap()
            .namespace(ID)
            .unwrap();
        let mut namespace = namespace.lock().unwrap();
        namespace
            .lookup(key)
            .unwrap()
            .map(|reference| namespace.read(&reference).unwrap())
    }
}

/// Native initialization creates the actual connected actor and all physical inventories.
fn spawn_player(
    server: &mut GameServer,
    session: u64,
    uuid: u128,
    xuid: &str,
    name: &str,
) -> (
    Entity,
    tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
) {
    let (tx, rx) = tokio::sync::mpsc::channel(65_536);
    let entity = server
        .spawn_player(PlayerSpawnData {
            session_id: session,
            runtime_id: session as i64,
            display_name: name.into(),
            xuid: Some(xuid.into()),
            uuid: Some(uuid::Uuid::from_u128(uuid).to_string()),
            position: glam::DVec3::new(12.5, 66.62, -7.5),
            outbound_tx: tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    (entity, rx)
}
/// Every authoritative area carries observable metadata; capture must preserve all native bytes.
fn populate(server: &mut GameServer, entity: Entity) {
    let world = server.ecs.world_mut();
    let registry = world
        .resource::<crate::server::game::types::ItemRegistryResource>()
        .0
        .clone();
    let stack = |count| {
        let stack = registry.stack("minecraft:diamond", count).unwrap();
        crate::item::enchant::with_item_tag(
            &stack,
            zuri_nbt::tag::Compound::builder()
                .with_compound(
                    "display",
                    zuri_nbt::tag::Compound::builder().with_string("Name", "Archive"),
                )
                .with_byte_array("unknown-property", vec![3, 1, 4])
                .build(),
        )
    };
    world
        .get_mut::<MainInventory>(entity)
        .unwrap()
        .0
        .slots_mut()[4] = stack(3);
    world
        .get_mut::<ArmourInventory>(entity)
        .unwrap()
        .inventory_mut()
        .slots_mut()[0] = stack(2);
    world.get_mut::<OffhandSlot>(entity).unwrap().0 = stack(1);
    world.get_mut::<PlayerUi>(entity).unwrap().0.slots_mut()[0] = stack(5);
    world.get_mut::<PlayerUi>(entity).unwrap().0.slots_mut()[1] = stack(6);
    world
        .get_mut::<EnderChestInventory>(entity)
        .unwrap()
        .0
        .slots_mut()[7] = stack(7);
    world.get_mut::<HeldSlot>(entity).unwrap().set(4);
    world.get_mut::<Health>(entity).unwrap().current = 7.0;
    *world.get_mut::<Hunger>(entity).unwrap() = Hunger {
        food_level: 14,
        saturation: 3.5,
        exhaustion: 1.5,
    };
    world.entity_mut(entity).insert(GameMode::Survival);
    assert!(crate::command::exec::set_mode_override(
        world,
        entity,
        Some(GameMode::Creative)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_capture_is_readonly_and_complete_across_process_style_reopen() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::open(root.path(), "capture", true, 1024, 4096).unwrap();
    let before =
        player_state::capture(fixture.server.ecs.world(), fixture.player.unwrap()).unwrap();
    fixture.command("archive-save first");
    assert!(fixture.logged("archive:captured"));
    assert_eq!(
        before,
        player_state::capture(fixture.server.ecs.world(), fixture.player.unwrap()).unwrap()
    );
    assert_eq!(before, fixture.record("first").unwrap().record.state);
    drop(fixture);
    let fixture = Fixture::open(root.path(), "read", false, 1024, 4096).unwrap();
    assert!(fixture.logged("archive:reloaded"));
    assert_eq!(before, fixture.record("first").unwrap().record.state);
    let players = fixture
        .server
        .ecs
        .world()
        .try_query::<&Player>()
        .map_or(0, |mut query| {
            query.iter(fixture.server.ecs.world()).count()
        });
    assert_eq!(players, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_capture_ack_survives_a_later_required_guest_trap() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::open(root.path(), "trap", true, 1024, 4096).unwrap();
    let before =
        player_state::capture(fixture.server.ecs.world(), fixture.player.unwrap()).unwrap();
    fixture.command("archive-save first");
    assert!(fixture.manager.lock().unwrap().readiness.check().is_err());
    assert_eq!(before, fixture.record("first").unwrap().record.state);
    drop(fixture);
    let fixture = Fixture::open(root.path(), "read", false, 1024, 4096).unwrap();
    assert!(fixture.logged("archive:reloaded"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_capability_denial_performs_no_archive_write_or_player_mutation() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::open(root.path(), "denied", true, 1024, 4096).unwrap();
    let before =
        player_state::capture(fixture.server.ecs.world(), fixture.player.unwrap()).unwrap();
    fixture.command("archive-save first");
    assert!(fixture.logged("archive:denied"));
    assert!(!root.path().join("archives").exists());
    assert_eq!(
        before,
        player_state::capture(fixture.server.ecs.world(), fixture.player.unwrap()).unwrap()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_conflict_and_namespace_quota_never_overwrite_the_first_capture() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::open(root.path(), "capture", true, 1, 4096).unwrap();
    fixture.command("archive-save first");
    fixture
        .server
        .ecs
        .world_mut()
        .get_mut::<Health>(fixture.player.unwrap())
        .unwrap()
        .current = 4.0;
    fixture.command("archive-save first");
    assert!(fixture.logged("archive:conflict"));
    fixture.command("archive-save second");
    assert!(fixture.logged("archive:quota"));
    assert!(fixture.record("second").is_none());
    assert_eq!(
        fixture.record("first").unwrap().record.state.data.health,
        7.0
    );
    assert!(fixture.manager.lock().unwrap().readiness.check().is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_retained_archive_keeps_past_identity_after_source_actor_removal() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::open(root.path(), "capture", true, 1024, 4096).unwrap();
    fixture.command("archive-save first");
    fixture
        .server
        .ecs
        .world_mut()
        .despawn(fixture.player.unwrap());
    let (replacement, receiver) = spawn_player(&mut fixture.server, 8, 2, "54321", "Replacement");
    fixture.player = Some(replacement);
    fixture._receiver = Some(receiver);
    let before = player_state::capture(fixture.server.ecs.world(), replacement).unwrap();
    fixture.command("archive-inspect");
    assert!(fixture.logged("archive:retained"));
    assert_eq!(
        before,
        player_state::capture(fixture.server.ecs.world(), replacement).unwrap()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_reference_account_validation_and_resource_release_survive_reload() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::open(root.path(), "capture", true, 1024, 4096).unwrap();
    fixture.command("archive-save first");
    drop(fixture);
    let fixture = Fixture::open(root.path(), "bad-reference", false, 1024, 4096).unwrap();
    assert!(fixture.logged("archive:bad-reference-rejected"));
    drop(fixture);
    let fixture = Fixture::open(root.path(), "resource-limit", false, 1024, 1).unwrap();
    assert!(fixture.logged("archive:resource-limit-released"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn corrupt_native_archive_stops_required_component_loading_instead_of_resetting() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::open(root.path(), "capture", true, 1024, 4096).unwrap();
    fixture.command("archive-save first");
    drop(fixture);
    let namespace = std::fs::read_dir(root.path().join("archives"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let path = std::fs::read_dir(namespace)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "blob")
        })
        .unwrap();
    let mut corrupt = std::fs::read(&path).unwrap();
    *corrupt.last_mut().unwrap() ^= 1;
    std::fs::write(&path, &corrupt).unwrap();
    assert!(Fixture::open(root.path(), "read", false, 1024, 4096).is_err());
    assert_eq!(std::fs::read(path).unwrap(), corrupt);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_native_inventory_is_rejected_before_capture_and_leaves_no_owned_charge_or_archive()
 {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::open(root.path(), "oversized", true, 1024, 4096).unwrap();
    let player = fixture.player.unwrap();
    let mut stack = fixture
        .server
        .ecs
        .world()
        .get::<MainInventory>(player)
        .unwrap()
        .0
        .item(4)
        .unwrap()
        .clone();
    stack = stack.with_nbt(vec![42; archive_store::MAX_SNAPSHOT_BYTES + 1]);
    let original = stack.nbt.as_ref().unwrap().as_ptr();
    fixture
        .server
        .ecs
        .world_mut()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .slots_mut()[4] = stack;
    fixture.command("archive-save first");
    assert!(fixture.logged("archive:oversized-rejected"));
    assert!(fixture.record("first").is_none());
    assert_eq!(
        fixture
            .server
            .ecs
            .world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(4)
            .unwrap()
            .nbt
            .as_ref()
            .unwrap()
            .as_ptr(),
        original
    );
    let manager = fixture.manager.lock().unwrap();
    assert!(manager.readiness.check().is_ok());
    assert_eq!(
        manager
            .plugins
            .get(&PluginId(ID.into()))
            .unwrap()
            .store
            .data()
            .resources
            .value_budget
            .used(),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminally_withheld_source_returns_unavailable_without_creating_a_healthy_or_empty_archive()
 {
    use crate::server::game::inventory_effects::{RecoverySource, fence_for_recovery};
    use wit_bindings::unastar::plugin::types::HostPlayer;
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::open(root.path(), "capture", true, 1024, 4096).unwrap();
    let player = fixture.player.unwrap();
    let source = RecoverySource::capture(fixture.server.ecs.world(), player).unwrap();
    fence_for_recovery(fixture.server.ecs.world_mut(), &source).unwrap();
    let mut manager = fixture.manager.lock().unwrap();
    let plugin = manager.plugins.get_mut(&PluginId(ID.into())).unwrap();
    let result = with_world(&mut plugin.store, fixture.server.ecs.world_mut(), |store| {
        let resource = make_player_resource(store, player).unwrap();
        HostPlayer::archive(store.data_mut(), resource, "withheld".into())
    });
    assert!(matches!(
        result,
        Err(wit_types::ArchiveError::Access(
            wit_types::AccessError::Unavailable
        ))
    ));
    assert!(fixture.server.ecs.world().get::<Health>(player).is_none());
    assert!(
        fixture
            .server
            .ecs
            .world()
            .get::<MainInventory>(player)
            .is_none()
    );
    assert_eq!(plugin.store.data().resources.value_budget.used(), 0);
    drop(manager);
    assert!(fixture.record("withheld").is_none());
}
