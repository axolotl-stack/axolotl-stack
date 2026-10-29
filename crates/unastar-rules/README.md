# unastar-rules

Bevy-free Bedrock rule parsing/evaluation shared by runtime, pack ingestion and
worldgen codegen (D10). Names are resolved before evaluation where possible;
the calling entity/item/world layer supplies facts and queries.

## Filters

`Filter::parse` accepts readable/compact groups, operator aliases, subjects and
observed integer encodings. `to_json` writes the canonical form. A
`FilterRegistry` maps names to `TestId`s; `FilterContext::fact` supplies boolean,
numeric or matching facts. Missing subjects return `None` and fail the test,
even with a negative operator. Unknown names are diagnostic and evaluate false.

`Subject` is schema-generated from the pinned Mojang `FilterSubject.json`;
numeric codes 0–9 follow its canonical enum order rather than a handwritten
mapping. Initialise `crates/unastar-data/data/vanilla_bp` before building this
crate. Only the schema type is emitted into `OUT_DIR`, not committed pack data.

A name having a built-in registry row is not evidence that every ECS context
implements it. `unastar-packs/tests/vanilla.rs` gates parsed-name coverage;
the BDS fidelity fixture proves compact spellings, comparison aliases and
unknown-filter false behaviour. Owning contexts must separately prove facts
and subject resolution, including non-actor subjects.

## Events

`EventNode` parses sequence/randomize/first_valid trees and leaf actions.
Evaluation returns ordered borrowed effects and deferred triggers; entity
composition applies these in a batch. The silverfish malformed `remove` is
accepted with a diagnostic and removes no group. BDS's synthetic pack confirms
sequence A, weighted B, first-valid C and deferred trigger D, without running
the second first-valid child. Unknown action keys are retained verbatim and
diagnosed, but invalidate their node: known sibling actions and triggers do
not execute either. This matches the isolated BDS unknown-action probe.

`EventTree` and `evaluate_tree` share that traversal with startup-bound entity
events. Leaf emission stays caller-owned; reusable scratch holds random-child
selection without retaining JSON in a bound tree. Typed runtime filter failures
propagate to the caller. Trigger filters remain deferred for an explicit caller
check; timing relative to ECS composition is not inferred from tree evaluation.

## Molang

`molang::parse` is the single lexer/parser. It normalises case and q./v./t./c.
aliases and supports the vanilla entity/item expressions, including strings,
ternaries, variables, math, queries and context-reference `->`.
`Expr::bind(&registry, QueryContext::{Runtime,PropertyDefault})` resolves names
before evaluation and enforces context availability: `had_component_group`
is property-default-only, while live actor queries cannot run in defaults.
The runtime evaluator uses caller `MolangContext`; the worldgen compiler
consumes the same AST and retains its build-only Rust emitter. Worldgen stays
ahead-of-time compiled, not interpreted.

## Player Drop direction

`math::player_throw_direction(yaw, pitch)` returns a scalar `[f32; 3]` without
ECS or vector-library dependencies. A build-time 65,536-entry sine table
inverts the rounded f32 lookup scale; using `index * TAU / 65536` or a Java
f64-sampled table changes captured negative-angle impulses. Runtime evaluation
does not allocate or evaluate transcendental functions.

The consumer's native fixture retains 49 action/timing cases plus 16 additional
angle controls. Its regression and an executable ECS constructor smoke cover
19 distinct stationary-source orientations. Other actor look-vector paths,
moving-source inheritance and native RNG seed matching are not calibrated by
these captures. Molang and Java worldgen math are intentionally unchanged.


```powershell
cargo test -p unastar-rules
cargo clippy -p unastar-rules --all-targets --all-features -- -D warnings
```

## Loot

`loot::decode_table(name, source, &Value)` decodes canonical Bedrock JSON into
owned typed data. `LootTables` accepts rows/overrides before it is shared
immutably. `loot::roll(&tables, &table, &dyn LootContext, &mut impl rand::Rng)`
returns owned `LootDrop { item_id, count, data, nbt }`; callers construct stacks
through their item registry. `LootContext` supplies tool identity, pre-resolved
enchantment ids, block-state matching, player-kill and looting facts. No ECS or
item-stack implementation is imported here.

Each `LootFunction` retains its typed `conditions` and `kind`; guards run before
the action accesses context or consumes its own random draws. The implemented
`entity_properties` subset is THIS actor's typed `on_fire` fact; no THIS actor
does not match either fire state. Other predicates remain explicit unsupported
features, not discarded clauses. `FurnaceSmelt` supplies a borrowed canonical
recipe identifier/aux result, while `furnace_smelt` preserves the loot count.
A missing provider returns `MissingContext`; a provider's recipe miss leaves the
original item unchanged. The entity/item layer owns the actual frozen recipe
registry and death context; no cooked-item-by-mob table is maintained here.

The parked evaluator's seven regression scenarios still run in unastar with
the real pack/block tables. Unsupported functions/conditions, tiered equipment
pools and entry-level nested pools are retained in decoded data and fail
explicitly when rolled. This is the E0 relocation/data cutover, not a claim
of I11's complete Bedrock loot semantics or MT19937 draw-order parity.
