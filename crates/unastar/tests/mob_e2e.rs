//! Loopback end-to-end checks of vanilla mobs: a jolyne client joins an
//! in-process Unastar server and checks the packets a real client would see.
//!
//! - `summoned_cow_is_hurt_killed_and_drops_loot`: summons a cow as an
//!   operator, punches it to death and checks hurt, death, removal and loot.
//! - `chicken_summon_flap_egg_and_kill_over_loopback`: summons a chicken 6
//!   blocks above the floor and times its wing-flap glide, throws an egg
//!   taken from the creative inventory, then kills the chicken.
//! - `natural_spawning_populates_new_chunks_over_loopback`: joins a superflat
//!   plains world and waits for the animals the world-generation pass spawns
//!   in the chunks loaded around the bot.
//! - `zombie_kills_the_player_who_then_respawns_over_loopback`: on hard at
//!   midnight, summons a zombie next to the survival bot, which stands still
//!   while the zombie closes in and hits it to death, then answers the
//!   respawn screen and checks it comes back at the world spawn with full
//!   health.
//!
//! The tests bind a UDP socket on `127.0.0.1` (never a public interface), so
//! they are ignored by default. Run them with:
//!
//! ```text
//! cargo test -p unastar --test mob_e2e -- --ignored --nocapture
//! ```
//!
//! `RUST_LOG=unastar=debug` adds server logs. The observed packet sequence
//! (elapsed time, packet name, key fields) is printed as it arrives.
//!
//! The server runs on its own thread and current-thread runtime, the client on
//! another, both with large stacks: decoding the biggest join packets
//! (`CraftingData`, `CreativeContent`) recurses deeper than default debug-build
//! stacks allow. The server's world, player data and plugin directories live in
//! a temporary directory that is the process working directory for the run,
//! so the tests take turns ([`LOOPBACK`]).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use jolyne::stream::RakNetTransport;
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsActorEvent, EnumsItemUseOnActorInventoryTransactionActionType,
    EnumsPlayerActionType, EnumsPlayerPositionModeComponentPositionMode, EnumsPlayerRespawnState,
    InventoryTransactionPacketTransaction, ItemUseOnActorInventoryTransaction, TextPacketBody,
    Vec3,
};
use jolyne::valentine::{
    CommandOriginDatajson, CommandRequestPacket, ContainerClosePacket,
    EnumsConnectionDisconnectFailReason, EnumsContainerEnumName, EnumsInteractPacketPayloadAction,
    EnumsItemStackNetResult, EnumsItemStackRequestActionType,
    EnumsItemUseInventoryTransactionActionType, FullContainerName, InteractPacket,
    InventoryTransactionPacket, ItemStackRequestCerealCraftCreativeActionData,
    ItemStackRequestCerealPlaceActionData, ItemStackRequestCerealSlotInfoData,
    ItemStackRequestPacket, ItemStackRequestPacketDataRequestData,
    ItemStackRequestPacketDataRequestDataActionsItem, ItemUseInventoryTransaction, McpePacket,
    McpePacketData, PlayerActionPacket, RespawnPacket,
    TypedClientNetIdstructItemStackRequestIdTagint32T0,
};
use jolyne::{BedrockStream, Client, ClientHandshakeConfig, GameData, Play};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use unastar::UnastarConfig;
use unastar::server::UnastarServer;
use unastar::world::rules::Difficulty;
use unastar::world::{WorldConfig, WorldGenerator};

/// Display name of the test client; listed as a server operator.
const BOT: &str = "MobBot";
/// Stack for both runtimes (see the module docs).
const STACK_SIZE: usize = 64 * 1024 * 1024;
/// Bound for starting the server and completing the join.
const JOIN_TIMEOUT: Duration = Duration::from_secs(30);
/// Bound for the cow's summon/hit/death phase.
const COW_TIMEOUT: Duration = Duration::from_secs(30);
/// Bound for the chicken's summon, glide, egg and fight.
const CHICKEN_TIMEOUT: Duration = Duration::from_secs(60);
/// Bound for watching the natural spawns ([`FIRST_SPAWN_WITHIN`] plus
/// [`POPULATION_SETTLE`] and slack).
const SPAWN_TIMEOUT: Duration = Duration::from_secs(120);
/// Bound for the zombie's summon, fight, death and respawn.
const ZOMBIE_TIMEOUT: Duration = Duration::from_secs(120);
/// How long the zombie may take to kill the bot ([`ZOMBIE_TIMEOUT`] less
/// the respawn and slack).
const ZOMBIE_KILLS_WITHIN: Duration = Duration::from_secs(60);
/// How long the zombie may go without a hit before the bot steps next to it
/// again (or a new zombie is summoned when it is gone).
const ZOMBIE_STALL: Duration = Duration::from_secs(5);
/// How long the death packets (`DeathInfo`, chat, `Respawn`, health 0) may
/// trail `ActorEvent(Death)`, and the respawn packets the bot's request.
const DEATH_PACKETS_WITHIN: Duration = Duration::from_secs(5);
/// How long the bot keeps listening after respawning to show it is still
/// connected.
const CONNECTED_AFTER_RESPAWN: Duration = Duration::from_secs(2);
/// Spacing between hits: longer than the 10-tick (0.5 s) hurt cooldown.
const HIT_SPACING: Duration = Duration::from_millis(600);
/// How long after `ActorEvent(Death)` the corpse's `RemoveActor` may take.
const REMOVE_WITHIN: Duration = Duration::from_secs(2);
/// Player eye height above the feet.
const EYE_HEIGHT: f32 = 1.62;
/// One server tick.
const TICK: Duration = Duration::from_millis(50);
/// Top surface of the void spawn platform (its stone fills y 0..16).
const PLATFORM_TOP: f32 = 16.0;
/// How far above the platform the chicken is summoned.
const GLIDE_HEIGHT: f32 = 6.0;
/// How long the chicken's height is sampled after it appears; it lands in
/// about 3 s.
const GLIDE_WINDOW: Duration = Duration::from_secs(8);
/// How long the thrown egg may fly before its `RemoveActor`.
const EGG_LANDS_WITHIN: Duration = Duration::from_secs(5);
/// How long after joining the first animal's `AddActor` may take.
const FIRST_SPAWN_WITHIN: Duration = Duration::from_secs(90);
/// How long after the first animal further spawns are collected.
const POPULATION_SETTLE: Duration = Duration::from_secs(10);
/// Natural spawns keep at least this far from every player (the default
/// `minecraft:distance_filter` minimum).
const SPAWN_DISTANCE_MIN: f32 = 24.0;
const COW: &str = "minecraft:cow";
const CHICKEN: &str = "minecraft:chicken";
const EGG: &str = "minecraft:egg";
/// The animals vanilla spawns on plains grass that the test waits for.
const PASSIVE_ANIMALS: [&str; 4] = [COW, "minecraft:sheep", "minecraft:pig", CHICKEN];
const ZOMBIE: &str = "minecraft:zombie";
/// The vanilla zombie's `minecraft:attack` damage before difficulty scaling.
const ZOMBIE_ATTACK: f32 = 3.0;
/// Hard multiplies mob damage on players by 1.5 (3 → 4.5 for the zombie).
const HARD_DAMAGE_SCALE: f32 = 1.5;
/// The zombie's melee cooldown (`behavior.melee_box_attack`): one hit per
/// second while the target stays in reach.
const ZOMBIE_ATTACK_TICKS: u64 = 20;
/// Blocks along +z from the bot the zombie is summoned, so it must walk to
/// it.
const ZOMBIE_SUMMON_DISTANCE: f32 = 4.0;
const PLAYER_MAX_HEALTH: f32 = 20.0;
const HEALTH: &str = "minecraft:health";

