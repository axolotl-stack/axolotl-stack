//! Async glue between a [`Transport`] and the sans-IO core.
//!
//! [`drive`] runs one join phase: `advance(now)`, write what it queued within
//! the phase deadline, then wait for a datagram until `poll_timeout` (the
//! deadline or a timer) and feed it to `handle_datagram`, until the phase
//! completes or fails. [`recv_raw`] and [`after_send`] do the same for a play
//! connection and its interval flush timer. [`now`] is the only clock: the
//! core receives Tokio's time as `std` instants.
//!
//! Writes and receives are cancel-safe: a transmit is only taken from the
//! connection after the transport reported capacity, and a receive is a
//! single `poll_recv`. A packet handed to a `send_*` method is queued before
//! the first await, so retrying a cancelled send queues it twice.

use std::future::poll_fn;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use super::transport::{Datagram, Transport};
#[cfg(any(feature = "server", feature = "client"))]
use crate::error::HandshakePhase;
use crate::error::{JolyneError, ProtocolError};
use crate::proto::conn::Connection;
#[cfg(any(feature = "server", feature = "client"))]
use crate::proto::phase::{Failure, Phase, Step};
use crate::proto::raw::RawPacket;

fn transport_error(error: impl std::error::Error + Send + Sync + 'static) -> JolyneError {
    JolyneError::transport(error)
}

/// The one clock of the async layer: the sans-IO core gets `std` instants,
/// read from Tokio's clock so they agree with the Tokio timers the driver
/// waits on (and with `tokio::time::pause` in tests).
pub(crate) fn now() -> Instant {
    tokio::time::Instant::now().into_std()
}

/// Writes every sealed batch to the transport and flushes it.
pub(crate) async fn flush_out<T: Transport>(
    transport: &mut T,
    conn: &mut Connection,
) -> Result<(), JolyneError> {
    if !conn.has_transmits() {
        return Ok(());
    }
    while conn.has_transmits() {
        poll_fn(|cx| Pin::new(&mut *transport).poll_send_ready(cx))
            .await
            .map_err(transport_error)?;
        // Taken only after the transport reported capacity (cancel safety).
        let Some(transmit) = conn.poll_transmit() else {
            break;
        };
        Pin::new(&mut *transport)
            .start_send(transmit)
            .map_err(transport_error)?;
    }
    poll_fn(|cx| Pin::new(&mut *transport).poll_flush(cx))
        .await
        .map_err(transport_error)
}

/// [`flush_out`] bounded by a handshake deadline: a peer that stops reading
/// cannot hold the join open past it.
#[cfg(any(feature = "server", feature = "client"))]
pub(crate) async fn flush_out_by<T: Transport>(
    transport: &mut T,
    conn: &mut Connection,
    deadline: Instant,
    phase: HandshakePhase,
) -> Result<(), JolyneError> {
    let deadline = tokio::time::Instant::from_std(deadline);
    match tokio::time::timeout_at(deadline, flush_out(transport, conn)).await {
        Ok(result) => result,
        Err(_) => Err(ProtocolError::Timeout(phase).into()),
    }
}

/// How long closing may spend draining output to a peer that stopped
/// reading.
pub(crate) const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Seals pending packets, drains them and closes the transport, giving up
/// after [`CLOSE_TIMEOUT`].
pub(crate) async fn close<T: Transport>(
    transport: &mut T,
    conn: &mut Connection,
) -> Result<(), JolyneError> {
    let drain = async {
        conn.flush()?;
        flush_out(transport, conn).await?;
        poll_fn(|cx| Pin::new(&mut *transport).poll_close(cx))
            .await
            .map_err(transport_error)
    };
    tokio::time::timeout(CLOSE_TIMEOUT, drain)
        .await
        .unwrap_or(Err(ProtocolError::CloseTimeout.into()))
}

/// Best-effort synchronous drain for `Drop`: sends what the transport accepts
/// right now without waiting.
pub(crate) fn flush_now<T: Transport>(transport: &mut T, conn: &mut Connection) {
    if conn.flush().is_err() {
        return;
    }
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    while conn.has_transmits() {
        match Pin::new(&mut *transport).poll_send_ready(&mut cx) {
            Poll::Ready(Ok(())) => {
                let Some(transmit) = conn.poll_transmit() else {
                    break;
                };
                if Pin::new(&mut *transport).start_send(transmit).is_err() {
                    return;
                }
            }
            _ => break,
        }
    }
    let _ = Pin::new(&mut *transport).poll_flush(&mut cx);
}

async fn recv_datagram<T: Transport>(transport: &mut T) -> Result<Datagram, JolyneError> {
    poll_fn(|cx| Pin::new(&mut *transport).poll_recv(cx))
        .await
        .ok_or(JolyneError::ConnectionClosed)?
        .map_err(transport_error)
}

/// Drives a handshake phase to completion.
#[cfg(any(feature = "server", feature = "client"))]
pub(crate) async fn drive<T: Transport, P: Phase>(
    transport: &mut T,
    mut phase: P,
) -> Result<P::Next, Failure<P>> {
    loop {
        phase = match phase.advance(now()) {
            Ok(Step::Next(next)) => return Ok(next),
            Ok(Step::Wait(phase)) => phase,
            Err(failure) => return Err(failure),
        };
        // Writes share the phase deadline: a peer that stops reading cannot
        // hold the join open past it.
        let deadline = phase.deadline();
        if let Err(error) = flush_out_by(transport, phase.connection(), deadline, P::PHASE).await {
            return Err(Failure::new(error, phase));
        }
        let wake = tokio::time::Instant::from_std(phase.poll_timeout());
        match tokio::time::timeout_at(wake, recv_datagram(transport)).await {
            Ok(Ok(datagram)) => {
                if let Err(error) = phase.handle_datagram(datagram) {
                    return Err(Failure::new(error, phase));
                }
            }
            Ok(Err(error)) => return Err(Failure::new(error, phase)),
            // Timer or deadline: `advance` handles both.
            Err(_) => {}
        }
    }
}

/// The next inbound packet in the play state, running the interval flush
/// timer while waiting.
///
/// Cancel-safe: pending output is drained before a packet is taken from the
/// queue, and nothing is awaited between taking it and returning it.
pub(crate) async fn recv_raw<T: Transport>(
    transport: &mut T,
    conn: &mut Connection,
) -> Result<RawPacket, JolyneError> {
    loop {
        // Fire a due interval flush even while packets keep arriving.
        if conn.poll_timeout().is_some() {
            conn.handle_timeout(now())?;
        }
        flush_out(transport, conn).await?;
        if let Some(packet) = conn.next_packet()? {
            return Ok(packet);
        }
        let datagram = match conn.poll_timeout() {
            Some(at) => {
                match tokio::time::timeout_at(
                    tokio::time::Instant::from_std(at),
                    recv_datagram(transport),
                )
                .await
                {
                    Ok(datagram) => datagram?,
                    Err(_) => {
                        conn.handle_timeout(now())?;
                        continue;
                    }
                }
            }
            None => recv_datagram(transport).await?,
        };
        conn.handle_datagram(datagram)?;
    }
}

/// Drains transmits produced by a queue operation (immediate mode, auto-seal
/// limits); this is where transport backpressure reaches the sender.
pub(crate) async fn after_send<T: Transport>(
    transport: &mut T,
    conn: &mut Connection,
) -> Result<(), JolyneError> {
    if conn.poll_timeout().is_some() {
        conn.handle_timeout(now())?;
    }
    if conn.has_transmits() {
        flush_out(transport, conn).await?;
    }
    Ok(())
}
