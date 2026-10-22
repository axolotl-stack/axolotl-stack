//! First-class proxying: accept a player, join the upstream server as that
//! player, and relay raw packets both ways.
//!
//! Encryption is terminated on each side (the proxy owns both key
//! exchanges). Packets are forwarded as their original frames, so unknown
//! and custom IDs pass through untouched as long as both connections use
//! [`UnknownPacketPolicy::Passthrough`] (see [`proxy_listener_config`]).
//!
//! Join order (gophertunnel's proxy pattern): the downstream player logs in,
//! the proxy joins upstream, then the downstream `StartGame` is built from the
//! upstream's game data (same runtime ID), upstream packs are re-offered
//! downstream, and everything the upstream sent after spawning (definitions,
//! chunks) is forwarded once both sides are in play.

use std::sync::Arc;

use p384::SecretKey;

use crate::FlushMode;
use crate::auth::AuthenticatedLogin;
use crate::config::BedrockListenerConfig;
use crate::error::JolyneError;
use crate::proto::custom::UnknownPacketPolicy;
use crate::proto::packs::{ResourcePack, ServerPacks};
use crate::proto::phase::Owner;
use crate::proto::raw::RawPacket;
use crate::stream::client::ClientHandshakeConfig;
use crate::stream::server::ServerHandshakeConfig;
use crate::stream::transport::Transport;
use crate::stream::{BedrockStream, Client, Failed, Handshake, Play, Server};
use crate::valentine::{EnumsConnectionDisconnectFailReason, McpePacketName};
use crate::world::WorldJoinParams;

/// A listener configuration suitable for proxies: unknown packet IDs pass
/// through instead of disconnecting the player.
pub fn proxy_listener_config(mut config: BedrockListenerConfig) -> BedrockListenerConfig {
    config.connection.unknown_packets = UnknownPacketPolicy::Passthrough;
    config
}

/// Which side a packet came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The player connected to the proxy.
    Downstream,
    /// The server the proxy joined.
    Upstream,
}

/// What to do with a relayed packet.
pub enum Verdict {
    Forward,
    Drop,
    Replace(RawPacket),
}

/// How a relay ended.
#[derive(Debug)]
pub struct RelayEnd {
    /// The side whose connection ended the relay.
    pub side: Side,
    /// `Disconnected` when that side sent a `Disconnect` (already forwarded
    /// to the other side), otherwise the transport or protocol error.
    pub error: JolyneError,
}

/// Both play sessions of a proxied player.
pub struct ProxySession<D: Transport, U: Transport> {
    pub downstream: BedrockStream<Play, Server, D>,
    pub upstream: BedrockStream<Play, Client, U>,
    pub login: AuthenticatedLogin,
}

async fn reasoned<T, D: Transport>(
    step: impl Future<Output = Result<T, Failed<D>>>,
) -> Result<T, JolyneError> {
    match step.await {
        Ok(next) => Ok(next),
        Err(failed) => Err(failed.disconnect().await),
    }
}

/// Logs the player in downstream, joins `upstream` as them, and spawns the
/// player with the upstream's world.
///
/// `dial` receives the verified downstream login and returns a connected
/// upstream handshake stream plus the identity to join with (e.g. an
/// offline identity carrying the player's name).
pub async fn accept<D, U, F, Fut>(
    downstream: BedrockStream<Handshake, Server, D>,
    server_key: &SecretKey,
    dial: F,
) -> Result<ProxySession<D, U>, JolyneError>
where
    D: Transport,
    U: Transport,
    F: FnOnce(&AuthenticatedLogin) -> Fut,
    Fut: Future<
        Output = Result<(BedrockStream<Handshake, Client, U>, ClientHandshakeConfig), JolyneError>,
    >,
{
    let login_stream = reasoned(downstream.accept_network_settings()).await?;
    let (secure, login) = reasoned(login_stream.authenticate()).await?;
    // Finish the downstream login stage first: it runs under `login_timeout`,
    // while the logged-in pack phase has no deadline until packs are offered.
    let packs = reasoned(secure.finish_handshake(&ServerHandshakeConfig {
        server_key: server_key.clone(),
    }))
    .await?;

    let dialed = match dial(&login).await {
        Ok((upstream, identity)) => upstream.join(identity).await,
        Err(error) => Err(error),
    };
    let (upstream, game_data) = match dialed {
        Ok(joined) => joined,
        Err(error) => {
            let _ = packs
                .disconnect(
                    EnumsConnectionDisconnectFailReason::Unknown,
                    &error.to_string(),
                )
                .await;
            return Err(error);
        }
    };

    let downstream = async {
        let start = reasoned(packs.negotiate_packs_with(upstream_pack_set(&upstream))).await?;
        reasoned(start.start_game(WorldJoinParams {
            pre_start_game: Vec::new(),
            start_game: game_data.start_game,
            item_registry: Arc::new(game_data.item_registry),
            // The upstream's copies are queued in its play session and get
            // forwarded verbatim by the relay.
            biome_definitions: None,
            available_entities: None,
            creative_content: None,
            chunk_radius: upstream.chunk_radius(),
        }))
        .await
    }
    .await;
    match downstream {
        Ok(downstream) => Ok(ProxySession {
            downstream,
            upstream,
            login,
        }),
        Err(error) => {
            let _ = upstream.close().await;
            Err(error)
        }
    }
}

