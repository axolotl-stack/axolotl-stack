"""T2: run a BDS sweep job (Endstone plugin + scripted client) on loopback.

    python tools/bds-sweep/sweep.py drops --blocks slice --samples 200 --out DIR
    python tools/bds-sweep/sweep.py drops --blocks all --states all --out DIR
    python tools/bds-sweep/sweep.py armor-slots --out DIR
    python tools/bds-sweep/sweep.py fuel --out DIR
    python tools/bds-sweep/sweep.py scenarios --family pilot --out DIR
    python tools/bds-sweep/sweep.py placements --out DIR

Starts BDS 1.26.51 under Endstone on a fresh superflat creative world
(`bds-sweep-1.26.51`, peaceful, operators by default), launches
`bds-sweep-client` (cargo build -p bds-sweep) as the sweep player and waits
for the plugin (`endstone-sweep`, installed editable into the shared venv) to
write `done.json` or `error.txt`. Results are appended to
`<out>/results.jsonl`; rerunning with the same `--out` skips finished items,
so a sweep can be interrupted and resumed. To hand the BDS slot over
mid-sweep, create `<out>/stop.request`: the runner stops BDS gracefully
within a second and restores `server.properties`. Raw results stay out of
the repository; `summarize.py` turns them into measured data.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import native  # noqa: E402
import placements  # noqa: E402
import scenarios  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
LEVEL = 'bds-sweep-1.26.51'
PLAYER = 'BDSSweep'
STOP_REQUEST = 'stop.request'
# Superflat: grass top at y=-61, so the player stands at y=-60.
# `miner` = x, y, z, yaw, pitch: facing east (yaw -90) at the west face (4) of
# `mine_block`, which is at eye level (eyes at y + 1.62).
ARENA = {'player': [0.5, -60, 0.5], 'block': [3, -60, 0], 'furnaces': [-8, -60, 6],
         'miner': [0.5, -60, 0.5, -90, 0], 'mine_block': [1, -59, 0], 'mine_face': 4,
         'target': [2.0, -60, 0.5],
         # Block scenarios: 8 x 8 cells, 6 blocks apart, east of the player and
         # inside its ticking range.
         'scenarios': {'origin': [8, -60, -24], 'spacing': 6, 'columns': 8},
         # Survival block clicks: cleared box (offsets) around the origin.
         'placements': {'origin': [4, -60, 4], 'clear': [[-3, -1, -4], [5, 4, 13]]}}
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


HELD_BLOCKS = ['minecraft:dirt', 'minecraft:netherrack', 'minecraft:oak_leaves']
# Per block, the effects measured on the empty hand: stone resolves the haste
# multipliers (150 ticks by hand), dirt the mining fatigue ones (15 ticks).
EFFECT_CELLS = {'minecraft:stone': ['haste1', 'haste2', 'haste3'],
                'minecraft:dirt': ['mining_fatigue1', 'mining_fatigue2']}


def block_cells(names: list[str]) -> list[tuple[str, dict]]:
    first = native.default_states()
    return [(name, SLICE_BLOCKS.get(name, first[name])) for name in names]


def non_tool_diggers() -> list[str]:
    """Items the parity catalog gives a `digger` facet through their native
    mining class alone (no `minecraft:digger` tag and no digger or shears
    class): armour, bows, bundles, spears, music discs, ... Education
    Edition items (glow sticks, sparklers) are skipped."""
    rows = json.loads((REPO / 'crates' / 'unastar-data' / 'src' / 'items.json').read_text(encoding='utf-8'))
    return sorted(row['identifier'] for row in rows
                  if row['mine_block_type'] in (1, 2) and 'minecraft:digger' not in row.get('tags', [])
                  and row['identifier'] not in ('minecraft:glow_stick', 'minecraft:sparkler'))


def wear_tools() -> list[str]:
    """Every tier of every digger and sword, shears, trident and mace."""
    return ([f'minecraft:{tier}_{kind}' for kind in DIGGERS for tier in TIERS]
            + ['minecraft:shears', 'minecraft:trident', 'minecraft:mace'])


def held_mining_items(args) -> list[dict]:
    """The empty hand (3 trials) and every held item (`--trials` trials) on
    each block: whether a non-tool item mines like the hand, and what one
    break costs its durability. The default set also breaks dirt and
    netherrack (cells the tool slice does not have) with every tool, for the
    wear a break costs each tool."""
    default = args.items in ('slice', 'non-tool-diggers')
    names = non_tool_diggers() if default else args.items.split(',')
    blocks = HELD_BLOCKS if args.blocks == 'slice' else args.blocks.split(',')
    items = []
    for block, states in block_cells(blocks):
        held = names + (wear_tools() if default and block != 'minecraft:oak_leaves' else [])
        for spec, repeats in [(None, 3)] + [({'item': name}, args.trials) for name in held]:
            items.append({'key': f'{state_key(block, states)}|{tool_key(spec)}', 'block': block,
                          'states': states, 'tool': spec, 'commands': [], 'max_ticks': args.mine_ticks,
                          'repeats': repeats})
    return items


def effect_mining_items(args) -> list[dict]:
    """Haste and mining fatigue levels on the empty hand (`EFFECT_CELLS`),
    `--trials` trials each, keyed like the slice's `+haste1` variants."""
    items = []
    for block, states in block_cells(list(EFFECT_CELLS)):
        for effect in EFFECT_CELLS[block]:
            name, level = effect.rstrip('0123456789'), int(effect[len(effect.rstrip('0123456789')):])
            items.append({'key': f'{state_key(block, states)}|hand+{effect}', 'block': block, 'states': states,
                          'tool': None, 'commands': [f'effect {{player}} {name} 1000 {level - 1} true'],
                          'max_ticks': args.mine_ticks, 'repeats': args.trials})
    return items


