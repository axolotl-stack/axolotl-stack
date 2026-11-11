//! Level data: unastar's own round trip, corrupt data, holders that leave
//! and come back, and the BDS 1.26.51 level-data capture
//! (`crates/unastar/tests/fixtures/bds_1_26_51_level_data.json`, written by
//! `tools/bds-sweep/level_data.py`).

use std::path::Path;
use std::sync::Arc;

use glam::{DVec3, IVec3};

use super::*;
use crate::command::exec::evidence::{ConsoleRecord, console_records};
use crate::command::exec::test_support::Harness;
use crate::command::{CommandSource, execute};
use crate::storage::LevelDBWorldProvider;
use crate::storage::level::LevelFiles;
use crate::storage::level::dat::LevelDat;
use crate::world::scoreboard::ScoreHolder;

/// The BDS level-data capture.
pub(crate) fn bds() -> &'static serde_json::Value {
    static CAPTURE: std::sync::LazyLock<serde_json::Value> = std::sync::LazyLock::new(|| {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/bds_1_26_51_level_data.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("level-data capture"))
            .expect("level-data capture JSON")
    });
    &CAPTURE
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

/// Writes a captured world (`saved` or `unastar_world`) into `dir`: its
/// level.dat and its level records in a LevelDB.
async fn write_captured_world(world: &serde_json::Value, dir: &Path) -> Arc<LevelDBWorldProvider> {
    std::fs::write(
        dir.join("level.dat"),
        hex(world["level.dat"].as_str().expect("level.dat")),
    )
    .unwrap();
    let provider = Arc::new(LevelDBWorldProvider::open(dir.join("db"), 0).unwrap());
    let puts = world["records"]
        .as_object()
        .expect("records")
        .iter()
        .map(|(key, value)| (key.as_bytes().to_vec(), hex(value.as_str().expect("hex"))))
        .collect();
    provider.save_level_records(puts, Vec::new()).await.unwrap();
    provider
}

/// What a world's level data opens as in a fresh harness.
async fn load_into(h: &mut Harness, dir: &Path, provider: Arc<LevelDBWorldProvider>) {
    let config = h
        .world()
        .resource::<crate::server::game::ServerConfigResource>()
        .0
        .clone();
    open(LevelFiles::new(dir), provider, &new_world(&config))
        .await
        .expect("level data opens")
        .apply(h.world())
        .expect("level data applies");
}

/// Saves the harness's level data into `dir` (a world opened there first).
async fn save(h: &mut Harness) {
    super::capture(h.world())
        .expect("level storage")
        .write()
        .await
        .expect("level data saved");
}

/// A harness with the chunks the capture's `lvl` ticking area kept loaded
/// on BDS (0 0 0 to 47 0 47: chunks 0 to 2).
fn loaded_harness() -> Harness {
    let mut h = Harness::new();
    let world = h.world();
    for x in 0..3 {
        for z in 0..3 {
            if (x, z) == (0, 0) {
                continue;
            }
            let chunk = world
                .spawn(crate::world::ecs::ChunkData::new(
                    crate::world::chunk::Chunk::new(x, z),
                ))
                .id();
            world
                .resource_mut::<crate::world::ecs::ChunkManager>()
                .insert(crate::world::ecs::ChunkPosition::new(x, z), chunk);
        }
    }
    h
}

/// A harness whose level data lives in `dir` (new world).
async fn harness_in(dir: &Path) -> (Harness, Arc<LevelDBWorldProvider>) {
    let mut h = loaded_harness();
    let provider = Arc::new(LevelDBWorldProvider::open(dir.join("db"), 0).unwrap());
    load_into(&mut h, dir, provider.clone()).await;
    (h, provider)
}

/// The lines of a list answer, sorted (to compare lists whose order is
/// the order of random record keys).
fn sorted_lines(records: &[ConsoleRecord]) -> Vec<String> {
    let mut lines: Vec<String> = records
        .iter()
        .flat_map(|r| r.text.lines().map(str::to_owned).collect::<Vec<_>>())
        .collect();
    lines.sort();
    lines
}

