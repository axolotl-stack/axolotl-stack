---
title: A symmetric round trip cannot catch byte-order bugs; test against fixed byte vectors
area: protocol
tags: [codec, uuid, endianness, testing]
date: 2026-10-24
origin: data-driven architecture slice I1 (UUID wire defect)
status: current
---

## Context

The UUID codec wrote `Uuid::as_bytes()` (big-endian) and read it back the same way, so every
round-trip test passed.

## What we learned

BDS writes each 8-byte half of a UUID little-endian. A codec that is wrong the same way in both
directions round-trips perfectly; only a test against independently sourced bytes (here
gophertunnel's writer, then a BDS capture) catches it. The bug affected multi-recipe UUIDs,
PlayerList and AddPlayer. For any new primitive or field layout, add a fixed-vector test, not just
a round trip.

## Where it lives

`crates/valentine/bedrock_core/src/bedrock/codec.rs` (test `uuid_uses_bds_half_little_endian_byte_order`);
spec `openspec/specs/protocol-codec/spec.md`.