def capture_mining_items(args) -> list[dict]:
    """One trial of each breaking situation, for packet captures of the whole
    flow (run with `--client-capture`): a break BDS completes itself (dirt by
    hand, stone with a diamond pickaxe, Haste II, a block above, a tool that
    wears), an instant break (short grass), a client prediction on time and
    early, and an abort after 5 ticks."""
    dirt = ('minecraft:dirt', {})
    cells = [
        ('hand', dirt, None, {}),
        ('diamond_pickaxe', ('minecraft:stone', {}), {'item': 'minecraft:diamond_pickaxe'}, {}),
        ('iron_shovel', dirt, {'item': 'minecraft:iron_shovel'}, {}),
        ('hand+instant', ('minecraft:short_grass', {}), None, {}),
        ('hand+predict14', dirt, None, {'predict_after': 14}),
        ('hand+predict10', dirt, None, {'predict_after': 10, 'max_ticks': 40}),
        ('hand+abort5', dirt, None, {'max_ticks': 5}),
        ('hand+haste2', dirt, None, {'commands': ['effect {player} haste 1000 1 true']}),
        ('hand+stone_above', dirt, None, {'commands': ['setblock 1 -58 0 stone'],
                                          'cleanup': ['setblock 1 -58 0 air']}),
    ]
    items = []
    for variant, (block, states), spec, extra in cells:
        items.append({'key': f'{state_key(block, states)}|{variant}', 'block': block, 'states': states,
                      'tool': spec, 'commands': [], 'max_ticks': args.mine_ticks, 'repeats': 1, **extra})
    return items


def mining_items(args) -> list[dict]:
    """Hand, every tier of each correct digger, shears, efficiency I-V on the
    diamond correct tool, and haste I/II and mining fatigue I with it
    (`--variants tools`); see `held_mining_items`, `effect_mining_items` and
    `capture_mining_items` for the other variant sets."""
    if args.variants == 'held':
        return held_mining_items(args)
    if args.variants == 'effects':
        return effect_mining_items(args)
    if args.variants == 'capture':
        return capture_mining_items(args)
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
        water = ['setblock 0 -60 0 water', 'setblock 0 -59 0 water']
        dry = ['setblock 0 -60 0 air', 'setblock 0 -59 0 air']
        # Airborne is not measured: the only way found to keep a survival
        # player off the ground (levitation, amplifier 255) lifts it out of
        # reach, so the block never breaks (on_ground was false in the probe).
        environments = [('+underwater', water, dry)]
        for spec, commands, suffix in variants:
            items.append({'key': f'{state_key(block, states)}|{tool_key(spec)}{suffix}', 'block': block,
                          'states': states, 'tool': spec, 'commands': commands, 'max_ticks': args.mine_ticks})
        for suffix, commands, cleanup in environments:
            for spec in (None, tool(best, 'diamond')):
                items.append({'key': f'{state_key(block, states)}|{tool_key(spec)}{suffix}', 'block': block,
                              'states': states, 'tool': spec, 'commands': commands, 'cleanup': cleanup,
                              'max_ticks': args.mine_ticks * 5})
    return items


