---
status: accepted
date: 2024-12-30
decision-makers: [project owner]
---

# Compile density functions to Rust at build time instead of interpreting them

## Context and Problem Statement

Java-style terrain is defined by a tree of density functions. Evaluating that tree at runtime
dominated chunk generation: about 69% of chunk time went to walking it (≈27 ms per chunk), with
~12,000 arena allocations and the same subtree evaluated 100+ times.

## Considered Options

- Trait objects (`Box<dyn DensityFunction>`)
- A boxed enum tree
- An index arena (`DensityArena`)
- Ahead-of-time compilation to straight-line Rust, with structural deduplication

## Decision Outcome

Chosen option: AOT compilation, because LLVM can inline and vectorise across nodes only when the
tree is code, and deduplicating identical subtrees only pays off once they are compiled. The
arena was removed rather than kept as a fallback.

### Consequences

- Good, because the density path stopped being the bottleneck.
- Bad, because changing worldgen data means a rebuild, and generated code is harder to debug.

## More Information

Code: `crates/unastar_noise/codegen/analyzer/`, `codegen/emitter/emitter_quote.rs`. Lessons:
`docs/solutions/worldgen/density-trees-must-be-compiled-and-deduplicated.md`,
`docs/solutions/worldgen/density-cache-markers-map-to-distinct-aot-scopes.md`. Extended to all of
worldgen by 0017.
