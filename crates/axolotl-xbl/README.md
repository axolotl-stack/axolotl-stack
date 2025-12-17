# Axolotl XBL

**Axolotl XBL** is a standalone, async Rust library for interacting with Xbox Live services. It is designed to handle the complex authentication flows required for Minecraft: Bedrock Edition servers and clients.

## Features

- **Token Exchange**: Handle XSTS token retrieval and exchange.
- **Device Code Flow**: Authenticate seamlessly using the device login flow.
- **Minecraft Services**: specifically tailored for `minecraftservices.com` authentication.
- **PlayFab Integration**: Support for PlayFab login flows (gatekeeper for NetherNet).

## Usage

```toml
[dependencies]
axolotl_xbl = "0.1"
```