/// The upstream's packs, re-offered downstream with the upstream's
/// `required` flag, CDN URLs and already-verified hashes.
fn upstream_pack_set<U: Transport>(upstream: &BedrockStream<Play, Client, U>) -> Arc<ServerPacks> {
    let packs = upstream
        .resource_packs()
        .iter()
        .filter_map(|pack| {
            let mut offered = ResourcePack::from_verified(
                pack.offer.uuid,
                pack.offer.version.clone(),
                pack.content.clone(),
                pack.sha256,
            )
            .ok()?;
            offered.content_key = pack.offer.content_key.clone();
            offered.sub_pack_name = pack.offer.sub_pack_name.clone();
            offered.cdn_url = pack.offer.cdn_url.clone();
            offered.has_scripts = pack.offer.has_scripts;
            offered.is_addon = pack.offer.is_addon;
            Some(Arc::new(offered))
        })
        .collect();
    Arc::new(ServerPacks {
        packs,
        required: upstream.resource_packs_required(),
        ..ServerPacks::default()
    })
}

impl<D: Transport, U: Transport> ProxySession<D, U> {
    /// Forwards packets in both directions until either side ends.
    ///
    /// `inspect` sees every packet before it is forwarded. Packets are queued
    /// on the far side and flushed whenever the near side's batch is drained,
    /// so batch boundaries are preserved. A `Disconnect` from either side is
    /// forwarded and both connections are closed.
    pub async fn relay(mut self, mut inspect: impl FnMut(Side, &RawPacket) -> Verdict) -> RelayEnd {
        if let Err(error) = self.downstream.set_flush_mode(FlushMode::Manual).await {
            return RelayEnd {
                side: Side::Downstream,
                error,
            };
        }
        if let Err(error) = self.upstream.set_flush_mode(FlushMode::Manual).await {
            return RelayEnd {
                side: Side::Upstream,
                error,
            };
        }
        let end = loop {
            tokio::select! {
                packet = self.downstream.recv_packet_raw() => {
                    let drained = self.downstream.owner().connection_ref().inbound_len() == 0;
                    if let Err(end) = forward(Side::Downstream, packet, &mut self.upstream, drained, &mut inspect).await {
                        break end;
                    }
                }
                packet = self.upstream.recv_packet_raw() => {
                    let drained = self.upstream.owner().connection_ref().inbound_len() == 0;
                    if let Err(end) = forward(Side::Upstream, packet, &mut self.downstream, drained, &mut inspect).await {
                        break end;
                    }
                }
            }
        };
        let _ = self.downstream.close().await;
        let _ = self.upstream.close().await;
        end
    }
}

async fn forward<R, T>(
    side: Side,
    packet: Result<RawPacket, JolyneError>,
    to: &mut BedrockStream<Play, R, T>,
    drained: bool,
    inspect: &mut impl FnMut(Side, &RawPacket) -> Verdict,
) -> Result<(), RelayEnd>
where
    R: crate::stream::Role,
    T: Transport,
    Play: crate::stream::PhaseOf<R>,
{
    let other = match side {
        Side::Downstream => Side::Upstream,
        Side::Upstream => Side::Downstream,
    };
    let packet = match packet {
        Ok(packet) => packet,
        Err(JolyneError::Disconnected(disconnect)) => {
            let _ = to.send_packet_raw(&disconnect.packet).await;
            let _ = to.flush().await;
            return Err(RelayEnd {
                side,
                error: JolyneError::Disconnected(disconnect),
            });
        }
        Err(error) => return Err(RelayEnd { side, error }),
    };
    let packet = match inspect(side, &packet) {
        Verdict::Forward => packet,
        Verdict::Drop => return flush_if(to, drained, other).await,
        Verdict::Replace(replacement) => replacement,
    };
    debug_assert!(!packet.is(McpePacketName::DisconnectPacket));
    if let Err(error) = to.send_packet_raw(&packet).await {
        return Err(RelayEnd { side: other, error });
    }
    flush_if(to, drained, other).await
}

async fn flush_if<R, T>(
    to: &mut BedrockStream<Play, R, T>,
    drained: bool,
    side: Side,
) -> Result<(), RelayEnd>
where
    R: crate::stream::Role,
    T: Transport,
    Play: crate::stream::PhaseOf<R>,
{
    if drained && let Err(error) = to.flush().await {
        return Err(RelayEnd { side, error });
    }
    Ok(())
}