fn console(h: &mut Harness, line: &str) -> Vec<ConsoleRecord> {
    console_records(&execute(h.world(), CommandSource::server(), line))
}

fn captured_records(step: &serde_json::Value) -> Vec<ConsoleRecord> {
    step["records"]
        .as_array()
        .expect("records")
        .iter()
        .map(|r| {
            ConsoleRecord::new(
                r["level"].as_str().expect("level"),
                r["text"].as_str().expect("text"),
            )
        })
        .collect()
}

/// The set session's console commands as unastar runs them: the captured
/// lines but the cow (unastar's chunk 0,0 harness has no chunk 1,1) and the
/// nether area (one dimension).
const UNASTAR_SKIPS: [&str; 3] = [
    "summon cow 20 -60 20",
    "scoreboard players set @e[type=cow,x=20,y=-60,z=20,r=3] obj1 13",
    "execute in nether run tickingarea add 0 0 0 0 0 0 netherarea",
];

/// The capture's set session, run on unastar's console, and the player's
/// two scores; returns the console records per command.
fn run_set_session(h: &mut Harness) -> Vec<(String, Vec<ConsoleRecord>)> {
    let mut out = Vec::new();
    for step in bds()["set"].as_array().expect("set") {
        let command = step["command"].as_str().expect("command");
        if UNASTAR_SKIPS.contains(&command) {
            continue;
        }
        out.push((command.to_owned(), console(h, command)));
    }
    let player = h.player_at("BDSExtractor", true, DVec3::new(5.5, -60.0, 6.5));
    h.world()
        .entity_mut(player)
        .insert(PlayerUniqueId(-8_589_934_590));
    h.run(player, "/scoreboard players set @s obj1 21");
    h.run(player, "/scoreboard players set @s obj2 -3");
    h.world().despawn(player);
    out
}

/// Everything the capture's queries answer on unastar's console.
fn run_queries(h: &mut Harness) -> Vec<(String, Vec<ConsoleRecord>)> {
    bds()["query"]
        .as_array()
        .expect("query")
        .iter()
        .map(|step| {
            let command = step["command"].as_str().expect("command").to_owned();
            let records = console(h, &command);
            (command, records)
        })
        .collect()
}

#[tokio::test]
async fn level_data_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let before = {
        let (mut h, _provider) = harness_in(dir.path()).await;
        run_set_session(&mut h);
        h.world().resource_mut::<TickCounter>().current = 640;
        let before = run_queries(&mut h);
        save(&mut h).await;
        before
    };
    let (mut h, _provider) = harness_in(dir.path()).await;
    assert_eq!(h.world().resource::<TickCounter>().current, 640);
    assert_eq!(
        h.world().resource::<LevelSpawn>().0,
        Some(IVec3::new(100, 70, -200))
    );
    // Difficulty comes from the configuration at every start, as on BDS.
    assert_eq!(*h.world().resource::<Difficulty>(), Difficulty::Easy);
    let after = run_queries(&mut h);
    for ((command, before), (_, after)) in before.iter().zip(&after) {
        match command.as_str() {
            // After a restart the areas are listed in the order of their
            // record keys (random uuids), as on BDS.
            "tickingarea list all-dimensions" => {
                assert_eq!(sorted_lines(before), sorted_lines(after))
            }
            // `memory` saves are gone after a restart, as on BDS (so s3
            // places no stone at 10 -60 30).
            "structure load ns:s2 10 -60 20"
            | "structure load s3 10 -60 30"
            | "testforblock 10 -60 30 stone" => {
                assert_eq!(after[0].level, "ERROR", "{command}: {after:?}")
            }
            _ => assert_eq!(before, after, "{command}"),
        }
    }
}

