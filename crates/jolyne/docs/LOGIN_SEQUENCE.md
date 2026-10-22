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
    Note right of Client: Contains chain data (skin, identity)
    
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

    Server->>Client: [DimensionData / JigsawStructureData / VoxelShapes] (optional)
    Server->>Client: StartGame
    Server->>Client: ItemRegistry
    Server->>Client: BiomeDefinitionList
    Server->>Client: AvailableActorIdentifiers
    Server->>Client: CreativeContent
    Note right of Server: One batch; BDS sends the definitions before the radius request

    Client->>Server: ServerboundLoadingScreen (Start)
    Client->>Server: RequestChunkRadius
    Note right of Server: radius must be >= 1; the server may override it

    Server->>Client: ChunkRadiusUpdated
    Server->>Client: NetworkChunkPublisherUpdate
    Server->>Client: PlayStatus (PlayerSpawn)

    Client->>Server: ServerboundLoadingScreen (End)
    Client->>Server: SetLocalPlayerAsInitialized
    Note right of Server: runtime ID must match StartGame

    Note over Client, Server: In-Game Loop (Tick Sync, MovePlayer, etc.)
```

## Implementation Details

### 1. Handshake Phase
- **NetworkSettings**: Negotiates compression.
- **Login**: Validates the XBOX Live chain (if online mode) and client identity.
- **Encryption**: ECDH key exchange using P-384 curves.

### 2. Resource Pack Phase
- Even if no packs are required, the server must send `ResourcePacksInfo`.
- The client must respond with `Completed` (after receiving `ResourcePackStack`) before `StartGame` is sent.
- **ClientCacheStatus**: Clients may send this to indicate blob cache support. It should be handled (usually ignored if cache not implemented) without blocking the flow.

### 3. Spawn Sequence
- `StartGame`, `ItemRegistry` and the definition lists are sent together, in BDS order.
- The client requests its chunk radius after `StartGame`; the server answers with `ChunkRadiusUpdated`, `NetworkChunkPublisherUpdate` (radius in blocks) and `PlayStatus(PlayerSpawn)`.
- `SetLocalPlayerAsInitialized` ends the spawn; its runtime ID must equal the `StartGame` runtime ID.

### Timeouts
- Server: `login_timeout` (default 30 s) from accept through `ClientToServerHandshake`, including authentication; `resource_pack_timeout` (5 min); `spawn_timeout` (60 s).
- Client: one `join_timeout` (120 s) across every phase.

## Acceptance Gates

This sequence is the contract for compatibility smoke tests, not just documentation.

- `proto::tests::join_sequence_order_matches_vanilla` asserts the server-side packet order through `PlayStatus(PlayerSpawn)`.
- An Unastar smoke test should boot the server, complete this sequence with real registry data, request a chunk radius, and remain connected.
- Resource-pack negotiation must wait for the correct completed client response before `StartGame`.
- `ClientCacheStatus` must be accepted without blocking the flow, even when blob-cache support is disabled.
- Each handshake stage should have a timeout and deterministic disconnect reason.
