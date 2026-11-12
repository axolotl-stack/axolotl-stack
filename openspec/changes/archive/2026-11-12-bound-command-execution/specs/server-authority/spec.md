# Spec Delta

## ADDED Requirements

### Requirement: Command lines cannot exhaust the stack
Parsing a command line and running it SHALL NOT recurse per subcommand or per nested command: the parser MUST check a line of any length, nested `run` lines included, with a stack of its own, and `/execute` MUST run its chain and the `/execute` lines it runs on a stack of its own. A chain whose contexts would run through more than 640 subcommands, counting those of the `/execute` lines it runs, MUST answer one failure and nothing else. See decision 0058.

#### Scenario: A chain past the limit
- **WHEN** the console runs `execute` followed by 641 nested `execute run` and `testforblock 0 -64 0 bedrock`
- **THEN** the only answer is the unastar failure for chains deeper than 640 subcommands, and the server keeps running

#### Scenario: A long line that does not parse
- **WHEN** the console runs `execute` with 2000 `positioned ~ ~ ~` subcommands ending in `bogus`
- **THEN** the answer is `commands.generic.syntax` at `bogus`, as BDS 1.26.51 answers

Evidence: crates/unastar/src/command/parser.rs (`check_tails`), crates/unastar/src/command/exec/execute.rs (`run`, `MAX_DEPTH`, tests `long_chains_answer_like_bds`, `the_limit_counts_every_subcommand`), crates/unastar/src/command/exec/testdata/bds-command-probe.json (`execute_depth`), [decision 0058](../../../../../../docs/decisions/0058-refuse-execute-chains-deeper-than-bds-survives.md)
