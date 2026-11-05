---
name: parity-update
description: Update the vanilla-parity catalog (docs/parity) for a gameplay change. Use whenever you implement, fix or remove vanilla behaviour in unastar (entities, blocks, items, commands, protocol, worldgen, systems), when asked to claim an area, or before finishing any gameplay task — a feature is not done until its parity rows change in the same commit.
---

# Update parity rows

The catalog is the record of how far unastar is with every vanilla thing. Full field reference:
`docs/parity/README.md`. Never edit `docs/parity/PARITY.md` or `docs/parity/source/*` by hand.

## Steps

1. **Find the rows.** Search the domain file for the ids you touched:
   `rg -n '<id or family>' docs/parity/<domain>.toml`. Domains: `entities`, `blocks`, `items`,
   `commands`, `protocol`, `worldgen`, `systems`. Items and entities have sub-rows
   (`<item>/<facet>`, `<actor>/<component>`); update the sub-rows, not just the parent.
2. **Set the status honestly.**
   - `partial` needs `gaps`: what still doesn't work, in behaviour terms.
   - `done` needs `owner` and `tests` (`path` or `path::test_fn`) — a real test that exercises it.
   - `verified` also needs `evidence`: a committed BDS/Java capture or fixture the test compares against.
   - Work only on an unmerged branch goes in `notes`, not in `status`.
3. **Text rules.** `gaps`, `notes`, `evidence` describe behaviour. No BDS/client internal class,
   function or symbol names, addresses or offsets.
4. **Claims.** Starting work on an area? Add a `[[claim]]` to `docs/parity/claims.toml`
   (`area`, `owner`, `since` as a bare TOML date, `domain`, plus `families`/`ids`/`prefixes`).
   When an OpenSpec change does the work, set `change = "<change-name>"`; `parity check` then
   fails once that change is archived, so the claim is removed in the archiving commit.
   Overlapping claims fail the check.
5. **Check against the proposal.** If the work belongs to an OpenSpec change, its proposal lists
   the target rows under "Parity rows". Every listed row should have moved; for any that didn't,
   say why in the row's `gaps` and in the archive summary.
6. **Regenerate and check:**

   ```sh
   cargo run -p parity -- generate
   cargo run -p parity -- check
   ```

   Fix every failure; don't loosen a status to make the check pass.
7. Commit the TOML and regenerated `PARITY.md` together with the code change.

If you copied odd BDS behaviour to get the row to `done`, also run the `log-quirk` skill.
