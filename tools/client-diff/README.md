# Bedrock client packet differential harness

Compare the actual offline client traffic sent by the pinned gophertunnel fork and jolyne to vanilla BDS, and retain vanilla replies as a behavioral reference. This tool measures divergences; it does not assert the clients are equivalent. Real Minecraft is an optional later capture and is **never started by `run.ps1`**.

**Observed acceptance status:** both offline clients spawn on BDS 1.26.51 and
complete `join-idle` and `join-chat`. The original `readNoHeader` rejection was
BDS's metadata validation rejecting retired UWP `DeviceOS: 7`, not malformed
length prefixes. The shared identity now uses Android (`1`); jolyne additionally
sends a modern multiplayer identity token. See the [secret-free observed
report](examples/rejected-login.md) for byte regressions, counts, and remaining
differences. The Go reference is unmodified.

## Prerequisites

Windows x64, PowerShell 7, Python 3.12, the repository's pinned Rust toolchain, and Go. If Go is absent, install it with `winget install GoLang.Go` and reopen the terminal (or add `C:\Program Files\Go\bin` to PATH). Endstone 0.11.12 is pinned to the BDS 1.26.51 baseline. Setup downloads BDS from Mojang; binaries, worlds, logs, and virtual environments are ignored and must never be redistributed. Read and comply with the [Minecraft EULA](https://www.minecraft.net/eula).

The reference pins are HashimTheArab/gophertunnel `80c811b6186016b3860c358368cfa47e507f26e9` and lunar-bedrock/go-raknet `87a6b7e53799290d70ac25bcd03645d7ead00d2c`; `gt-client/go.mod` records exact pseudo-versions through replacements of the original sandertv module paths. The fork's go-nethernet and go-xsapi versions remain unchanged.

## Run

From the repository root, in PowerShell:

```powershell
./tools/client-diff/endstone-capture/setup.ps1
./tools/client-diff/run.ps1
```

The orchestrator restores the world template between clients, starts Endstone/BDS, runs each scenario with each client, stops the server, and compares c2s and s2c captures. The Endstone plugin records independent server-side packet observations. Runtime files remain below ignored runtime/output directories, not in source control. The world uses a fixed seed, disabled daylight/random ticks/mob spawning, and fixed time/weather. Do not run multiple harness instances on the same ports/runtime directory.

On first execution, the runner creates `endstone-capture/runtime/world-template`
from a server initialized with no player connections, after graceful shutdown.
Every capture restores that closed-world snapshot. Delete that ignored template
to rebuild it after changing world settings. Reports are `<scenario>-c2s.md` and
`<scenario>-s2c.md` in the printed `runs/` directory; command logs and each
Endstone per-endpoint JSONL are retained alongside them. Failed client commands
remain failures even when their partial packet logs are successfully compared.

Individual client and comparison commands:

```powershell
cd tools/client-diff/gt-client
go run . -address 127.0.0.1:19132 -scenario join-idle -scenarios ../scenarios.json -output gt.jsonl
cd ../../..
cargo run -p client-diff -- jolyne --address 127.0.0.1:19132 --scenario join-idle --output jolyne.jsonl
cargo run -p client-diff -- diff tools/client-diff/gt-client/gt.jsonl jolyne.jsonl --direction c2s --output report.md
cargo run -p client-diff -- diff tools/client-diff/gt-client/gt.jsonl jolyne.jsonl --direction s2c
```

`diff` accepts two or more logs and compares each subsequent log with the first. Direction is filtered before alignment. Reports contain an exact packet-ID LCS sequence alignment, original side-by-side hex dumps (different bytes bracketed), and identical/masked-equal/different/missing counts. Missing counts include unmatched packets from either side. Very large captures fail explicitly rather than allocating an unbounded alignment matrix.

`run.ps1 -Port <udp-port>` moves the vanilla server off 19132 (it rewrites the
port lines of the harness-owned runtime `server.properties`; IPv6 uses
`Port+1`), so the harness can run next to another server.

## Codec conformance

```powershell
./tools/client-diff/run.ps1 -Mode conformance -Port 29132
cargo run -p client-diff -- conformance tools/client-diff/runs/<run> --output report.md --failures failures.ndjson
```

Conformance mode captures `join-idle`, `join-chat` and `tour-world` with both
clients and `tour-interact` with the gophertunnel client, then decodes **every**
captured packet — client-side logs and the Endstone server-side
`player-*.jsonl` logs of what BDS sent and received — with Valentine's owned and
borrowed decoders. A packet passes only if both decoders consume the whole
payload, both re-encodings are byte-identical to the capture, and the borrowed
view materialises to the owned value. The `conformance` subcommand accepts files
or directories (searched for `*.jsonl`), writes a packet type × count table, and
exits non-zero on any failure. Failing packets go to `--failures` and become
regression fixtures under `crates/valentine/tests/fixtures/conformance`; schema
defects they reveal are corrected in `valentine_gen` and reported in
`crates/valentine_gen/PROTOCOLGEN_ISSUES.md`. The committed result is
[`conformance-report.md`](conformance-report.md).

