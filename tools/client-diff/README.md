# Bedrock client packet differential harness

Compare the actual offline client traffic sent by the pinned gophertunnel fork and jolyne to vanilla BDS, and retain vanilla replies as a behavioral reference. This tool measures divergences; it does not assert the clients are equivalent. Real Minecraft is an optional later capture and is **never started by `run.ps1`**.

**Observed acceptance status:** BDS currently rejects both offline clients at
Login, before spawn. The runner retains rejection captures/reports for both
scenario attempts and exits nonzero; it does not pretend chat or idle completed.
See the [secret-free observed report](examples/rejected-login.md), including the
plain-BDS and unmodified-upstream isolation experiments.

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

## Capture and normalization contract

Every line is `{seq,t_ms,dir,id,name,hex}`. `seq` is monotonically increasing within that vantage point, `t_ms` is elapsed time, `dir` is `c2s` or `s2c`, and `hex` is the actual decrypted, decompressed, unbatched packet including its varuint header, **without batch-entry length**. Packet names are descriptive; IDs and bytes determine comparison. Secrets in login JWTs remain in raw logs: do not publish captures without inspection.

Normalization decodes/encodes using `valentine::bedrock::version::v1_26_51`. `masks.toml` documents each supported mask. JWT normalization removes signatures and explicitly listed volatile claims but preserves remaining identity/client claims. Login protocol versions, screen types, movement input flags, command text, and resource-pack choices remain visible. Float tolerance is pairwise absolute distance; it does not round values into buckets. Unknown or undecodable packets compare raw. Subclient headers remain significant. ClientToServerHandshake is empty on this protocol and needs no payload mask. Server-generated session IDs outside the supported typed fields remain reported rather than blindly erased.

## Add a scenario

Edit `scenarios.json`, shared by both clients. Each scenario has `steps` and `idle_ms`. Each step has `after_ms` (delay since the preceding step, or since spawn for the first step), `kind` (`chat` or `command`), and `text`. Both clients wait for completed spawn/local-player initialization, execute the steps in order, then idle for `idle_ms` and disconnect. Keep fixed identity values in the shared `identity` object; private keys and signatures remain per connection.

## Later: real Minecraft

`real-client/launch.ps1` is a manual launcher only. It opens a `minecraft://connect/` URI, waits, and terminates the game process. Review its parameters/help and explicitly run it only when a user is present, BDS/Endstone is ready, and the player can complete any interactive client setup. Capture through Endstone and compare that capture using `diff`; no game invocation belongs in automated verification.

## Version compatibility

[Endstone 0.11.13 changelog](https://github.com/EndstoneMC/endstone/blob/v0.11.13/CHANGELOG.md) moves to BDS 1.26.52. [Bedrock 26.52](https://minecraft.wiki/w/Bedrock_Edition_26.52) still uses protocol **2193**: the patch did not increment the protocol number. The harness deliberately stays on 0.11.12/1.26.51, rather than assuming equal protocol numbers imply identical server behavior.

## Validation

```powershell
cargo fmt --all
cargo test -p client-diff
cargo clippy -p client-diff --all-targets -- -D warnings
cd tools/client-diff/gt-client
go vet ./...
```

Fixture tests cover packet insertion alignment, configurable timestamp masking,
embedded certificate/JWT normalization, raw fallback for unknown/truncated or
noncanonical packets, float tolerance boundaries, mixed known/unknown capture
batches, signed fixed identity, and full timing intervals after slow sends.
