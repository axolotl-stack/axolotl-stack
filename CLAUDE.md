# Axolotl Stack

## Project Overview
A Minecraft Bedrock Edition server written in Rust. Monorepo with multiple crates under `crates/`.

## Key Conventions
- **Rust Edition**: 2024 (workspace-level, set in root `Cargo.toml`)
- **Async Runtime**: Tokio
- **ECS**: bevy_ecs 0.17
- **Plugin System**: WASM-based (wasmtime), migrating to Component Model + WIT

## Crate Structure
- `unastar` - Main server binary
- `unastar-api` - Plugin API types and traits (guest SDK)
- `unastar-api-macros` - Proc macros for plugin authoring
- `example-plugin` - Example WASM plugin
- `valentine` - Protocol implementation (Bedrock)
- `jolyne` - Network layer
- `tokio-raknet` - RakNet transport
- `unastar-data` - Data generation and codegen
