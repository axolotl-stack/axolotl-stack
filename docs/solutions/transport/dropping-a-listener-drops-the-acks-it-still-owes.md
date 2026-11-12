---
title: Dropping a RaknetListener drops the ACKs its closing sessions still owe
area: transport
tags: [raknet, listener, disconnect, ack, capture-diff, flaky-test]
date: 2026-11-10
origin: "commit test(tokio-raknet): keep the capture-diff listener until the test ends"
status: current
---

## Context

Under CPU load about 1 % of `capture_diff::baseline_session_matches_bds` runs failed KA-6 with
"0x15 not ACKed". The capture showed the server going silent at the moment the client's
disconnect notification arrived: no ACK and no further 50 ms game tick.

## What we learned

A Vanilla session that receives `0x15` goes to `Closing` and ACKs it on the normal ACK clock,
10 ms after the oldest pending datagram (PARITY KA-6). If the application writes during those
10 ms, the write is refused and `Err(Session(Closed))` reaches it through `recv_message`. The
test's server loop treated that as the end of the connection. Its task owned the
`RaknetListener`, so ending the loop dropped the listener and the stream. Those are the last
senders of the muxer's outbound channel, so the muxer exited before the ACK timer fired.
Whether a server tick landed inside that 10 ms window depended on phase and load: 10 failures
in 880 loaded runs. With the fix below and the loop's exit reason logged, 7 of 200 runs took
that path and still saw the `0x15` ACKed.

A real server keeps its listener after a connection ends, so the ACK still leaves. The harness
now does the same: after its loop ends, the server task holds the listener until the test aborts
it. With that change, 440 loaded runs (with the ACK-1 fix) had no failures. When a test
owns a listener, keep it alive until the test is done with the wire, not until its connection
is done.

## Where it lives

`crates/tokio-raknet/tests/capture_diff.rs` (`session`); refusal of writes while closing:
`crates/tokio-raknet/src/session/manager.rs` (`queue_app_buffer`) and
`crates/tokio-raknet/src/transport/listener/online.rs` (`handle_outgoing_msg`); the muxer stops
when every outbound sender is gone: `crates/tokio-raknet/src/transport/listener.rs`
(`run_listener_muxer`).
