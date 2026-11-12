//! Client packets BDS 1.26.51 relays to other players or answers on its
//! own, and the ones it ignores.
//!
//! Measured with `tools/bds-sweep/protocol_probe.py` (`relay`, `cache`,
//! `ddui`; fixtures under `crates/bds-extractor/captures/1.26.51/protocol/`):
//!
//! - `Emote` for the sender itself reaches the other players with the
//!   server-side flag (1) added; `EmoteList` reaches them unchanged; neither
//!   is echoed to the sender.
//! - `ClientCacheBlobStatus` is answered with a `ClientCacheMissResponse`
//!   holding the missing blobs the server has, even when the client claimed
//!   no blob cache (unastar never has any).
//! - `ServerboundDataDrivenScreenClosed` with a close reason BDS does not know
//!   ([`DDUI_CLOSE_REASONS`], any case) is a malformed packet: BDS sends a
//!   terminating `PacketViolationWarning` and closes the connection. A known
//!   reason gets no answer.
//! - `PartyChanged`, `ResourcePacksReadyForValidation`, `ServerboundDataStore`
//!   and `PartyDestinationCookieResponse` get no answer (no packet in any
//!   run); `Interact` mouse-over, leave-vehicle while not riding and NPC-open
//!   on a player get none either.

use bevy_ecs::prelude::{Entity, World};
use jolyne::valentine::types::{EnumsPacketViolationSeverity, EnumsPacketViolationType};
use jolyne::valentine::{
    ClientCacheBlobStatusPacket, ClientCacheMissResponsePacket, EmoteListPacket, EmotePacket,
    McpePacket, McpePacketName, PacketViolationWarningPacket,
    ServerboundDataDrivenScreenClosedPacket,
};

use crate::entity::components::{PlayerSession, RuntimeEntityId};
use crate::server::broadcast::players_seeing;

/// The `Emote` flag BDS adds to every emote it relays (server side).
pub const EMOTE_FLAG_SERVER: u8 = 1;

/// Close reasons BDS accepts in `ServerboundDataDrivenScreenClosed`, in any
/// ASCII case (`ProgrammaticClose` is accepted); the first three are the
/// ones its violation message names (2026-11-11 `ddui` run, 15 reasons).
pub const DDUI_CLOSE_REASONS: [&str; 5] = [
    "programmaticclose",
    "programmaticcloseall",
    "clientcanceled",
    "userbusy",
    "invalidform",
];

fn own_runtime_id(world: &World, player: Entity, claimed: u64) -> Option<i64> {
    let runtime_id = world.get::<RuntimeEntityId>(player)?.0;
    (claimed == runtime_id as u64).then_some(runtime_id)
}

fn send_to_viewers(world: &World, player: Entity, packet: McpePacket) {
    for viewer in players_seeing(world, player) {
        if let Some(session) = world.get::<PlayerSession>(viewer) {
            let _ = session.send(packet.clone());
        }
    }
}

/// Relays a player's own emote to the players who see it, with the
/// server-side flag added. Emotes for another actor are ignored.
pub fn relay_emote(world: &World, player: Entity, emote: &EmotePacket) {
    if own_runtime_id(world, player, emote.actor_runtime_id.actor_runtime_id).is_none() {
        tracing::debug!(?player, "Ignoring Emote for another actor");
        return;
    }
    let mut relayed = emote.clone();
    relayed.flags |= EMOTE_FLAG_SERVER;
    send_to_viewers(world, player, relayed.into());
}

/// Relays a player's own emote list, unchanged, to the players who see it.
pub fn relay_emote_list(world: &World, player: Entity, list: &EmoteListPacket) {
    if own_runtime_id(world, player, list.runtimeid.actor_runtime_id).is_none() {
        tracing::debug!(?player, "Ignoring EmoteList for another actor");
        return;
    }
    send_to_viewers(world, player, list.clone().into());
}

/// Answers a blob status: the server keeps no blob cache, so no blob is
/// ever missing on its side and the response is empty.
pub fn answer_blob_status(world: &World, player: Entity, _status: &ClientCacheBlobStatusPacket) {
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(
            ClientCacheMissResponsePacket {
                missing_blobs: Vec::new(),
            }
            .into(),
        );
    }
}

/// The violation BDS sends for an unknown close reason.
pub fn ddui_close_violation(reason: &str) -> PacketViolationWarningPacket {
    PacketViolationWarningPacket {
        violation_type: EnumsPacketViolationType::Packetmalformed,
        violation_severity: EnumsPacketViolationSeverity::Terminatingconnection,
        violation_packet_id: McpePacketName::ServerboundDataDrivenScreenClosedPacket as i32,
        violation_context: format!(
            "This value is bound to 'DataDrivenScreenClosedReason' and expects one of the \
             following strings: '{}', '{}', '{}', ... ({} others). Found: '{reason}'\n\
             readNoHeader failed! packetId: {}",
            DDUI_CLOSE_REASONS[0],
            DDUI_CLOSE_REASONS[1],
            DDUI_CLOSE_REASONS[2],
            DDUI_CLOSE_REASONS.len() - 3,
            McpePacketName::ServerboundDataDrivenScreenClosedPacket as u32,
        )
        .into(),
    }
}

/// A closed data-driven screen: nothing to do for a known reason (unastar
/// opens no such screens); an unknown reason ends the connection as BDS
/// does.
pub fn screen_closed(
    world: &World,
    player: Entity,
    closed: &ServerboundDataDrivenScreenClosedPacket,
) {
    let reason: &str = &closed.close_reason;
    if DDUI_CLOSE_REASONS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(reason))
    {
        return;
    }
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(ddui_close_violation(&closed.close_reason).into());
    }
}

#[cfg(test)]
pub(crate) mod tests;
