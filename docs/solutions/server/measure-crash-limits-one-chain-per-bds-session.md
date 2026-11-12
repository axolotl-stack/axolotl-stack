---
title: Measure where BDS crashes one case per session, and test depth on a test thread
area: server
tags: [commands, execute, recursion, bds-sweep, stack]
date: 2026-11-12
origin: commands round 4 (decision 0058)
status: current
---

## Context

`/execute` chains recursed in unastar and could overflow the stack. To copy BDS's limit we had
to find where BDS stops, and BDS has no limit: it exits.

## What we learned

- A case that makes BDS exit takes the rest of the session with it. `command_probe.py depth`
  runs the cases in ascending order and starts BDS again after each exit, recording `exited`;
  bisect coarse first (each exit costs a restart of about ten seconds).
- Separate depth from line length: a short subcommand (`align x`) at the same depth crashed too,
  and a long line that parses but leaves no context (`as @e[type=pig]`) did not.
- A player cannot send long chains at all: a CommandRequest over 550 bytes is a packet violation
  and a disconnect, so depth measurements go through the console.
- Cargo test threads have a 2 MiB stack and debug frames are large: 640 nested `execute run`
  overflowed there even though production might survive. A depth test on a normal test thread is
  the cheapest proof that nothing recurses; keep explicit stacks (parser tails, run frames).

## Where it lives

`tools/bds-sweep/command_probe.py` (`depth`), `crates/unastar/src/command/parser.rs`
(`check_tails`), `crates/unastar/src/command/exec/execute.rs` (`run`, `MAX_DEPTH`, module
`depth`).
