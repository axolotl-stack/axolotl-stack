//! Loopback end-to-end check of vanilla mobs: a jolyne client joins an
//! in-process Unastar server, summons a cow as an operator, punches it to
//! death and checks the packets a real client would see.
//!
//! The test binds a UDP socket on `127.0.0.1` (never a public interface), so
//! it is ignored by default. Run it with:
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
//! a temporary directory that is the process working directory for the run.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use jolyne::stream::RakNetTransport;
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsActorEvent, EnumsItemUseOnActorInventoryTransactionActionType,
    InventoryTransactionPacketTransaction, ItemUseOnActorInventoryTransaction, Vec3,
};
use jolyne::valentine::{
    CommandOriginDatajson, CommandRequestPacket, EnumsConnectionDisconnectFailReason,
    InventoryTransactionPacket, McpePacket, McpePacketData,
};
use jolyne::{BedrockStream, Client, ClientHandshakeConfig, GameData, Play};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use unastar::UnastarConfig;
use unastar::server::UnastarServer;
use unastar::world::WorldConfig;

/// Display name of the test client; listed as a server operator.
const BOT: &str = "MobBot";
/// Stack for both runtimes (see the module docs).
const STACK_SIZE: usize = 64 * 1024 * 1024;
/// Bound for starting the server and completing the join.
const JOIN_TIMEOUT: Duration = Duration::from_secs(30);
/// Bound for the summon/hit/death phase.
const COMBAT_TIMEOUT: Duration = Duration::from_secs(30);
/// Spacing between hits: longer than the 10-tick (0.5 s) hurt cooldown.
const HIT_SPACING: Duration = Duration::from_millis(600);
/// How long after `ActorEvent(Death)` the cow's `RemoveActor` may take.
const REMOVE_WITHIN: Duration = Duration::from_secs(2);
/// Player eye height above the feet.
const EYE_HEIGHT: f32 = 1.62;
const COW: &str = "minecraft:cow";
const HEALTH: &str = "minecraft:health";

