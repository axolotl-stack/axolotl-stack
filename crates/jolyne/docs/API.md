# jolyne API tiers

jolyne has one default path and two ways down. Start at Tier 1; drop a tier
only for the reason listed for it. Each tier is built from the one below it,
so dropping down never means re-implementing what the tier above did.

| Tier | For | Lives in |
|---|---|---|
| 1. High-level | Servers and clients that join players with the vanilla flow | crate root |
| 2. Step-by-step join | Custom join flows: per-player packs, custom admission, work between phases | `jolyne::stream` |
| 3. Low-level | Raw/borrowed packets, batches, custom packets, observers, your own runtime or socket | `jolyne::raw`, `jolyne::proto` |

## Tier 1: high-level (default)

```text
BedrockListener::raknet() / ::nethernet()  -> .config(..).bind()
  listener.accept()                        -> BedrockStream<Handshake, Server, _>
  stream.accept_join_sequence(&world, &key) -> (BedrockStream<Play, Server, _>, AuthenticatedLogin)
BedrockStream::connect(addr) / connect_with(addr, ClientConfig)
  stream.join(ClientHandshakeConfig)        -> (BedrockStream<Play, Client, _>, GameData)
play.recv_packet() / send_packet(..) / disconnect(..) / close()
```

Common server needs are configuration, not a tier change:

| Need | Where |
|---|---|
| Resource packs for everyone | `BedrockListenerConfig::resource_packs` |
| Player cap | `BedrockListenerConfig::max_players` |
| Allow/deny list, bans | `BedrockListenerConfig::admission` (`AdmissionHook`, closures work) |
| MOTD / player counts | `BedrockListenerConfig::status` (`status::StaticStatus` or your `StatusProvider`) |
| Online (Xbox) / offline auth | `BedrockListenerConfig::{online_mode, allow_legacy_auth}` |
| Encryption, compression, timeouts | `BedrockListenerConfig::{encryption_enabled, compression, *_timeout}` |
| Batching | `FlushMode` (`Immediate`, `Interval`, `Manual`) via `connection.flush` or `set_flush_mode` |
| Client identity / Xbox Live | `ClientHandshakeConfig::{random, with_xbox_live}` |

Crate-root re-exports are exactly Tier 1:

- Server: `BedrockListener`, `BedrockListenerConfig`, `AuthenticatedLogin`,
  `ValidatedIdentity`, `ClientData`, `WorldTemplate`.
- Client: `ClientConfig`, `ClientHandshakeConfig`, `GameData`.
- Both: `BedrockStream`, `Server`, `Client`, `Play`, `FlushMode`,
  `JolyneError`, `GAME_VERSION`, `PROTOCOL_VERSION`, and the `valentine`
  packet module.

Modules used for Tier 1 configuration stay public as-is: `config`
(`CompressionConfig`, `ConnectionOptions`, `AdmissionHook`), `status`,
`error`, `world`, `gamedata`, `auth` (verification for custom flows).

## Tier 2: step-by-step join (`jolyne::stream`)

`BedrockStream<Phase, Role, Transport>` exposes each join step as a consuming
method, so the compiler enforces the order:

```text
Server: Handshake --accept_network_settings--> Login --authenticate--> SecurePending
        --finish_handshake--> ResourcePacks --negotiate_packs[_with]--> StartGame
        --start_game--> Play
Client: Handshake --request_settings--> Login --send_login--> SecurePending
        --await_handshake--> ResourcePacks --handle_packs--> StartGame
        --await_start_game--> Play
```

Use it to: offer per-player packs (`negotiate_packs_with`), admit after your
own async lookup (between `authenticate` and `finish_handshake`, where
`login()` is available), do work between phases, or answer a failure your way.
Every step returns `Failed<T>` on error: `Failed::disconnect()` sends the
reasoned `Disconnect` (fixed, non-leaking message) and closes;
`into_parts()` keeps the connection.

`jolyne::stream` holds the phase markers (`Handshake`, `Login`,
`SecurePending`, `ResourcePacks`, `StartGame`, `Play`), `Failed`, the
`Transport` trait with `RakNetTransport`/`NetherNetTransport`,
`raknet_types`/`nethernet_types` aliases, and `stream::{server, client}` with
`from_transport` for transports you construct. `listener::RawListener` lets a
`BedrockListener` accept from any transport source.

## Tier 3: low-level (`jolyne::raw`, `jolyne::proto`)

`jolyne::raw` is the async low-level surface, for hot loops and protocol
tools:

- `BedrockTransport<T>`: a joined connection outside the typestate
  (`BedrockStream::into_transport`), for proxies and tick loops.
- `RawPacket` / `PacketHeader`, `recv_packet_raw`, `recv_packet_borrowed`
  (no payload allocation), `send_packet_raw`, `send_batch[_raw]`.
- `CustomPacket` and `UnknownPacketPolicy` for IDs valentine does not know.
- `PacketObserver` and `ConnectionMetrics` for per-packet instrumentation.

`jolyne::proto` is the sans-IO core: `conn::Connection` (framing,
compression, encryption, batching), the join phases (`proto::server`,
`proto::client`) driven by `Phase::advance(now)`, `packs`, `crypto`,
`handshake`. Use it with your own socket, runtime or clock; nothing in it
performs I/O.

`jolyne::proxy` (relay with per-side encryption) is a Tier 3 helper.

## What is sealed or crate-private

- Sealed: `State`, `Role`, `PhaseOf` (no foreign phases or roles).
- Crate-private: the driver (`stream::driver`), player slots
  (`stream::slots`), compression engines (`proto::compression`), and every
  join-phase constructor that would skip a check.
- Bench-only (`bench` feature): standalone batch encode/decode helpers in
  `proto::batch`.
- `BedrockStream::owner()` (the phase's sans-IO owner) stays public as the
  documented Tier 3 escape hatch from Tier 2.

## Moves from the current root

| Item | Now |
|---|---|
| `RawPacket`, `PacketHeader`, `CustomPacket`, `UnknownPacketPolicy`, `BedrockTransport` | `jolyne::raw` |
| `Failed` | `jolyne::stream` |
| `RawListener`, `RakNetBuilder`, `NetherNetBuilder` | `jolyne::listener` |
| `ConnectionOptions` | `jolyne::config` |
| `ClientHandshakeConfig` | crate root (also `jolyne::stream::client`) |