/// Serialises the tests: each makes its own temporary directory the process
/// working directory.
static LOOPBACK: Mutex<()> = Mutex::new(());

#[test]
#[ignore = "loopback socket run: use the native lock"]
fn summoned_cow_is_hurt_killed_and_drops_loot() {
    run_loopback(Scenario::Cow, |config| {
        // A wide void platform: chunks generate inline and a panicking cow
        // cannot reach the edge during the fight.
        config.world = WorldConfig::void_spawn_platform(4);
    });
}

#[test]
#[ignore = "loopback socket run: use the native lock"]
fn chicken_summon_flap_egg_and_kill_over_loopback() {
    run_loopback(Scenario::Chicken, |config| {
        // The cow's platform; creative mode lets the bot take an egg from the
        // creative inventory (the server has no `/give`).
        config.world = WorldConfig::void_spawn_platform(4);
        config.server.default_gamemode = "creative".into();
    });
}

#[test]
#[ignore = "loopback socket run: use the native lock"]
fn natural_spawning_populates_new_chunks_over_loopback() {
    run_loopback(Scenario::NaturalSpawning, |config| {
        // Grass at y = 3 and plains everywhere: cows, sheep, pigs and chickens
        // spawn there. Generated inline like the void platform.
        config.world = WorldConfig {
            generator: WorldGenerator::SuperFlat,
            ..WorldConfig::default()
        };
    });
}

#[test]
#[ignore = "loopback socket run: use the native lock"]
fn zombie_kills_the_player_who_then_respawns_over_loopback() {
    run_loopback(Scenario::Zombie, |config| {
        // The cow's platform, wide enough that the bot's own spawn chunk
        // generates inline. Hard scales the zombie's 3 damage to 4.5;
        // survival lets it be hurt and targeted (the zombie ignores creative
        // players). The bot is an operator by default.
        config.world = WorldConfig::void_spawn_platform(4);
        config.server.difficulty = Difficulty::Hard;
        config.server.default_gamemode = "survival".into();
    });
}

/// What the joined bot does.
#[derive(Debug, Clone, Copy)]
enum Scenario {
    Cow,
    Chicken,
    NaturalSpawning,
    Zombie,
}

impl Scenario {
    fn timeout(self) -> Duration {
        match self {
            Self::Cow => COW_TIMEOUT,
            Self::Chicken => CHICKEN_TIMEOUT,
            Self::NaturalSpawning => SPAWN_TIMEOUT,
            Self::Zombie => ZOMBIE_TIMEOUT,
        }
    }

    async fn run(self, bot: &mut Bot) {
        match self {
            Self::Cow => fight_cow(bot).await,
            Self::Chicken => glide_egg_and_kill_chicken(bot).await,
            Self::NaturalSpawning => watch_natural_spawns(bot).await,
            Self::Zombie => zombie_kills_and_bot_respawns(bot).await,
        }
    }
}

/// Starts a server configured by `configure` (on top of loopback, offline and
/// [`BOT`] as operator defaults) in a temporary working directory, joins it
/// and runs `scenario`.
fn run_loopback(scenario: Scenario, configure: impl FnOnce(&mut UnastarConfig)) {
    let _turn = LOOPBACK.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_test_writer()
        .try_init();

    let dir = tempfile::tempdir().expect("temporary server directory");
    // `UnastarServer::new` opens `worlds/default/db` and `plugins` relative to
    // the working directory; keep them inside the temporary directory.
    std::env::set_current_dir(dir.path()).expect("enter the temporary directory");

    let port = std::net::UdpSocket::bind("127.0.0.1:0")
        .and_then(|socket| socket.local_addr())
        .expect("free loopback port")
        .port();
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();

    let mut config = UnastarConfig::default();
    config.server.bind_address = addr.to_string();
    config.server.online_mode = false;
    config.server.max_players = 2;
    config.server.operators = vec![BOT.into()];
    config.players.data_dir = dir.path().join("playerdata");
    configure(&mut config);
    config.validate().expect("test config is valid");

    let server = ServerThread::start(Arc::new(config));
    let client = std::thread::Builder::new()
        .name("mob-e2e-client".into())
        .stack_size(STACK_SIZE)
        .spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_stack_size(STACK_SIZE)
                .enable_all()
                .build()
                .expect("client runtime")
                .block_on(run_client(addr, scenario))
        })
        .expect("spawn the client thread");
    let client_result = client.join();
    let server_result = server.stop();

    std::env::set_current_dir(std::env::temp_dir()).expect("leave the temporary directory");
    drop(dir);

    // A server failure is the root cause of any client timeout.
    if let Err(panic) = server_result.and(client_result) {
        std::panic::resume_unwind(panic);
    }
}

/// The server on its own thread, stopped by dropping `run()` and the runtime.
/// Setup and run failures panic the thread; [`ServerThread::stop`] returns it.
struct ServerThread {
    shutdown: oneshot::Sender<()>,
    thread: JoinHandle<()>,
}

impl ServerThread {
    fn start(config: Arc<UnastarConfig>) -> Self {
        let (shutdown, stop) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("mob-e2e-server".into())
            .stack_size(STACK_SIZE)
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("server runtime");
                runtime.block_on(async move {
                    let mut server = UnastarServer::new(config)
                        .await
                        .unwrap_or_else(|error| panic!("server setup: {error}"));
                    tokio::select! {
                        result = server.run() => {
                            panic!("server stopped on its own: {result:?}")
                        }
                        _ = stop => {}
                    }
                });
                // Network and accept tasks die with the runtime.
                runtime.shutdown_timeout(Duration::from_secs(2));
            })
            .expect("spawn the server thread");
        Self { shutdown, thread }
    }

    fn stop(self) -> std::thread::Result<()> {
        let _ = self.shutdown.send(());
        self.thread.join()
    }
}

async fn run_client(addr: SocketAddr, scenario: Scenario) {
    let (play, game_data) = tokio::time::timeout(JOIN_TIMEOUT, join(addr))
        .await
        .expect("server start and join within the timeout");
    let mut bot = Bot::start(play, &game_data);
    let outcome = tokio::time::timeout(scenario.timeout(), scenario.run(&mut bot)).await;
    bot.shutdown().await;
    if outcome.is_err() {
        panic!(
            "{scenario:?} scenario did not finish within {:?}",
            scenario.timeout()
        );
    }
}

