//! Scripted player for the BDS sweep harness (`tools/bds-sweep`).
//!
//! Joins an offline BDS over loopback and behaves like an idle vanilla
//! client so the server simulates it: `PlayerAuthInput` every 50 ms at the
//! position and rotation the server last set, latency probes answered,
//! respawns and dimension changes acknowledged. The Endstone sweep plugin
//! drives experiments server-side through this player (held items, armour
//! slots, `/execute as`) and sends it orders as chat messages:
//!
//! * `bds-sweep:mine X Y Z FACE PREDICT`: start destroying the block (block
//!   action `StartDestroyBlock`), then `ContinueDestroyBlock` every tick; when
//!   `PREDICT` > 0, send `PredictDestroyBlock` that many ticks after the start
//!   and stop;
//! * `bds-sweep:stop`: abort the current mining (`AbortDestroyBlock`).
//!
//! Exits successfully when the server closes the session.

use anyhow::{Context, Result};
use clap::Parser;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::{
    BedrockStream, JolyneError,
    valentine::{
        ActorRuntimeId, BlockPos, EnumsClientPlayMode, EnumsInputMode, EnumsNewInteractionModel,
        EnumsPlayerActionType, EnumsPlayerAuthInputPacketPayloadInputData, EnumsPlayerRespawnState,
        McpePacket, McpePacketData, McpePacketName, PlayerActionPacket, PlayerAuthInputPacket,
        PlayerBlockActionData, PlayerInputTick, RespawnPacket, Vec2, Vec3,
    },
};
use std::net::SocketAddr;
use std::time::Duration;
use tracing::info;

#[derive(Parser, Debug)]
#[command(about = "Idle scripted player for BDS sweeps")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:19132")]
    addr: SocketAddr,
    #[arg(long, default_value = "BDSSweep")]
    name: String,
    /// Login timeout in seconds
    #[arg(long, default_value = "120")]
    timeout: u64,
}

/// A mining order in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mining {
    pos: [i32; 3],
    face: i32,
    /// Ticks after the start at which to predict the break; 0 = never.
    predict_after: u32,
    elapsed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Order {
    Mine(Mining),
    Stop,
}

/// Parses a plugin order from a chat message (anywhere in the text).
fn parse_order(text: &str) -> Option<Order> {
    let rest = &text[text.find("bds-sweep:")? + "bds-sweep:".len()..];
    let mut words = rest.split_whitespace();
    match words.next()? {
        "stop" | "stop\"" => Some(Order::Stop),
        "mine" => {
            // The order arrives inside the Debug form of the text body, so
            // the last number is followed by the closing quote.
            let numbers: Vec<i64> = words
                .take(5)
                .map(|word| word.trim_end_matches(|c: char| !c.is_ascii_digit()).parse())
                .collect::<Result<_, _>>()
                .ok()?;
            let [x, y, z, face, predict_after] = numbers[..] else {
                return None;
            };
            Some(Order::Mine(Mining {
                pos: [x as i32, y as i32, z as i32],
                face: face as i32,
                predict_after: u32::try_from(predict_after).ok()?,
                elapsed: 0,
            }))
        }
        _ => None,
    }
}

/// The own player's state needed to tick like a vanilla client.
struct Player {
    runtime_id: u64,
    position: Vec3,
    /// Pitch (x) and yaw (y), as the server last set them.
    rotation: Vec2,
    tick: u64,
    mining: Option<Mining>,
    /// A block action to send with the next input (abort).
    pending: Option<PlayerBlockActionData>,
}

fn block_action(action: EnumsPlayerActionType, mining: &Mining) -> PlayerBlockActionData {
    PlayerBlockActionData {
        player_action_type: action,
        position: BlockPos {
            x: mining.pos[0],
            y: mining.pos[1],
            z: mining.pos[2],
        },
        facing: mining.face,
    }
}

impl Player {
    /// The block actions of this tick's input, advancing the mining order.
    fn block_actions(&mut self) -> Vec<PlayerBlockActionData> {
        let mut actions: Vec<_> = self.pending.take().into_iter().collect();
        if let Some(mining) = &mut self.mining {
            let action = if mining.elapsed == 0 {
                EnumsPlayerActionType::Startdestroyblock
            } else if mining.predict_after != 0 && mining.elapsed >= mining.predict_after {
                EnumsPlayerActionType::Predictdestroyblock
            } else {
                EnumsPlayerActionType::Continuedestroyblock
            };
            actions.push(block_action(action, mining));
            mining.elapsed += 1;
            if action == EnumsPlayerActionType::Predictdestroyblock {
                self.mining = None;
            }
        }
        actions
    }

    fn order(&mut self, order: Order) {
        match order {
            Order::Mine(mining) => self.mining = Some(mining),
            Order::Stop => {
                if let Some(mining) = self.mining.take() {
                    self.pending = Some(block_action(
                        EnumsPlayerActionType::Abortdestroyblock,
                        &mining,
                    ));
                }
            }
        }
    }

    fn auth_input(&mut self) -> McpePacket {
        self.tick += 1;
        let actions = self.block_actions();
        let mut input_data = Vec::new();
        let player_block_actions = if actions.is_empty() {
            None
        } else {
            input_data.push(EnumsPlayerAuthInputPacketPayloadInputData::Performblockactions);
            Some(actions)
        };
        McpePacket::from(PlayerAuthInputPacket {
            player_rotation: self.rotation.clone(),
            player_head_rotation: self.rotation.y,
            interact_rotation: self.rotation.clone(),
            position: self.position.clone(),
            input_data,
            input_mode: EnumsInputMode::Mouse,
            play_mode: EnumsClientPlayMode::Normal,
            new_interaction_model: EnumsNewInteractionModel::Crosshair,
            client_tick: PlayerInputTick {
                inputtick: self.tick,
            },
            player_block_actions,
            ..Default::default()
        })
    }

