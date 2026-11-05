---
title: Turning router splines into f64 Horner polynomials is fast, but it does not give Java's bits
area: worldgen
tags: [spline, density-function, f32, precision, native, aot]
date: 2024-12-30
origin: thoughts/shared/plans/2024-12-30-density-function-aot-compilation.md (removed; see git history)
status: current
---

## Context
Router splines (offset, factor, jaggedness) are evaluated for every column. The first AOT
plan precomputed each Hermite segment as an f64 cubic `a + b*t + c*t^2 + d*t^3`
(`b = d0*dt`, `c = 3(v1-v0) - 2*d0*dt - d1*dt`, `d = 2(v0-v1) + d0*dt + d1*dt`). It
evaluated that cubic with Horner's method and used a binary search over `const`
segment arrays for 5+ segments. Its checks only compared the cubic with our own f64
Hermite form.

## What we learned
- That lowering is still the **calibrated** backend. It is mathematically equal to Java's
  spline but not bit-equal, for three reasons:
  1. Java evaluates splines in `f32`: locations, values, derivatives and the coordinate are
     all floats.
  2. Java computes a segment as `lerp(t, v0, v1) + t*(1-t)*(a + t*(b-a))` with
     `a = d0*w - (v1-v0)` and `b = -d1*w + (v1-v0)`. Folding that into polynomial
     coefficients, or keeping f64 intermediates, changes the rounding.
  3. Outside the first and last points Java extrapolates linearly,
     `value + derivative * (coord - location)` (unless the derivative is 0). The calibrated
     `eval_spline` clamps to the first or last value.
- Nested splines must stay inside the interval that selected them. Flattening nested
  splines into one segment table loses this.
- For native Java parity we emit a second lowering: f32 literals, Java's operation order
  (`native_density::spline_segment`), no reassociation. The calibrated backend keeps the
  polynomial form, so its terrain output is unchanged.

## Where it lives
- `crates/unastar_noise/codegen/emitter/emitter_quote.rs` (`emit_java_spline` native;
  `SplineSegment::from_hermite`, `eval_spline` calibrated)
- `crates/unastar_noise/src/native_density.rs` (`spline_segment`)
