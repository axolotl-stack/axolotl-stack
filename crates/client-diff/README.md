# client-diff

Packet capture, comparison and BDS parity scenarios for Bedrock 1.26.51
(protocol 2193). The capture side (BDS/Endstone runner, gophertunnel bot,
shared `scenarios.json`) lives in [`tools/client-diff`](../../tools/client-diff/README.md).

| Module | Purpose |
|---|---|
| `scenario` | The shared step script (`tools/client-diff/scenarios.json`): types and validation identical to the bot's. |
| `driver` | Runs a scenario over a real offline Jolyne client and records it in the bot's JSONL format. |
| `parity` | Retained BDS scenarios (`scenarios/<name>/`) and the comparison of a run against them. |
| `recording` | Client-side wire recorder shared with `bds-extractor`. |
| crate root, `conformance` | Packet-ID LCS diff of two captures and the codec round-trip check. |

## BDS parity scenarios

A player-visible behaviour is done when a scripted scenario recorded on BDS
1.26.51 and the same scenario run against unastar produce the same server
packets. Each retained scenario is a directory under `scenarios/`:

- `recording.jsonl`: the BDS capture cut to the scenario window (from the
  first step packet to the step packet of the first step left out), without
  chunk streaming and `PlayerAuthInput` ticks; original lines, so it can be
  hashed and diffed.
- `manifest.toml`: the scenario name and step count in `scenarios.json` (with
  the SHA-256 of those steps, so editing them invalidates the recording), the
  source capture, server, protocol, the player's IDs, the window length and
  recording SHA-256, the world (`superflat`) and setup, the compared packet
  types, the timing and float tolerances, the masks (each with a reason), and
  the listed differences: `known_failing` (unastar does not match yet; each
  names the `owner` lane of the fix) and `accepted` (integrator-accepted),
  each with a reason.

`crates/unastar/tests/bds_scenarios.rs` runs every retained scenario against
unastar in-process: the BDS world (default superflat, bedrock at y=-64, dirt
-63..-62, grass block -61, player spawning at feet (0.5, -60, 0.5)), the bot's
identity as an operator, and the scenario's own setup commands run for real.
A command unastar does not know fails the scenario with an `unsupported
command` difference. The test fails on every difference the manifest does not
list and on listed differences that no longer occur; the report (also written
to `target/tmp/bds-scenarios/<name>.md`) lists the first differences, shows
the differing packets side by side and the whole alignment.

The comparison keeps the server packets of the manifest's types inside the
window, applies the masks (`ignore`; `runtime_id`, `unique_id` and `stack_id`
compare IDs by order of first appearance with the player as `self`), aligns
both streams by packet type and target entity (LCS), compares aligned
packets field by field (`actor_data.data[id=…]` and attribute lists by key,
floats within `float_tolerance`) and their delay after the preceding step
packet (`timing_tolerance_ms`), and checks the step packets and command
feedback.

### Recipe: add a scenario

1. Script it in `tools/client-diff/scenarios.json` (steps documented in
   `tools/client-diff/README.md`); include the vanilla interaction, not only
   the happy path. Start from a clean state with commands (`/clear`, `/effect
   @s clear`, `/give`, `/setblock`, ...).
2. Take the native lock: `bash C:/Users/jvigu/orca/stab-coord/native-lock.sh acquire <lane> 1800 "<scenario>"`.
3. Record on BDS (never on port 19132):
   `./tools/client-diff/run.ps1 -Mode capture -Scenario <scenario> -Port 19140 -Output tools/client-diff/runs/<run>`;
   release the lock.
4. Retain it: `cargo run -p client-diff -- retain --run tools/client-diff/runs/<run>/<dir>/<scenario>-vanilla-go.jsonl --scenario <scenario> --steps <n> --name <dir-name> --description "<what it shows>"`
   (`--steps` takes a prefix of the scenario; the first step left out must
   send a packet, e.g. a command).
5. Read `scenarios/<dir-name>/manifest.toml`: adjust `compare.packets`, the
   masks and tolerances, each with a reason.
6. Add `<test_name> => "<dir-name>"` to the `scenarios!` list in
   `crates/unastar/tests/bds_scenarios.rs`.
7. Run it (loopback run: native lock):
   `cargo test -p unastar --all-features --test bds_scenarios <test_name> -- --ignored`
   (the comparisons are ignored in the default `cargo test`).
8. Fix unastar until it passes, or list each remaining difference under
   `known_failing` with the owning lane and the reason; `accepted` needs the
   integrator.
9. `cargo test -p client-diff` checks every retained scenario's hashes and
   that each recording matches itself.
10. Commit the scenario directory, the script change and the test line together.

`cargo run -p client-diff -- decode <capture.jsonl> --names SetActorData,MobEffect`
prints decoded packets of any capture for reading.

### Driver differences (Rust driver vs gophertunnel bot)

The Rust driver ports the bot's step logic (`gt-client/session.go`): the
same step packets, stack network ID tracking from InventoryContent,
InventorySlot, server InventoryTransactions and ItemStackResponses, the same
predicted destinations, request numbering, look-at rotations, tick queue and
timing. Where the client traffic necessarily differs:

- Login and join are Jolyne's: Login token claims, `ClientCacheStatus`,
  `RequestChunkRadius` value and the loading-screen packets during spawn
  (the bot reports both loading screens after spawning). These precede the
  scenario window.
- Recording happens on the transport (frames as sent and received) in Rust,
  at the connection's packet callback in Go; `t_ms` counts from recorder
  creation in both.
- Ticks run on one task interleaved with steps and server packets in Rust,
  on separate goroutines in Go: within a tick the order of a step packet and
  the tick's `PlayerAuthInput` can differ by one tick.
- `PlayerAuthInput` input flags are a list in Valentine and a bitset in
  gophertunnel; the encoded flags are the same.
- `CraftResultsDeprecated` items are encoded by Valentine's item descriptor
  types (name descriptor, aux 0); the bot's fork writes the same layout.
