//! Async glue between a [`Transport`] and the sans-IO core.
//!
//! Writes and receives are cancel-safe: a transmit is only taken from the
//! connection after the transport reported capacity, and a receive is a
//! single `poll_recv`. A packet handed to a `send_*` method is queued before
//! the first await, so retrying a cancelled send queues it twice.

use std::future::poll_fn;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

use super::transport::{Datagram, Transport};
use crate::error::JolyneError;
#[cfg(any(feature = "server", feature = "client"))]
use crate::error::ProtocolError;
use crate::proto::conn::Connection;
#[cfg(any(feature = "server", feature = "client"))]
use crate::proto::phase::{Failure, Phase, Step};
use crate::proto::raw::RawPacket;

fn transport_error(error: impl std::error::Error + Send + Sync + 'static) -> JolyneError {
    JolyneError::transport(error)
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
        let transmit = conn.poll_transmit().expect("checked has_transmits");
        Pin::new(&mut *transport)
            .start_send(transmit)
            .map_err(transport_error)?;
    }
    poll_fn(|cx| Pin::new(&mut *transport).poll_flush(cx))
        .await
        .map_err(transport_error)
}

/// Seals pending packets, drains them and closes the transport.
pub(crate) async fn close<T: Transport>(
    transport: &mut T,
    conn: &mut Connection,
) -> Result<(), JolyneError> {
    conn.flush()?;
    flush_out(transport, conn).await?;
    poll_fn(|cx| Pin::new(&mut *transport).poll_close(cx))
        .await
        .map_err(transport_error)
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
                let transmit = conn.poll_transmit().expect("checked has_transmits");
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
        phase = match phase.advance(Instant::now()) {
            Ok(Step::Next(next)) => return Ok(next),
            Ok(Step::Wait(phase)) => phase,
            Err(failure) => return Err(failure),
        };
        // Writes share the phase deadline: a peer that stops reading cannot
        // hold the join open past it.
        let deadline = tokio::time::Instant::from_std(phase.deadline());
        match tokio::time::timeout_at(deadline, flush_out(transport, phase.connection())).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(Failure::new(error, phase)),
            Err(_) => return Err(Failure::new(ProtocolError::Timeout(P::PHASE), phase)),
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
pub(crate) async fn recv_raw<T: Transport>(
    transport: &mut T,
    conn: &mut Connection,
) -> Result<RawPacket, JolyneError> {
    loop {
        // Fire a due interval flush even while packets keep arriving.
        if conn.poll_timeout().is_some() {
            conn.handle_timeout(Instant::now())?;
        }
        if let Some(packet) = conn.next_packet()? {
            if conn.has_transmits() {
                flush_out(transport, conn).await?;
            }
            return Ok(packet);
        }
        flush_out(transport, conn).await?;
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
                        conn.handle_timeout(Instant::now())?;
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
        conn.handle_timeout(Instant::now())?;
    }
    if conn.has_transmits() {
        flush_out(transport, conn).await?;
    }
    Ok(())
}
