# Unastar long-term direction (finished product, WASM-first plugins)

This document is the **decision + plan** for where Unastar is going long-term.

For stakeholder needs, use-cases, and evolving design notes (event ordering, shared APIs like economy/permissions, manifest/capability UX), see `crates/unastar/STAKEHOLDERS.md`.

---

## Goals (priority order)

1. **Quality & correctness**
   - Protocol correctness and stability over feature count.
   - Predictable tick behavior (clear phase ordering).
   - Maintainable boundaries (networking vs simulation vs persistence vs plugins).

2. **Performance**
   - Target: stable 20 TPS (50ms/tick), with headroom for spikes.
   - Keep hot paths data-oriented (ECS + cache-friendly structures).
   - Measure and budget plugin impact (time + allocations + event volume).

3. **Extensibility as a mainstream server product**
   - Drop-in plugin ecosystem (PMMP/Nukkit style), not “compile your own server.”
   - Safe by default: plugins should not be able to crash the host or access the machine without explicit permission.

4. **Vanilla baseline (not perfect parity)**
   - Provide a “vanilla-ish” baseline: sensible defaults + recognizable behavior.
   - Allow overriding/augmenting gameplay without forking the core.

---

## Identity choice

Unastar is a **finished server product** with a **WASM-first plugin model**.

Implications:
- Primary distribution is a **server binary** + a `plugins/` folder.
- Primary extension mechanism is **WASM plugins** with a **stable, versioned host API**.
- “Library mode” can exist for development/testing, but it should not drive architecture or compatibility promises.
- Dynamic libraries (`.dll/.so`) are not a primary plugin strategy (Rust ABI/versioning costs are not worth it).

---

## Core architectural constraints (non-negotiables)

### 1) Strict tick phases
Unastar must have an explicit per-tick pipeline to avoid reentrancy and surprise work:
1. Ingest network input → normalize into internal events.
2. Run simulation (ECS schedules).
3. Run plugin callbacks (batched) with time budgets.
4. Apply plugin actions (validated) as a controlled phase.
5. Emit network output (batched flush).

No ad-hoc “run ECS now” from random code paths.

### 2) Single source of truth for world state
World/chunk state must have one authoritative storage model to avoid divergence.
This is required for plugin correctness (plugins must observe consistent state).

### 3) Event semantics must be explicit (priority + cancellation)
For Bukkit/PMMP-style cancellable events, ordering matters:
- Decision/cancellable events run through a strict priority pipeline.
- Monitor/post-result events can be batched.

### 4) Plugin API must be coarse-grained and bounded
WASM overhead is manageable if we avoid death-by-hostcall and bound work:
- prefer actions-out over imperative hostcalls,
- use IDs/handles (not copies of large structs),
- batch where semantics allow (monitor/high-volume),
- cap event volume, query volume, and per-tick plugin work.

---

## Plugin system (WASM) – target design

### Packaging
- `plugins/<plugin_id>/plugin.wasm`
- `plugins/<plugin_id>/plugin.toml` (manifest)
  - plugin id/name/version
  - required host API version
  - requested capabilities (fs/network/write player data/etc)
  - subscriptions/filters (which events the plugin wants)

Future-friendly: allow embedding the manifest inside `plugin.wasm` as a custom section so the host can read permissions without executing plugin code.

### API shape
- Prefer WIT/component model style interfaces (or an equivalent strongly-typed IDL).
- Plugin entrypoints should be **event-driven**:
  - `on_load(...)`
  - `on_unload(...)`
  - `on_tick(events: list<Event>) -> list<Action>`

Note: not all events are equally batchable. Decision/cancellable events should preserve priority ordering; monitor/high-volume events are where batching pays off.

### Performance policy
- Per-plugin time budget per tick (soft + hard limits).
- Memory limit per plugin instance.
- Hard caps on:
  - number of events delivered per tick,
  - number of actions accepted per tick,
  - size of strings/byte blobs crossing the boundary.
- Metrics per plugin: time, allocations, trap count, dropped events.

### Security policy
- Default deny: filesystem, network, environment, process spawning.
- Explicit capabilities in manifest; server config controls whether to grant them.
- Prefer “safe host APIs” over raw access (e.g., HTTP via a controlled host function rather than raw sockets).

### Operational UX
- Hot reload: optional, but if supported it must be designed (clean teardown, handle invalidation, in-flight events).
- Good logs: plugin name/version in every error; clear trap reporting.

---

## Vanilla vs extendable: how we’ll do the “good mix”

Baseline approach:
- Core provides engine capabilities (world storage, entity replication, chunk streaming, etc.).
- “Vanilla-ish gameplay” is implemented as first-party modules (native Rust systems) that can be disabled/overridden.
- Plugins override policy at event boundaries (permissions, commands, block break allow/deny, join rules) without becoming the inner loop.

---

## What must change in the current codebase to align

This is not the full code review (see `crates/unastar/REVIEW.md`), but the key long-term alignment work:

1. **Eliminate overlapping legacy vs ECS implementations** (chunk streaming, world storage paths, despawn/cleanup paths).
2. **Enforce a single source of truth for chunks/world state** so plugins and core see consistent data.
3. **Split `server/game.rs`** into modules so “event in → simulation → actions out” is explicit.
4. **Add an internal event bus + action queue** (even before WASM), so the simulation has a clean extension surface.

---

## Implementation plan (long-term)

### Phase 0: commit to the product direction
1. Treat `LONGTERM.md` as the contract: WASM-first plugins, finished product.
2. Draft the plugin manifest spec (fields + semantics) and an API versioning policy.

### Phase 1: refactor toward clean tick phases and single world state
1. Remove duplicate chunk streaming/world data paths; unify on one model.
2. Ensure disconnect/despawn is a scheduled ECS phase (no mid-tick ECS runs).
3. Split `server/game.rs` into cohesive modules.

### Phase 2: define the internal extension boundary (pre-WASM)
1. Introduce internal events + actions (Rust types) and route core logic through them.
2. Implement policy hooks in core using this boundary (e.g., block break allow/deny).

This phase makes the eventual WASM boundary much cleaner.

### Phase 3: add WASM runtime behind a feature flag
1. Embed a WASM runtime (e.g., Wasmtime) and implement the minimal host API.
2. Load plugins from `plugins/` with manifest parsing.
3. Deliver batched events and apply returned actions.
4. Add sandboxing primitives: timeouts/interrupts, memory caps, capability gating.

### Phase 4: ship the first “real” plugins and tooling
1. Provide a plugin SDK (`unastar-plugin-sdk`) that generates bindings and a starter template.
2. Provide example plugins (chat/commands/permissions/anti-cheat hooks).
3. Add developer tooling: better logs, plugin diagnostics, compatibility error messages.

### Phase 5: stabilization and ecosystem support
1. Freeze API v1 (or v0.x with a clear break policy).
2. Conformance tests for the host API.
3. Document best practices (batching, avoiding hostcall spam).