#[test]
#[ignore = "loopback socket run: use the native lock"]
fn summoned_cow_is_hurt_killed_and_drops_loot() {
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
    // A wide void platform: chunks generate inline and a panicking cow
    // cannot reach the edge during the fight.
    config.world = WorldConfig::void_spawn_platform(4);
    config.players.data_dir = dir.path().join("playerdata");
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
                .block_on(run_client(addr))
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

async fn run_client(addr: SocketAddr) {
    let (play, game_data) = tokio::time::timeout(JOIN_TIMEOUT, join(addr))
        .await
        .expect("server start and join within the timeout");
    let mut bot = Bot::start(play, &game_data);
    let outcome = tokio::time::timeout(COMBAT_TIMEOUT, fight_cow(&mut bot)).await;
    bot.shutdown().await;
    outcome.expect("summon, hits and death within the timeout");
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

/// a. summon, b. hurt, c. death, removal and drops.
async fn fight_cow(bot: &mut Bot) {
    // a. `/summon` as an operator: the cow appears with full health.
    bot.command("/summon minecraft:cow ~ ~ ~2");
    let cow = bot
        .expect("AddActor minecraft:cow", |packet| match packet {
            McpePacketData::AddActorPacket(actor) if &*actor.actor_type == COW => {
                Some(actor.target_runtime_id.actor_runtime_id)
            }
            // Command errors come back as system text.
            McpePacketData::TextPacket(text) if format!("{:?}", text.body).contains("Error:") => {
                panic!("/summon was refused: {:?}", text.body)
            }
            _ => None,
        })
        .await;
    let health = *bot
        .health_on_add
        .get(&cow)
        .expect("the cow's AddActor carries minecraft:health");
    assert_eq!(health, (10.0, 10.0), "a fresh cow has 10/10 health");

    // b. + c. hit until it dies, respecting the hurt cooldown. Drops can share
    // the death tick's batch, so they are collected from here on.
    let mut hurt_events = 0;
    let mut health_trace = vec![health.0];
    let mut died_at = None;
    let mut drops = Vec::new();
    for _ in 0..20 {
        bot.stay_in_reach(cow).await;
        bot.attack(cow);
        let deadline = Instant::now() + HIT_SPACING;
        while let Some(packet) = bot.next_until(deadline).await {
            match packet {
                McpePacketData::ActorEventPacket(event)
                    if event.target_runtime_id.actor_runtime_id == cow =>
                {
                    match event.event_id {
                        EnumsActorEvent::Hurt => hurt_events += 1,
                        EnumsActorEvent::Death => died_at = Some(Instant::now()),
                        _ => {}
                    }
                }
                McpePacketData::UpdateAttributesPacket(update)
                    if update.target_runtime_id.actor_runtime_id == cow =>
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
        "every health update lowers the cow's health: {health_trace:?}"
    );
    assert!(
        health_trace.len() > 1,
        "hits produce UpdateAttributes health"
    );
    let died_at = died_at.expect("the cow dies (ActorEvent Death) within 20 hits");

    // c. the corpse leaves within ~2 s; beef (and maybe leather) drops.
    let mut removed_after = None;
    let collect_until = died_at + REMOVE_WITHIN + Duration::from_millis(1000);
    while let Some(packet) = bot.next_until(collect_until).await {
        match packet {
            McpePacketData::RemoveActorPacket(remove)
                if remove.target_actor_id.actor_unique_id as u64 == cow =>
            {
                removed_after = Some(died_at.elapsed());
            }
            McpePacketData::AddItemActorPacket(item) => drops.push(item.item.id),
            _ => {}
        }
    }
    let removed_after = removed_after.expect("RemoveActor for the dead cow");
    assert!(
        removed_after <= REMOVE_WITHIN,
        "RemoveActor {removed_after:?} after death"
    );
    let beef = bot.item_id("minecraft:beef");
    let leather = bot.item_id("minecraft:leather");
    assert!(drops.contains(&beef), "beef drops: {drops:?}");
    assert!(
        drops.iter().all(|id| *id == beef || *id == leather),
        "only beef and leather drop: {drops:?} (beef {beef}, leather {leather})"
    );
}

/// The joined client: a background task owns the stream, sends queued packets
/// and forwards everything received.
struct Bot {
    runtime_id: u64,
    /// Where the server last placed the bot (eye position).
    position: Vec3,
    item_ids: HashMap<String, i16>,
    /// Last known position of every actor, by runtime ID.
    actors: HashMap<u64, Vec3>,
    /// (current, max) health from each actor's `AddActor`.
    health_on_add: HashMap<u64, (f32, f32)>,
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
            item_ids: game_data
                .item_registry
                .item_data
                .iter()
                .map(|item| (item.item_name.to_string(), item.item_id))
                .collect(),
            actors: HashMap::new(),
            health_on_add: HashMap::new(),
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
                self.log(format!("<- MovePlayer (self) to {:?}", moved.position));
            }
            McpePacketData::ActorEventPacket(event) => self.log(format!(
                "<- ActorEvent {:?} runtime {} data {}",
                event.event_id, event.target_runtime_id.actor_runtime_id, event.data
            )),
            McpePacketData::UpdateAttributesPacket(update)
                if update.target_runtime_id.actor_runtime_id != self.runtime_id =>
            {
                let health = update
                    .attribute_list
                    .iter()
                    .find(|attribute| &*attribute.name == HEALTH)
                    .map(|attribute| (attribute.current_value, attribute.max_value));
                self.log(format!(
                    "<- UpdateAttributes runtime {} health {health:?}",
                    update.target_runtime_id.actor_runtime_id
                ));
            }
            McpePacketData::SetActorMotionPacket(motion)
                if motion.target_runtime_id.actor_runtime_id != self.runtime_id =>
            {
                self.log(format!(
                    "<- SetActorMotion runtime {} {:?}",
                    motion.target_runtime_id.actor_runtime_id, motion.motion
                ));
            }
            McpePacketData::RemoveActorPacket(remove) => {
                self.actors
                    .remove(&(remove.target_actor_id.actor_unique_id as u64));
                self.log(format!(
                    "<- RemoveActor unique {}",
                    remove.target_actor_id.actor_unique_id
                ));
            }
            McpePacketData::AddItemActorPacket(item) => {
                let name = self
                    .item_ids
                    .iter()
                    .find(|(_, id)| **id == item.item.id)
                    .map_or("?", |(name, _)| name.as_str());
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
