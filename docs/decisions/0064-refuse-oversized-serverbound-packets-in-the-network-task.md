---
status: proposed
date: 2026-11-12
decision-makers: [protocol lane]
supersedes:
---

# Refuse the serverbound packets BDS refuses in the network task, before the game loop

## Context and Problem Statement

BDS 1.26.51 refuses a `CommandRequest` over 550 bytes before reading it, and refuses strings over
per-field limits while reading a packet (request ids, chat, book text), each with a terminating
`PacketViolationWarning` and a closed connection
([solution note](../solutions/protocol/serverbound-packet-limits.md)). The owner decided to copy
this. Where should unastar check: in jolyne's codec, in the network task, or in the game handlers?

## Considered Options

- Bounds in the generated codec (valentine/jolyne decode errors)
- A check in unastar's network task between receiving the raw packet and decoding it
- Checks in each game handler (commands, chat, books)

## Decision Outcome

Chosen option: "a check in unastar's network task", because the limits are BDS server behaviour
(not protocol facts a proxy or client should enforce), the size check needs the raw packet before
decoding, and the field checks must fire whatever the game state is (a book edit with no book open
is refused too), which handlers would not see. jolyne only exposes what is needed:
`RawPacket::payload_len` and a `decode_borrowed` that keeps the decode-error metric.

### Consequences

- Good, because a refused packet never reaches the tick loop, and the violation and close follow
  the existing terminating-packet path of the network task.
- Good, because the checks live in one module (`crates/unastar/src/network/packet_limits.rs`) with
  tests against every measured boundary.
- Bad, because every received packet now passes a match on its id in the network task (cheap; no
  allocation).

### Confirmation

`crates/unastar/src/network/packet_limits/tests.rs` rebuilds every probe packet and compares the
violation bytes with the capture.
