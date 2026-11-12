---
title: Time a delayed ACK from the oldest datagram it names, not per datagram
area: transport
tags: [raknet, ack, capture-diff, flaky-test, timing, ci]
date: 2026-11-10
origin: "commit test(tokio-raknet): time ACK-1 from the oldest datagram an ACK names (CI flake seen on cinnabar-mc/axolotl-stack PRs 77 and 79)"
status: current
---

## Context

`capture_diff::baseline_session_matches_bds` failed intermittently on the Linux CI runner with
ACK-1 `n=15 min=0.5 p50=11.5`: one server ACK seemed to leave 0.5 ms after a client datagram,
although Vanilla ACKs wait 10 ms. The analysis timed every datagram from its arrival to the first
ACK naming it and asserted the minimum was at least 9 ms.

## What we learned

The transport was right and the measurement was wrong. Mojang RakNet makes an ACK due 10 ms after
the **oldest** unacknowledged datagram, and that ACK names everything pending. A datagram that
arrives just as the ACK falls due is ACKed with it after a fraction of a millisecond. BDS does
this itself. On the server side of the four real-client captures, 27 BDS ACKs name a datagram
0.17-0.42 ms old beside one of 10 ms or more, yet once BDS has an RTT sample the oldest datagram
an ACK names is never under 10.18 ms old (ACKs of split fragments aside). Applied to BDS's ACKs
in those captures, the old per-datagram check (every datagram at least 9 ms) fails.

What made it intermittent: at the 5 s keepalive, the reliable ping waits until nothing is in
flight, so it often follows the unreliable ping by about one ACK round (~10 ms). The client's pong
to it then races the server's ACK timer for the client's first ping datagram. Linux timers (about
1 ms) and CPU load decide the race. Locally on Windows with `timeBeginPeriod(1)` under load
(24 busy loops plus 16 parallel test processes), 31 of 240 runs failed ACK-1, every one of them
this case. No ACK left before its oldest datagram was 10 ms old. With Windows' default 15.6 ms
timers it failed 4 times in 200.

The fix measures each ACK from the oldest datagram it names for the first time. ACKs naming a
split fragment or a datagram received before the RTT sample are skipped: Mojang ACKs at once
until it has an RTT sample and when a split message completes, so such an ACK may leave early by
design. After the fix: 0 ACK-1 failures in 440 loaded runs. The measure still catches real early
ACKs. Three transport mutations, a 5 ms ACK delay, an ACK clock not reset after an ACK, and ACKs
flushed with outgoing data, each failed ACK-1 in 3 of 3 runs. It is also safe against relay
scheduling delays. The relay logs a datagram before forwarding it and an ACK after receiving it,
so a stalled relay can only make an ACK look later.

When a test bounds a protocol timer, measure the quantity the rule defines (here the ACK's age
relative to its oldest datagram). Check that the reference implementation passes the assertion
on its own captures before trusting it.

## Where it lives

`crates/tokio-raknet/examples/raknet_capture/analysis.rs` (`Side::ack_delays_after_rtt`);
`crates/tokio-raknet/tests/capture_diff.rs` (`baseline_session_matches_bds`,
`ack_delay_counts_from_the_oldest_datagram_like_mojang`); `crates/tokio-raknet/PARITY.md` ACK-1;
`docs/quirks/bds.md` (Network / RakNet).
