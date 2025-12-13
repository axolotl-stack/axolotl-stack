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

    Server->>Client: StartGame
    Server->>Client: ItemRegistry
    Note right of Server: Required for items to exist
    Server->>Client: BiomeDefinitionList
    Note right of Server: Fixes crashes on 1.21.80+
    Server->>Client: AvailableEntityIdentifiers
    Server->>Client: CreativeContent
    Note right of Server: Required for Creative Inventory
    
    Server->>Client: ChunkRadiusUpdated
    Server->>Client: NetworkChunkPublisherUpdate
    
    Server->>Client: PlayStatus (PlayerSpawn)
    
    Client->>Server: SetLocalPlayerAsInitialized
    Note right of Client: Client is done loading and ready to play

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
- `StartGame`: Contains world settings, basic level info, and player position.
- `ItemRegistry`: Defines all custom and vanilla items for the session.
- `BiomeDefinitionList`: **CRITICAL** for newer clients.
- `CreativeContent`: Populates the creative inventory.
- `PlayStatus(PlayerSpawn)`: Tells the client to remove the loading screen.
- `SetLocalPlayerAsInitialized`: Sent by the **Client** to confirm they are ready.