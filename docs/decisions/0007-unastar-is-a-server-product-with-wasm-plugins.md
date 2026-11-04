---
status: accepted
date: 2025-12-26
decision-makers: [project owner]
---

# Unastar is a finished server product with WASM-first plugins

## Context and Problem Statement

Unastar could be a library others build servers from, or a server binary people download and
extend. That choice drives the plugin model, API stability and architecture.

## Considered Options

- Library / framework ("compile your own server")
- Server binary with native dynamic-library plugins (`.dll` / `.so`)
- Server binary with WebAssembly component plugins behind a versioned WIT API

## Decision Outcome

Chosen option: a server binary with WASM component plugins, because Rust has no stable ABI
(native plugins break on every compiler or layout change), and WASM gives isolation, resource
limits and default-deny capabilities, so a plugin cannot crash the host or reach the machine.

### Consequences

- Good, because plugins are drop-in (`plugins/<id>/plugin.wasm` + `plugin.toml`) and sandboxed.
- Good, because the API is a versioned contract (`unastar:plugin@0.2.0`).
- Bad, because every host call crosses a boundary, so the API must be coarse-grained and bounded.

## More Information

Origin: [docs/research/2025-12-26-unastar-long-term-direction.md](../research/2025-12-26-unastar-long-term-direction.md).
Contract: [openspec/specs/plugin-host/spec.md](../../openspec/specs/plugin-host/spec.md). That
document's "vanilla-ish baseline, not perfect parity" goal is superseded by 0013.