#[tokio::test]
async fn removed_areas_and_structures_are_deleted_and_memory_saves_are_not_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, provider) = harness_in(dir.path()).await;
    for line in [
        "tickingarea add 0 0 0 15 0 15 a",
        "tickingarea add 16 0 16 31 0 31 b",
        "structure save keep 0 0 0 1 1 1 false disk true",
        "structure save gone 0 0 0 1 1 1 false disk true",
        "structure save memory 0 0 0 1 1 1 false memory true",
    ] {
        console(&mut h, line);
    }
    save(&mut h).await;
    let keys = |records: Vec<crate::storage::level::RawRecord>| -> Vec<String> {
        records
            .into_iter()
            .map(|(key, _)| String::from_utf8(key).unwrap())
            .filter(|key| key.starts_with("tickingarea_") || key.starts_with("structuretemplate_"))
            .map(|key| key.split('_').next().unwrap().to_owned() + &key[key.len() - 4..])
            .collect()
    };
    assert_eq!(keys(provider.load_level_records().await.unwrap()).len(), 4);
    console(&mut h, "tickingarea remove b");
    console(&mut h, "structure delete gone");
    save(&mut h).await;
    let records = provider.load_level_records().await.unwrap();
    let names: Vec<String> = records
        .iter()
        .map(|(key, _)| String::from_utf8_lossy(key).into_owned())
        .filter(|key| key.starts_with("tickingarea_") || key.starts_with("structuretemplate_"))
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&"structuretemplate_mystructure:keep".to_owned()));
}

#[tokio::test]
async fn unreadable_level_data_stops_the_start_and_stays_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    {
        let (mut h, _provider) = harness_in(dir.path()).await;
        console(&mut h, "tickingarea add 0 0 0 15 0 15 a");
        console(&mut h, "scoreboard objectives add o dummy");
        save(&mut h).await;
    }
    let level_dat = dir.path().join("level.dat");
    let good = std::fs::read(&level_dat).unwrap();

    // A level.dat that does not parse is an error and is left alone.
    std::fs::write(&level_dat, &good[..good.len() - 1]).unwrap();
    let provider = Arc::new(LevelDBWorldProvider::open(dir.path().join("db"), 0).unwrap());
    let new = new_world(&Default::default());
    assert!(
        open(LevelFiles::new(dir.path()), provider.clone(), &new)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&level_dat).unwrap(), good[..good.len() - 1]);
    std::fs::write(&level_dat, &good).unwrap();

    // So is a scoreboard record that does not decode.
    provider
        .save_level_records(
            vec![(b"scoreboard".to_vec(), vec![10, 0, 0, 1])],
            Vec::new(),
        )
        .await
        .unwrap();
    assert!(matches!(
        open(LevelFiles::new(dir.path()), provider.clone(), &new).await,
        Err(LevelDataError::Record { .. })
    ));
    provider
        .save_level_records(Vec::new(), vec![b"scoreboard".to_vec()])
        .await
        .unwrap();

    // An unreadable ticking area is skipped, kept, and not deleted by a save.
    let bad = b"tickingarea_00000000-0000-0000-0000-000000000000".to_vec();
    provider
        .save_level_records(vec![(bad.clone(), vec![1, 2, 3])], Vec::new())
        .await
        .unwrap();
    let mut h = Harness::new();
    load_into(&mut h, dir.path(), provider.clone()).await;
    assert_eq!(h.world().resource::<TickingAreas>().areas().len(), 1);
    save(&mut h).await;
    let kept = provider.load_level_records().await.unwrap();
    assert!(kept.contains(&(bad, vec![1, 2, 3])));
}

#[test]
fn players_and_saved_actors_keep_their_scores_while_away() {
    let mut h = Harness::new();
    let player = h.player("Alice", true);
    h.world().entity_mut(player).insert(PlayerUniqueId(77));
    h.run(player, "/scoreboard objectives add o dummy");
    h.run(player, "/scoreboard players set @s o 5");
    h.world().despawn(player);
    let board = h.world().resource::<Scoreboard>();
    assert_eq!(board.score(&ScoreHolder::OfflinePlayer(77), "o"), Some(5));

    // Back with the same id: the same scoreboard id and score.
    let player = h.player("Alice", true);
    h.world().entity_mut(player).insert(PlayerUniqueId(77));
    let board = h.world().resource::<Scoreboard>();
    assert_eq!(board.score(&ScoreHolder::Entity(player), "o"), Some(5));
    assert_eq!(board.id_of(&ScoreHolder::Entity(player)), Some(1));

    // A saved actor keeps its score under its unique id while unloaded.
    let cow = h
        .world()
        .spawn(crate::entity::mob::persistence::MobUniqueId(-9))
        .id();
    h.world()
        .resource_mut::<Scoreboard>()
        .set_score(&ScoreHolder::Entity(cow), "o", 3);
    h.world().despawn(cow);
    assert_eq!(
        h.world()
            .resource::<Scoreboard>()
            .score(&ScoreHolder::UnloadedActor(-9), "o"),
        Some(3)
    );
}

