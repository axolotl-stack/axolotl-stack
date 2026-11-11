"""BDS 1.26.51's answers to commands the main command capture does not cover.

`tools/bds-sweep/command-script.txt` drives one operator player; the
commands here answer differently on the console (owner level: `reloadconfig`,
`reloadpacketlimitconfig`, `wsserver`, `script`, `place`) or need a fresh
world. One session on a BDS slot (BDS_RUNTIME, BDS_PORT), a new flat world
(seed 12345) with the chunks around the origin kept loaded:

    run OUT --extractor EXE   console lines (CONSOLE), then an operator
                              player (PLAYER, bds-extractor --uuid); answers
                              in OUT/console.json and OUT/player
    fixture OUT               writes crates/unastar/src/command/exec/testdata/
                              bds-command-probe.json (console records and the
                              player's CommandOutput per command)
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import level_data  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
FIXTURE = REPO / 'crates' / 'unastar' / 'src' / 'command' / 'exec' / 'testdata' / 'bds-command-probe.json'
LEVEL = 'command-probe'
PLAYER_UUID = '6c0ffee0-5eed-4a11-9d00-0000000000c3'

CONSOLE = [
    'tickingarea add 0 0 0 47 0 47 probe true',
    '!wait-loaded',
    'reloadconfig',
    'reloadpacketlimitconfig',
    'wsserver ws://127.0.0.1:1',
    'wsserver out',
    'script debugger listen 19399',
    'script debugger close',
    'script debugger connect',
    'script profiler start',
    'script profiler stop',
    'script diagnostics startcapture',
    'script diagnostics stopcapture',
    'packstack resource',
    'packstack server',
    'packstack client',
    'packstack server verbose',
    'packstack server verbose exclude-vanilla',
    'place feature minecraft:no_such_feature',
    'place structure minecraft:no_such_structure 0 -60 0',
    'place jigsaw minecraft:no_such_pool minecraft:bottom 2 0 -60 0',
    'locate biome plains',
    'locate biome minecraft:plains',
    'locate biome minecraft:desert',
    'locate structure village',
    'ride @e[type=cow] stop_riding',
]

PLAYER = [
    '/locate biome plains',
    '/locate biome minecraft:plains',
    '/locate biome minecraft:desert',
    '/locate structure village',
    '/locate structure village true',
    '/summon pig 3 -60 3',
    '/ride @s start_riding @e[type=pig,c=1]',
    '/ride @s stop_riding',
    '/ride @e[type=pig,c=1] evict_riders',
    '/ride @s summon_ride horse',
    '/ride @s stop_riding',
    '/packstack server',
    '/packstack client verbose',
    '/place feature minecraft:no_such_feature',
    '/wsserver ws://127.0.0.1:1',
    '/reloadconfig',
]


def run(out: Path, extractor: Path) -> None:
    world = bds_runtime.SERVER / 'worlds' / LEVEL
    if world.exists():
        shutil.rmtree(world)
    try:
        with level_data.start(LEVEL, out / 'probe.log') as bds:
            console = level_data.run_commands(level_data.Console(bds), CONSOLE)
            (out / 'console.json').write_text(json.dumps(console, indent=1) + '\n', encoding='utf-8')
            level_data.run_player(extractor, out, 'player', PLAYER, uuid=PLAYER_UUID)
    finally:
        if world.exists():
            shutil.rmtree(world)


def redact(value):
    """Local user paths BDS prints (the diagnostics capture file) as
    `<home>/...`."""
    if isinstance(value, str):
        return re.sub(r'[A-Za-z]:/Users/[^/]+/', '<home>/', value)
    if isinstance(value, list):
        return [redact(v) for v in value]
    if isinstance(value, dict):
        return {k: redact(v) for k, v in value.items()}
    return value


def fixture(out: Path) -> None:
    data = {
        'source': 'BDS 1.26.51, a new flat world (seed 12345): console lines, then an operator player',
        'generator': 'tools/bds-sweep/command_probe.py',
        'console': level_data.answers(out / 'console.json'),
        'player': level_data.outputs(out / 'player'),
    }
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    FIXTURE.write_text(json.dumps(redact(data), indent=1) + '\n', encoding='utf-8', newline='\n')
    print('fixture', FIXTURE, FIXTURE.stat().st_size, 'bytes')


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('step', choices=['run', 'fixture'])
    parser.add_argument('out', type=Path)
    parser.add_argument('--extractor', type=Path)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    if args.step == 'run':
        if args.extractor is None:
            parser.error('run needs --extractor')
        run(out, args.extractor)
    else:
        fixture(out)
    return 0


if __name__ == '__main__':
    sys.exit(main())
