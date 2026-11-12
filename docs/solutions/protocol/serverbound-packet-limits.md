---
title: Measuring the limits BDS puts on serverbound packets
area: protocol
tags: [limits, packet-violation, commands, chat, books, measurement, bds-extractor]
date: 2026-11-12
origin: OpenSpec change protocol-packet-limits
status: current
---

## Context

The owner asked to copy BDS's refusal of command packets over 550 bytes. The boundary, what is
counted and which other packets have limits were measured with `protocol_probe.py size`
(fixtures and per-session `sent.json` sizes in
`crates/bds-extractor/captures/1.26.51/protocol/size/`).

## What we learned

- One session per packet kind, each sending longer and longer packets 300 ms apart, finds a
  boundary in one run: the first refused packet ends the session. `protocol_probe.single` stops
  after the first failing session, so the `size` probe runs each session on its own.
- There are two kinds of limit. A size limit (`CommandRequest` only, 550 bytes counting the
  packet header and payload, the value of the batch length prefix) is checked before the packet
  is read and its message has no reader suffix. Field limits are checked while reading and end in
  `readNoHeader failed! packetId: N`. The second kind fires whatever the game state is: a
  `BookEdit` with no book open is refused the same way.
- Field limits count characters (code points), not bytes: two- and four-byte characters were
  tried for books, chat and request ids. The size limit counts bytes.
- BDS's message can name the wrong field: a request id over 39 characters is refused with
  `Command exceeds maximum size of 512 characters.`. Only varying each field on its own showed
  which one triggers it; a first try that padded the request id while varying the command length
  looked like a 512-byte command limit.
- After a terminating violation BDS closes the connection without a `Disconnect`; the
  `DisconnectPacket` in the recordings is the extractor's own.
- unastar checks in the network task (`network::packet_limits::admit`) between
  `recv_packet_raw` and `decode_borrowed`, so a refused packet never reaches the game loop.
  jolyne gained `RawPacket::payload_len` and a metric-keeping `decode_borrowed` on the stream.
