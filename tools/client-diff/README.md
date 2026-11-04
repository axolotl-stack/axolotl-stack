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

The reference pins are HashimTheArab/gophertunnel `80c811b6186016b3860c358368cfa47e507f26e9` and lunar-bedrock/go-raknet `6124f52657d4d5f006ea2af2d525260998dd3173` (moved from `87a6b7e53799` on 2026-10-24, matching `tools/bench-compare/go`); `gt-client/go.mod` records exact pseudo-versions through replacements of the original sandertv module paths. The fork's go-nethernet and go-xsapi versions remain unchanged.

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
| `move_item`, `drop`, `craft` | `slot`,`to`,`count`,`recipe`, craft: optional `consume`,`action`,`text`,`recipe_network_id`,`cost`,`result`,`result_nbt` | `ItemStackRequest` take/drop/craft |
| `take_cursor`, `place_cursor`, `drop_cursor`, `swap`, `destroy` | `slot`,`to`,`count` | `ItemStackRequest` take into / place from / drop from the cursor (UI slot 0), swap, destroy |
| `open_inventory`, `close_inventory` | — | `Interact` OpenInventory; `ContainerClose` window 0 type 0xff |
| `attack`, `interact` | `entity` (type, e.g. `minecraft:zombie`) | aim, then `InventoryTransaction` use-item on entity |
| `use` | optional `x`,`y`,`z` (aim at that block first) | `InventoryTransaction` use-item click-air with the held stack, then `PlayerAuthInput` with `StartUsingItem` |
| `release`, `consume` | — | `InventoryTransaction` release-item, action 0 (release) or 1 (consume), with the held stack and head position |

Inventory steps also take `"stale": true` (send an unassigned source stack ID) and, for
`move_item`/`place_cursor`, `"to_container": "armor"|"offhand"` (destination slot `to` in
that container). `-Mode capture -Scenario inventory-moves` runs the inventory scenario
whose BDS capture is replayed by unastar's inventory engine tests
(`crates/unastar/src/server/game/inventory/testdata/bds-inventory-moves.jsonl`).

Crafting: `move_item` with `"to_container": "crafting"` moves into the
inventory's 2x2 grid (`to` = PlayerUI slot 28–31). `craft` looks up
single-ingredient and smithing recipes by recipe ID (e.g.
`minecraft:smithing_netherite_sword`, `minecraft:smithing_armor_trim`) and
multi recipes by UUID (repair is `00000000-0000-0000-0000-000000000001`);
`"consume": [{"container": "crafting", "slot": 28}, ...]` consumes from the
listed slots (optional `count`, default 1) instead of inventory slot `slot`.
`"action": "optional"` sends `CraftRecipeOptional` with `text` as the request's
filter string (anvil; recipe network ID 0), `"action": "grindstone"` sends
`CraftGrindstoneRecipe` (repair-and-disenchant) whose ID is the stack network
ID of the first consumed input (BDS 1.26.51 rejects any other value with
status 9) and `cost`. `"result"`/`"result_nbt"` add a predicted output
(`CraftResultsDeprecated`; JSON integers become TAG_Int). The crafted stack is
taken to inventory slot `to`, merging into a matching stack there.

Block windows: `move_item` also takes `"from_container"`/`"to_container"`
`chest`, `barrel`, `ingredient`, `fuel`, `result`, `anvil_input` (slot 1),
`anvil_material` (2), `smithing_input` (51), `smithing_material` (52),
`smithing_template` (53), `grindstone_input` (16) or `grindstone_additional`
(17) (slots of the window the last `ContainerOpen` opened, up to 53), and
`close_container` closes that window (it sends nothing if the server opened
none). BDS lists the anvil, smithing table and grindstone slots from index 0
in `InventoryContent`; the bot maps them to the UI slots above. Items left in
those slots stay there after the window closes (BDS does not return them),
so scenarios take refused inputs back out before closing. Scenarios
`containers-dry`, `containers` and `chest-obstruction` use chests;
`smithing-table`, `anvil`, `anvil-wear`, `anvil-fall`, `grindstone`,
`grindstone-curse`, `grindstone-named` and `grindstone-book` cover the
workstations (`blocks-dry` is their smoke run).

