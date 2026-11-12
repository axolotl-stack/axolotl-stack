"""BDS 1.26.51's answers to commands the main command capture does not cover.

`tools/bds-sweep/command-script.txt` drives one operator player; the
commands here answer differently on the console (owner level: `reloadconfig`,
`reloadpacketlimitconfig`, `wsserver`, `script`, `place`) or need a fresh
world. One session on a BDS slot (BDS_RUNTIME, BDS_PORT), a new flat world
(seed 12345) with the chunks around the origin kept loaded:

    run OUT --extractor EXE   console lines (CONSOLE), then an operator
                              player (PLAYER, bds-extractor --uuid); answers
                              in OUT/console.json and OUT/player
    depth OUT                 long /execute chains on the console (DEPTH),
                              BDS started again after each chain that makes
                              it exit; answers in OUT/depth.json
    fixture OUT               writes crates/unastar/src/command/exec/testdata/
                              bds-command-probe.json (console records, the
                              player's CommandOutput per command and the
                              chains, when OUT/depth.json exists)

BDS answers a CommandRequest packet longer than 550 bytes (a line of 508
characters from bds-extractor) with PacketViolationWarning "Server-bound
packet too large!" and disconnects the player, so the chains run on the
console.
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
    # Items /give refuses (testdata/bds-give.json, `refused`): in the
    # command's item list, not in it, and air.
    '/give @s chemistry_table',
    '/give @s colored_torch_bp',
    '/give @s colored_torch_rg',
    '/give @s element_0',
    '/give @s acacia_double_slab',
    '/give @s air',
    '/give @s minecraft:air',
    # Where a malformed JSON message's syntax error points.
    *('/tellraw @s ' + message for message in [
        '{bad json', '{bad json more words', '[1]', '[1,2,3]', '[{"text":"x"}]', '[]', '[ ]', '{', '[',
        '{}', 'x', '{"rawtext":', '{"rawtext":[}', '{"a" "b"}', '{"a":1 "b":2}', '{1}', '{:1}', '{ bad }',
        "{'a':1}", '{"x":1,}', '{"rawtext" [{"text":"x"}]}', '{"rawtext":[{"text":"x"]}',
        '{"rawtext":[{"text":x}]}', '{"rawtext":[{"text":"x"}]', '{"rawtext":tru}', '{"rawtext":null}',
        '{"rawtext":1x}', '{"rawtext":1.5e3}', '{"rawtext":[.5]}', '{"rawtext":[1 2]}', '{"rawtext":[]]}',
        '{"rawtext":"unterminated}', '{"rawtext":[{"text":"x",}]}', '{"rawtext":[{"text":"x"}],}',
        '{"rawtext":[{"text":"a b c d e f g h"} oops]}',
        '{"rawtext":[{"text":"x"}]}}', '{"rawtext":[{"text":"x"}]} junk', '{"rawtext":[{"text":"x"}]}{',
        '{"rawtext":[{"text":"x"}]}:', '{"rawtext":[{"text":"x"}]} ]', '{} {}',
        # Past the syntax check: JSON errors, or the message's own.
        '{"rawtext":true}', '{"rawtext":-1}', '{"rawtext":"a\\"b"}', '{"rawtext":{}}', '{"rawtext":[true]}',
        '{"rawtext":[1.]}', '{bad:1}', '{a1:1}', '{1a:1}', '{true:1}', '{"a":}', '{"rawtext":[1,]}',
        '{"rawtext":[,]}', '{"rawtext":[1,,2]}', '{"rawtext":[{"text":"x"},]}',
        '{"rawtext":[{text:"x"}]}', '{"rawtext":[{"text":[1,]}]}',
    ]),
    # Where other syntax errors point.
    '/testfor @e[type=cow,bogus=1]',
    '/kill @e[typ=cow]',
    '/give @s stone{x}',
    '/give @s stone[',
    '/give @s stone,',
    '/give @s {stone}',
    '/setblock 0 -60 0 stone ["a"=1]x',
]

# Long /execute chains, `prefix + (repeat + ' ') * count + end`. BDS exits
# on some (no answer); the others answer whatever their length.
_TEST = 'testforblock 0 -64 0 bedrock'
DEPTH = [
    {'prefix': '', 'repeat': 'execute run', 'count': count, 'end': _TEST} for count in (640, 641, 642, 643)
] + [
    {'prefix': 'execute ', 'repeat': repeat, 'count': count, 'end': 'run ' + _TEST}
    for repeat, counts in [('positioned ~ ~ ~', (710, 712)), ('align x', (700, 725)),
                           ('if block 0 -64 0 bedrock', (700, 750))]
    for count in counts
] + [
    {'prefix': 'execute ', 'repeat': 'positioned ~ ~ ~', 'count': 2000, 'end': 'bogus'},
    {'prefix': '', 'repeat': 'execute run', 'count': 800, 'end': 'bogus'},
    {'prefix': 'execute as @e[type=pig] ', 'repeat': 'positioned ~ ~ ~', 'count': 800, 'end': 'run ' + _TEST},
    {'prefix': 'execute if block 0 -64 0 air ', 'repeat': 'positioned ~ ~ ~', 'count': 800,
     'end': 'run ' + _TEST},
]


def chain_line(chain: dict) -> str:
    return chain['prefix'] + (chain['repeat'] + ' ') * chain['count'] + chain['end']


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


def depth(out: Path) -> None:
    """Each chain of DEPTH on the console; a chain that makes BDS exit is
    recorded with `exited` and BDS is started again for the next."""
    world = bds_runtime.SERVER / 'worlds' / LEVEL
    if world.exists():
        shutil.rmtree(world)
    results, pending, session = [], list(DEPTH), 0
    try:
        while pending:
            session += 1
            with level_data.start(LEVEL, out / f'depth-{session}.log') as bds:
                con = level_data.Console(bds)
                level_data.run_commands(con, ['tickingarea add 0 0 0 0 0 0 probe true'])
                con.wait_loaded([(0, -64, 0)])
                while pending:
                    chain = pending.pop(0)
                    try:
                        records = con.records(chain_line(chain), timeout=120)
                    except RuntimeError:
                        records = []
                    exited = bds.process.poll() is not None
                    results.append({**chain, 'records': [] if exited else records, 'exited': exited})
                    print(chain['repeat'], chain['count'], chain['end'], 'exited' if exited else records)
                    if exited:
                        break
    finally:
        (out / 'depth.json').write_text(json.dumps(results, indent=1) + '\n', encoding='utf-8')
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
    if (out / 'depth.json').exists():
        data['execute_depth'] = [
            {**chain, 'records': level_data.strip_banners(chain['records'])}
            for chain in json.loads((out / 'depth.json').read_text(encoding='utf-8'))
        ]
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    FIXTURE.write_text(json.dumps(redact(data), indent=1) + '\n', encoding='utf-8', newline='\n')
    print('fixture', FIXTURE, FIXTURE.stat().st_size, 'bytes')


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('step', choices=['run', 'depth', 'fixture'])
    parser.add_argument('out', type=Path)
    parser.add_argument('--extractor', type=Path)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    if args.step == 'run':
        if args.extractor is None:
            parser.error('run needs --extractor')
        run(out, args.extractor)
    elif args.step == 'depth':
        depth(out)
    else:
        fixture(out)
    return 0


if __name__ == '__main__':
    sys.exit(main())
