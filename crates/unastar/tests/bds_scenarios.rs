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

use std::io::Write;
use std::net::{SocketAddr, UdpSocket};
use std::path::Path;
use std::sync::{Arc, Mutex};

use client_diff::driver;
use client_diff::parity::{Case, RuleValue, Side, WorldKind, scenarios_root, shared_script};
use client_diff::scenario::ScenariosFile;
use unastar::config::{GameRuleSetting, SpawnLocation, SpawnRule, UnastarConfig};
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
    config.server.default_chunk_radius = 4;
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
            config.spawn_rules = vec![SpawnRule {
                always_at_location: true,
                location: Some(SpawnLocation {
                    x: 0.5,
                    y: -60.0,
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

fn run_case(name: &str) {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let case = Case::load(&scenarios_root().join(name)).unwrap();
    let file = ScenariosFile::load(&shared_script()).unwrap();
    let script = case.script(&file).unwrap();
    let directory = tempfile::tempdir().unwrap();
    std::env::set_current_dir(directory.path()).unwrap();
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
    drink_water_bottle => "drink-water-bottle",
}
