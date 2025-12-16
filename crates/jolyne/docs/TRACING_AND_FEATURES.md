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

## 2. Refactor Plan: Logging

**Objective:** Audit the codebase to align with the standards above.

1.  **Transport Layer (`transport.rs`):**
    *   Ensure raw packet send/recv is `trace` (no fields).
    *   Spans should capture the `peer_addr`.
2.  **Stream Layer (`client.rs`, `server.rs`):**
    *   Transition `#[instrument]` to `skip_all` + `trace`.
    *   Use `fields(...)` in `instrument` to capture strictly necessary IDs (e.g. `packet_id` when dispatching).
    *   Add `debug` events for state transitions.
    *   Move `println!` or unstructured `info!` to structured events.
3.  **Auth Layer:**
    *   Ensure secrets are NEVER logged (redact tokens/keys).
    *   Log auth failures as `WARN`.

---

## 3. Feature Flags Proposal

We will transition `jolyne` to a feature-gated architecture to reduce bloat for pure-client or pure-server use cases.

### Proposed `Cargo.toml`

```toml
[features]
default = [] # Force user to choose

# Role Features
client = ["dep:reqwest", "dep:jsonwebtoken", ...] 
server = ["dep:jsonwebtoken", ...]

# Meta Features
full = ["client", "server"]
```

### Module Reorganization

*   `src/stream/client.rs` -> `#[cfg(feature = "client")]`
*   `src/stream/server.rs` -> `#[cfg(feature = "server")]`
*   `src/listener.rs` -> `#[cfg(feature = "server")]`
*   `src/auth/client.rs` -> `#[cfg(feature = "client")]`
*   `src/auth/server.rs` -> `#[cfg(feature = "server")]` (if applicable)

### Dependencies

*   `reqwest`: Only needed for `client` (XBox Live Auth / Realms).
*   `tokio-raknet`: Core dependency, likely needed for both, but `RaknetListener` is server-only.