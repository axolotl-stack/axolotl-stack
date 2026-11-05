---
name: log-quirk
description: Record a BDS (or Java Edition worldgen) behaviour that unastar copies on purpose even though it looks permissive, odd or buggy. Use whenever you implement vanilla behaviour that surprised you, match a capture that contradicts the wiki or older servers, or find an approximation of Java worldgen.
---

# Log a copied quirk

Policy: unastar copies BDS 1.26.51 exactly, quirks included, and records each quirk so it can be
revisited one decision at a time (see `docs/decisions/`).

- Bedrock behaviour → `docs/quirks/bds.md`
- Java Edition 1.21.11 worldgen (Java-style generator) → `docs/quirks/java.md`

## Steps

1. Search first: `rg -n -i '<keyword>' docs/quirks/`. If a row exists, update it instead of adding one.
2. Add one row to the table, in the matching area group:

   | Area | Behaviour BDS shows | How it was observed | Why it looks odd | unastar status |
   |---|---|---|---|---|

   - **Behaviour**: what an outside observer sees, not how BDS does it internally.
   - **How it was observed**: a reproducible black-box source — capture path, fixture, sweep
     command, test name, with counts where you have them (e.g. "0 of 14 captured sessions").
   - **Why it looks odd**: what a reasonable implementer would expect instead.
   - **unastar status**: `Copied (<test path>)`, `Planned (<parity row id>)`, or `Tooling only`.
3. Never write BDS/client internal class, function or symbol names, addresses or offsets.
4. If the quirk is the reason a parity row could move to `done`/`verified`, reference the row's test.
