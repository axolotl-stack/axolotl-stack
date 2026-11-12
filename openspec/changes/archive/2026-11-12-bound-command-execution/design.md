# Design: bound command execution

## Parsing

`parse_overloads` matches one overload at a time. A last parameter that hands the rest of the
line on (a chained subcommand, or `run`'s command) returns it as a tail instead of recursing;
`check_tails` tries the tails with a stack of frames (one per command and start position, failures
memoised), so a frame that matches to the end of the line makes the line valid and a failing frame
hands its furthest position to the frame below. At run time the next subcommand is parsed without
checking its tail again (`parse_checked_overloads`).

## Running

`execute::run` drives a loop: `run_chain` runs subcommands one after the other until the chain
ends or reaches `run`, which becomes a `RunFrame` (contexts left, the command, the answer so far).
The top frame runs its command for its next context: a nested `/execute` starts a chain of the
same loop, anything else runs through the registry. Every context carries `depth`, the subcommands
it came through; a chain with contexts past `MAX_DEPTH` (640) ends the whole command with one
failure, as BDS answers nothing when it exits.

## Alternatives

Per-subcommand limits and no limit at all: decision 0058.
