# Proposal: bound command execution

## Why

`/execute` ran by recursion: each subcommand parsed and ran the rest of the chain and `run
execute …` went back through the dispatcher, so a long enough chain overflowed the thread's stack
and crashed the server. Commands round 3 left the depth as an owner decision. BDS 1.26.51 has no
limit either: a chain of about 641 nested runs or 711 `positioned` subcommands makes it exit.

Prior work: decision 0036 (command order) and the commands rows of `docs/parity/commands.toml`;
no `docs/solutions` note covers command recursion. The `server-authority` spec covers command
permission only.

## What Changes

- Measure the chains BDS answers and the ones it exits on (`tools/bds-sweep/command_probe.py
  depth`), and how its syntax check treats a long line.
- Check the whole line before anything runs, nested `run` lines included, with an explicit stack
  (BDS answers a syntax error at the end of a 2000-subcommand chain).
- Run chains and nested `/execute` lines on an explicit stack of run frames; refuse a chain past
  640 subcommands (decision 0058).
- Same pass, same parser: BDS's token-by-token check of `json` parameters and its syntax-error
  tokens (`{ } [ ] , : =`), measured with the command probe.

## Impact

- `crates/unastar/src/command/` (parser, `parser/json.rs`, `exec/execute.rs`, `mod.rs`,
  `context.rs`), the command probe and its fixture.
- Docs: decision 0058, quirks (execute depth, JSON messages, syntax tokens), parity rows
  `execute` and `tellraw`.
