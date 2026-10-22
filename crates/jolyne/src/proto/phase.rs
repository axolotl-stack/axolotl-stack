//! The typestate contract shared by every handshake phase.
//!
//! A phase owns its [`Connection`]. The driver feeds it datagrams
//! ([`Phase::handle_datagram`]), drains [`Phase::poll_transmit`], sleeps until
//! [`Phase::poll_timeout`], and calls [`Phase::advance`] with the current time.
//! `advance` consumes the phase and returns either the same phase (waiting for
//! more input) or the next one. Failures hand the phase back inside a
//! [`Failure`] so the caller can still send a `Disconnect`.

use std::time::Instant;

use super::conn::{Connection, Datagram, Transmit};
use super::raw::{DEFAULT_MAX_PACKETS_PER_BATCH, RawPacket};
use crate::error::{HandshakePhase, JolyneError, ProtocolError};
use crate::valentine::EnumsConnectionDisconnectFailReason;

/// Result of driving a phase.
pub enum Step<S, N> {
    /// Needs more input; the phase is handed back.
    Wait(S),
    /// The phase completed.
    Next(N),
}

/// A failed transition. `state` is the phase that failed, still owning the
/// connection (boxed: failures are rare and owners are large).
pub struct Failure<S> {
    pub error: JolyneError,
    pub state: Box<S>,
}

impl<S> Failure<S> {
    pub fn new(error: impl Into<JolyneError>, state: S) -> Self {
        Self {
            error: error.into(),
            state: Box::new(state),
        }
    }

    pub fn into_parts(self) -> (JolyneError, S) {
        (self.error, *self.state)
    }
}

impl<S> std::fmt::Debug for Failure<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Failure")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

/// Anything that owns a connection: phases, intermediate owners, sessions.
pub trait Owner: Sized {
    fn connection(&mut self) -> &mut Connection;
    fn connection_ref(&self) -> &Connection;
    fn into_connection(self) -> Connection;

    fn poll_transmit(&mut self) -> Option<Transmit> {
        self.connection().poll_transmit()
    }

    /// Queues a `Disconnect` with the reason mapped from `error` (when the
    /// peer should be told) and seals it.
    fn queue_failure_disconnect(&mut self, error: &JolyneError, now: Instant) {
        self.connection().queue_failure_disconnect(error, now);
    }

    /// Queues `Disconnect{reason, message}` and seals it.
    fn queue_disconnect(
        &mut self,
        reason: EnumsConnectionDisconnectFailReason,
        message: &str,
        now: Instant,
    ) -> Result<(), JolyneError> {
        self.connection().queue_disconnect(reason, message, now)
    }
}

/// A handshake phase waiting for packets.
pub trait Phase: Owner {
    type Next;
    const PHASE: HandshakePhase;

    /// The mandatory absolute deadline of this phase.
    fn deadline(&self) -> Instant;

    /// Handles one inbound packet.
    fn on_packet(
        self,
        packet: RawPacket,
        now: Instant,
    ) -> Result<Step<Self, Self::Next>, Failure<Self>>;

    /// Phase-specific timer (e.g. resource pack chunk pacing).
    fn phase_timer(&self) -> Option<Instant> {
        None
    }

    /// Called from [`Phase::advance`] before packets are processed.
    fn on_timer(self, _now: Instant) -> Result<Step<Self, Self::Next>, Failure<Self>> {
        Ok(Step::Wait(self))
    }

    /// Feeds one transport message. Errors are terminal.
    fn handle_datagram(&mut self, datagram: Datagram) -> Result<(), JolyneError> {
        self.connection().handle_datagram(datagram)
    }

    /// The earliest instant the driver must call [`Phase::advance`] again.
    fn poll_timeout(&self) -> Instant {
        let mut at = self.deadline();
        for timer in [self.connection_ref().poll_timeout(), self.phase_timer()]
            .into_iter()
            .flatten()
        {
            at = at.min(timer);
        }
        at
    }

    /// Processes timers and queued packets. Checks the deadline first.
    fn advance(self, now: Instant) -> Result<Step<Self, Self::Next>, Failure<Self>> {
        if now >= self.deadline() {
            return Err(Failure::new(ProtocolError::Timeout(Self::PHASE), self));
        }
        let mut this = self;
        if let Err(error) = this.connection().handle_timeout(now) {
            return Err(Failure::new(error, this));
        }
        this = match this.on_timer(now)? {
            Step::Wait(this) => this,
            next => return Ok(next),
        };
        loop {
            match this.connection().next_packet() {
                Err(error) => return Err(Failure::new(error, this)),
                Ok(None) => return Ok(Step::Wait(this)),
                Ok(Some(packet)) => match this.on_packet(packet, now)? {
                    Step::Wait(next) => this = next,
                    next => return Ok(next),
                },
            }
        }
    }
}

/// Builds the error for a packet a phase did not expect.
#[cfg(any(feature = "server", feature = "client"))]
pub(crate) fn unexpected(phase: HandshakePhase, packet: &RawPacket) -> JolyneError {
    let got = match packet.name() {
        Some(name) => format!("{name:?}"),
        None => format!("packet id {}", packet.id()),
    };
    ProtocolError::UnexpectedPacket { phase, got }.into()
}

const MAX_DEFERRED_BYTES: usize = 16 * 1024 * 1024;

/// Packets that arrived before the phase that handles them, replayed in
/// order into the play session. Bounded by count and bytes; each packet is
/// compacted so it does not pin its whole batch allocation.
#[derive(Default)]
pub struct Deferred {
    packets: Vec<RawPacket>,
    bytes: usize,
}

impl Deferred {
    pub fn push(&mut self, packet: RawPacket) -> Result<(), JolyneError> {
        if self.packets.len() == DEFAULT_MAX_PACKETS_PER_BATCH {
            return Err(ProtocolError::TooManyPackets {
                max: DEFAULT_MAX_PACKETS_PER_BATCH,
            }
            .into());
        }
        let bytes = self.bytes.saturating_add(packet.inner_frame().len());
        if bytes > MAX_DEFERRED_BYTES {
            return Err(ProtocolError::BatchTooLarge {
                actual: bytes,
                max: MAX_DEFERRED_BYTES,
            }
            .into());
        }
        self.bytes = bytes;
        self.packets.push(packet.into_compact());
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.packets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    /// Moves the deferred packets ahead of the connection's unread packets.
    pub fn replay_into(self, connection: &mut Connection) {
        connection.prepend_inbound(self.packets.into_iter());
    }
}