    fn action(&self, action: EnumsPlayerActionType) -> McpePacket {
        McpePacket::from(PlayerActionPacket {
            player_runtime_id: ActorRuntimeId {
                actor_runtime_id: self.runtime_id,
            },
            action,
            face: -1,
            ..Default::default()
        })
    }

    /// Follows server-set positions and returns the acknowledgements a
    /// vanilla client sends.
    fn handle(&mut self, packet: McpePacketData) -> Vec<McpePacket> {
        match packet {
            McpePacketData::MovePlayerPacket(packet)
                if packet.player_runtime_id.actor_runtime_id == self.runtime_id =>
            {
                self.position = packet.position;
                self.rotation = packet.rotation;
            }
            McpePacketData::TextPacket(text) => {
                if let Some(order) = parse_order(&format!("{:?}", text.body)) {
                    info!("order {order:?}");
                    self.order(order);
                }
            }
            McpePacketData::CorrectPlayerMovePredictionPacket(packet) => {
                self.position = packet.pos;
            }
            McpePacketData::ChangeDimensionPacket(packet) => {
                self.position = packet.position;
                return vec![self.action(EnumsPlayerActionType::Changedimensionack)];
            }
            McpePacketData::RespawnPacket(packet) => match packet.state {
                EnumsPlayerRespawnState::Searchingforspawn => {
                    return vec![
                        McpePacket::from(RespawnPacket {
                            position: packet.position,
                            state: EnumsPlayerRespawnState::Clientreadytospawn,
                            player_runtime_id: ActorRuntimeId {
                                actor_runtime_id: self.runtime_id,
                            },
                        }),
                        self.action(EnumsPlayerActionType::Respawn),
                    ];
                }
                EnumsPlayerRespawnState::Readytospawn => self.position = packet.position,
                _ => {}
            },
            McpePacketData::NetworkStackLatencyPacket(mut latency) if latency.is_from_server => {
                latency.is_from_server = false;
                return vec![McpePacket::from(latency)];
            }
            _ => {}
        }
        Vec::new()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".parse().expect("static filter")),
        )
        .init();
    let (mut play, game) = tokio::time::timeout(Duration::from_secs(args.timeout), async {
        BedrockStream::connect(args.addr)
            .await?
            .join(ClientHandshakeConfig::random(args.addr, &args.name))
            .await
    })
    .await
    .context("login timed out")?
    .context("login")?;
    info!("spawned as {}", args.name);
    let mut player = Player {
        runtime_id: game.start_game.runtime_id.actor_runtime_id,
        position: game.start_game.position.clone(),
        rotation: game.start_game.rotation.clone(),
        tick: 0,
        mining: None,
        pending: None,
    };
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            _ = ticker.tick() => play.send_packet(player.auth_input()).await?,
            packet = play.recv_packet_raw() => {
                let packet = match packet {
                    Ok(packet) => packet,
                    Err(JolyneError::Disconnected(disconnect)) => {
                        info!("server closed the session: {:?}", disconnect.packet.body());
                        return Ok(());
                    }
                    Err(error) => return Err(error.into()),
                };
                if matches!(
                    packet.name(),
                    Some(
                        McpePacketName::MovePlayerPacket
                            | McpePacketName::CorrectPlayerMovePredictionPacket
                            | McpePacketName::ChangeDimensionPacket
                            | McpePacketName::RespawnPacket
                            | McpePacketName::NetworkStackLatencyPacket
                            | McpePacketName::TextPacket
                    )
                ) {
                    for reply in player.handle(packet.decode()?.data) {
                        play.send_packet(reply).await?;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player() -> Player {
        Player {
            runtime_id: 1,
            position: Vec3::default(),
            rotation: Vec2::default(),
            tick: 0,
            mining: None,
            pending: None,
        }
    }

    #[test]
    fn orders_parse_from_chat_text() {
        assert_eq!(
            parse_order(
                r#"MessageOnly(TextPacketPayloadMessageOnly { message: "bds-sweep:mine 1 -59 0 4 0" })"#
            ),
            Some(Order::Mine(Mining {
                pos: [1, -59, 0],
                face: 4,
                predict_after: 0,
                elapsed: 0
            }))
        );
        assert_eq!(parse_order("bds-sweep:stop"), Some(Order::Stop));
        assert_eq!(parse_order("bds-sweep:mine 1 2"), None);
        assert_eq!(parse_order("hello"), None);
    }

    #[test]
    fn mining_sends_start_continue_then_predict() {
        let mut player = player();
        player.order(parse_order("bds-sweep:mine 1 -59 0 4 3").unwrap());
        let kinds: Vec<_> = (0..5)
            .map(|_| {
                player
                    .block_actions()
                    .iter()
                    .map(|a| a.player_action_type)
                    .collect::<Vec<_>>()
            })
            .collect();
        use EnumsPlayerActionType::*;
        assert_eq!(
            kinds,
            [
                vec![Startdestroyblock],
                vec![Continuedestroyblock],
                vec![Continuedestroyblock],
                vec![Predictdestroyblock],
                vec![],
            ]
        );
    }

    #[test]
    fn stop_aborts_once() {
        let mut player = player();
        player.order(parse_order("bds-sweep:mine 1 -59 0 4 0").unwrap());
        player.block_actions();
        player.order(Order::Stop);
        let actions = player.block_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].player_action_type,
            EnumsPlayerActionType::Abortdestroyblock
        );
        assert!(player.block_actions().is_empty());
    }
}
