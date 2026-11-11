"""PvP race and packet-order captures on plain BDS 1.26.51.

Plays a script with `order_probe` (three players in one process, see
`crates/bds-sweep/src/bin/order_probe.rs`) against a new flat survival world
in which every player joins as an operator, and keeps the probe's
`events.jsonl` with the script and the BDS log:

    BDS_RUNTIME=... BDS_PORT=... python pvp_race.py run SCRIPT [--out DIR]
    python pvp_race.py script CASE > SCRIPT     (scripts of the race matrix)

Output goes to `<runtime>/captures/1.26.51/pvp-race-<UTC>/` unless `--out`
is given. Black-box only: everything recorded is what the server sent.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
LEVEL = 'pvp-race-1.26.51'
WORLD = dict(level_type='FLAT', level_seed=12345, gamemode='survival', difficulty='normal',
             default_player_permission_level='operator', max_players=3)
PROBE = REPO / 'target' / 'release' / 'order_probe.exe'


def run(script: Path, out: Path, timeout: int, join_order: str = 'ABC',
        extra: dict[str, str] | None = None) -> int:
    out.mkdir(parents=True)
    shutil.copyfile(script, out / 'script.txt')
    properties = bds_runtime.base_properties(LEVEL, **WORLD)
    properties.update(extra or {})
    command = [str(PROBE), '--addr', bds_runtime.ADDR, '--script', str(out / 'script.txt'),
               '--out', str(out), '--join-order', join_order]
    (out / 'session.json').write_text(json.dumps({
        'bds_version': bds_runtime.EXPECTED_VERSION,
        'server': 'bedrock_server.exe (no Endstone)',
        'server_properties': properties,
        'probe_command': ['order_probe', *command[1:]],
    }, indent=2) + '\n', encoding='utf-8')
    world = bds_runtime.SERVER / 'worlds' / LEVEL
    if world.exists():
        shutil.rmtree(world)
    try:
        with bds_runtime.Bds(properties, out / 'bds.log', endstone=False):
            with (out / 'probe.log').open('w', encoding='utf-8') as log:
                try:
                    code = subprocess.run(command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT,
                                          timeout=timeout).returncode
                except subprocess.TimeoutExpired:
                    code = 124
    finally:
        if world.exists():
            shutil.rmtree(world)
    return code


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest='command', required=True)
    run_parser = sub.add_parser('run')
    run_parser.add_argument('script', type=Path)
    run_parser.add_argument('--out', type=Path)
    run_parser.add_argument('--timeout', type=int, default=1800)
    run_parser.add_argument('--join-order', default='ABC')
    run_parser.add_argument('--property', action='append', default=[], metavar='KEY=VALUE',
                            help='extra server.properties value (repeatable)')
    script_parser = sub.add_parser('script')
    script_parser.add_argument('case')
    script_parser.add_argument('--trials', type=int, default=10)
    args = parser.parse_args()
    if args.command == 'script':
        import pvp_race_scripts
        sys.stdout.write(pvp_race_scripts.script(args.case, args.trials))
        return 0
    if not PROBE.exists():
        parser.error(f'{PROBE} missing: cargo build --release -p bds-sweep --bin order_probe')
    stamp = dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    out = (args.out or bds_runtime.CAPTURES / f'pvp-race-{stamp}').resolve()
    extra = dict(item.split('=', 1) for item in args.property)
    code = run(args.script.resolve(), out, args.timeout, args.join_order, extra)
    print(f'probe exit {code}; output {out}')
    return code


if __name__ == '__main__':
    sys.exit(main())