#[test]
fn holders_without_a_score_are_not_saved_but_their_ids_stay_used() {
    let mut board = Scoreboard::default();
    board.add_objective("o", "o", "dummy");
    board.set_score(&ScoreHolder::Fake("a".into()), "o", 1);
    board.set_score(&ScoreHolder::Fake("b".into()), "o", 2);
    board.reset_score(&ScoreHolder::Fake("b".into()), "o");
    let stored = board.to_stored(|_| None);
    assert_eq!(stored.entries.len(), 1);
    assert_eq!(stored.last_unique_id, 2);
    let mut back = Scoreboard::from_stored(stored).unwrap();
    assert_eq!(back.set_score(&ScoreHolder::Fake("c".into()), "o", 3), 3);
}

#[tokio::test]
async fn new_worlds_get_bds_fresh_level_dat_with_the_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _provider) = harness_in(dir.path()).await;
    save(&mut h).await;
    let dat = LevelDat::read(&std::fs::read(dir.path().join("level.dat")).unwrap()).unwrap();
    assert_eq!(dat.storage_version, 10);
    assert_eq!(dat.int("Generator").unwrap(), Some(2));
    assert_eq!(dat.int("SpawnY").unwrap(), Some(UNSET_SPAWN_Y));
    // One start counted down from the largest count, as BDS's first start.
    assert_eq!(dat.long("worldStartCount").unwrap(), Some(4_294_967_294));
    assert_eq!(dat.byte("keepinventory").unwrap(), Some(0));
    assert!(!dat.root.contains_key("locatorbar"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("levelname.txt")).unwrap(),
        "default"
    );
}

/// Writes the world `tools/bds-sweep/level_data.py import` opens on BDS:
/// the capture's set session run on unastar, saved into
/// `UNASTAR_LEVEL_DATA_OUT`.
#[tokio::test]
#[ignore = "writes a world for the BDS import step of tools/bds-sweep/level_data.py"]
async fn write_level_for_bds_import() {
    let out = std::path::PathBuf::from(
        std::env::var("UNASTAR_LEVEL_DATA_OUT").expect("UNASTAR_LEVEL_DATA_OUT"),
    );
    std::fs::create_dir_all(&out).unwrap();
    let (mut h, _provider) = harness_in(&out).await;
    run_set_session(&mut h);
    h.world().resource_mut::<TickCounter>().current = 640;
    save(&mut h).await;
}

// --- the BDS 1.26.51 capture ----------------------------------------------------

/// The world BDS saved after the set session, loaded into a harness.
async fn bds_world(dir: &Path) -> (Harness, Arc<LevelDBWorldProvider>) {
    let provider = write_captured_world(&bds()["saved"], dir).await;
    let mut h = loaded_harness();
    load_into(&mut h, dir, provider.clone()).await;
    (h, provider)
}

fn captured_messages(step: &serde_json::Value) -> Vec<crate::command::CommandMessage> {
    step["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .map(|m| crate::command::CommandMessage {
            key: m["key"].as_str().expect("key").to_owned(),
            params: m["params"]
                .as_array()
                .expect("params")
                .iter()
                .map(|p| p.as_str().expect("param").to_owned())
                .collect(),
            success: m["success"].as_bool().expect("success"),
        })
        .collect()
}

