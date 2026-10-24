# Jolyne: Tracing Standards & Feature Architecture

This document outlines the logging standards and proposed feature flag architecture for the `jolyne` crate.

## 1. Tracing Standards

We use the `tracing` crate. Logs must be structured, low-overhead, and intent-driven.

### Levels & Usage

| Level | Purpose | Guidelines |
| :--- | :--- | :--- |
| **TRACE** | High-frequency "heartbeats" or flow tracing. | **MUST** use `skip_all`. **MUST NOT** contain fields/data in the event itself (context should come from the parent Span). Used solely to trace execution flow in hot paths (e.g., "packet received", "function entered"). |
| **DEBUG** | State updates, non-critical data insights. | Use fields to show *what* changed (e.g., `state="Handshake" -> "Login"`, `packet_id=0x01`). Do not dump large blobs. |
| **INFO** | Lifecycle events. | Connect, Disconnect, Auth Success, World Join. Things an operator cares about. |
| **WARN** | Recoverable errors / Suspicious activity. | Invalid signatures, protocol violations that don't crash the connection immediately, timeouts. |
| **ERROR** | Unrecoverable failures / Bugs. | IO Errors that kill the connection, Logic errors, Panics. |

### Span Conventions

*   **Functions:** Use `#[instrument(skip_all, level = "trace")]` for internal methods.
*   **Fields:** ALWAYS use `skip_all`. explicitly explicitly allow specific fields using `fields(...)` syntax if necessary for *context* (e.g. `uuid`, `endpoint`).
*   **Context:** Data fields (like `ip`, `uuid`) should be attached to the *Connection Span* (the top-level span for the connection), not repeated in every child log.

### Examples

#### Function Instrumentation
```rust
// GOOD
#[instrument(skip_all, level = "trace", fields(packet_id = %packet.id))]
async fn handle_packet(&mut self, packet: &Packet) {
    // ...
    tracing::debug!("Processing packet logic"); // Context (packet_id) is inherited from span
}

// BAD
#[instrument] // Don't record arguments by default, they are often too large
async fn handle_packet(&mut self, packet: Packet) {
    tracing::trace!("Got packet {:?}", packet); // No data in trace!
}
```

#### Manual Spans
```rust
// GOOD
let span = tracing::trace_span!("connection", ip = %addr, uuid = field::Empty);
let _enter = span.enter();

// later
span.record("uuid", &uuid.to_string());
```

---

## 2. Where jolyne traces

- **Join steps** (`stream::server`, `stream::client`): every step method, `close`
  and `disconnect` carry `#[instrument(skip_all, level = "debug", fields(peer))]`.
- **Core** (`proto`): `debug` events at phase transitions (settings negotiated,
  login accepted, encryption established, player initialized). The core has no
  spans of its own; events inherit the driver's step span.
- **Hot paths** (batch encode/decode, crypto, per-packet I/O) emit nothing at
  `debug` or above. Per-packet visibility is opt-in: `TRACE` events under the
  `jolyne::wire` target (`RUST_LOG=jolyne::wire=trace`, one line per batch and
  packet) and the `PacketObserver` hook (zero cost when unset); counters are in
  `ConnectionMetrics`. An inbound `PacketViolationWarning` is logged at `warn`.
- **Auth**: secrets are never logged; failures are `warn`.

## 3. Feature flags

```toml
[features]
default = ["full", "raknet"]
client = ["dep:jsonwebtoken"]
server = ["dep:reqwest", "dep:jsonwebtoken", "dep:md-5"]
full = ["client", "server"]
raknet = ["dep:tokio-raknet"]
nethernet = ["dep:tokio-nethernet"]
discovery = ["nethernet", "tokio-nethernet/discovery"]
xbox-signaling = ["nethernet", "tokio-nethernet/xbox-signaling"]
endpoint = ["nethernet", "tokio-nethernet/endpoint"]
# Standalone batch helpers for crates/bench-compare; connections never use them.
bench = []
```

- `proto::server`, `stream::server`, `listener` and server-side login
  verification (`auth::{decode_login_request, authenticate_login, legacy,
  openid, types}`) require `server`.
- `proto::client`, `stream::client`, `auth::client` require `client`;
  `proxy` requires both.
- The `server` feature requires at least one transport (`compile_error!`).
- CI (`.github/workflows/test.yml`) runs `clippy --lib -D warnings` on
  `client,raknet`, `client,nethernet`, `server,raknet`, `server,nethernet`,
  `server,discovery`, plus `--all-targets --all-features`.
