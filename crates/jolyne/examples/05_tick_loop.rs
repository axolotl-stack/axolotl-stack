//! Tier 3 (`jolyne::raw` surface): a tick-driven play loop.
//!
//! Game servers work in ticks (20 per second): read what players sent,
//! update the world, send the results. With the default
//! `FlushMode::Immediate` every `send_packet` is its own batch (own
//! compression, encryption and RakNet frame). `FlushMode::Manual` only
//! queues packets until `flush()`, so each tick goes out as one batch per
//! player. Incoming packets are read with `recv_packet_borrowed`, a view
//! that shares the received batch's bytes instead of allocating a copy of
//! every string and list.
//!
//! The join itself is still Tier 1 (`accept_join_sequence`). The same
//! packet methods exist on `jolyne::raw::BedrockTransport`
//! (`play.into_transport()`) for loops that do not need the typestate.
//!
//! ```text
//! cargo run -p jolyne --example 05_tick_loop -- [ADDR=127.0.0.1:19132]
//! ```
//!
//! Offline mode; join with `02_client` or a game client and chat: messages
//! are echoed back on the next tick.

use std::sync::Arc;
use std::time::Duration;

use jolyne::stream::raknet_types::ServerPlayStream;
use jolyne::valentine::{
    BorrowedMcpePacketData, SetTimePacket, TextPacket, TextPacketBody, TextPacketBodyView,
    TextPacketPayloadAuthorAndMessage, TextPacketPayloadAuthorAndMessageMessageType,
};
use jolyne::{BedrockListener, BedrockListenerConfig, FlushMode, JolyneError, WorldTemplate};
use p384::SecretKey;
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

/// 20 ticks per second, as vanilla.
const TICK: Duration = Duration::from_millis(50);
/// Ticks per Minecraft day (`SetTime` wraps here).
const DAY: u64 = 24_000;

/// Runs one player's play loop until they leave.
async fn play_loop(mut play: ServerPlayStream) -> Result<(), JolyneError> {
    let name = play.identity().display_name().to_owned();
    // From now on `send_packet` only queues; `flush` writes one batch.
    play.set_flush_mode(FlushMode::Manual).await?;

    let mut ticker = tokio::time::interval(TICK);
    // A slow tick delays the next one instead of firing a burst to catch up.
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut tick: u64 = 0;
    loop {
        tokio::select! {
            // The tick wins ties, so a flood of packets cannot starve it.
            biased;
            _ = ticker.tick() => {
                tick += 1;
                // World update for this tick: once a second, sync the clock.
                if tick.is_multiple_of(20) {
                    let time = (tick % DAY) as i32;
                    play.send_packet(SetTimePacket { time }.into()).await?;
                }
                // Everything queued this tick (time, chat echoes) leaves as
                // one batch.
                play.flush().await?;
            }
            // Between ticks, drain what the player sends. Dropping this
            // future when the tick fires is safe: `recv_packet_*` is
            // cancel-safe, a packet is either returned or stays queued.
            packet = play.recv_packet_borrowed() => match packet {
                Ok(packet) => {
                    if let BorrowedMcpePacketData::TextPacket(text) = packet.data
                        && let TextPacketBodyView::AuthorAndMessage(chat) = text.body
                    {
                        let message = chat.message.to_string_lossy();
                        info!(%name, %message, tick, "chat");
                        // Queued for the next flush, not written now.
                        play.send_packet(echo(&name, &message).into()).await?;
                    }
                }
                Err(error) if error.is_recoverable() => warn!(%name, %error, "skipped packet"),
                Err(JolyneError::Disconnected(_) | JolyneError::ConnectionClosed) => break,
                Err(error) => return Err(error),
            },
        }
    }
    let metrics = play.metrics();
    info!(
        %name,
        ticks = tick,
        packets_out = metrics.packets_out,
        batches_out = metrics.batches_out,
        "left"
    );
    Ok(())
}

/// A chat line from `name`, as servers relay chat.
fn echo(name: &str, message: &str) -> TextPacket {
    TextPacket {
        body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
            message_type: TextPacketPayloadAuthorAndMessageMessageType::Chat,
            player_name: name.into(),
            message: message.into(),
        }),
        ..TextPacket::default()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:19132".into());
    let mut listener = BedrockListener::raknet()
        .addr(addr.as_str())
        .config(BedrockListenerConfig {
            online_mode: false,
            ..BedrockListenerConfig::default()
        })
        .bind()
        .await?;
    info!(addr = ?listener.local_addr(), "listening");

    let world = Arc::new(WorldTemplate::default());
    let server_key = SecretKey::random(&mut rand::thread_rng());
    loop {
        let stream = listener.accept().await?;
        let peer = stream.peer_addr();
        let (world, server_key) = (world.clone(), server_key.clone());
        tokio::spawn(async move {
            let result = match stream.accept_join_sequence(&world, &server_key).await {
                Ok((play, _login)) => play_loop(play).await,
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                warn!(%peer, %error, "player dropped");
            }
        });
    }
}