ARMOR_DAMAGE = [
    # Small hits: the toughness term matters; summed over many hits.
    {'amount': 4, 'hits': 20},
    {'amount': 16, 'hits': 10},
    # Large hits clamp the reduction to defence/5 points: exact defence.
    # Last, and the worn piece is unbreakable, so it cannot break the item.
    {'amount': 1000, 'hits': 1},
]


def armor_value_items(args) -> list[dict]:
    """No armour (baseline), then every item with the native tag
    `minecraft:is_armor` plus the extra `--items`, each in the armour slot the
    measured `armor_slots.json` assigns (chest, legs or feet if one takes it,
    else head)."""
    slots_path = REPO / 'crates' / 'unastar-data' / 'data' / 'measured' / 'armor_slots.json'
    slots = json.loads(slots_path.read_text(encoding='utf-8'))
    tags = native.item_tags()
    names = sorted(name for name, item_tags in tags.items() if 'minecraft:is_armor' in item_tags)
    if args.items not in ('slice', 'all'):
        names += [name for name in args.items.split(',') if name not in names]
    items = [{'key': 'none', 'item': None, 'slot': 'head'}]
    for name in names:
        if name not in slots:
            raise SystemExit(f'{name} is not in {slots_path}: run the armor-slots sweep with --items all first')
        slot = next((s for s in ('chest', 'legs', 'feet') if slots[name][s]), 'head')
        items.append({'key': name, 'item': name, 'slot': slot})
    return items


SPEARS = [f'minecraft:{tier}_spear' for tier in TIERS]


# One hit of each amount per piece: wear rules round the damage per 4 points.
ARMOR_WEAR_AMOUNTS = [1, 3, 4, 5, 7, 8, 12, 16, 40]


def armor_wear_items(args) -> list[dict]:
    """Every item with the native tag `minecraft:is_armor` (or `--items`), in
    the slot the measured `armor_slots.json` assigns, like `armor-values`."""
    return [item for item in armor_value_items(args) if item['item']]


