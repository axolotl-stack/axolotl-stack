//! Internal departure facts carried from the actual transport into synchronous teardown.

use bevy_ecs::prelude::Component;
use jolyne::valentine::{
    DisconnectPacketMessagesUnion, EnumsConnectionDisconnectFailReason, McpePacket, McpePacketData,
};

/// Missing observations remain explicit instead of inventing a normal logout.
#[derive(Clone, Debug, Component)]
pub(crate) enum QuitCause {
    Unknown,
    PeerDisconnect {
        reason: EnumsConnectionDisconnectFailReason,
        message: Option<String>,
    },
    PeerClosed,
    ServerDisconnect {
        reason: Option<EnumsConnectionDisconnectFailReason>,
        message: Option<String>,
    },
    ServerShutdown,
    QueueOverflow,
    TransportFailure {
        reason: Option<EnumsConnectionDisconnectFailReason>,
        message: String,
    },
}

/// Preserve the protocol's message when the terminating packet actually carries one.
pub(crate) fn outgoing(packet: &McpePacket) -> QuitCause {
    match &packet.data {
        McpePacketData::DisconnectPacket(packet) => {
            let message = match &packet.messages {
                DisconnectPacketMessagesUnion::DisconnectPacketMessages(messages) => {
                    Some(messages.message.to_string())
                }
                DisconnectPacketMessagesUnion::Empty1 => None,
            };
            QuitCause::ServerDisconnect {
                reason: Some(packet.reason),
                message,
            }
        }
        _ => QuitCause::ServerDisconnect {
            reason: None,
            message: None,
        },
    }
}

/// Preserve the existing transport's semantic failure category, without debug payloads.
pub(crate) fn failed(error: &jolyne::JolyneError) -> QuitCause {
    QuitCause::TransportFailure {
        reason: error.disconnect_reason(),
        message: error.disconnect_message().into(),
    }
}

/// A decoded client packet carries the same protocol reason fields in the opposite direction.
pub(crate) fn incoming(packet: &McpePacket) -> QuitCause {
    match outgoing(packet) {
        QuitCause::ServerDisconnect {
            reason: Some(reason),
            message,
        } => QuitCause::PeerDisconnect { reason, message },
        _ => QuitCause::Unknown,
    }
}
