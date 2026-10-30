#!/usr/bin/env bash
# Runs the end-to-end chunk loading matrix for one server executable and
# appends every run to a JSON file (see tools/perf/README.md).
#
#   tools/perf/load_matrix.sh <server.exe> <label> <out.json> [generators] [quick]
#
# generators: "java" (default) or "java bedrock" (the executable must be
# built with bedrock-worldgen). "quick" runs only radius 12, 1 player.
set -euo pipefail
exe="$1"; label="$2"; out="$3"; generators="${4:-java}"; quick="${5:-}"
bench="${LOAD_BENCH:-target/release/examples/load_bench.exe}"

run() {
  echo "== $label $*" >&2
  "$bench" --server-exe "$exe" --label "$label" --json "$out" "$@" > /dev/null
}

for gen in $generators; do
  if [ "$quick" = quick ]; then
    run --generator "$gen" --radius 12 --players 1 --world fresh --fly-seconds 20
    run --generator "$gen" --radius 12 --players 1 --world generated
    continue
  fi
  for radius in 8 12 16; do
    run --generator "$gen" --radius "$radius" --players 1 --world fresh
    run --generator "$gen" --radius "$radius" --players 1 --world generated
  done
  run --generator "$gen" --radius 12 --players 1 --world fresh --fly-seconds 20
  run --generator "$gen" --radius 12 --players 8 --world fresh
  run --generator "$gen" --radius 12 --players 8 --world fresh --fly-seconds 20
  run --generator "$gen" --radius 12 --players 8 --world generated
done