/// Connects (retrying until the listener is up) and completes the join.
async fn join(addr: SocketAddr) -> (BedrockStream<Play, Client, RakNetTransport>, GameData) {
    let started = Instant::now();
    loop {
        let attempt = tokio::time::timeout(Duration::from_secs(3), BedrockStream::connect(addr));
        match attempt.await {
            Ok(Ok(stream)) => {
                let (play, game_data) = stream
                    .join(ClientHandshakeConfig::random(addr, BOT))
                    .await
                    .expect("join completes");
                println!(
                    "[{:>6} ms] joined as {BOT}: runtime id {}, spawn {:?}",
                    started.elapsed().as_millis(),
                    play.runtime_id(),
                    game_data.start_game.position,
                );
                return (play, game_data);
            }
            Ok(Err(error)) => println!("connect failed ({error}); retrying"),
            Err(_) => println!("connect timed out; retrying"),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// `/summon` as an operator; returns the runtime ID from the mob's `AddActor`.
async fn summon(bot: &mut Bot, identifier: &str, at: &str) -> u64 {
    bot.command(&format!("/summon {identifier} {at}"));
    bot.expect(&format!("AddActor {identifier}"), |packet| match packet {
        McpePacketData::AddActorPacket(actor) if &*actor.actor_type == identifier => {
            Some(actor.target_runtime_id.actor_runtime_id)
        }
        // Command errors come back as system text.
        McpePacketData::TextPacket(text) if format!("{:?}", text.body).contains("Error:") => {
            panic!("/summon was refused: {:?}", text.body)
        }
        _ => None,
    })
    .await
}

/// a. summon, b. hurt, c. death, removal and drops.
async fn fight_cow(bot: &mut Bot) {
    // a. the cow appears with full health.
    let cow = summon(bot, COW, "~ ~ ~2").await;
    let health = *bot
        .health_on_add
        .get(&cow)
        .expect("the cow's AddActor carries minecraft:health");
    assert_eq!(health, (10.0, 10.0), "a fresh cow has 10/10 health");

    // b. + c. beef (and maybe leather) drops.
    let drops = kill(bot, cow).await;
    let beef = bot.item_id("minecraft:beef");
    let leather = bot.item_id("minecraft:leather");
    assert!(drops.contains(&beef), "beef drops: {drops:?}");
    assert!(
        drops.iter().all(|id| *id == beef || *id == leather),
        "only beef and leather drop: {drops:?} (beef {beef}, leather {leather})"
    );
}

/// a. summon 6 blocks up and time the glide, b. throw an egg, c. kill.
async fn glide_egg_and_kill_chicken(bot: &mut Bot) {
    // a. the chicken appears with full health 6 blocks above the platform
    // (an absolute height, independent of where the server puts the bot),
    // behind the bot (it faces +z with the default rotation) and so out of
    // the egg's path.
    let summit = PLATFORM_TOP + GLIDE_HEIGHT;
    let chicken = summon(bot, CHICKEN, &format!("~ {summit} ~-3")).await;
    let appeared = Instant::now();
    let health = *bot
        .health_on_add
        .get(&chicken)
        .expect("the chicken's AddActor carries minecraft:health");
    assert_eq!(health, (4.0, 4.0), "a fresh chicken has 4/4 health");

    // Its height over the glide, from MoveActorDelta (elapsed since AddActor, y).
    let start_y = bot.actors[&chicken].y;
    let mut heights = vec![(Duration::ZERO, start_y)];
    let window_end = appeared + GLIDE_WINDOW;
    while let Some(packet) = bot.next_until(window_end).await {
        let moved = match &packet {
            McpePacketData::MoveActorDeltaPacket(moved) => {
                moved.move_data.actor_runtime_id.actor_runtime_id == chicken
            }
            McpePacketData::MoveActorAbsolutePacket(moved) => {
                moved.move_data.actor_runtime_id.actor_runtime_id == chicken
            }
            _ => false,
        };
        let y = bot.actors[&chicken].y;
        if moved && heights.last().is_some_and(|(_, last)| *last != y) {
            heights.push((appeared.elapsed(), y));
        }
    }
    let trace: Vec<String> = heights
        .iter()
        .map(|(at, y)| format!("{}ms:{y:.2}", at.as_millis()))
        .collect();
    bot.log(format!(
        "chicken height over the glide: {}",
        trace.join(" ")
    ));
    let floor = heights
        .iter()
        .map(|(_, y)| *y)
        .fold(f32::INFINITY, f32::min);
    let landed_after = heights
        .iter()
        .find(|(_, y)| *y <= floor + 0.01)
        .map(|(at, _)| *at)
        .expect("a lowest height");
    assert!(
        (start_y - summit).abs() < 0.05 && (floor - PLATFORM_TOP).abs() < 0.05,
        "the chicken comes down from {summit} to the platform at {PLATFORM_TOP}: {trace:?}"
    );
    let free_fall = TICK * free_fall_ticks(GLIDE_HEIGHT);
    bot.log(format!(
        "chicken landed after {landed_after:?}; a free fall takes {free_fall:?}"
    ));
    // The glide's terminal speed is about 0.11 blocks per tick, a free fall
    // reaches 0.6: the chicken needs about four times as long.
    assert!(
        landed_after > free_fall * 2,
        "the chicken glides: landed after {landed_after:?}, a free fall takes {free_fall:?}"
    );

    // b. an egg from the creative inventory, used in the air, flies and is
    // removed where it lands.
    let egg_item = bot.item_id(EGG);
    bot.take_from_creative(egg_item, 16, 0).await;
    bot.use_held_item();
    let egg = bot
        .expect("AddActor minecraft:egg", |packet| match packet {
            McpePacketData::AddActorPacket(actor) if &*actor.actor_type == EGG => {
                Some(actor.target_runtime_id.actor_runtime_id)
            }
            _ => None,
        })
        .await;
    let thrown = Instant::now();
    tokio::time::timeout(
        EGG_LANDS_WITHIN,
        bot.expect("RemoveActor for the egg", |packet| match packet {
            McpePacketData::RemoveActorPacket(remove)
                if remove.target_actor_id.actor_unique_id as u64 == egg =>
            {
                Some(())
            }
            _ => None,
        }),
    )
    .await
    .unwrap_or_else(|_| panic!("the egg lands within {EGG_LANDS_WITHIN:?}"));
    bot.log(format!(
        "egg removed {:?} after its AddActor",
        thrown.elapsed()
    ));

    // c. raw chicken (and maybe feathers) drops.
    let drops = kill(bot, chicken).await;
    let raw = bot.item_id(CHICKEN);
    let feather = bot.item_id("minecraft:feather");
    assert!(drops.contains(&raw), "raw chicken drops: {drops:?}");
    assert!(
        drops.iter().all(|id| *id == raw || *id == feather),
        "only raw chicken and feathers drop: {drops:?} (chicken {raw}, feather {feather})"
    );
}

/// Ticks a body without wing-flap drag needs to fall `height` blocks from
/// rest: each tick it moves, then gains 0.08 blocks/tick downwards and keeps
/// 98 % of its speed.
fn free_fall_ticks(height: f32) -> u32 {
    let (mut fallen, mut speed, mut ticks) = (0.0, 0.0, 0);
    while fallen < height {
        fallen += speed;
        speed = (speed + 0.08) * 0.98;
        ticks += 1;
    }
    ticks
}

/// Waits for the animals the world-generation pass and the spawn cycle put in
/// the chunks around the bot.
async fn watch_natural_spawns(bot: &mut Bot) {
    let joined = Instant::now();
    let mut first_animal = None;
    let mut spawned: Vec<(String, Vec3)> = Vec::new();
    loop {
        let deadline =
            first_animal.map_or(joined + FIRST_SPAWN_WITHIN, |at| at + POPULATION_SETTLE);
        let Some(packet) = bot.next_until(deadline).await else {
            break;
        };
        if let McpePacketData::AddActorPacket(actor) = packet {
            let identifier = (*actor.actor_type).to_owned();
            if first_animal.is_none() && PASSIVE_ANIMALS.contains(&identifier.as_str()) {
                first_animal = Some(Instant::now());
            }
            spawned.push((identifier, actor.position));
        }
    }
    let first_animal = first_animal.unwrap_or_else(|| {
        panic!("a passive animal spawns within {FIRST_SPAWN_WITHIN:?}; AddActor seen: {spawned:?}")
    });
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (identifier, _) in &spawned {
        *counts.entry(identifier.as_str()).or_default() += 1;
    }
    bot.log(format!(
        "first animal {:?} after joining; AddActor counts {counts:?}",
        first_animal - joined
    ));
    // The spawn rule measures from the player's position (the eye) to the
    // spawn block's corner; mobs stand at its centre.
    let eye = bot.position.clone();
    for (identifier, at) in &spawned {
        let (dx, dy, dz) = (
            at.x.floor() - eye.x,
            at.y.floor() - eye.y,
            at.z.floor() - eye.z,
        );
        let distance = (dx * dx + dy * dy + dz * dz).sqrt();
        assert!(
            distance >= SPAWN_DISTANCE_MIN,
            "{identifier} at {at:?} spawned {distance:.1} blocks from the bot at {eye:?}"
        );
    }
}

/// a. midnight without natural spawns, b. a zombie closes in on the
/// standing bot and hits it to death, c. the bot answers the respawn screen
/// and comes back at the world spawn with full health.
async fn zombie_kills_and_bot_respawns(bot: &mut Bot) {
    // a. zombies burn in daylight: midnight, with the clock stopped there.
    // Natural spawns could add other attackers on the dark platform.
    bot.run_command("/gamerule dodaylightcycle false", "has been updated")
        .await;
    bot.run_command("/gamerule domobspawning false", "has been updated")
        .await;
    bot.run_command("/time set midnight", "Set the time to")
        .await;
    // The default spawn location puts the bot's eye at y 17, its feet inside
    // the platform; a real client lands and reports itself standing on top.
    // The bot sends no movement, so it is teleported there instead.
    let (x, z) = (bot.position.x, bot.position.z);
    bot.run_command(
        &format!("/tp {x:.2} {:.2} {z:.2}", PLATFORM_TOP + EYE_HEIGHT),
        "Teleported to",
    )
    .await;

    // b.
    let (fight, zombie) = watch_zombie_fight(bot).await;
    check_zombie_fight(bot, &fight, zombie);

    // c.
    answer_respawn_screen(bot).await;
    let listen_until = Instant::now() + CONNECTED_AFTER_RESPAWN;
    while let Some(packet) = bot.next_until(listen_until).await {
        if let McpePacketData::DisconnectPacket(disconnect) = packet {
            panic!("disconnected after respawning: {disconnect:?}");
        }
    }
    assert!(bot.connected(), "the bot stays connected after respawning");
}

/// What the bot saw of the zombie fight and its death.
#[derive(Default)]
struct ZombieFight {
    /// Arrival of each `ActorEvent(StartAttacking)` of the zombie.
    swings: Vec<Instant>,
    /// Horizontal distance from the bot to the zombie at its first swing.
    first_swing_distance: Option<f32>,
    /// The bot's own `ActorEvent(Hurt)` count.
    hurt: usize,
    /// The bot's health updates: (server tick, health, arrival).
    health: Vec<(u64, f32, Instant)>,
    /// `SetActorMotion` to the bot, with the horizontal direction from the
    /// zombie to the bot when it arrived.
    knockbacks: Vec<(Vec3, (f32, f32))>,
    /// When the bot stepped back next to the zombie.
    teleports: Vec<Instant>,
    zombies_summoned: usize,
    died: Option<Instant>,
    /// `DeathInfo` (key, parameters).
    death_info: Option<(String, Vec<String>)>,
    /// The translated death message in chat (key, parameters).
    death_chat: Option<(String, Vec<String>)>,
    /// `Respawn` packets of the respawn screen (state, position).
    respawn_screen: Vec<(EnumsPlayerRespawnState, Vec3)>,
}

impl ZombieFight {
    /// Whether the death and everything that follows it arrived.
    fn death_seen(&self) -> bool {
        self.died.is_some()
            && self.death_info.is_some()
            && self.death_chat.is_some()
            && self.respawn_screen.len() >= 2
            && self
                .health
                .last()
                .is_some_and(|(_, health, _)| *health == 0.0)
    }
}

/// Summons a zombie [`ZOMBIE_SUMMON_DISTANCE`] blocks from the standing bot
/// and records the fight until the bot's death and respawn screen. The bot
/// steps next to the zombie (or summons another one when it is gone) when
/// no hit lands for [`ZOMBIE_STALL`]. Returns the record and the zombie.
async fn watch_zombie_fight(bot: &mut Bot) -> (ZombieFight, u64) {
    let me = bot.runtime_id;
    // On the platform's surface, whatever height the server has for the bot.
    let at = format!("~ {PLATFORM_TOP} ~{ZOMBIE_SUMMON_DISTANCE}");
    let mut fight = ZombieFight::default();
    let mut zombie = summon(bot, ZOMBIE, &at).await;
    fight.zombies_summoned = 1;
    let kill_deadline = Instant::now() + ZOMBIE_KILLS_WITHIN;
    let mut last_hit = Instant::now();
    loop {
        let deadline = fight
            .died
            .map_or(kill_deadline, |died| died + DEATH_PACKETS_WITHIN);
        if fight.death_seen() || Instant::now() >= deadline {
            break;
        }
        if fight.died.is_none() && last_hit.elapsed() >= ZOMBIE_STALL {
            if let Some(feet) = bot.actors.get(&zombie).cloned() {
                bot.log(format!(
                    "no hit for {ZOMBIE_STALL:?}: stepping next to the zombie"
                ));
                bot.command(&format!(
                    "/tp {:.2} {:.2} {:.2}",
                    feet.x,
                    feet.y + EYE_HEIGHT,
                    feet.z - 1.0
                ));
                fight.teleports.push(Instant::now());
            } else {
                bot.log("the zombie is gone: summoning another".into());
                zombie = summon(bot, ZOMBIE, &at).await;
                fight.zombies_summoned += 1;
            }
            last_hit = Instant::now();
        }
        let wake = if fight.died.is_some() {
            deadline
        } else {
            deadline.min(last_hit + ZOMBIE_STALL)
        };
        let Some(packet) = bot.next_until(wake).await else {
            assert!(bot.connected(), "the bot stays connected during the fight");
            continue;
        };
        match packet {
            McpePacketData::ActorEventPacket(event)
                if event.target_runtime_id.actor_runtime_id == zombie
                    && event.event_id == EnumsActorEvent::StartAttacking =>
            {
                fight.swings.push(Instant::now());
                if fight.first_swing_distance.is_none() {
                    let (dx, dz) = bot.direction_from(zombie);
                    fight.first_swing_distance = Some(dx.hypot(dz));
                }
            }
            McpePacketData::ActorEventPacket(event)
                if event.target_runtime_id.actor_runtime_id == me =>
            {
                match event.event_id {
                    EnumsActorEvent::Hurt => fight.hurt += 1,
                    EnumsActorEvent::Death => fight.died = Some(Instant::now()),
                    _ => {}
                }
            }
            McpePacketData::UpdateAttributesPacket(update)
                if update.target_runtime_id.actor_runtime_id == me =>
            {
                if let Some(health) = update
                    .attribute_list
                    .iter()
                    .find(|attribute| &*attribute.name == HEALTH)
                {
                    fight.health.push((
                        update.tick.inputtick,
                        health.current_value,
                        Instant::now(),
                    ));
                    last_hit = Instant::now();
                }
            }
            McpePacketData::SetActorMotionPacket(motion)
                if motion.target_runtime_id.actor_runtime_id == me =>
            {
                let away = bot.direction_from(zombie);
                fight.knockbacks.push((motion.motion, away));
            }
            McpePacketData::DeathInfoPacket(info) => {
                fight.death_info = Some((
                    info.death_cause_attack_name.to_string(),
                    info.death_cause_message_list
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                ));
            }
            McpePacketData::TextPacket(text) => {
                if let TextPacketBody::MessageAndParams(message) = text.body
                    && message.message.starts_with("death.")
                {
                    fight.death_chat = Some((
                        message.message.to_string(),
                        message
                            .parameter_list
                            .iter()
                            .map(ToString::to_string)
                            .collect(),
                    ));
                }
            }
            McpePacketData::RespawnPacket(respawn) => {
                fight.respawn_screen.push((respawn.state, respawn.position));
            }
            McpePacketData::DisconnectPacket(disconnect) => {
                panic!("disconnected during the fight: {disconnect:?}")
            }
            _ => {}
        }
    }
    (fight, zombie)
}

/// Checks the recorded fight: the zombie closed in and swung, each hit hurt
/// the bot by the zombie's damage on hard (with a held weapon's bonus when
/// the zombie spawned holding one) once per cooldown and knocked it away,
/// and the bot died to it with the death screen and chat message.
fn check_zombie_fight(bot: &Bot, fight: &ZombieFight, zombie: u64) {
    let trace: Vec<(u64, f32)> = fight
        .health
        .iter()
        .map(|(tick, health, _)| (*tick, *health))
        .collect();
    let swings = fight.swings.len();
    bot.log(format!(
        "zombie fight: {} zombie(s) summoned, {swings} swings, first at {:?} blocks, \
         {} hurt events, {} knockbacks, {} step(s) back to the zombie, health (tick, value) {trace:?}",
        fight.zombies_summoned,
        fight.first_swing_distance,
        fight.hurt,
        fight.knockbacks.len(),
        fight.teleports.len(),
    ));
    assert!(
        fight.died.is_some(),
        "the zombie kills the bot (ActorEvent Death) within {ZOMBIE_KILLS_WITHIN:?}: \
         health {trace:?}, {swings} swings"
    );

    // The zombie walked up to the bot before its first swing.
    let first_swing = fight
        .first_swing_distance
        .expect("the zombie swings (ActorEvent StartAttacking)");
    assert!(
        first_swing < ZOMBIE_SUMMON_DISTANCE - 1.0,
        "the zombie closes in from {ZOMBIE_SUMMON_DISTANCE} blocks before swinging: \
         it swung at {first_swing:.2}"
    );

    // Its damage: the attack plus a held weapon's, scaled for hard.
    let weapon = bot
        .mainhand
        .get(&zombie)
        .copied()
        .filter(|id| *id != 0)
        .map(|id| bot.item_name(id));
    let weapon_bonus = weapon.map_or(0.0, |name| {
        unastar_data::melee::attack_damage(Some(name)) - unastar_data::melee::attack_damage(None)
    });
    let damage = (ZOMBIE_ATTACK + weapon_bonus) * HARD_DAMAGE_SCALE;
    bot.log(format!(
        "the zombie holds {weapon:?}: {damage} damage per hit on hard"
    ));
    let mut expected = Vec::new();
    let mut health = PLAYER_MAX_HEALTH;
    while health > 0.0 {
        health = (health - damage).max(0.0);
        expected.push(health);
    }
    let observed: Vec<f32> = trace.iter().map(|(_, health)| *health).collect();
    assert!(
        observed.len() == expected.len()
            && observed
                .iter()
                .zip(&expected)
                .all(|(seen, wanted)| (seen - wanted).abs() < 1e-3),
        "each hit takes {damage} health: expected {expected:?}, got {trace:?}"
    );
    let hits = observed.len();

    // One swing per hit (two when the zombie holds something), one hurt
    // event and one knockback per hit.
    let swings_per_hit = if weapon.is_some() { 2 } else { 1 };
    assert_eq!(
        swings,
        hits * swings_per_hit,
        "ActorEvent StartAttacking per hit (holding {weapon:?})"
    );
    assert_eq!(fight.hurt, hits, "the bot's ActorEvent Hurt per hit");
    assert_eq!(
        fight.knockbacks.len(),
        hits,
        "SetActorMotion knockback per hit"
    );
    for (motion, (dx, dz)) in &fight.knockbacks {
        assert!(
            motion.x * dx + motion.z * dz > 0.0 && motion.y > 0.0,
            "the knockback {motion:?} pushes the bot up and away from the zombie \
             (direction {dx:.2}, {dz:.2})"
        );
    }

    // Hits land every cooldown while the bot stays in reach.
    let mut spaced = 0;
    for pair in fight.health.windows(2) {
        let ((before, _, before_at), (after, _, after_at)) = (pair[0], pair[1]);
        let stepped = fight
            .teleports
            .iter()
            .any(|at| *at > before_at && *at <= after_at);
        if !stepped {
            assert_eq!(
                after - before,
                ZOMBIE_ATTACK_TICKS,
                "consecutive hits are one cooldown apart: {trace:?}"
            );
            spaced += 1;
        }
    }
    assert!(
        spaced > 0,
        "at least two consecutive hits without a step: {trace:?}"
    );

    // The death screen and the chat name the zombie.
    let message = (
        "death.attack.mob".to_owned(),
        vec![BOT.to_owned(), "%entity.zombie.name".to_owned()],
    );
    assert_eq!(
        fight.death_info.as_ref(),
        Some(&message),
        "DeathInfo names the zombie"
    );
    assert_eq!(
        fight.death_chat.as_ref(),
        Some(&message),
        "the chat death message names the zombie"
    );
    let states: Vec<_> = fight
        .respawn_screen
        .iter()
        .map(|(state, _)| *state)
        .collect();
    assert_eq!(
        states,
        [
            EnumsPlayerRespawnState::Searchingforspawn,
            EnumsPlayerRespawnState::Readytospawn
        ],
        "the respawn screen opens: Respawn state 0 then 1"
    );
    for (_, position) in &fight.respawn_screen {
        assert!(
            near(position, &bot.spawn),
            "the respawn screen points at the world spawn {:?}: {position:?}",
            bot.spawn
        );
    }
}

/// Answers the respawn screen as the client's respawn button does
/// (`Respawn` client-ready-to-spawn, then `PlayerAction` respawn, which the
/// server ignores once the player lives) and checks the respawn: the
/// server's ready-to-spawn acknowledgement, `MovePlayer` to the world spawn,
/// `ActorEvent` spawn-alive and full health.
async fn answer_respawn_screen(bot: &mut Bot) {
    let me = bot.runtime_id;
    bot.log("-> Respawn ClientReadyToSpawn, PlayerAction Respawn".into());
    bot.send(RespawnPacket {
        position: Vec3::default(),
        state: EnumsPlayerRespawnState::Clientreadytospawn,
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: me,
        },
    });
    bot.send(PlayerActionPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: me,
        },
        action: EnumsPlayerActionType::Respawn,
        ..Default::default()
    });
    let deadline = Instant::now() + DEATH_PACKETS_WITHIN;
    let (mut acknowledged, mut moved, mut alive, mut health) = (None, None, false, None);
    while acknowledged.is_none() || moved.is_none() || !alive || health.is_none() {
        let Some(packet) = bot.next_until(deadline).await else {
            panic!(
                "the respawn completes within {DEATH_PACKETS_WITHIN:?}: Respawn ack {acknowledged:?}, \
                 MovePlayer {moved:?}, spawn-alive {alive}, health {health:?}, connected {}",
                bot.connected()
            );
        };
        match packet {
            McpePacketData::RespawnPacket(respawn) => acknowledged = Some(respawn.state),
            McpePacketData::MovePlayerPacket(move_player)
                if move_player.player_runtime_id.actor_runtime_id == me =>
            {
                moved = Some((move_player.position, move_player.position_mode));
            }
            McpePacketData::ActorEventPacket(event)
                if event.target_runtime_id.actor_runtime_id == me
                    && event.event_id == EnumsActorEvent::SpawnAlive =>
            {
                alive = true;
            }
            McpePacketData::UpdateAttributesPacket(update)
                if update.target_runtime_id.actor_runtime_id == me && health.is_none() =>
            {
                health = update
                    .attribute_list
                    .iter()
                    .find(|attribute| &*attribute.name == HEALTH)
                    .map(|attribute| attribute.current_value);
            }
            McpePacketData::DisconnectPacket(disconnect) => {
                panic!("disconnected while respawning: {disconnect:?}")
            }
            _ => {}
        }
    }
    assert_eq!(
        acknowledged,
        Some(EnumsPlayerRespawnState::Readytospawn),
        "the server acknowledges the client's ready-to-spawn"
    );
    let (position, mode) = moved.expect("MovePlayer after the respawn");
    assert!(
        near(&position, &bot.spawn),
        "MovePlayer respawns the bot at the world spawn {:?}: {position:?}",
        bot.spawn
    );
    assert_eq!(
        mode,
        EnumsPlayerPositionModeComponentPositionMode::Respawn,
        "MovePlayer in respawn mode"
    );
    assert_eq!(
        health,
        Some(PLAYER_MAX_HEALTH),
        "UpdateAttributes restores full health"
    );
}

