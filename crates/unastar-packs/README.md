# unastar-packs

Bevy-free behaviour-pack ingestion shared by datagen, build scripts, worldgen
codegen and startup loaders. The brarchive reader, comment-aware JSON parser
and versioned pack stack were moved from `unastar_bedrock_codegen` when actor
definitions became a second consumer; consumers must not keep private copies.

- `brarchive::{parse,read_file}` validates magic/version, entry-table bounds,
  UTF-8 names and payload bounds.
- `json::parse` accepts BOM and line/block comments outside string literals.
- `packs::PackStack::open(server_dir)` loads the BDS stack: definitions,
  vanilla, then numeric-version vanilla layers through 1.26.51.
- `PackStack::single_pack(name, root)` reads one pack, including the pinned
  bedrock-samples `behavior_pack` directory.
- `inputs(Family)` provides build-script dependencies; `files(Family)` gives
  raw entries with layer/origin; `resolve(Family)` is for identifier-keyed
  worldgen families. Path-keyed families must not use `resolve`.
- `actor::{load,load_value,resolve_stack}` returns typed actor envelopes,
  component/group JSON, typed properties and parsed event trees. Normalisation
  uses `unastar-rules`; component bodies keep arrays and goal priorities.
  `actor::diff` compares the resolved normalised definitions.
  Callers supply an `ActorParser`, assembled from canonical Mojang component
  schemas, `ActorDocument` member schemas and the legacy form index. Register
  addon component kinds before loading; unknown current-format component
  names reject the file. Schema-declared extras such as `upgrades` are retained.
  Legacy unknown bodies are retained with `ignored_components` metadata and
  must not become active ECS components.
  A component first listed by a newer stable catalog requires at least that
  `format_version`; names in the oldest pinned catalog, the legacy form index
  and registered addon kinds are available at every accepted format. The
  build's minimal registry serializes these gates (`x-introduced`), so the
  embedded parser enforces them. Legacy/addon registrations never relax a
  published gate. Pattern-named components remain ungated.

Parser-fidelity rules and evidence limits are documented next to each loader.
The vanilla integration suite checks every entity's normalised round-trip,
filter-name coverage, Molang parse/query coverage and the optional D9 BDS
install comparison. That comparison is a data check, not a substitute for
crafted-pack BDS acceptance and event-ordering probes.

```powershell
cargo test -p unastar-packs
$env:UNASTAR_BDS_DIR = 'C:/path/to/BDS-1.26.51'
cargo test -p unastar-packs --test vanilla bedrock_samples_match_the_bds_layer_stack
```

The samples submodule must be initialised. Without `UNASTAR_BDS_DIR`, only the
install comparison skips, with a message. Samples' 128 actor definitions
match BDS 1.26.51's layered files; BDS also has `minecraft:agent`.

`tools/bds_fidelity.py --generate NEW_DIR` emits a synthetic 60-actor pack
without starting a server. After reserving the machine-wide BDS slot, run
`python tools/bds_fidelity.py --harness PATH/TO/tools/bds-sweep --output NEW_DIR`
to exercise it through TOOLING's shared lifecycle helper. Scoreboard queries
record real event effects without a connected player; summon command results
record registration. The runner compares the observations with
`tests/fixtures/bds-1.26.51-fidelity.json`, including deferred event order.
The paired `bds-1.26.51-fidelity-inputs.json` contains our authored synthetic
inputs, not copied vanilla pack data. A Rust golden test compares all 60
accept/reject results and accepted-definition normalisation fixed points
against the captured matrix.
`--check-report PATH` checks an existing report without starting BDS.
`--only NAME,NAME` runs a subset during a short reserved slot.
It uses a disposable world and pack and restores `server.properties`; it never
starts Minecraft. The baseline accepts readable/compact aliases, all comparison
operators, null events and the malformed silverfish removal. Format 1.26.51
loads; 1.26.52 does not. Unknown component names warn through 1.21.80 and reject
from 1.21.90; unknown entity members reject, while top/description extras load.
An unknown event action prevents its known sibling actions from executing.
Compact numeric subjects 0–9 now normalise through the canonical generated
Mojang enum. Property defaults bind in their own query context; one bad default
disables the actor's entire property table but does not reject the actor.
Raw properties remain lossless; runtime consumers use
`Description::active_properties()`, and compiled defaults are retained in
`PropertyDef::default_expression` instead of reparsed on each spawn.