#[tokio::test]
async fn the_set_commands_answer_like_bds() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _provider) = harness_in(dir.path()).await;
    let ours = run_set_session(&mut h);
    let theirs: Vec<(String, Vec<ConsoleRecord>)> = bds()["set"]
        .as_array()
        .expect("set")
        .iter()
        .filter(|step| !UNASTAR_SKIPS.contains(&step["command"].as_str().unwrap()))
        .map(|step| {
            (
                step["command"].as_str().unwrap().to_owned(),
                captured_records(step),
            )
        })
        .collect();
    assert_eq!(ours.len(), theirs.len());
    for ((command, ours), (_, theirs)) in ours.iter().zip(&theirs) {
        assert_eq!(ours, theirs, "{command}");
    }
}

#[tokio::test]
async fn a_world_bds_saved_answers_like_bds_after_a_reload() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _provider) = bds_world(dir.path()).await;
    let saved = LevelDat::read(&hex(bds()["saved"]["level.dat"].as_str().unwrap())).unwrap();
    let ours = run_queries(&mut h);
    for ((command, ours), step) in ours.iter().zip(bds()["query"].as_array().unwrap()) {
        if command == "time query gametime" {
            // BDS had run some ticks since loading; unastar has not: the
            // count goes on from the saved `currentTick`.
            let tick = saved.long("currentTick").unwrap().unwrap();
            assert_eq!(ours[0].text, format!("Gametime is {tick}"));
            continue;
        }
        assert_eq!(*ours, captured_records(step), "{command}");
    }
}

#[tokio::test]
async fn a_world_bds_saved_is_saved_back_as_bds_wrote_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, provider) = bds_world(dir.path()).await;
    save(&mut h).await;

    let theirs = LevelDat::read(&hex(bds()["saved"]["level.dat"].as_str().unwrap())).unwrap();
    let mut ours = LevelDat::read(&std::fs::read(dir.path().join("level.dat")).unwrap()).unwrap();
    // A start counts one more start; the save time and the difficulty
    // (the configuration's, as BDS takes server.properties') are new.
    assert_eq!(
        ours.long("worldStartCount").unwrap().unwrap(),
        theirs.long("worldStartCount").unwrap().unwrap() - 1
    );
    assert_eq!(ours.int("Difficulty").unwrap(), Some(1));
    for name in ["worldStartCount", "LastPlayed", "Difficulty"] {
        ours.set(name, theirs.root[name].clone());
    }
    assert_eq!(ours, theirs);
    assert_eq!(
        std::fs::read(dir.path().join("level.dat_old")).unwrap(),
        hex(bds()["saved"]["level.dat"].as_str().unwrap())
    );

    let records = provider.load_level_records().await.unwrap();
    let captured = bds()["saved"]["records"].as_object().unwrap();
    for (key, value) in &records {
        let key = String::from_utf8(key.clone()).unwrap();
        let theirs = hex(captured[&key].as_str().unwrap());
        if key == "scoreboard" {
            // BDS writes the display slots in no fixed order.
            let sorted = |bytes: &[u8]| {
                let mut board = StoredScoreboard::decode(bytes).unwrap();
                board.displays.sort_by(|a, b| a.slot.cmp(&b.slot));
                board
            };
            assert_eq!(sorted(value), sorted(&theirs));
        } else {
            assert_eq!(*value, theirs, "{key}");
        }
    }
    let level_keys = captured
        .keys()
        .filter(|key| crate::storage::level::records::is_level_record(key.as_bytes()))
        .count();
    assert_eq!(records.len(), level_keys);
}

#[tokio::test]
async fn players_see_offline_players_and_their_own_scores_like_bds() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _provider) = bds_world(dir.path()).await;
    let viewer = h.player_at("Viewer", true, DVec3::new(100.5, 70.0, -199.5));
    for step in bds()["viewer"].as_array().unwrap() {
        let command = step["command"].as_str().unwrap();
        let ours = h.run(viewer, command);
        assert_eq!(ours.messages, captured_messages(step), "{command}");
    }
    h.world().despawn(viewer);

    let player = h.player_at("BDSExtractor", true, DVec3::new(5.5, -60.0, 6.5));
    h.world()
        .entity_mut(player)
        .insert(PlayerUniqueId(-8_589_934_590));
    for step in bds()["player_query"].as_array().unwrap() {
        let command = step["command"].as_str().unwrap();
        if command == "/tag @s list" {
            // Tags are player data, not level data.
            continue;
        }
        let ours = h.run(player, command);
        assert_eq!(ours.messages, captured_messages(step), "{command}");
    }
}

