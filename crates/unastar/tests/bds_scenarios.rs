//! BDS parity scenarios: every scenario retained under
//! `crates/client-diff/scenarios/` is run against unastar with the same
//! client steps the gophertunnel bot sent to BDS 1.26.51, and the server
//! packets are compared with the recording (see `crates/client-diff/README.md`).
//!
//! The server runs in-process on a loopback port with the world the BDS
//! recordings used: the default superflat (bedrock at y=-64, two dirt, grass
//! at y=-61), the player spawning at feet (0.5, -60, 0.5) as an operator.
//!
//! Exclusive loopback socket runs: take the native lock first
//! (`native-lock.sh acquire <lane> ...`).
//!
//! The scenario comparisons measure parity rather than guard regressions:
//! they are ignored in the default `cargo test` and run with
//! `cargo test -p unastar --all-features --test bds_scenarios -- --ignored`.
//! `every_retained_scenario_runs` stays in the default run.

use std::io::Write;
use std::net::{SocketAddr, UdpSocket};
use std::path::Path;
use std::sync::{Arc, Mutex};

use client_diff::driver;
use client_diff::parity::{Case, RuleValue, Side, WorldKind, scenarios_root, shared_script};
use client_diff::scenario::ScenariosFile;
use unastar::config::{GameRuleSetting, SpawnLocation, SpawnRule, UnastarConfig};
use unastar::storage::{LevelDBPlayerProvider, PlayerData, PlayerProvider, StoredItemStack};
use unastar::world::rules::Difficulty;

/// One server at a time: the server works in the process's current directory.
static SERIAL: Mutex<()> = Mutex::new(());

fn free_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A `Write` the test reads back after the run.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// unastar configured like the world the BDS recording joined.
fn config(case: &Case, port: u16, operator: &str, players: &Path) -> UnastarConfig {
    let world = &case.manifest.world;
    let mut config = UnastarConfig::default();
    config.server.bind_address = format!("127.0.0.1:{port}");
    config.server.online_mode = false;
    config.server.max_chunk_radius = 4;
    config.server.operators = vec![operator.into()];
    config.server.default_gamemode = world.game_mode.clone();
    config.server.difficulty = match world.difficulty.as_str() {
        "peaceful" => Difficulty::Peaceful,
        "easy" => Difficulty::Easy,
        "normal" => Difficulty::Normal,
        "hard" => Difficulty::Hard,
        other => panic!("{}: unknown difficulty {other:?}", case.name),
    };
    config.server.game_rules = world
        .game_rules
        .iter()
        .map(|(name, value)| {
            let setting = match *value {
                RuleValue::Bool(value) => GameRuleSetting::Bool(value),
                RuleValue::Int(value) => GameRuleSetting::Int(value),
                RuleValue::Float(value) => GameRuleSetting::Float(value),
            };
            (name.clone(), setting)
        })
        .collect();
    config.players.data_dir = players.to_owned();
    match world.kind {
        WorldKind::Superflat => {
            config.world.generator = unastar::world::WorldGenerator::VanillaFlat;
            // Spawn locations are eye positions: feet at y=-60, as on BDS
            // (whose StartGame and MovePlayer carry the eye height).
            config.spawn_rules = vec![SpawnRule {
                always_at_location: true,
                location: Some(SpawnLocation {
                    x: 0.5,
                    y: -60.0 + 1.62,
                    z: 0.5,
                    ..SpawnLocation::default()
                }),
                ..SpawnRule::default()
            }];
        }
    }
    config
        .validate()
        .unwrap_or_else(|error| panic!("{}: world setup unsupported: {error}", case.name));
    config
}

/// Saves the scenario player as a known player with the recording's
/// enchanting seed (so enchanting offers can match) and the world's game
/// mode; everything else is a fresh player's.
fn seed_player(case: &Case, uuid: uuid::Uuid, players: &Path) {
    let game_mode = match case.manifest.world.game_mode.as_str() {
        "survival" => 0,
        "creative" => 1,
        "adventure" => 2,
        other => panic!("{}: unknown game mode {other:?}", case.name),
    };
    let data = PlayerData {
        uuid: uuid.to_string(),
        position: [0.5, -60.0, 0.5],
        game_mode,
        inventory: vec![StoredItemStack::default(); 36],
        ender_chest: vec![StoredItemStack::default(); 27],
        enchantment_seed: Some(case.manifest.source.player_enchantment_seed as u32),
        ..PlayerData::default()
    };
    let provider = LevelDBPlayerProvider::open(players.join("db")).unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        provider.save(uuid, &data).await.unwrap();
        provider.close().await.unwrap();
    });
}

