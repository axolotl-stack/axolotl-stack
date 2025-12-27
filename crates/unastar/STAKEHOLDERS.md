# Unastar platform notes (living stakeholder + use-case document)

This is a **living document**. It is explicitly allowed to change as we learn more.

Purpose:
- Capture stakeholder needs, product expectations, and “sharp edges” that influence architecture.
- Record constraints that should prevent us from building features that will be rewritten later.
- Provide a reference when making trade-offs (performance vs correctness vs ecosystem UX).

Relationship to other docs:
- `crates/unastar/LONGTERM.md` is the **overview/decision**.
- `crates/unastar/REVIEW.md` is the **code review + near-term fixes**.

---

## Stakeholders (who we optimize for)

### 1) Server owners/admins (primary)
Needs:
- Download a binary, configure it, run it.
- Drop-in plugins (no Rust toolchain required).
- Clear logs and “why did this plugin fail?” messages.
- Safe defaults: no plugin should read the machine or crash the server by default.
- Operational controls: enable/disable plugins, permissions/capabilities, reload policies.

### 2) Plugin developers (primary)
Needs:
- A stable, versioned host API and a clear compatibility story.
- Good local dev experience: templates, SDK, strong typing, decent debugging.
- Predictable event semantics (priority, cancellation, ordering).
- Ability to build “systems plugins” (economy/permissions) that other plugins can use.

### 3) Players (secondary, but non-negotiable experience)
Needs:
- Low lag, stable TPS.
- Correct-ish gameplay and consistent behavior.
- Fairness (anti-cheat hooks, permissions, moderation).

### 4) Hosting providers / ops (secondary)
Needs:
- Bounded CPU and memory usage (especially under malicious plugins).
- Good metrics and resource attribution (which plugin is causing lag?).
- Predictable persistence behavior and low corruption risk.

### 5) Commercial/private plugin authors (secondary, ecosystem multiplier)
Needs:
- A way to distribute plugins without shipping source code.
- Some notion of plugin licensing (even if “out of scope” at first).

---

## Product positioning (finished product, not a framework)

The default distribution target is a **server binary** with **drop-in plugins**.

“Library mode” can exist as a development tool, but we should avoid designs that require users to compile their own server for routine extension.

---

## Non-functional requirements (what architecture must protect)

### Quality & correctness
- Deterministic tick pipeline: no hidden double-runs of simulation.
- Single source of truth for world state (chunks/blocks/entities).
- Clear separation between: network ingestion → simulation → plugin logic → network emission.

### Performance
- Target: stable 20 TPS (50ms/tick), with headroom.
- Performance budgets apply to plugins as well as core code.
- Avoid death-by-boundary-crossing: do not design the plugin API as a million tiny RPCs.

### Security
- Plugins are untrusted by default.
- Capability-based permissions: filesystem/network/env/process are denied unless explicitly granted.
- Time + memory limits are required (a plugin can stall a tick without “unsafe”).

---

## Plugin model: WASM-first

Core idea:
- Plugins are WASM modules loaded at runtime.
- They communicate with the server through a stable host API.
- The host API is versioned and backward compatible within major versions.

Why this matters for architecture:
- We must keep plugin-facing data models **stable** and **small**.
- We must avoid exposing internal ECS/component layouts to plugins.
- We must design for plugin-to-plugin interactions without shared memory.

---

## Event system semantics (priority, cancellation, and “batching”)

This is the most important “don’t paint ourselves into a corner” area.

### Key requirement: Bukkit/PMMP-style priority pipeline for cancellable events
For decision-making/cancellable events (examples: “block break attempt”, “chat send”, “command run”):
- We need deterministic ordering across listeners.
- Plugins must be able to observe and modify the event state (including cancellation).
- Later listeners must see earlier listeners’ changes.

This implies we cannot “broadcast a batch to all plugins simultaneously” and expect Bukkit-like semantics.

### Event categories (proposal)
1. **Decision events (cancellable / modifiable)**\n
   Must run through a strict priority pipeline.\n
   Examples: `BlockBreakAttempt`, `PlayerChatSend`, `CommandExecute`.\n

2. **Monitor events (post-result / read-only)**\n
   Outcome is already decided; plugins observe only.\n
   Examples: `BlockBroken`, `PlayerChatSent`, `CommandExecuted`.\n

3. **Telemetry/high-volume events (best-effort)**\n
   Batched, filtered, possibly dropped under load.\n
   Examples: movement sampling, metric hooks.\n

### Priority pipeline (proposal)
For decision events:
- Run listeners by priority: `Lowest → Low → Normal → High → Highest`.\n
- Then run `Monitor` listeners after the decision is final.\n
- Support “ignore cancelled” for listeners that want to run even if cancelled.

