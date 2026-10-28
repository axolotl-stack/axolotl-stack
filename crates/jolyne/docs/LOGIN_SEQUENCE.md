# Bedrock Edition Login Sequence

This document outlines the protocol sequence for a client connecting to a Bedrock Dedicated Server (BDS).

```mermaid
sequenceDiagram
    participant Client
    participant Server

    Note over Client, Server: RakNet Connection Established

    Client->>Server: RequestNetworkSettings
    Server->>Client: NetworkSettings
    Note right of Server: Sets compression threshold/algo

    Client->>Server: Login
    Note right of Client: Multiplayer Token (identity) + ClientData JWT (skin, device)
    
    opt Encryption
        Server->>Client: ServerToClientHandshake
        Note right of Server: JWT with Server Public Key & Salt
        Client->>Server: ClientToServerHandshake
        Note over Client, Server: Encryption Enabled
    end

    Server->>Client: PlayStatus (LoginSuccess)
    Server->>Client: ResourcePacksInfo
    
    par Async Status Updates
        Client->>Server: ClientCacheStatus (Optional)
        Note right of Client: Sent if supported, usually early
    and Resource Pack Negotiation
        loop Pack Loop
            Client->>Server: ResourcePackClientResponse
            
            alt Needs Packs
                Server->>Client: ResourcePackDataInfo
                Client->>Server: ResourcePackChunkRequest
                Server->>Client: ResourcePackChunkData
            else All Packs Downloaded / None Needed
                Server->>Client: ResourcePackStack
                Client->>Server: ResourcePackClientResponse (Completed)
            end
        end
    end

    Note over Server: Start Game Sequence

    Server->>Client: [DimensionData] (optional), JigsawStructureData, VoxelShapes
    Server->>Client: StartGame
    Server->>Client: ItemRegistry
    Server->>Client: BiomeDefinitionList
    Server->>Client: AvailableActorIdentifiers
    Server->>Client: CreativeContent
    Note right of Server: One batch; BDS sends the definitions before the radius request

    Client->>Server: ServerboundLoadingScreen (Start)
    Client->>Server: RequestChunkRadius
    Note right of Server: radius must be >= 1; the server may override it

    alt start_game (gophertunnel order)
        Server->>Client: ChunkRadiusUpdated + PlayStatus (PlayerSpawn)
        Note right of Server: no NetworkChunkPublisherUpdate: jolyne has no terrain
    else start_game_streamed (BDS order)
        Server->>Client: ChunkRadiusUpdated
        Note right of Server: session handed to the application
        Server->>Client: NetworkChunkPublisherUpdate, LevelChunk*, PlayStatus (PlayerSpawn)
        Server->>Client: LevelChunk* (streaming while the client loads)
    end

    Client->>Server: ServerboundLoadingScreen (End)
    Client->>Server: SetLocalPlayerAsInitialized
    Note right of Server: runtime ID must match StartGame; spawn_timeout applies

    Note over Client, Server: In-Game Loop (Tick Sync, MovePlayer, etc.)
```

## Implementation Details

### 1. Handshake Phase
- **NetworkSettings**: Negotiates compression.
- **Login**: Verifies the provider-signed multiplayer `Token` (online mode) or reads the self-signed one (offline), then verifies and validates the ClientData JWT signed by the token's `cpk`.
- **Encryption**: ECDH key exchange using P-384 curves.

### 2. Resource Pack Phase
- Even if no packs are required, the server must send `ResourcePacksInfo`. Its world template version must parse as a `SemVersion`: without a template, BDS sends the nil UUID with `0.0.0`. The 26.50+ client rejects an empty version (gophertunnel's default) with `BadPacket` (error `InitialConnection-90`, codeword "Block") before answering.
- The client must respond with `Completed` (after receiving `ResourcePackStack`) before `StartGame` is sent.
- **ClientCacheStatus**: Clients may send this to indicate blob cache support. It should be handled (usually ignored if cache not implemented) without blocking the flow.

### 3. Spawn Sequence
- `DimensionData` (only for custom dimensions), `JigsawStructureData` and `VoxelShapes` precede `StartGame`, followed by `ItemRegistry` and the definition lists, in one batch in BDS/gophertunnel order. The client disconnects with "Missing structure data from server." when `JigsawStructureData` is missing (gophertunnel sends empty lists; BDS sends its full definitions).
- The client requests its chunk radius after `StartGame`. A client reads a `NetworkChunkPublisherUpdate` before `PlayStatus(PlayerSpawn)` as "terrain for this view is coming" and may withhold `SetLocalPlayerAsInitialized` until every column of it has arrived (Cinnabar does; BDS follows its pre-spawn publisher update with the spawn-area `LevelChunk`s). Announcing a view without sending it deadlocks such clients into `Timeout(Spawn)` (Unastar, 2026-10-28), so the view is only ever announced by whoever sends the chunks:
  - `start_game` (gophertunnel `handleRequestChunkRadius` order): jolyne answers with `ChunkRadiusUpdated` and `PlayStatus(PlayerSpawn)` in one batch, no publisher update, and hands over the session at `SetLocalPlayerAsInitialized`. The client starts with an empty view; the application publishes it with its first chunks (Dragonfly does the same).
  - `start_game_streamed` (BDS order): jolyne answers with `ChunkRadiusUpdated` alone and hands over the session at once, not yet initialized (`is_initialized()` is false). The application sends `NetworkChunkPublisherUpdate`, the chunks it already has, and `PlayStatus(PlayerSpawn)`; it never holds `PlayerSpawn` back for chunk generation, and keeps streaming while the client loads. The play receive path (`Session::next_packet`, `BedrockStream::recv_packet*`) checks `SetLocalPlayerAsInitialized` and fails with `Timeout(Spawn)` at the spawn deadline; packets the client sent during the join are replayed into the session first.
- `SetLocalPlayerAsInitialized` ends the spawn; its runtime ID must equal the `StartGame` runtime ID.

### Timeouts
- Server: `login_timeout` (default 30 s) from accept through `ClientToServerHandshake`, including authentication; `resource_pack_timeout` (5 min); `spawn_timeout` (60 s).
- Client: one `join_timeout` (120 s) across every phase.

## Acceptance Gates

This sequence is the contract for compatibility smoke tests, not just documentation.

- `proto::tests::join_sequence_follows_the_bds_capture` checks the server-side packet order through `PlayStatus(PlayerSpawn)` against a recorded BDS 1.26.51 trace; `proto::tests::start_game_and_spawn_answers_leave_as_single_batches` pins the batching; `proto::tests::spawn_does_not_announce_terrain_it_does_not_send` and `proto::tests::streamed_spawn_lets_the_application_deliver_the_view_first` join with a client that waits for its announced view; `proto::tests::streamed_spawn_keeps_the_spawn_deadline_and_runtime_id_check` and `stream::tests::streamed_spawn_{completes,times_out}_on_the_play_receive_path` cover the streamed spawn's checks.
- `proto::tests::post_encryption_packets_match_bds_bytes` pins the `PlayStatus(LoginSuccess)` and `ResourcePacksInfo` payloads to a BDS 1.26.51 capture and `ResourcePackStack` to the bytes real clients accepted before the sans-IO rewrite (base game version `1.26.51`; BDS sends `*`).
- An Unastar smoke test should boot the server, complete this sequence with real registry data, request a chunk radius, and remain connected.
- Resource-pack negotiation must wait for the correct completed client response before `StartGame`.
- `ClientCacheStatus` must be accepted without blocking the flow, even when blob-cache support is disabled.
- Each handshake stage should have a timeout and deterministic disconnect reason.
