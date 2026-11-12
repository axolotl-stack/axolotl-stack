"""Run a `bds-sweep-client --script` scenario against plain BDS and record it.

    python tools/bds-sweep/script.py tools/bds-sweep/scripts/breaking.txt --out DIR

Starts BDS 1.26.51 (no Endstone) on a fresh superflat survival world in the
runtime of `BDS_RUNTIME`/`BDS_PORT` (operators by default, so the script's
commands run), lets `bds-sweep-client` play the script and records both
packet directions to `DIR/client-capture/packets.jsonl`. Run the same script
against unastar with `bds-sweep-client --addr <unastar> --script ...` (the
player must be an operator there) and compare the two captures. Raw output
stays outside the repository.
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
LEVEL = 'bds-script-1.26.51'
PLAYER = 'BDSSweep'


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('script', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--client', type=Path, default=REPO / 'target' / 'debug' / 'bds-sweep-client.exe')
    parser.add_argument('--timeout', type=float, default=600, help='seconds for the whole script')
    args = parser.parse_args()
    if not args.client.exists():
        parser.error(f'{args.client} missing: cargo build -p bds-sweep')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    world = bds_runtime.SERVER / 'worlds' / LEVEL
    if world.exists():
        shutil.rmtree(world)
    properties = bds_runtime.base_properties(
        LEVEL, level_type='FLAT', gamemode='survival', difficulty='peaceful',
        default_player_permission_level='operator', view_distance=6, tick_distance=4)
    command = [str(args.client), '--addr', bds_runtime.ADDR, '--name', PLAYER, '--script', str(args.script.resolve()),
               '--capture-dir', str(out / 'client-capture')]
    started = time.monotonic()
    try:
        with bds_runtime.Bds(properties, out / 'bds.log', endstone=False):
            with (out / 'client.log').open('a', encoding='utf-8') as log:
                client = subprocess.run(command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT,
                                        timeout=args.timeout)
    finally:
        if world.exists():
            shutil.rmtree(world)
    print(f'client exited {client.returncode} after {time.monotonic() - started:.0f}s -> {out}')
    return client.returncode


if __name__ == '__main__':
    sys.exit(main())