Interaction scenarios report the join loading screen like the vanilla client
(`ServerBoundLoadingScreen` start and end, sent right after spawn because the
fork's spawn sequence sends neither). Without them BDS 1.26.51 never simulates
the player: effects never tick or expire, hunger never drains, and both block
`click` and air `use` transactions are answered with a block, attribute,
actor-data and inventory resync (the `containers-dry` runs of 2026-10-28, and
the `item-use-dry` runs of 2026-10-29 before the change). With them, opening a
chest yields `ContainerOpen` and air use eats, drinks and shoots. Placing
cobblestone on the ground is still answered with an `UpdateBlock` resync of
the target column; the bot sends block runtime ID 0 for blocks it has not seen
in an `UpdateBlock`.

`item-use-food`, `item-use-drink`, `item-use-stew` and `item-use-ranged` eat
(apple, golden apple, cooked beef, rotten flesh, honey), drink (milk, water
potion), eat suspicious stew 0–9 and use a bow, snowball, egg, crossbow,
trident and ender pearl in survival; `item-use-dry` is their short smoke run.
`item-use-potions` drinks every potion 0–46 (natural regeneration off,
health restored before each, harming drunk before healing),
`item-use-throwables` throws splash potions at the feet and 3/5 blocks ahead,
an ender pearl and a tipped arrow, `item-use-lingering` throws a lingering
potion at the feet and stays in the cloud, `item-use-tridents` throws a plain
trident and teleports onto it, a loyalty trident and a riptide trident in
rain, and `item-use-repair` repairs two damaged wooden pickaxes in the
crafting grid; `item-use-2-dry` is their smoke run. `item-use-splash-distance`
and `item-use-splash-sideways` measure how splash effect duration falls with
distance (the latter teleports the player between throw and impact), and
`item-use-tridents-water` throws an impaling trident at a drowned on land and
in water and uses riptide 1/3 in a 2-deep pool. Retained spawn packets
from these runs (and `fishing_hook` from `fishing`) are in
`crates/bds-extractor/captures/1.26.51/items/`.

Milestone-7 steps: block windows also name `enchanting_input` (UI 14),
`enchanting_material` (15), `brewing_input` (0), `brewing_result` (1-3),
`brewing_fuel` (4), `shulker_box` (0-26), `crafter`, `stonecutter_input` (3),
`loom_input`/`loom_dye`/`loom_material` (9-11) and
`cartography_input`/`cartography_additional` (12-13); BDS 1.26.51 accepts
crafter items only through `chest` (LevelEntity) slots 0-8. `craft` takes
`"action": "enchant"` with `option` (index into the last
`PlayerEnchantOptions`), `"action": "loom"` with `pattern`, and
`to_container` to place the result into the window. `move` takes `flags`
(PlayerAuthInput flag names, e.g. `start_sneaking`, `start_gliding`);
`break` with `ticks` mines like a survival player (StartBreak, CrackBreak
per tick, PredictDestroyBlock, StopBreak); `book` sends `BookEdit`
(`action` replace/add/delete/swap/sign, `page`, `to`, `text`, `title`,
`author`); `toggle_crafter` sends `PlayerToggleCrafterSlotRequest`;
`map_info` requests the held map; `wait_event` waits for an `ActorEvent`
(`event`, `timeout_ms`, e.g. 13 for a fish bite); `"lenient": true` skips a
step the bot cannot build. Scenarios: `items3-dry`, `enchanting`,
`enchanting-slots`, `brewing`, `brewing-lingering`, `shulker`, `shulker-dye`,
`crafter`, `stations-2`, `cartography`, `cart-zoom`, `cart-clone`,
`cart-lock`, `itemuses-shield`, `itemuses-elytra`, `itemuses-elytra-2`,
`itemuses-misc`, `fishing`, `lead-nametag`, `lead-nametag-2`. Map
zoom/clone/lock requests sent as `CraftRecipe` with the multi recipe IDs
crash BDS 1.26.51 (Endstone 0.11.12) or return status 9.
The Rust driver (`client-diff jolyne`, `crates/client-diff/src/driver`) runs
every step kind with the same client packets; its few necessary differences
are listed in [crates/client-diff/README.md](../../crates/client-diff/README.md#driver-differences-rust-driver-vs-gophertunnel-bot).

## Capture and normalization contract

Every line is `{seq,t_ms,dir,id,name,hex}`. `seq` is monotonically increasing within that vantage point, `t_ms` is elapsed time, `dir` is `c2s` or `s2c`, and `hex` is the actual decrypted, decompressed, unbatched packet including its varuint header, **without batch-entry length**. Packet names are descriptive; IDs and bytes determine comparison. Secrets in login JWTs remain in raw logs: do not publish captures without inspection.

Normalization decodes/encodes using `valentine::bedrock::version::v1_26_51`. `masks.toml` documents each supported mask. JWT normalization removes signatures and explicitly listed volatile claims but preserves remaining identity/client claims. Login protocol versions, screen types, movement input flags, command text, and resource-pack choices remain visible. Float tolerance is pairwise absolute distance; it does not round values into buckets. Unknown or undecodable packets compare raw. Subclient headers remain significant. ClientToServerHandshake is empty on this protocol and needs no payload mask. Server-generated session IDs outside the supported typed fields remain reported rather than blindly erased.

## Add a scenario

Edit `scenarios.json`, shared by both clients (step types and validation: `gt-client/main.go` and `crates/client-diff/src/scenario.rs`, kept identical). Each scenario has `steps`, `idle_ms` and optionally `auth_input` (see [Codec conformance](#codec-conformance)). Each step has `after_ms` (delay since the preceding step, or since spawn for the first step), `kind`, and the kind's fields. Both clients wait for completed spawn/local-player initialization, execute the steps in order, then idle for `idle_ms` and disconnect. Keep fixed identity values in the shared `identity` object; private keys and signatures remain per connection.

To make a recorded scenario a permanent BDS parity test against unastar, follow the recipe in [crates/client-diff/README.md](../../crates/client-diff/README.md#recipe-add-a-scenario).

The retained scenario comparisons (`crates/unastar/tests/bds_scenarios.rs`) are a parity measurement, not a regression suite: they are ignored in the default `cargo test` and run with `cargo test -p unastar --all-features --test bds_scenarios -- --ignored`. They are expected to fail until each owning lane closes its differences; a scenario passing is what moves its parity row to `verified`.

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
