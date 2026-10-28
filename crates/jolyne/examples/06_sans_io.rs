//! Tier 3 (`jolyne::proto`): drive the sans-IO core yourself.
//!
//! Everything above this tier is built on `jolyne::proto`: a server and a
//! client join phase per handshake step, each owning a
//! `proto::conn::Connection` (framing, compression, encryption, batching).
//! Nothing in it performs I/O or reads a clock: you hand it received bytes
//! and the current time, and take the bytes it wants sent. That is what
//! lets you run jolyne on your own socket, event loop or simulated clock.
//!
//! This example joins a server and a client entirely in memory: the "wire"
//! is moving each side's `poll_transmit()` output into the other side's
//! `handle_datagram()`. No sockets are opened.
//!
//! ```text
//! cargo run -p jolyne --example 06_sans_io
//! ```
//!
//! With a real transport, each phase is driven by the same four calls (this
//! is what `jolyne::stream` does for RakNet and NetherNet):
//!
//! ```text
//! loop {
//!     while let Some(t) = phase.poll_transmit() { socket.send(t.payload, t.reliable) }
//!     select! {
//!         bytes = socket.recv()                => phase.handle_datagram(Datagram::Contiguous(bytes))?,
//!         _     = sleep_until(phase.poll_timeout()) => {} // deadline or timer
//!     }
//!     phase = match phase.advance(Instant::now()) {
//!         Ok(Step::Wait(phase)) => phase,          // needs more input
//!         Ok(Step::Next(next))  => break next,     // phase done: the next owner
//!         Err(failure) => { /* failure.error; queue_failure_disconnect, flush, close */ }
//!     }
//! }
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, bail};
use jolyne::auth::client::{LoginCredentials, build_login, encode_login_request};
use jolyne::proto::client::{ClientLogin, ClientSettings};
use jolyne::proto::conn::{Connection, Datagram, Framing};
use jolyne::proto::phase::{Owner, Phase, Step};
use jolyne::proto::server::ServerHandshake;
use jolyne::valentine::{
    McpePacket, McpePacketData, TextPacket, TextPacketBody, TextPacketPayloadMessageOnly,
};
use jolyne::{BedrockListenerConfig, ClientConfig, WorldTemplate};
use p384::SecretKey;
use uuid::Uuid;

/// The in-memory "network": delivers every transmit `from` has queued to
/// `to`. A socket would carry `transmit.payload` (and `transmit.reliable`)
/// instead.
fn pump(from: &mut impl Owner, to: &mut impl Owner) -> anyhow::Result<()> {
    while let Some(transmit) = from.poll_transmit() {
        to.connection()
            .handle_datagram(Datagram::Contiguous(transmit.payload))?;
    }
    Ok(())
}

/// Runs a phase on everything received so far. The clock is read here and
/// nowhere else in the core; a simulation could pass any `Instant`.
fn advance<P: Phase>(phase: P) -> anyhow::Result<Step<P, P::Next>> {
    phase
        .advance(Instant::now())
        .map_err(|failure| anyhow::anyhow!("{} failed: {}", P::PHASE, failure.error))
}

/// [`advance`], expecting the phase to complete.
fn next<P: Phase>(phase: P) -> anyhow::Result<P::Next> {
    match advance(phase)? {
        Step::Next(next) => Ok(next),
        Step::Wait(_) => bail!("{} is still waiting", P::PHASE),
    }
}

