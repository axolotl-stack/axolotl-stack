//! Scripted player for the BDS sweep harness (`tools/bds-sweep`).
//!
//! Joins an offline BDS over loopback and behaves like an idle vanilla
//! client so the server simulates it: `PlayerAuthInput` every 50 ms at the
//! position the server last set, latency probes answered, respawns and
//! dimension changes acknowledged. The Endstone sweep plugin drives every
//! experiment server-side through this player (held items, armour slots,
//! `/execute as`). Exits successfully when the server closes the session.

use anyhow::{Context, Result};
use clap::Parser;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::{
    BedrockStream, JolyneError,
    valentine::{
        ActorRuntimeId, EnumsClientPlayMode, EnumsInputMode, EnumsNewInteractionModel,
        EnumsPlayerActionType, EnumsPlayerRespawnState, McpePacket, McpePacketData, McpePacketName,
        PlayerActionPacket, PlayerAuthInputPacket, PlayerInputTick, RespawnPacket, Vec3,
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

/// The own player's state needed to tick like a vanilla client.
struct Player {
    runtime_id: u64,
    position: Vec3,
    tick: u64,
}

impl Player {
    fn auth_input(&mut self) -> McpePacket {
        self.tick += 1;
        McpePacket::from(PlayerAuthInputPacket {
            position: self.position.clone(),
            input_mode: EnumsInputMode::Mouse,
            play_mode: EnumsClientPlayMode::Normal,
            new_interaction_model: EnumsNewInteractionModel::Crosshair,
            client_tick: PlayerInputTick {
                inputtick: self.tick,
            },
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
        tick: 0,
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
