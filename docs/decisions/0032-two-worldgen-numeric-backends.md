---
status: accepted
date: 2026-10-27
decision-makers: [project owner]
---

# Keep two worldgen numeric backends: calibrated and native Java-exact

## Context and Problem Statement

Bit-exact Java worldgen needs f32 spline evaluation in Java's operation order, unfused interpolation
and per-input interpolation. That costs speed and changes the output the Bedrock-style generator was
calibrated against.

## Considered Options

- One Java-exact backend for everything
- One fast backend, accepting Java mismatches
- Two backends: calibrated (fast f64, used by Bedrock-style) and native (Java-exact)

## Decision Outcome

Chosen option: two backends, because Java exactness and the calibrated Bedrock output cannot both be
kept by one numeric path.

### Consequences

- Good, because each generator gets the numerics its truth source needs.
- Bad, because there are two code paths to keep correct; the calibrated `eval_spline` clamps past the
  end points where Java extrapolates.

## More Information

`crates/unastar_noise/README.md`. Lesson: `docs/solutions/worldgen/spline-polynomial-lowering-is-not-java-bit-exact.md`.
Recorded on 2026-11-04 from commits of 2026-10-27; no earlier written decision exists.
