//! Loopback end-to-end check of vanilla commands: a jolyne client joins an
//! in-process Unastar server as an operator, receives BDS's command list and
//! runs `/give`, `/gamemode`, `/tp` and `/time`, checking the BDS-shaped
//! feedback (keys, parameters and side packets from the BDS 1.26.51 command
//! capture) a real client would see.
//!
//! Binds a UDP socket on `127.0.0.1` only; ignored by default. Run with:
//!
//! ```text
//! cargo test -p unastar --test commands_e2e -- --ignored --nocapture
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use jolyne::valentine::bedrock::codec::BedrockCodec;
use jolyne::valentine::types::{EnumsGameType, SyncWorldClocksPacketData};
use jolyne::valentine::{CommandOriginDatajson, CommandRequestPacket, McpePacket, McpePacketData};
use jolyne::{BedrockStream, ClientHandshakeConfig};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use unastar::UnastarConfig;
use unastar::server::UnastarServer;
use unastar::world::WorldConfig;

const BOT: &str = "CmdBot";
/// Decoding the largest join packets needs big stacks in debug builds.
const STACK_SIZE: usize = 64 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(60);
/// `AvailableCommandsPacket` id; the capture digests cover header + payload.
const AVAILABLE_COMMANDS_ID: u8 = 76;

#[test]
#[ignore = "loopback socket run: use the native lock"]
fn operator_commands_answer_like_bds_over_loopback() {
    let dir = tempfile::tempdir().expect("temporary server directory");
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
    config.server.default_gamemode = "survival".into();
    config.players.data_dir = dir.path().join("playerdata");
    config.world = WorldConfig::void_spawn_platform(2);
    config.validate().expect("test config is valid");
    let config = Arc::new(config);

    let (stop, stopped) = oneshot::channel::<()>();
    let server = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("server runtime");
            runtime.block_on(async move {
                let mut server = UnastarServer::new(config).await.expect("server setup");
                tokio::select! {
                    result = server.run() => panic!("server stopped on its own: {result:?}"),
                    _ = stopped => {}
                }
            });
            runtime.shutdown_timeout(Duration::from_secs(2));
        })
        .expect("server thread");
    let client = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_stack_size(STACK_SIZE)
                .enable_all()
                .build()
                .expect("client runtime")
                .block_on(async { tokio::time::timeout(TIMEOUT, run_client(addr)).await })
        })
        .expect("client thread");
    let client_result = client.join();
    let _ = stop.send(());
    let server_result = server.join();
    std::env::set_current_dir(std::env::temp_dir()).expect("leave the temporary directory");
    drop(dir);
    if let Err(panic) = server_result.and(client_result.map(|outcome| {
        outcome.expect("the command scenario finishes within the timeout");
    })) {
        std::panic::resume_unwind(panic);
    }
}

/// The SHA-256 the committed bds-extractor manifest records for the
/// operator's AvailableCommands capture.
fn captured_commands_sha256() -> String {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../bds-extractor/captures/1.26.51/operator/manifest.json"
    ))
    .unwrap();
    manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == "commands/operator.bin")
        .and_then(|entry| entry["sha256"].as_str())
        .unwrap()
        .to_owned()
}

struct Client {
    play: BedrockStream<jolyne::Play, jolyne::Client, jolyne::stream::RakNetTransport>,
}

impl Client {
    async fn next(&mut self) -> McpePacketData {
        loop {
            let raw = self.play.recv_packet_raw().await.expect("connection open");
            if let Ok(packet) = raw.decode() {
                return packet.data;
            }
        }
    }

    /// Sends a command and collects everything up to (and including) its
    /// CommandOutput.
    async fn command(
        &mut self,
        line: &str,
    ) -> (Vec<(String, Vec<String>)>, u32, Vec<McpePacketData>) {
        println!("-> {line}");
        self.play
            .send_packet(McpePacket::from(CommandRequestPacket {
                command: line.into(),
                origin: CommandOriginDatajson {
                    type_: "player".into(),
                    request_id: "e2e".into(),
                    ..Default::default()
                },
                is_internal: false,
                version: "latest".into(),
            }))
            .await
            .expect("send the command");
        let mut side = Vec::new();
        loop {
            match self.next().await {
                McpePacketData::CommandOutputPacket(output) => {
                    assert_eq!(&*output.origin_data.request_id, "e2e");
                    assert_eq!(&*output.output.output_type, "alloutput");
                    let messages = output
                        .output
                        .output_messages
                        .iter()
                        .map(|m| {
                            (
                                m.message_id.to_string(),
                                m.parameters.iter().map(ToString::to_string).collect(),
                            )
                        })
                        .collect();
                    println!("<- {messages:?}");
                    return (messages, output.output.success_count, side);
                }
                other => side.push(other),
            }
        }
    }
}

