# Design: the enderman

## Context

Most of the enderman is random: where a teleport lands, when the next one comes, which block it
takes. Its replays (`mob/tests/enderman.rs`) compare distributions. Many seeded trials here are
set against BDS's counts with a two-proportion or Poisson z-test (3 standard errors), or a
two-sample Kolmogorov–Smirnov test. Deterministic parts (the cone, the step, the wire packets)
are compared exactly.

## Decisions

- **One attempt, many callers.** `mob::life::teleport::random_teleport` is the only attempt.
  The timer, the light and dark rolls, hurts (two attempts), dodges (up to 64) and the
  far-target fallback all call it; the far-target aim calls the same landing rule.
- **The look-at point** is a fitted fraction of the box height (2.55 / 2.9). The definition
  authors no `look_at_locations`, and the other vanilla user (the creaking) authors fields this
  change rejects.
- **The cone** comes from the authored `field_of_view` (`1 - cos(fov / 2)`), which the
  measurement confirms for 26°. Only the narrowing cone is accepted.
- **The carried block** lives in `MobEquipment`'s main hand (decision 0066).
- **Teleports on the wire.** `MobNetState::teleported` makes the next `MoveActorDelta` go to
  every viewer at once with `force_move`, bypassing the bandwidth cadence, as BDS sends it.

## Risks

- The dodge's 64 tries and the far-target jitter and fallback are provisional: BDS shows at
  least about 5 tries and six aimed landings.
- The test arena has no rain, so the rain items are not replayed.