### “Better batching” that still preserves Bukkit semantics
The naive batching that breaks ordering is: send the same events to all plugins at once.

Instead, we can batch *within the sequential pipeline*:
- Collect a set of decision events that occurred this tick (or this phase).\n
- For each priority level, iterate listeners in deterministic order.\n
- For each listener (or per-plugin listener group at that priority), call into WASM with a **batch of events**.\n
- Plugin returns modifications (cancel/uncancel, changed fields, requested actions).\n
- Host applies those modifications before invoking the next listener.\n

This preserves the core semantics (“Plugin B sees what Plugin A did”) while reducing boundary crossings compared to “call per event per plugin”.

Important: within a batch, modifications are per-event; cancellation is per-event; a plugin can choose to ignore cancelled events.

### Action application timing
To avoid reentrancy hazards:
- Plugins should not directly mutate the world during the decision phase.\n
- Plugins produce **actions** which the host validates and applies in a controlled phase.\n
  - Example: for a cancelled `BlockBreakAttempt`, no `SetBlock(Air)` action is applied.\n

---

## Plugin-to-plugin APIs (economy, permissions, etc.)

WASM sandboxes mean plugins cannot directly call each other’s code or share memory.

We need explicit patterns that scale to ecosystems:

### Option A: Host-mediated event-bus protocol (recommended for MVP)
Pattern:
- Plugin A emits a request event (with correlation ID).\n
- Plugin B (economy provider) listens, processes, and responds by modifying the event or emitting a response event.\n
- Plugin A observes the final outcome in a monitor phase.\n

Pros:
- Decoupled: “shop” doesn’t need a specific economy plugin.\n
- Works well with priority/cancellation semantics.\n

Cons:
- Requires good conventions and schemas for “standard” events.\n

### Option B: Host-mediated “services” (KV store / database) (recommended as a baseline capability)
Pattern:
- Host provides a scoped persistent store per plugin namespace.\n
- For common shared APIs (economy/permissions), the host can define standard keys or tables.\n

Pros:
- Fast and simple; no plugin-to-plugin roundtrips.\n
- Persistence is stable even if plugins are removed.\n

Cons:
- The host becomes the spec owner (“what is an economy record?”).\n

### Option C: WASM component model dynamic linking (future)
Potentially the “cleanest” long-term plugin-to-plugin interface story, but it is a moving target and significantly increases loader complexity.\n
Do not block MVP on this.

---

## Plugin manifest and capability negotiation

### Requirements
- The server must be able to decide permissions **without executing plugin code**.\n
- Plugin permissions must be explicit and reviewable.\n

### Packaging options
1. **Sidecar manifest**: `plugins/<id>/plugin.toml` next to `plugin.wasm`.\n
2. **Embedded manifest**: put manifest text inside the `.wasm` as a **custom section**.\n

Supporting both is reasonable:
- Prefer embedded for “single file distribution”.\n
- Fall back to sidecar for simplicity and tooling.\n

### Embedded manifest specifics (proposal)
- Reserve a custom section name, e.g. `unastar:manifest`.\n
- Content is UTF-8 TOML (or JSON) text.\n
- The host reads the custom section bytes and parses it before instantiation.\n

Implementation notes (future work):
- Use a WASM parser to read custom sections (do not instantiate the module).\n
- Alternatively, some runtimes expose custom section retrieval APIs.\n

### Capability workflow (proposal)
1. Admin drops `plugin.wasm` into `plugins/`.\n
2. Host reads manifest and requested capabilities.\n
3. Host decides grant/deny via:\n
   - config policy (headless default),\n
   - interactive prompt (dev mode),\n
   - allowlist/denylist.\n
4. If denied:\n
   - plugin fails to load, or\n
   - plugin loads with reduced capabilities (explicitly visible in logs).\n

---

## Persistence model (host-provided)

Default stance:
- Plugins do not write arbitrary files by default.\n
- Provide host-mediated persistence instead.\n

Baseline feature (recommended):
- A per-plugin namespaced KV store with:\n
  - atomic set/get,\n
  - optional batch transactions,\n
  - optional TTL.\n

Optional upgrades:
- A host-provided SQL database (SQLite) behind a capability.\n
- A “player data” API that avoids plugins reinventing serialization formats.\n

---

## Open questions (to revisit)

- Event priority surface: do we match Bukkit exactly (including `EventPriority` names), or a simpler equivalent?\n
- Do we allow plugins to register multiple listeners for the same event type at different priorities (likely yes)?\n
- How much synchronous querying is allowed during event callbacks vs “actions out only”?\n
- What is the MVP list of standard cross-plugin event schemas (economy, permissions, claims, chat formatting)?\n
- Hot reload: supported, limited, or not in v0?\n