fn message(key: &str, params: &[&str]) -> (String, Vec<String>) {
    (key.into(), params.iter().map(|p| p.to_string()).collect())
}

fn texts(packets: &[McpePacketData]) -> Vec<String> {
    packets
        .iter()
        .filter_map(|p| match p {
            McpePacketData::TextPacket(text) => Some(format!("{:?}", text.body)),
            _ => None,
        })
        .collect()
}

async fn run_client(addr: SocketAddr) {
    let stream = loop {
        match tokio::time::timeout(Duration::from_secs(3), BedrockStream::connect(addr)).await {
            Ok(Ok(stream)) => break stream,
            _ => tokio::time::sleep(Duration::from_millis(250)).await,
        }
    };
    let (play, _game) = stream
        .join(ClientHandshakeConfig::random(addr, BOT))
        .await
        .expect("join completes");
    let mut client = Client { play };

    // The join ends with BDS's command list, byte for byte.
    let commands = loop {
        if let McpePacketData::AvailableCommandsPacket(packet) = client.next().await {
            break packet;
        }
    };
    let mut bytes = vec![AVAILABLE_COMMANDS_ID];
    commands.encode(&mut bytes).expect("encode");
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        captured_commands_sha256(),
        "AvailableCommands equals the BDS operator capture"
    );

    // /give: BDS's output, recipient message and inventory resync.
    let (messages, count, side) = client.command("/give @s wheat 64").await;
    assert_eq!(count, 1);
    assert_eq!(
        messages,
        [message("commands.give.success", &["Wheat", "64", BOT])]
    );
    assert!(
        texts(&side)
            .iter()
            .any(|t| t.contains("commands.give.successRecipient"))
    );
    assert!(
        side.iter()
            .any(|p| matches!(p, McpePacketData::InventoryContentPacket(_)))
    );

    // /gamemode: UpdatePlayerGameType and the gameMode.changed message.
    let (messages, count, side) = client.command("/gamemode creative").await;
    assert_eq!(count, 1);
    assert_eq!(
        messages,
        [message(
            "commands.gamemode.success.self",
            &["%createWorldScreen.gameMode.creative"]
        )]
    );
    assert!(side.iter().any(
        |p| matches!(p, McpePacketData::UpdatePlayerGameTypePacket(u)
        if u.player_game_type == EnumsGameType::Creative)
    ));
    assert!(texts(&side).iter().any(|t| t.contains("gameMode.changed")));

    // /tp: MovePlayer with the eye position and the victim message.
    let (messages, count, side) = client.command("/tp 0 100 0").await;
    assert_eq!(count, 1);
    assert_eq!(
        messages,
        [message(
            "commands.tp.success.coordinates",
            &[BOT, "0.50", "100.00", "0.50"]
        )]
    );
    assert!(
        side.iter()
            .any(|p| matches!(p, McpePacketData::MovePlayerPacket(m)
        if (m.position.x, m.position.y, m.position.z) == (0.5, 101.62, 0.5)))
    );
    assert!(
        texts(&side)
            .iter()
            .any(|t| t.contains("commands.tp.successVictim"))
    );

    // /time set day: the clock moves and is synced.
    let (messages, count, _) = client.command("/time set day").await;
    assert_eq!(count, 1);
    assert_eq!(messages, [message("commands.time.set", &["1000"])]);
    let synced = loop {
        if let McpePacketData::SyncWorldClocksPacket(packet) = client.next().await
            && let SyncWorldClocksPacketData::SyncStateData(state) = packet.data
        {
            break state.clock_data[0].time;
        }
    };
    assert!((1000..1100).contains(&synced), "clock synced at {synced}");

    // Unknown commands and syntax errors answer like BDS.
    let (messages, count, _) = client.command("/notacommand").await;
    assert_eq!(count, 0);
    assert_eq!(
        messages,
        [message("commands.generic.unknown", &["notacommand"])]
    );
    let (messages, _, _) = client.command("/gamemode foo").await;
    assert_eq!(
        messages,
        [message(
            "commands.generic.syntax",
            &["/gamemode ", "foo", ""]
        )]
    );
}