#[tokio::test]
async fn a_rejoining_player_gets_the_displays_and_scores_like_bds() {
    use crate::command::exec::evidence::decode;
    use jolyne::valentine::McpePacketData;
    use jolyne::valentine::types::SetScorePacketScoreInfoItem;

    let dir = tempfile::tempdir().unwrap();
    let (mut h, _provider) = bds_world(dir.path()).await;
    // The cow BDS had loaded, and the player.
    let cow = h
        .world()
        .spawn((
            crate::entity::components::RuntimeEntityId(41),
            crate::entity::mob::persistence::MobUniqueId(-8_589_934_591),
        ))
        .id();
    let player = h.player_at("BDSExtractor", true, DVec3::new(5.5, -60.0, 6.5));
    h.world()
        .entity_mut(player)
        .insert(PlayerUniqueId(-8_589_934_590));
    let runtime = h
        .world()
        .get::<crate::entity::components::RuntimeEntityId>(player)
        .unwrap()
        .0;
    let ours: Vec<McpePacketData> = crate::command::exec::scoreboard_join_packets(h.world())
        .into_iter()
        .map(|packet| match packet.data {
            McpePacketData::SetScorePacket(mut packet) => {
                for entry in &mut packet.score_info {
                    match entry {
                        SetScorePacketScoreInfoItem::ChangePlayerScore(change)
                            if change.player_unique_id.player_unique_id == runtime =>
                        {
                            change.player_unique_id.player_unique_id = -8_589_934_590
                        }
                        SetScorePacketScoreInfoItem::ChangeEntityScore(change)
                            if change.actor_id.actor_unique_id == 41 =>
                        {
                            change.actor_id.actor_unique_id = -8_589_934_591
                        }
                        _ => {}
                    }
                }
                McpePacketData::SetScorePacket(packet)
            }
            other => other,
        })
        .collect();
    let theirs: Vec<McpePacketData> = bds()["player_join"]["scoreboard_packets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|packet| decode(&hex(packet.as_str().unwrap())))
        .collect();
    assert_eq!(ours, theirs);
    let _ = cow;
}

#[tokio::test]
async fn start_game_carries_the_level_time_clock_and_world_spawn_like_bds() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _provider) = bds_world(dir.path()).await;
    let captured = &bds()["player_join"]["start_game"];
    // The capture's join came after BDS had run since loading; the level
    // time is the captured one when the count stands there.
    let tick = captured["level_current_time"].as_u64().unwrap();
    h.world().resource_mut::<TickCounter>().current = tick;
    super::publish(h.world());
    let mut start = jolyne::valentine::StartGamePacket::default();
    h.world()
        .resource::<StartGameWorldState>()
        .apply(&mut start);
    assert_eq!(start.level_current_time, tick);
    assert_eq!(
        i64::from(start.settings.day_cycle_stop_time),
        captured["day_cycle_stop_time"].as_i64().unwrap()
    );
    let spawn = &captured["default_spawn_block_position"];
    assert_eq!(
        [
            start.settings.default_spawn_block_position.x,
            start.settings.default_spawn_block_position.y,
            start.settings.default_spawn_block_position.z
        ],
        ["x", "y", "z"].map(|axis| spawn[axis].as_i64().unwrap() as i32)
    );
}

