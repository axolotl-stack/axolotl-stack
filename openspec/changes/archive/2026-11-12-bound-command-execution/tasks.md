# Tasks

## 1. Measure

- [x] 1.1 Add `depth` to `tools/bds-sweep/command_probe.py` (one chain per line, BDS started again after it exits) and the `/give` and `/tellraw` lines; run it on a BDS slot and write the fixture; verify `execute_depth` holds 640 answered and 641 exited nested runs

## 2. Implement

- [x] 2.1 Check tails with an explicit stack; verify a 2000-subcommand chain answers BDS's syntax error
- [x] 2.2 Run chains and nested `/execute` lines on run frames with `MAX_DEPTH`; verify `long_chains_answer_like_bds` and `the_limit_counts_every_subcommand` pass on a test thread's stack
- [x] 2.3 BDS's JSON check and syntax-error tokens; verify `json_messages_answer_like_bds` and `syntax_errors_point_at_the_token_bds_shows` pass

## 3. Record

- [x] 3.1 Decision 0058, quirks, parity rows `execute` and `tellraw`; verify `cargo run -p parity -- check`