fn run_case(name: &str) {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let case = Case::load(&scenarios_root().join(name)).unwrap();
    let file = ScenariosFile::load(&shared_script()).unwrap();
    let script = case.script(&file).unwrap();
    let directory = tempfile::tempdir().unwrap();
    std::env::set_current_dir(directory.path()).unwrap();
    seed_player(&case, file.identity.uuid, &directory.path().join("players"));
    let port = free_port();
    let config = config(
        &case,
        port,
        &file.identity.display_name,
        &directory.path().join("players"),
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_stack_size(64 * 1024 * 1024)
        .build()
        .unwrap();
    let capture = Capture::default();
    let outcome = runtime.block_on(async {
        let mut server = unastar::server::UnastarServer::new(Arc::new(config))
            .await
            .unwrap();
        let server_task =
            tokio::spawn(async move { server.run().await.map_err(|e| e.to_string()) });
        let address: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        // The listener binds asynchronously: wait until the port is taken.
        for _ in 0..300 {
            if UdpSocket::bind(address).is_err() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let outcome =
            driver::run(address, &file.identity, &script, Box::new(capture.clone())).await;
        server_task.abort();
        outcome
    });
    let outcome = outcome.unwrap_or_else(|error| panic!("{name}: driver failed: {error:#}"));
    let text = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    let records = client_diff::parse(&text, "unastar run").unwrap();
    let comparison = client_diff::parity::compare(
        &case,
        &script,
        &Side {
            records: &records,
            runtime_id: outcome.runtime_id,
            unique_id: outcome.unique_id,
        },
        &outcome.notes,
    )
    .unwrap();
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("bds-scenarios");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join(format!("{name}.md")), &comparison.report).unwrap();
    std::fs::write(out.join(format!("{name}-unastar.jsonl")), &text).unwrap();
    assert!(comparison.passed(), "{}", comparison.report);
}

macro_rules! scenarios {
    ($($test:ident => $dir:literal),* $(,)?) => {
        $(
            #[test]
            #[ignore = "BDS parity measurement; run with: cargo test -p unastar --all-features --test bds_scenarios -- --ignored (see tools/client-diff/README.md)"]
            fn $test() {
                run_case($dir);
            }
        )*

        /// Every retained scenario has a test here.
        #[test]
        fn every_retained_scenario_runs() {
            let mut listed = vec![$($dir),*];
            listed.sort_unstable();
            let retained: Vec<String> = Case::all().unwrap().into_iter().map(|case| case.name).collect();
            assert_eq!(listed, retained);
        }
    };
}

scenarios! {
    anvil => "anvil",
    blocks_attachables => "blocks-attachables",
    blocks_bed => "blocks-bed",
    blocks_beds => "blocks-beds",
    blocks_beehive => "blocks-beehive",
    blocks_bell => "blocks-bell",
    blocks_buttons => "blocks-buttons",
    blocks_cake => "blocks-cake",
    blocks_campfire => "blocks-campfire",
    blocks_cauldron => "blocks-cauldron",
    blocks_comparator_container => "blocks-comparator-container",
    blocks_comparators => "blocks-comparators",
    blocks_composter => "blocks-composter",
    blocks_connections => "blocks-connections",
    blocks_crop_care => "blocks-crop-care",
    blocks_crops => "blocks-crops",
    blocks_dispensers => "blocks-dispensers",
    blocks_doors => "blocks-doors",
    blocks_end_portal => "blocks-end-portal",
    blocks_falling_blocks => "blocks-falling-blocks",
    blocks_fence_gates => "blocks-fence-gates",
    blocks_fire => "blocks-fire",
    blocks_flower_pot => "blocks-flower-pot",
    blocks_iron_door_lever => "blocks-iron-door-lever",
    blocks_jukebox => "blocks-jukebox",
    blocks_nether_portal => "blocks-nether-portal",
    blocks_note_block_use => "blocks-note-block-use",
    blocks_note_blocks => "blocks-note-blocks",
    blocks_observer => "blocks-observer",
    blocks_pistons => "blocks-pistons",
    blocks_plant_survival => "blocks-plant-survival",
    blocks_pressure_plates => "blocks-pressure-plates",
    blocks_rails => "blocks-rails",
    blocks_redstone_dust => "blocks-redstone-dust",
    blocks_redstone_torch => "blocks-redstone-torch",
    blocks_repeaters => "blocks-repeaters",
    blocks_respawn_anchor => "blocks-respawn-anchor",
    blocks_signs => "blocks-signs",
    blocks_slabs => "blocks-slabs",
    blocks_stairs => "blocks-stairs",
    blocks_tnt => "blocks-tnt",
    blocks_torches => "blocks-torches",
    blocks_trapdoors => "blocks-trapdoors",
    blocks_weighted_plates => "blocks-weighted-plates",
    bow_shot => "bow-shot",
    brewing => "brewing",
    drink_milk => "drink-milk",
    drink_water_bottle => "drink-water-bottle",
    eat_food => "eat-food",
    eat_stew => "eat-stew",
    elytra => "elytra",
    enchanting => "enchanting",
    fishing => "fishing",
    grid_repair => "grid-repair",
    grindstone => "grindstone",
    item_uses_misc => "item-uses-misc",
    lead_name_tag => "lead-name-tag",
    lingering_potion => "lingering-potion",
    setup_commands => "setup-commands",
    shield => "shield",
    shulker_box => "shulker-box",
    smithing_table => "smithing-table",
    splash_potion => "splash-potion",
    stonecutter_loom => "stonecutter-loom",
    tipped_arrow => "tipped-arrow",
    trident_throw => "trident-throw",
}