#[tokio::test]
async fn the_clock_loads_from_level_dat_like_bds() {
    // The capture's probe: the saved world with level.dat `Time` 1111 while
    // the `WorldClocks` record still holds 4321.
    let dir = tempfile::tempdir().unwrap();
    let provider = write_captured_world(&bds()["saved"], dir.path()).await;
    let mut dat = LevelDat::read(&std::fs::read(dir.path().join("level.dat")).unwrap()).unwrap();
    dat.set("Time", Tag::Long(1111));
    std::fs::write(dir.path().join("level.dat"), dat.write()).unwrap();
    let mut h = loaded_harness();
    load_into(&mut h, dir.path(), provider).await;
    let probe = &bds()["clocks"]["answers"][0];
    assert_eq!(probe["command"], "time query daytime");
    assert_eq!(
        console(&mut h, "time query daytime"),
        captured_records(probe)
    );
}

#[tokio::test]
async fn players_get_no_feedback_while_send_command_feedback_is_off_like_bds() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _provider) = bds_world(dir.path()).await;
    assert!(!h.world().resource::<GameRules>().sends_command_feedback());
    let player = h.player_at("BDSExtractor", true, DVec3::new(5.5, -60.0, 6.5));
    for step in bds()["player_set"].as_array().unwrap() {
        let command = step["command"].as_str().unwrap();
        let output = h.run(player, command);
        let ours =
            crate::server::game::commands::player_answer(h.world(), &output, Default::default())
                .map(|packet| {
                    (
                        packet.output.output_type.to_string(),
                        packet.output.success_count,
                    )
                });
        let theirs = step["output"].as_object().map(|output| {
            (
                output["type"].as_str().unwrap().to_owned(),
                output["success_count"].as_u64().unwrap() as u32,
            )
        });
        assert_eq!(ours, theirs, "{command}: {output:?}");
        if theirs.is_some() {
            assert_eq!(output.messages, captured_messages(step), "{command}");
        }
    }
}

#[tokio::test]
async fn bds_reads_the_level_unastar_writes() {
    // unastar writes the world BDS was shown (the capture's set session).
    let dir = tempfile::tempdir().unwrap();
    let provider = {
        let (mut h, provider) = harness_in(dir.path()).await;
        run_set_session(&mut h);
        h.world().resource_mut::<TickCounter>().current = 640;
        save(&mut h).await;
        provider
    };
    let shown = &bds()["unastar_world"];

    // The same level.dat but the save time, and the same records but the
    // ticking areas' random uuids.
    let mut ours = LevelDat::read(&std::fs::read(dir.path().join("level.dat")).unwrap()).unwrap();
    let theirs = LevelDat::read(&hex(shown["level.dat"].as_str().unwrap())).unwrap();
    ours.set("LastPlayed", theirs.root["LastPlayed"].clone());
    assert_eq!(ours, theirs);
    let without_uuids = |records: Vec<(String, Vec<u8>)>| -> Vec<(String, Vec<u8>)> {
        let mut records: Vec<(String, Vec<u8>)> = records
            .into_iter()
            .map(|(key, value)| match key.strip_prefix("tickingarea_") {
                Some(_) => ("tickingarea_".to_owned(), value),
                None => (key, value),
            })
            .collect();
        records.sort();
        records
    };
    let ours = without_uuids(
        provider
            .load_level_records()
            .await
            .unwrap()
            .into_iter()
            .map(|(key, value)| (String::from_utf8(key).unwrap(), value))
            .collect(),
    );
    let theirs = without_uuids(
        shown["records"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), hex(value.as_str().unwrap())))
            .collect(),
    );
    assert_eq!(ours, theirs);

    // BDS answered the queries on it as unastar does after loading it (in
    // the order of the shown world's keys for the areas).
    let dir = tempfile::tempdir().unwrap();
    let provider = write_captured_world(shown, dir.path()).await;
    let mut h = loaded_harness();
    load_into(&mut h, dir.path(), provider).await;
    let ours = run_queries(&mut h);
    for ((command, ours), step) in ours.iter().zip(bds()["import"].as_array().unwrap()) {
        if command == "time query gametime" {
            // BDS had run since loading the saved 640 ticks.
            assert_eq!(ours[0].text, "Gametime is 640");
            continue;
        }
        assert_eq!(*ours, captured_records(step), "{command}");
    }
}
