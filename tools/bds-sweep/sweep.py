"""T2: run a BDS sweep job (Endstone plugin + scripted client) on loopback.

    python tools/bds-sweep/sweep.py drops --blocks slice --samples 200 --out DIR
    python tools/bds-sweep/sweep.py drops --blocks all --states all --out DIR
    python tools/bds-sweep/sweep.py armor-slots --out DIR
    python tools/bds-sweep/sweep.py fuel --out DIR

Starts BDS 1.26.51 under Endstone on a fresh superflat creative world
(`bds-sweep-1.26.51`, peaceful, operators by default), launches
`bds-sweep-client` (cargo build -p bds-sweep) as the sweep player and waits
for the plugin (`endstone-sweep`, installed editable into the shared venv) to
write `done.json` or `error.txt`. Results are appended to
`<out>/results.jsonl`; rerunning with the same `--out` skips finished items,
so a sweep can be interrupted and resumed. Raw results stay out of the
repository; `summarize.py` turns them into measured data.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import native  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
LEVEL = 'bds-sweep-1.26.51'
PLAYER = 'BDSSweep'
# Superflat: grass top at y=-61, so the player stands at y=-60.
# `miner` = x, y, z, yaw, pitch: facing east (yaw -90) at the west face (4) of
# `mine_block`, which is at eye level (eyes at y + 1.62).
ARENA = {'player': [0.5, -60, 0.5], 'block': [3, -60, 0], 'furnaces': [-8, -60, 6],
         'miner': [0.5, -60, 0.5, -90, 0], 'mine_block': [1, -59, 0], 'mine_face': 4}
SETUP = [
    'gamerule domobspawning false',
    'gamerule dodaylightcycle false',
    'gamerule doweathercycle false',
    'gamerule randomtickspeed 0',
    'gamerule sendcommandfeedback false',
    'gamerule commandblockoutput false',
    'gamerule domobloot true',
    'gamerule dotiledrops true',
    'time set 6000',
    'weather clear',
    f'gamemode creative "{PLAYER}"',
]
TIERS = ('wooden', 'stone', 'copper', 'iron', 'golden', 'diamond', 'netherite')
DIGGERS = ('pickaxe', 'axe', 'shovel', 'hoe', 'sword')
SLICE_BLOCKS = {
    'minecraft:stone': {},
    'minecraft:diamond_ore': {},
    'minecraft:coal_ore': {},
    'minecraft:gravel': {},
    'minecraft:oak_leaves': {'persistent_bit': False, 'update_bit': False},
    'minecraft:glass': {},
    'minecraft:grass_block': {},
    'minecraft:wheat': {'growth': 7},
    'minecraft:oak_log': {'pillar_axis': 'y'},
    'minecraft:web': {},
}
SLICE_ITEMS = ['minecraft:diamond_helmet', 'minecraft:iron_chestplate', 'minecraft:leather_leggings',
               'minecraft:netherite_boots', 'minecraft:carved_pumpkin', 'minecraft:elytra',
               'minecraft:turtle_helmet', 'minecraft:shield', 'minecraft:totem_of_undying', 'minecraft:stick']
SLICE_FUELS = ['minecraft:coal', 'minecraft:charcoal', 'minecraft:oak_planks', 'minecraft:stick',
               'minecraft:lava_bucket', 'minecraft:blaze_rod', 'minecraft:coal_block', 'minecraft:stone']


def tool(kind: str, tier: str, **enchants: int) -> dict:
    spec = {'item': f'minecraft:{tier}_{kind}'}
    if enchants:
        spec['enchants'] = {f'minecraft:{name}': level for name, level in enchants.items()}
    return spec


def tool_key(spec: dict | None) -> str:
    if not spec:
        return 'hand'
    key = spec['item'].removeprefix('minecraft:')
    for name, level in sorted((spec.get('enchants') or {}).items()):
        key += f'+{name.removeprefix("minecraft:")}{level}'
    return key


def tools_for(block: str, mining: dict) -> list[dict | None]:
    """Hand, every tier of each correct digger, one wrong digger, shears,
    and silk touch / fortune I-III on the diamond correct tool."""
    facts = mining.get(block, {'effective': [], 'harvest': []})
    kinds = [k for k in DIGGERS if k in facts['effective'] or k in facts['harvest']]
    tools: list[dict | None] = [None]
    tools += [tool(kind, tier) for kind in kinds for tier in TIERS]
    wrong = next(k for k in DIGGERS if k not in kinds)
    tools.append(tool(wrong, 'diamond'))
    tools.append({'item': 'minecraft:shears'})
    best = kinds[0] if kinds else 'pickaxe'
    tools.append(tool(best, 'diamond', silk_touch=1))
    tools += [tool(best, 'diamond', fortune=level) for level in (1, 2, 3)]
    return tools


def state_key(block: str, states: dict) -> str:
    inner = ','.join(f'{k}={json.dumps(v)}' for k, v in sorted(states.items()))
    return f'{block}[{inner}]'


def drops_items(args) -> list[dict]:
    mining = native.mining()
    if args.blocks == 'slice':
        cells = list(SLICE_BLOCKS.items())
    else:
        palette = native.block_states()
        names = sorted({e['name'] for e in palette}) if args.blocks == 'all' else args.blocks.split(',')
        wanted = set(names)
        if args.states == 'all':
            cells = [(e['name'], e['states']) for e in palette if e['name'] in wanted]
        else:
            first = native.default_states()
            cells = [(name, first[name]) for name in names]
    items = []
    for block, states in cells:
        if block == 'minecraft:air':
            continue
        for spec in tools_for(block, mining):
            items.append({'key': f'{state_key(block, states)}|{tool_key(spec)}', 'block': block,
                          'states': states, 'tool': spec, 'samples': args.samples})
    return items


def item_names(spec: str, slice_: list[str]) -> list[str]:
    if spec == 'slice':
        return slice_
    if spec == 'all':
        return [name for name in native.items() if name != 'minecraft:air']
    return spec.split(',')


def mining_items(args) -> list[dict]:
    """Hand, every tier of each correct digger, shears, efficiency I-V on the
    diamond correct tool, and haste I/II and mining fatigue I with it."""
    facts = native.mining()
    cells = list(SLICE_BLOCKS.items()) if args.blocks == 'slice' else [
        (name, native.default_states()[name]) for name in args.blocks.split(',')]
    items = []
    for block, states in cells:
        kinds = [k for k in DIGGERS if k in facts.get(block, {}).get('effective', [])]
        best = kinds[0] if kinds else 'pickaxe'
        variants: list[tuple[dict | None, list[str], str]] = [(None, [], '')]
        variants += [(tool(kind, tier), [], '') for kind in kinds for tier in TIERS]
        variants.append(({'item': 'minecraft:shears'}, [], ''))
        variants += [(tool(best, 'diamond', efficiency=level), [], '') for level in range(1, 6)]
        for effect, amplifier in (('haste', 0), ('haste', 1), ('mining_fatigue', 0)):
            variants.append((tool(best, 'diamond'), [f'effect {{player}} {effect} 1000 {amplifier} true'],
                             f'+{effect}{amplifier + 1}'))
        for spec, commands, suffix in variants:
            items.append({'key': f'{state_key(block, states)}|{tool_key(spec)}{suffix}', 'block': block,
                          'states': states, 'tool': spec, 'commands': commands, 'max_ticks': args.mine_ticks})
    return items


def build_job(args, out: Path) -> dict:
    job = {'kind': args.kind.replace('-', '_'), 'player': PLAYER, 'output_dir': str(out),
           'arena': ARENA, 'setup_commands': SETUP}
    if args.kind == 'drops':
        job['items'] = drops_items(args)
    elif args.kind == 'mining':
        job['items'] = mining_items(args)
    elif args.kind == 'armor-slots':
        job['items'] = [{'key': name, 'item': name} for name in item_names(args.items, SLICE_ITEMS)]
    elif args.kind == 'fuel':
        job.update(furnace='furnace', lit_furnace='minecraft:lit_furnace', input='cobblestone',
                   max_ticks=args.max_ticks,
                   items=[{'key': name, 'item': name} for name in item_names(args.items, SLICE_FUELS)])
    return job


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('kind', choices=['drops', 'mining', 'armor-slots', 'fuel'])
    parser.add_argument('--out', type=Path)
    parser.add_argument('--blocks', default='slice', help="'slice', 'all' or comma-separated identifiers")
    parser.add_argument('--states', default='first', choices=['first', 'all'])
    parser.add_argument('--items', default='slice', help="'slice' or comma-separated identifiers")
    parser.add_argument('--samples', type=int, default=200)
    parser.add_argument('--max-ticks', type=int, default=24000, help='fuel: longest burn to wait for')
    parser.add_argument('--mine-ticks', type=int, default=600, help='mining: longest break to wait for')
    parser.add_argument('--client', type=Path, default=REPO / 'target' / 'debug' / 'bds-sweep-client.exe')
    parser.add_argument('--timeout', type=float, default=6 * 3600, help='seconds for the whole job')
    args = parser.parse_args()
    if not args.client.exists():
        parser.error(f'{args.client} missing: cargo build -p bds-sweep')
    check = subprocess.run([str(bds_runtime.PYTHON), '-c', 'import endstone_bds_sweep'], capture_output=True)
    if check.returncode != 0:
        parser.error('endstone-sweep is not installed: '
                     f'{bds_runtime.PYTHON} -m pip install -e tools/bds-sweep/endstone-sweep')
    stamp = dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    out: Path = (args.out or bds_runtime.SWEEPS / f'{args.kind}-{stamp}').resolve()
    out.mkdir(parents=True, exist_ok=True)
    for stale in ('done.json', 'error.txt', 'loaded.json'):
        (out / stale).unlink(missing_ok=True)
    job = build_job(args, out)
    job_path = out / 'job.json'
    job_path.write_text(json.dumps(job, indent=1), encoding='utf-8')
    print(f'{len(job["items"])} items -> {out}')

    world = bds_runtime.SERVER / 'worlds' / LEVEL
    if world.exists():
        shutil.rmtree(world)
    properties = bds_runtime.base_properties(
        LEVEL, level_type='FLAT', gamemode='creative', force_gamemode='true', difficulty='peaceful',
        default_player_permission_level='operator', view_distance=6, tick_distance=4)
    env = {'BDS_SWEEP_JOB': str(job_path), 'CLIENT_DIFF_CAPTURE_DIR': str(out / 'client-diff-plugin')}
    started = time.monotonic()
    status = 'timeout'
    try:
        with bds_runtime.Bds(properties, out / 'bds.log', endstone=True, env=env):
            with (out / 'client.log').open('a', encoding='utf-8') as log:
                client = subprocess.Popen([str(args.client), '--name', PLAYER], cwd=REPO,
                                          stdout=log, stderr=subprocess.STDOUT)
                try:
                    while time.monotonic() - started < args.timeout:
                        if (out / 'done.json').exists():
                            status = 'done'
                            break
                        if (out / 'error.txt').exists():
                            status = 'error'
                            break
                        if client.poll() is not None:
                            status = f'client exited {client.returncode}'
                            break
                        time.sleep(1)
                finally:
                    if client.poll() is None:
                        client.kill()
    finally:
        if world.exists():
            shutil.rmtree(world)
    elapsed = time.monotonic() - started
    with (out / 'runs.jsonl').open('a', encoding='utf-8') as runs:
        runs.write(json.dumps({'started': stamp, 'status': status, 'seconds': round(elapsed, 1),
                               'items': len(job['items'])}) + '\n')
    print(f'{status} after {elapsed:.0f}s')
    if status == 'error':
        print((out / 'error.txt').read_text(encoding='utf-8'))
    return 0 if status == 'done' else 1


if __name__ == '__main__':
    sys.exit(main())