/// [`advance`], expecting the phase to need more input.
fn wait<P: Phase>(phase: P) -> anyhow::Result<P> {
    match advance(phase)? {
        Step::Wait(phase) => Ok(phase),
        Step::Next(_) => bail!("{} completed early", P::PHASE),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // The async runtime is only for the two login token calls below (online
    // mode fetches keys over HTTP); the core itself is synchronous.
    let server_config = Arc::new(BedrockListenerConfig {
        online_mode: false,
        ..BedrockListenerConfig::default()
    });
    let client_config = ClientConfig::default();
    let server_key = SecretKey::random(&mut rand::thread_rng());
    let client_key = SecretKey::random(&mut rand::thread_rng());
    // Only used as the peer address shown to the admission hook.
    let peer: SocketAddr = "127.0.0.1:0".parse()?;

    // Both sides start with a plain connection; `Framing::RakNet` puts the
    // `0xFE` batch marker RakNet carries in front of every batch.
    let now = Instant::now();
    let mut server = ServerHandshake::new(
        Connection::new(Framing::RakNet, server_config.connection.clone()),
        server_config.clone(),
        now,
    );
    // Queues `RequestNetworkSettings`.
    let mut client = ClientSettings::new(
        Connection::new(Framing::RakNet, client_config.connection.clone()),
        &client_config,
        now,
    )?;

    // Network settings: the server answers and both enable compression.
    pump(&mut client, &mut server)?;
    let mut server = next(server)?; // -> ServerLogin, `NetworkSettings` queued
    pump(&mut server, &mut client)?;
    let login: ClientLogin = next(client)?;

    // Login: the client signs its tokens (offline: self-signed) and sends
    // them; the server hands the request to the caller for verification.
    let tokens = build_login(
        &client_key,
        "SansIo",
        Uuid::new_v4(),
        &LoginCredentials::Offline,
    )
    .await?;
    let request = encode_login_request(&tokens.auth_info_json, &tokens.client_data_jwt)?;
    let mut client = login
        .login(client_key, request.into(), Instant::now())
        .map_err(|failure| failure.error)?;
    pump(&mut client, &mut server)?;
    let request = next(server)?; // -> LoginRequest
    let (auth_info, client_data) =
        jolyne::auth::decode_login_request(request.connection_request())?;
    let verified = jolyne::auth::authenticate_login(
        &auth_info,
        &client_data,
        server_config.online_mode,
        server_config.allow_legacy_auth,
    )
    .await?;
    // Runs the admission hook, if configured. `max_players` is counted by
    // the listener, not the core: refuse over your own cap with
    // `request.refuse_server_full(now)` instead.
    let authenticated = request
        .authenticated(verified, peer, Instant::now())
        .map_err(|failure| failure.error)?;

    // Encryption: `ServerToClientHandshake` goes out in plaintext, then both
    // directions switch to AES; the client proves its key by answering.
    let mut encryption = match authenticated
        .start_encryption(&server_key, None, Instant::now())
        .map_err(|failure| failure.error)?
    {
        Step::Wait(encryption) => encryption,
        Step::Next(_) => bail!("encryption is enabled in the default config"),
    };
    pump(&mut encryption, &mut client)?;
    let mut client = wait(client)?; // answers with `ClientToServerHandshake`
    pump(&mut client, &mut encryption)?;
    let mut ready = next(encryption)?; // -> PacksReady, `LoginSuccess` queued
    pump(&mut ready, &mut client)?;
    let mut client = next(client)?; // -> ClientPacks

    // Resource packs (none configured): info, the client's answer, stack,
    // completion.
    let mut packs = ready
        .start_packs(Instant::now())
        .map_err(|failure| failure.error)?;
    pump(&mut client, &mut packs)?;
    pump(&mut packs, &mut client)?; // `ResourcePacksInfo`
    let mut client = wait(client)?;
    pump(&mut client, &mut packs)?;
    let mut packs = wait(packs)?;
    pump(&mut packs, &mut client)?; // `ResourcePackStack`
    let mut client = next(client)?; // -> ClientSpawn
    pump(&mut client, &mut packs)?;
    let world = next(packs)?; // -> WorldReady

    // Spawn: `StartGame` and definitions, the chunk radius exchange, then
    // the client reports it is initialized.
    let world_template = WorldTemplate::default();
    let mut spawn = world
        .start_game(world_template.to_join_params(1), Instant::now())
        .map_err(|failure| failure.error)?;
    pump(&mut spawn, &mut client)?;
    let mut client = wait(client)?; // sends `RequestChunkRadius`
    pump(&mut client, &mut spawn)?;
    let mut spawn = wait(spawn)?; // radius, `PlayerSpawn`
    pump(&mut spawn, &mut client)?;
    let mut joined = next(client)?; // sends `SetLocalPlayerAsInitialized`
    pump(&mut joined.session, &mut spawn)?;
    let mut session = next(spawn)?; // -> ServerSession: both sides in play
    println!(
        "joined in memory: {} as runtime id {}",
        session.login.identity.display_name(),
        session.session.runtime_id()
    );

    // Play: queue a packet, seal the batch, carry it over, read it back.
    let hello = TextPacket {
        body: TextPacketBody::MessageOnly(TextPacketPayloadMessageOnly {
            message: "hello over an in-memory wire".into(),
            ..TextPacketPayloadMessageOnly::default()
        }),
        ..TextPacket::default()
    };
    let conn = joined.session.connection();
    conn.queue_packet(&McpePacket::from(hello), Instant::now())?;
    conn.flush()?;
    pump(&mut joined.session, &mut session)?;
    // Packets the client sent during the join (e.g. loading screen events)
    // were deferred and come first.
    loop {
        let packet = session
            .connection()
            .next_packet()?
            .context("the text packet did not arrive")?;
        if let McpePacketData::TextPacket(text) = packet.decode()?.data {
            println!("server received: {:?}", text.body);
            break;
        }
    }
    Ok(())
}