/// Whether two positions are the same to wire precision.
fn near(a: &Vec3, b: &Vec3) -> bool {
    (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01 && (a.z - b.z).abs() < 0.01
}

/// Hits `mob` until it dies, respecting the hurt cooldown, and checks hurt,
/// health, death and removal; returns the item IDs that dropped.
async fn kill(bot: &mut Bot, mob: u64) -> Vec<i16> {
    let health = bot
        .health_on_add
        .get(&mob)
        .expect("the mob's AddActor carries minecraft:health")
        .0;
    // Drops can share the death tick's batch, so they are collected from here
    // on.
    let mut hurt_events = 0;
    let mut health_trace = vec![health];
    let mut died_at = None;
    let mut drops = Vec::new();
    for _ in 0..20 {
        bot.stay_in_reach(mob).await;
        bot.attack(mob);
        let deadline = Instant::now() + HIT_SPACING;
        while let Some(packet) = bot.next_until(deadline).await {
            match packet {
                McpePacketData::ActorEventPacket(event)
                    if event.target_runtime_id.actor_runtime_id == mob =>
                {
                    match event.event_id {
                        EnumsActorEvent::Hurt => hurt_events += 1,
                        EnumsActorEvent::Death => died_at = Some(Instant::now()),
                        _ => {}
                    }
                }
                McpePacketData::UpdateAttributesPacket(update)
                    if update.target_runtime_id.actor_runtime_id == mob =>
                {
                    if let Some(attribute) = update
                        .attribute_list
                        .iter()
                        .find(|attribute| &*attribute.name == HEALTH)
                    {
                        health_trace.push(attribute.current_value);
                    }
                }
                McpePacketData::AddItemActorPacket(item) => drops.push(item.item.id),
                _ => {}
            }
            if died_at.is_some() {
                break;
            }
        }
        if died_at.is_some() {
            break;
        }
    }
    println!("health over the fight: {health_trace:?}, hurt events: {hurt_events}");
    assert!(hurt_events > 0, "hits produce ActorEvent(Hurt)");
    assert!(
        health_trace.windows(2).all(|pair| pair[1] < pair[0]),
        "every health update lowers the mob's health: {health_trace:?}"
    );
    assert!(
        health_trace.len() > 1,
        "hits produce UpdateAttributes health"
    );
    let died_at = died_at.expect("the mob dies (ActorEvent Death) within 20 hits");

    // The corpse leaves within ~2 s.
    let mut removed_after = None;
    let collect_until = died_at + REMOVE_WITHIN + Duration::from_millis(1000);
    while let Some(packet) = bot.next_until(collect_until).await {
        match packet {
            McpePacketData::RemoveActorPacket(remove)
                if remove.target_actor_id.actor_unique_id as u64 == mob =>
            {
                removed_after = Some(died_at.elapsed());
            }
            McpePacketData::AddItemActorPacket(item) => drops.push(item.item.id),
            _ => {}
        }
    }
    let removed_after = removed_after.expect("RemoveActor for the dead mob");
    assert!(
        removed_after <= REMOVE_WITHIN,
        "RemoveActor {removed_after:?} after death"
    );
    drops
}

/// The joined client: a background task owns the stream, sends queued packets
/// and forwards everything received.
struct Bot {
    runtime_id: u64,
    /// Where the server last placed the bot (eye position).
    position: Vec3,
    /// The world spawn (eye position) from `StartGame`, where the bot joined.
    spawn: Vec3,
    item_ids: HashMap<String, i16>,
    /// Last known position of every actor, by runtime ID.
    actors: HashMap<u64, Vec3>,
    /// (current, max) health from each actor's `AddActor`.
    health_on_add: HashMap<u64, (f32, f32)>,
    /// Main-hand item ID of each mob, from its `MobEquipment` (0 when empty).
    mainhand: HashMap<u64, i16>,
    /// Creative item network ID by item ID, from `CreativeContent`.
    creative: HashMap<i32, u32>,
    started: Instant,
    outbound: Option<mpsc::UnboundedSender<McpePacket>>,
    inbound: mpsc::UnboundedReceiver<McpePacketData>,
    io: Option<tokio::task::JoinHandle<()>>,
}

impl Bot {
    fn start(mut play: BedrockStream<Play, Client, RakNetTransport>, game_data: &GameData) -> Self {
        let (outbound, mut outbound_rx) = mpsc::unbounded_channel::<McpePacket>();
        let (inbound_tx, inbound) = mpsc::unbounded_channel();
        let runtime_id = play.runtime_id();
        let io = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    packet = outbound_rx.recv() => match packet {
                        Some(packet) => {
                            if let Err(error) = play.send_packet(packet).await {
                                println!("send failed: {error}");
                                return;
                            }
                        }
                        None => {
                            let _ = tokio::time::timeout(
                                Duration::from_secs(2),
                                play.disconnect(EnumsConnectionDisconnectFailReason::Disconnected, ""),
                            )
                            .await;
                            return;
                        }
                    },
                    // Cancel-safe: an unfinished receive keeps its data.
                    raw = play.recv_packet_raw() => match raw {
                        Ok(raw) => {
                            let name = raw.name();
                            match raw.decode() {
                                Ok(packet) => {
                                    if inbound_tx.send(packet.data).is_err() {
                                        return;
                                    }
                                }
                                Err(error) => println!("undecodable {name:?}: {error}"),
                            }
                        }
                        Err(error) => {
                            println!("connection closed: {error}");
                            return;
                        }
                    },
                }
            }
        });
        Self {
            runtime_id,
            position: game_data.start_game.position.clone(),
            spawn: game_data.start_game.position.clone(),
            item_ids: game_data
                .item_registry
                .item_data
                .iter()
                .map(|item| (item.item_name.to_string(), item.item_id))
                .collect(),
            actors: HashMap::new(),
            health_on_add: HashMap::new(),
            mainhand: HashMap::new(),
            creative: HashMap::new(),
            started: Instant::now(),
            outbound: Some(outbound),
            inbound,
            io: Some(io),
        }
    }

    fn send(&self, packet: impl Into<McpePacket>) {
        self.outbound
            .as_ref()
            .expect("bot is running")
            .send(packet.into())
            .expect("bot connection is open");
    }

    /// Whether the connection is still open.
    fn connected(&self) -> bool {
        self.io.as_ref().is_some_and(|io| !io.is_finished())
    }

    /// Runs an operator command and waits for its system reply containing
    /// `reply`.
    async fn run_command(&mut self, line: &str, reply: &str) {
        self.command(line);
        self.expect(&format!("the reply to {line}"), |packet| match packet {
            McpePacketData::TextPacket(text) => {
                let body = format!("{:?}", text.body);
                assert!(!body.contains("Error"), "{line} was refused: {body}");
                body.contains(reply).then_some(())
            }
            _ => None,
        })
        .await;
    }

    /// The horizontal direction (not normalised) from `actor`'s last known
    /// position to the bot.
    fn direction_from(&self, actor: u64) -> (f32, f32) {
        let at = self.actors.get(&actor).cloned().unwrap_or_default();
        (self.position.x - at.x, self.position.z - at.z)
    }

    fn command(&self, line: &str) {
        self.log(format!("-> CommandRequest {line:?}"));
        self.send(CommandRequestPacket {
            command: line.into(),
            origin: CommandOriginDatajson {
                type_: "player".into(),
                ..Default::default()
            },
            is_internal: false,
            version: "latest".into(),
        });
    }

    fn attack(&self, target: u64) {
        let hit_position = self.actors.get(&target).cloned().unwrap_or_default();
        self.log(format!(
            "-> InventoryTransaction ItemUseOnActor Attack target {target}"
        ));
        self.send(InventoryTransactionPacket {
            transaction: InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(
                ItemUseOnActorInventoryTransaction {
                    runtime_id: ActorRuntimeId {
                        actor_runtime_id: target,
                    },
                    action_type: EnumsItemUseOnActorInventoryTransactionActionType::Attack,
                    slot: 0,
                    from_position: self.position.clone(),
                    hit_position,
                    ..Default::default()
                },
            ),
            ..Default::default()
        });
    }

    /// Takes `amount` of `item` from the creative inventory into the empty
    /// hotbar `slot`, as the client does when the menu is clicked; needs
    /// creative mode.
    async fn take_from_creative(&mut self, item: i16, amount: u8, slot: u8) {
        if self.creative.is_empty() {
            self.expect("CreativeContent", |packet| {
                matches!(packet, McpePacketData::CreativeContentPacket(_)).then_some(())
            })
            .await;
        }
        let creative_item_net_id = *self
            .creative
            .get(&i32::from(item))
            .unwrap_or_else(|| panic!("item {item} in the creative inventory"));
        const REQUEST: i32 = -1;
        let slot_info = |container_name, slot, net_id_variant| ItemStackRequestCerealSlotInfoData {
            fullcontainername: FullContainerName {
                container_name,
                dynamic_id: None,
            },
            slot,
            net_id_variant,
        };
        // The creative menu is the inventory screen: BDS only validates the
        // created-output and combined containers while it is open.
        self.log("-> Interact OpenInventory".into());
        self.send(InteractPacket {
            action: EnumsInteractPacketPayloadAction::Openinventory,
            ..Default::default()
        });
        self.log(format!(
            "-> ItemStackRequest CraftCreative {creative_item_net_id} + Place {amount} into slot {slot}"
        ));
        self.send(ItemStackRequestPacket {
            requests: vec![ItemStackRequestPacketDataRequestData {
                client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 {
                    id: REQUEST,
                },
                actions: vec![
                    ItemStackRequestPacketDataRequestDataActionsItem::CraftCreativeActionData(
                        ItemStackRequestCerealCraftCreativeActionData {
                            actiontype: EnumsItemStackRequestActionType::Craftcreative,
                            creative_item_net_id,
                            numberofrequestedcrafts: 1,
                        },
                    ),
                    ItemStackRequestPacketDataRequestDataActionsItem::PlaceActionData(
                        ItemStackRequestCerealPlaceActionData {
                            actiontype: EnumsItemStackRequestActionType::Place,
                            amount,
                            // The client's created-output slot, whose stack
                            // the client names by this request's id.
                            source: slot_info(
                                EnumsContainerEnumName::Createdoutputcontainer,
                                50,
                                REQUEST,
                            ),
                            destination: slot_info(
                                EnumsContainerEnumName::Combinedhotbarandinventorycontainer,
                                slot,
                                0,
                            ),
                        },
                    ),
                ],
                ..Default::default()
            }],
        });
        let result = self
            .expect("ItemStackResponse", |packet| match packet {
                McpePacketData::ItemStackResponsePacket(response) => response
                    .responses
                    .iter()
                    .find(|info| info.client_request_id.id == REQUEST)
                    .map(|info| info.result),
                _ => None,
            })
            .await;
        assert_eq!(
            result,
            EnumsItemStackNetResult::Success,
            "the creative take is accepted"
        );
        self.log("-> ContainerClose inventory".into());
        self.send(ContainerClosePacket {
            container_id: 0,
            container_type: 0xff,
            server_initiated_close: false,
        });
    }

    /// Uses the held item in the air (a right click), which throws an egg.
    fn use_held_item(&self) {
        self.log("-> InventoryTransaction ItemUse Use".into());
        self.send(InventoryTransactionPacket {
            transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(
                Box::new(ItemUseInventoryTransaction {
                    action_type: EnumsItemUseInventoryTransactionActionType::Use,
                    from_position: self.position.clone(),
                    ..Default::default()
                }),
            ),
            ..Default::default()
        });
    }

    /// Teleports next to `target` when knockback or panic carried it more
    /// than 3 blocks away (the server allows 6).
    async fn stay_in_reach(&mut self, target: u64) {
        let Some(at) = self.actors.get(&target).cloned() else {
            return;
        };
        let (dx, dy, dz) = (
            at.x - self.position.x,
            at.y + EYE_HEIGHT - self.position.y,
            at.z - self.position.z,
        );
        if dx * dx + dy * dy + dz * dz <= 9.0 {
            return;
        }
        let (x, y, z) = (at.x, at.y + EYE_HEIGHT, at.z - 1.5);
        self.command(&format!("/tp {x:.2} {y:.2} {z:.2}"));
        let me = self.runtime_id;
        self.expect("MovePlayer teleport", |packet| match packet {
            McpePacketData::MovePlayerPacket(moved)
                if moved.player_runtime_id.actor_runtime_id == me =>
            {
                Some(())
            }
            _ => None,
        })
        .await;
    }

    /// Waits (unbounded; the caller's timeout applies) for the first packet
    /// `pick` accepts.
    async fn expect<T>(
        &mut self,
        what: &str,
        mut pick: impl FnMut(&McpePacketData) -> Option<T>,
    ) -> T {
        loop {
            let packet = self
                .next()
                .await
                .unwrap_or_else(|| panic!("closed before {what}"));
            if let Some(found) = pick(&packet) {
                return found;
            }
        }
    }

    async fn next(&mut self) -> Option<McpePacketData> {
        let packet = self.inbound.recv().await?;
        self.observe(&packet);
        Some(packet)
    }

    /// The next packet, or `None` once `deadline` passes.
    async fn next_until(&mut self, deadline: Instant) -> Option<McpePacketData> {
        tokio::time::timeout_at(deadline, self.next())
            .await
            .ok()
            .flatten()
    }

    /// Tracks actor and self positions and logs the packets this test is about.
    fn observe(&mut self, packet: &McpePacketData) {
        match packet {
            McpePacketData::AddActorPacket(actor) => {
                let id = actor.target_runtime_id.actor_runtime_id;
                self.actors.insert(id, actor.position.clone());
                let health = actor
                    .attributes_list
                    .iter()
                    .find(|attribute| &*attribute.attribute_name == HEALTH)
                    .map(|attribute| (attribute.current_value, attribute.max_value));
                if let Some(health) = health {
                    self.health_on_add.insert(id, health);
                }
                self.log(format!(
                    "<- AddActor {} runtime {id} unique {} at {:?} health {health:?}",
                    &*actor.actor_type, actor.target_actor_id.actor_unique_id, actor.position,
                ));
            }
            McpePacketData::MoveActorDeltaPacket(moved) => {
                let data = &moved.move_data;
                if let Some(at) = self.actors.get_mut(&data.actor_runtime_id.actor_runtime_id) {
                    at.x = data.new_position_x.unwrap_or(at.x);
                    at.y = data.new_position_y.unwrap_or(at.y);
                    at.z = data.new_position_z.unwrap_or(at.z);
                }
            }
            McpePacketData::MoveActorAbsolutePacket(moved) => {
                let data = &moved.move_data;
                self.actors.insert(
                    data.actor_runtime_id.actor_runtime_id,
                    data.position.clone(),
                );
            }
            McpePacketData::MovePlayerPacket(moved)
                if moved.player_runtime_id.actor_runtime_id == self.runtime_id =>
            {
                self.position = moved.position.clone();
                self.log(format!(
                    "<- MovePlayer (self) to {:?} mode {:?}",
                    moved.position, moved.position_mode
                ));
            }
            McpePacketData::ActorEventPacket(event) => self.log(format!(
                "<- ActorEvent {:?} runtime {} data {}",
                event.event_id, event.target_runtime_id.actor_runtime_id, event.data
            )),
            McpePacketData::UpdateAttributesPacket(update) => {
                let health = update
                    .attribute_list
                    .iter()
                    .find(|attribute| &*attribute.name == HEALTH)
                    .map(|attribute| (attribute.current_value, attribute.max_value));
                let id = update.target_runtime_id.actor_runtime_id;
                let whose = if id == self.runtime_id { " (self)" } else { "" };
                self.log(format!(
                    "<- UpdateAttributes runtime {id}{whose} tick {} health {health:?}",
                    update.tick.inputtick
                ));
            }
            McpePacketData::SetActorMotionPacket(motion) => {
                let id = motion.target_runtime_id.actor_runtime_id;
                let whose = if id == self.runtime_id { " (self)" } else { "" };
                self.log(format!(
                    "<- SetActorMotion runtime {id}{whose} {:?}",
                    motion.motion
                ));
            }
            McpePacketData::MobEquipmentPacket(equipment)
                if equipment.slot == 0 && equipment.container_id == 0 =>
            {
                let id = equipment.target_runtime_id.actor_runtime_id;
                self.mainhand.insert(id, equipment.item.id);
                self.log(format!(
                    "<- MobEquipment runtime {id} main hand {} (id {})",
                    self.item_name(equipment.item.id),
                    equipment.item.id
                ));
            }
            McpePacketData::DeathInfoPacket(info) => self.log(format!(
                "<- DeathInfo {:?} {:?}",
                info.death_cause_attack_name, info.death_cause_message_list
            )),
            McpePacketData::RespawnPacket(respawn) => self.log(format!(
                "<- Respawn {:?} at {:?} runtime {}",
                respawn.state, respawn.position, respawn.player_runtime_id.actor_runtime_id
            )),
            McpePacketData::RemoveActorPacket(remove) => {
                self.actors
                    .remove(&(remove.target_actor_id.actor_unique_id as u64));
                self.log(format!(
                    "<- RemoveActor unique {}",
                    remove.target_actor_id.actor_unique_id
                ));
            }
            McpePacketData::AddItemActorPacket(item) => {
                let name = self.item_name(item.item.id);
                self.log(format!(
                    "<- AddItemActor runtime {} {name} (id {}) x{} at {:?}",
                    item.target_runtime_id.actor_runtime_id,
                    item.item.id,
                    item.item.stacksize,
                    item.position
                ));
            }
            McpePacketData::TakeItemActorPacket(take) => self.log(format!(
                "<- TakeItemActor item {} by {}",
                take.item_runtime_id.actor_runtime_id, take.actor_runtime_id.actor_runtime_id
            )),
            McpePacketData::TextPacket(text) => self.log(format!("<- Text {:?}", text.body)),
            McpePacketData::CreativeContentPacket(content) => {
                for entry in &content.entries {
                    self.creative
                        .entry(entry.item_instance.id)
                        .or_insert(entry.creative_net_id.id);
                }
            }
            McpePacketData::ItemStackResponsePacket(response) => {
                for info in &response.responses {
                    self.log(format!(
                        "<- ItemStackResponse request {} {:?}",
                        info.client_request_id.id, info.result
                    ));
                }
            }
            McpePacketData::CommandOutputPacket(output) => {
                self.log(format!("<- CommandOutput {:?}", output.output))
            }
            McpePacketData::DisconnectPacket(disconnect) => {
                self.log(format!("<- Disconnect {disconnect:?}"))
            }
            _ => {}
        }
    }

    fn item_id(&self, name: &str) -> i16 {
        *self
            .item_ids
            .get(name)
            .unwrap_or_else(|| panic!("{name} in the item registry"))
    }

    /// The registry name of item `id`, or `?` for an unknown ID (0 is air).
    fn item_name(&self, id: i16) -> &str {
        if id == 0 {
            return "minecraft:air";
        }
        self.item_ids
            .iter()
            .find(|(_, item)| **item == id)
            .map_or("?", |(name, _)| name.as_str())
    }

    fn log(&self, line: String) {
        println!("[{:>6} ms] {line}", self.started.elapsed().as_millis());
    }

    /// Disconnects and stops the background task.
    async fn shutdown(&mut self) {
        self.outbound = None;
        if let Some(io) = self.io.take() {
            let _ = tokio::time::timeout(Duration::from_secs(3), io).await;
        }
    }
}
