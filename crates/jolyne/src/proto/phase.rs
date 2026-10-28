//! The typestate contract shared by every handshake phase.
//!
//! A phase owns its [`Connection`]. The driver feeds it datagrams
//! ([`Phase::handle_datagram`]), drains [`Owner::poll_transmit`], sleeps until
//! [`Phase::poll_timeout`], and calls [`Phase::advance`] with the current time.
//! `advance` consumes the phase and returns either the same phase (waiting for
//! more input) or the next one. Failures hand the phase back inside a
//! [`Failure`] so the caller can still send a `Disconnect`.

use std::time::{Duration, Instant};

use super::conn::{Connection, Datagram, Transmit};
use super::raw::RawPacket;
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
    /// Why the transition failed.
    pub error: JolyneError,
    /// The phase that failed, still owning its connection.
    pub state: Box<S>,
}

impl<S> Failure<S> {
    /// Wraps `error` together with the phase that produced it.
    pub fn new(error: impl Into<JolyneError>, state: S) -> Self {
        Self {
            error: error.into(),
            state: Box::new(state),
        }
    }

    /// Splits the failure into the error and the unboxed phase.
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
    /// Mutable access to the owned connection.
    fn connection(&mut self) -> &mut Connection;
    /// Shared access to the owned connection.
    fn connection_ref(&self) -> &Connection;
    /// Consumes the owner and returns its connection.
    fn into_connection(self) -> Connection;

    /// Pops the next outbound transport message, if any is queued.
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

    /// The play session, for owners that hold one. The play receive path
    /// uses it to finish a streamed spawn (`SetLocalPlayerAsInitialized`
    /// check and spawn deadline); join phases have none.
    fn play_session(&mut self) -> Option<&mut super::session::Session> {
        None
    }
}

/// A handshake phase waiting for packets.
pub trait Phase: Owner {
    /// The owner [`Phase::advance`] yields once this phase completes.
    type Next;
    /// Which handshake phase this is, reported in timeout and unexpected-packet errors.
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

/// `now + after` without `Instant`'s overflow panic. Timeouts come from user
/// config, where a huge value (e.g. `Duration::MAX` for "never") is plausible;
/// it is capped at about a century, which no session outlives. The cap fits
/// every supported platform's `Instant`, so `now` is only a formal fallback.
pub(crate) fn deadline_after(now: Instant, after: Duration) -> Instant {
    const CENTURY: Duration = Duration::from_secs(100 * 365 * 24 * 60 * 60);
    now.checked_add(after.min(CENTURY)).unwrap_or(now)
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

/// Packets that arrived before the phase that handles them, in arrival
/// order, bounded by `ConnectionOptions::max_deferred_{packets,bytes}`. Each
/// packet is compacted once so it does not pin its whole batch allocation.
pub struct Deferred {
    packets: Vec<RawPacket>,
    bytes: usize,
    max_packets: usize,
    max_bytes: usize,
}

impl Deferred {
    /// Creates an empty buffer bounded by `connection`'s deferral limits.
    pub fn new(connection: &Connection) -> Self {
        let options = connection.options();
        Self {
            packets: Vec::new(),
            bytes: 0,
            max_packets: options.max_deferred_packets,
            max_bytes: options.max_deferred_bytes,
        }
    }

    /// Compacts and appends `packet`; fails with `ProtocolError::DeferredOverflow`
    /// when the packet or byte limit would be exceeded.
    pub fn push(&mut self, packet: RawPacket) -> Result<(), JolyneError> {
        let bytes = self.bytes.saturating_add(packet.inner_frame().len());
        if self.packets.len() == self.max_packets || bytes > self.max_bytes {
            return Err(ProtocolError::DeferredOverflow {
                max_packets: self.max_packets,
                max_bytes: self.max_bytes,
            }
            .into());
        }
        self.bytes = bytes;
        self.packets.push(packet.into_compact());
        Ok(())
    }

    /// Number of deferred packets.
    pub fn len(&self) -> usize {
        self.packets.len()
    }

    /// Whether no packets are deferred.
    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    /// Moves the deferred packets ahead of the connection's unread packets,
    /// so the next owner (phase or session) sees them first, in order.
    pub fn replay_into(self, connection: &mut Connection) {
        connection.prepend_inbound(self.packets.into_iter());
    }
}