Scenarios with `"auth_input": true` make both clients tick like vanilla
(`PlayerAuthInput` every 50 ms, respawn and dimension-change acknowledgements),
which BDS needs to simulate the player (item pickup, death/respawn, dimension
changes). `{x}`, `{y}` and `{z}` in chat/command text expand to the player's
feet block, fixed at first use after the spawn correction. `tour-world` drives
the world through commands (weather/time, items with NBT, block placement and
`destroy` particles, sculk sensor/shrieker/catalyst events, mob spawning,
damage and deaths, explosions, lightning, a wither boss bar, effects, XP,
titles, scoreboards, camera/fog/HUD, music, survival death and respawn,
teleports and an overworld→nether→overworld round trip).

`tour-interact` adds gophertunnel-only interaction steps; X/Y/Z are block
offsets from the same feet block and slots 0–8/9–35 are hotbar/inventory:

| Kind | Fields | Packets |
|---|---|---|
| `move` | `dx`,`dy`,`dz`,`ticks` | `PlayerAuthInput` movement |
| `break` | `x`,`y`,`z`,`face` | aim, then `PlayerAuthInput` block actions |
| `click` | `x`,`y`,`z`,`face` | aim, then `InventoryTransaction` use-item on block |
| `select` | `slot` | `MobEquipment` |
| `move_item`, `drop`, `craft` | `slot`,`to`,`count`,`recipe` | `ItemStackRequest` take/drop/craft |
| `attack`, `interact` | `entity` (type, e.g. `minecraft:zombie`) | aim, then `InventoryTransaction` use-item on entity |

BDS 1.26.51 answers this bot's block `click` transactions with a block and
inventory resync instead of placing or opening, so container windows
(`ContainerOpen` and friends) are not yet covered; the click and resync
packets themselves are. Jolyne runs only chat/command scenarios and rejects
interaction steps before connecting.

## Capture and normalization contract

Every line is `{seq,t_ms,dir,id,name,hex}`. `seq` is monotonically increasing within that vantage point, `t_ms` is elapsed time, `dir` is `c2s` or `s2c`, and `hex` is the actual decrypted, decompressed, unbatched packet including its varuint header, **without batch-entry length**. Packet names are descriptive; IDs and bytes determine comparison. Secrets in login JWTs remain in raw logs: do not publish captures without inspection.

Normalization decodes/encodes using `valentine::bedrock::version::v1_26_51`. `masks.toml` documents each supported mask. JWT normalization removes signatures and explicitly listed volatile claims but preserves remaining identity/client claims. Login protocol versions, screen types, movement input flags, command text, and resource-pack choices remain visible. Float tolerance is pairwise absolute distance; it does not round values into buckets. Unknown or undecodable packets compare raw. Subclient headers remain significant. ClientToServerHandshake is empty on this protocol and needs no payload mask. Server-generated session IDs outside the supported typed fields remain reported rather than blindly erased.

## Add a scenario

Edit `scenarios.json`, shared by both clients. Each scenario has `steps`, `idle_ms` and optionally `auth_input` (see [Codec conformance](#codec-conformance)). Each step has `after_ms` (delay since the preceding step, or since spawn for the first step), `kind` (`chat` or `command` with `text`, or a gophertunnel-only interaction kind), and the kind's fields. Both clients wait for completed spawn/local-player initialization, execute the steps in order, then idle for `idle_ms` and disconnect. Keep fixed identity values in the shared `identity` object; private keys and signatures remain per connection.

## Later: real Minecraft

`real-client/launch.ps1` is a manual launcher only. It opens a `minecraft://connect/` URI, waits, and terminates the game process. Review its parameters/help and explicitly run it only when a user is present, BDS/Endstone is ready, and the player can complete any interactive client setup. Capture through Endstone and compare that capture using `diff`; no game invocation belongs in automated verification.

## RakNet wire captures

`raknet/capture.ps1` records Mojang RakNet (BDS) behaviour through a loopback recording relay under path-MTU limits, loss, reordering, kicks and client silence; `real-client/capture.ps1` records the Minecraft client the same way in one command. Both feed `crates/tokio-raknet/tests/capture_diff.rs`; see [raknet/README.md](raknet/README.md). The `raknet-idle` and `raknet-kick` scenarios and the bot's `-linger` flag exist for these captures.

## Version compatibility

[Endstone 0.11.13 changelog](https://github.com/EndstoneMC/endstone/blob/v0.11.13/CHANGELOG.md) moves to BDS 1.26.52. [Bedrock 26.52](https://minecraft.wiki/w/Bedrock_Edition_26.52) still uses protocol **2193**: the patch did not increment the protocol number. The harness deliberately stays on 0.11.12/1.26.51, rather than assuming equal protocol numbers imply identical server behavior.

## Validation

```powershell
cargo fmt --all
cargo test -p client-diff
cargo clippy -p client-diff --all-targets -- -D warnings
cd tools/client-diff/gt-client
go vet ./...
go test ./...
```

Fixture tests cover packet insertion alignment, configurable timestamp masking,
embedded certificate/JWT normalization, raw fallback for unknown/truncated or
noncanonical packets, float tolerance boundaries, mixed known/unknown capture
batches, signed fixed identity, and full timing intervals after slow sends.