def attack_items(args) -> list[dict]:
    """Hand, every tier of every digger and sword, trident and mace, and
    sharpness I-V on the diamond sword; `--items` adds held items (`spears`:
    every spear tier), spaced `--gap` extra ticks apart (spears declare a
    0.65 s attack cooldown)."""
    tools: list[dict | None] = [None]
    tools += [tool(kind, tier) for kind in DIGGERS for tier in TIERS]
    tools += [{'item': 'minecraft:trident'}, {'item': 'minecraft:mace'}]
    tools += [tool('sword', 'diamond', sharpness=level) for level in range(1, 6)]
    items = [{'key': tool_key(spec), 'tool': spec} for spec in tools]
    if args.items not in ('slice', 'all'):
        names = SPEARS if args.items == 'spears' else args.items.split(',')
        items += [{'key': tool_key({'item': name}), 'tool': {'item': name}, 'gap': args.gap} for name in names]
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
    elif args.kind == 'armor-values':
        job.update(max_health=2000, damage=ARMOR_DAMAGE, items=armor_value_items(args))
    elif args.kind == 'armor-wear':
        job.update(max_health=2000, amounts=ARMOR_WEAR_AMOUNTS, items=armor_wear_items(args))
    elif args.kind == 'attack':
        job.update(target='minecraft:pig', items=attack_items(args))
        if args.gamemode:
            job['gamemode'] = args.gamemode
    elif args.kind == 'scenarios':
        job['items'] = scenarios.items(args.family)
    elif args.kind == 'placements':
        job['items'] = placements.items()
    elif args.kind == 'item-blocks':
        job['items'] = [{'key': name.removeprefix('minecraft:'), 'item': name}
                        for name in item_names(args.items, native.items())]
    elif args.kind == 'fuel':
        job.update(furnace='furnace', lit_furnace='minecraft:lit_furnace', input='cobblestone',
                   max_ticks=args.max_ticks,
                   items=[{'key': name, 'item': name} for name in item_names(args.items, SLICE_FUELS)])
    return job


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('kind', choices=['drops', 'mining', 'armor-slots', 'armor-values', 'armor-wear', 'attack',
                                         'fuel', 'scenarios', 'placements', 'item-blocks'])
    parser.add_argument('--out', type=Path)
    parser.add_argument('--blocks', default='slice', help="'slice', 'all' or comma-separated identifiers")
    parser.add_argument('--states', default='first', choices=['first', 'all'])
    parser.add_argument('--items', default='slice', help="'slice' or comma-separated identifiers")
    parser.add_argument('--samples', type=int, default=200)
    parser.add_argument('--family', default='pilot', help="scenarios: 'all' or comma-separated families")
    parser.add_argument('--max-ticks', type=int, default=24000, help='fuel: longest burn to wait for')
    parser.add_argument('--mine-ticks', type=int, default=600, help='mining: longest break to wait for')
    parser.add_argument('--variants', default='tools', choices=['tools', 'held', 'effects', 'capture'],
                        help="mining: 'tools' (every tier, efficiency, effects with the diamond tool), "
                             "'held' (--items held on --blocks; default every non-tool digger on dirt, "
                             "netherrack and leaves), 'effects' (haste/mining fatigue levels by hand) or 'capture' "
                             "(one trial of each breaking situation, for --client-capture)")
    parser.add_argument('--trials', type=int, default=3, help='mining held/effects: trials per item')
    parser.add_argument('--gamemode', choices=['survival', 'creative'],
                        help='attack: the player game mode (survival shows wear per hit)')
    parser.add_argument('--gap', type=int, default=0, help='attack: extra ticks between trials of --items')
    parser.add_argument('--client-capture', action='store_true',
                        help="record the client's packets to <out>/client-capture/packets.jsonl")
    parser.add_argument('--client', type=Path, default=REPO / 'target' / 'debug' / 'bds-sweep-client.exe')
    parser.add_argument('--timeout', type=float, default=6 * 3600, help='seconds for the whole job')
    args = parser.parse_args()
    # A queue of sweep.py runs can be paused for someone else's BDS slot:
    # while this file exists, every new run exits without starting BDS.
    hold = bds_runtime.SWEEPS / 'hold'
    if hold.exists():
        print(f'held: {hold} exists; not starting BDS')
        return 3
    if not args.client.exists():
        parser.error(f'{args.client} missing: cargo build -p bds-sweep')
    check = subprocess.run([str(bds_runtime.PYTHON), '-c', 'import endstone_bds_sweep'], capture_output=True)
    if check.returncode != 0:
        parser.error('endstone-sweep is not installed: '
                     f'{bds_runtime.PYTHON} -m pip install -e tools/bds-sweep/endstone-sweep')
    stamp = dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    out: Path = (args.out or bds_runtime.SWEEPS / f'{args.kind}-{stamp}').resolve()
    out.mkdir(parents=True, exist_ok=True)
    for stale in ('done.json', 'error.txt', 'loaded.json', STOP_REQUEST):
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
    env = {'BDS_SWEEP_JOB': str(job_path), 'CLIENT_DIFF_CAPTURE_DIR': str(out / 'client-diff-plugin'),
           # This checkout's plugin source wins over the shared venv's editable
           # install, so a worktree runs its own jobs without reinstalling it.
           'PYTHONPATH': os.pathsep.join(filter(None, [
               str(REPO / 'tools' / 'bds-sweep' / 'endstone-sweep' / 'src'), os.environ.get('PYTHONPATH')]))}
    started = time.monotonic()
    status = 'timeout'
    client_command = [str(args.client), '--addr', bds_runtime.ADDR, '--name', PLAYER]
    if args.client_capture or args.kind in ('placements', 'item-blocks'):
        # The client records both directions: BDS's answer to every click,
        # the stack descriptors BDS sends.
        client_command += ['--capture-dir', str(out / 'client-capture')]
    try:
        with bds_runtime.Bds(properties, out / 'bds.log', endstone=True, env=env):
            with (out / 'client.log').open('a', encoding='utf-8') as log:
                client = subprocess.Popen(client_command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT)
                logins = 1
                try:
                    while time.monotonic() - started < args.timeout:
                        if (out / 'done.json').exists():
                            status = 'done'
                            break
                        if (out / 'error.txt').exists():
                            status = 'error'
                            break
                        # Handover: `stop.request` in the output directory stops BDS
                        # gracefully; finished items stay in results.jsonl.
                        if (out / STOP_REQUEST).exists():
                            (out / STOP_REQUEST).unlink()
                            status = 'stopped on request (resume with the same --out)'
                            break
                        if client.poll() is not None:
                            # A login can time out while BDS is still busy right after
                            # start; retry during the first minutes of the run.
                            if client.returncode != 0 and logins < 3 and time.monotonic() - started < 300:
                                logins += 1
                                client = subprocess.Popen(client_command, cwd=REPO, stdout=log,
                                                          stderr=subprocess.STDOUT)
                                continue
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
