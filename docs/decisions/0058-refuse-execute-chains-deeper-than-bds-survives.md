---
status: accepted
date: 2026-11-12
decision-makers: [project owner]
---

# Refuse `/execute` chains deeper than BDS survives

## Context and Problem Statement

unastar ran `/execute` by recursion: each subcommand parsed and ran the rest of the chain, and
`run execute …` went through the dispatcher again. A long enough chain overflowed the thread's
stack and crashed the server. What should a long chain do?

Measured on BDS 1.26.51 (console, a new flat world; `tools/bds-sweep/command_probe.py depth`,
fixture `crates/unastar/src/command/exec/testdata/bds-command-probe.json`, `execute_depth`):

- BDS has no limit. A chain whose contexts reach the end makes BDS exit with no answer: 641
  nested `execute run` exit, 640 answer; 712 `positioned ~ ~ ~` exit, 710 answer; `align x`
  answers at 700 and exits at 725; `if block` answers at 700 and exits at 750. The depth it dies
  at depends on the subcommand, not on the line's length (a 30000-character `say` answers).
- The whole line is checked before anything runs, nested `run` lines included: 2000 chained
  subcommands or 800 nested `execute run` ending in `bogus` answer `commands.generic.syntax` at
  `bogus`.
- A chain that runs out of contexts stops there at any length (`as @e[type=pig]` with no pig, or
  a false `if block`, before 800 subcommands answer as usual).
- A player cannot send such a line: BDS answers a CommandRequest packet longer than 550 bytes
  with `PacketViolationWarning` "Server-bound packet too large!" and disconnects the player.
  Long chains come from the console, functions and command blocks.

Copying BDS exactly would mean exiting the server.

## Considered Options

- Refuse a chain at 640 subcommands, the deepest every measured chain survives.
- Refuse at a depth per subcommand kind (710 for `positioned`, 640 for nested runs, ...).
- No limit: run chains of any depth without recursion.

## Decision Outcome

Accepted by the owner on 2026-11-11.

Chosen: refuse at 640. The parser and the chain runner keep their own stacks (no recursion), so
nothing depends on the thread's stack. A chain counts every subcommand it runs through, the
subcommands of the `/execute` lines it runs included; when a chain with at least one context
would run its 641st, the whole command answers one unastar failure ("/execute chains deeper than
640 subcommands are refused by unastar (BDS 1.26.51 exits)") and nothing else, where BDS would
have exited. Chains that leave no context, and lines that do not parse, answer as BDS does at
any length.

### Consequences

- Good: every chain BDS answers for every measured subcommand is answered the same; nothing an
  operator types can crash the server.
- Bad: `positioned`, `align` and `if block` chains of 641 to about 710 subcommands, which BDS
  answers, are refused.
- Neutral: per-kind limits would copy more answers but would copy stack frame sizes, which change
  with every BDS build.

## More Information

Quirk: [docs/quirks/bds.md](../quirks/bds.md), "Commands". Parity row: `execute` in
[docs/parity/commands.toml](../parity/commands.toml).
