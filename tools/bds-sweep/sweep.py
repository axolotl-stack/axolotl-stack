"""T2: run a BDS sweep job (Endstone plugin + scripted client) on loopback.

    python tools/bds-sweep/sweep.py drops --blocks slice --samples 200 --out DIR
    python tools/bds-sweep/sweep.py drops --blocks all --states all --out DIR
    python tools/bds-sweep/sweep.py armor-slots --out DIR
    python tools/bds-sweep/sweep.py fuel --out DIR
    python tools/bds-sweep/sweep.py blocks --scenarios doors,torches,crops --out DIR
    python tools/bds-sweep/sweep.py blocks --scenarios redstone --out DIR
    python tools/bds-sweep/sweep.py blocks --scenarios utility --out DIR
    python tools/bds-sweep/sweep.py blocks --scenarios all --out DIR

Starts BDS 1.26.51 under Endstone on a fresh superflat creative world
(`bds-sweep-1.26.51`, peaceful, operators by default) on UDP `--port`
(default 19140, IPv6 on the next port; never the 19132 game port), launches
`bds-sweep-client` (cargo build -p bds-sweep) as the sweep player and waits
for the plugin (`endstone-sweep`, installed editable into the shared venv) to
write `done.json` or `error.txt`. Results are appended to
`<out>/results.jsonl`; rerunning with the same `--out` skips finished items,
so a sweep can be interrupted and resumed. To hand the BDS slot over
mid-sweep, create `<out>/stop.request`: the runner stops BDS gracefully
within a second and restores `server.properties`. Raw results stay out of
the repository; `summarize.py` turns them into measured data. `--dry-run`
writes `job.json` and prints the client command without starting BDS.
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
import scenarios  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
LEVEL = 'bds-sweep-1.26.51'
PLAYER = 'BDSSweep'
STOP_REQUEST = 'stop.request'
# Superflat: grass top at y=-61, so the player stands at y=-60.
# `miner` = x, y, z, yaw, pitch: facing east (yaw -90) at the west face (4) of
# `mine_block`, which is at eye level (eyes at y + 1.62).
# `blocks`: the stone floor of the block-behaviour arena (x/z corners) is the
# superflat top layer, so the player stands on it at y=-60.
ARENA = {'player': [0.5, -60, 0.5], 'block': [3, -60, 0], 'furnaces': [-8, -60, 6],
         'miner': [0.5, -60, 0.5, -90, 0], 'mine_block': [1, -59, 0], 'mine_face': 4,
         'target': [2.0, -60, 0.5], 'blocks': {'min': [-10, -12], 'max': [24, 24], 'floor': -61}}
# Placement and survival scenarios (`--scenarios blocks` runs them all).
BLOCK_SCENARIOS = ('doors', 'trapdoors', 'fence_gates', 'torches', 'attachables', 'signs', 'beds', 'stairs', 'slabs',
                   'connections', 'crops', 'crop_care', 'plant_survival', 'falling_blocks')
# Redstone and fire scenarios (`--scenarios redstone` runs them all); their
# summary is `summarize.py redstone`.
REDSTONE_SCENARIOS = ('redstone_dust', 'repeaters', 'redstone_torch', 'comparators', 'comparator_container',
                      'buttons', 'pressure_plates', 'weighted_plates', 'iron_door_lever', 'note_blocks', 'pistons',
                      'fire')
# Blocks whose every palette state the redstone scenarios map to a runtime
# id, so the summary can name the block states that sounds carry as data.
PALETTE_BLOCKS = ('air', 'stone', 'dirt', 'lever', 'redstone_wire', 'redstone_lamp', 'lit_redstone_lamp',
                  'unpowered_repeater', 'powered_repeater', 'unpowered_comparator', 'powered_comparator',
                  'redstone_torch', 'unlit_redstone_torch', 'stone_button', 'wooden_button', 'stone_pressure_plate',
                  'wooden_pressure_plate', 'iron_door', 'noteblock', 'fire', 'netherrack', 'oak_planks',
                  'redstone_block', 'piston', 'sticky_piston', 'piston_arm_collision',
                  'sticky_piston_arm_collision', 'obsidian', 'chest', 'torch', 'light_weighted_pressure_plate',
                  'heavy_weighted_pressure_plate')
# Interactive-block scenarios (`--scenarios utility` runs them all); their
# summary is `summarize.py utility`.
UTILITY_SCENARIOS = ('observer', 'dispensers', 'tnt', 'note_block_use', 'rails', 'composter', 'cauldron', 'campfire',
                     'beehive', 'bell', 'jukebox', 'flower_pot', 'cake', 'end_portal', 'nether_portal',
                     'respawn_anchor', 'bed')
UTILITY_PALETTE_BLOCKS = ('observer', 'dispenser', 'dropper', 'chest', 'tnt', 'rail', 'golden_rail', 'composter',
                          'cauldron', 'campfire', 'beehive', 'bell', 'jukebox', 'flower_pot', 'cake',
                          'end_portal_frame', 'end_portal', 'obsidian', 'portal', 'respawn_anchor', 'bed', 'water',
                          'flowing_water', 'lava', 'flowing_lava', 'glowstone', 'grass_block')
# Server packets the `blocks` job's client logs per step.
BLOCK_TRACE = ('LevelSoundEventPacket', 'LevelEventPacket', 'LevelEventGenericPacket', 'UpdateBlockPacket',
               'UpdateBlockSyncedPacket', 'UpdateSubChunkBlocksPacket', 'BlockEventPacket', 'AddItemActorPacket')
# Also logged when a utility scenario runs: actors, block actor data,
# messages, spawn points and hunger.
UTILITY_TRACE = ('AddActorPacket', 'RemoveActorPacket', 'BlockActorDataPacket', 'ActorEventPacket',
                 'SetActorDataPacket', 'SetActorMotionPacket', 'TextPacket', 'SetSpawnPositionPacket',
                 'UpdateAttributesPacket', 'PlaySoundPacket')
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


def attack_items(args) -> list[dict]:
    """Hand, every tier of every digger and sword, trident and mace, and
    sharpness I-V on the diamond sword."""
    tools: list[dict | None] = [None]
    tools += [tool(kind, tier) for kind in DIGGERS for tier in TIERS]
    tools += [{'item': 'minecraft:trident'}, {'item': 'minecraft:mace'}]
    tools += [tool('sword', 'diamond', sharpness=level) for level in range(1, 6)]
    if args.items not in ('slice', 'all'):
        tools += [{'item': name} for name in args.items.split(',')]
    return [{'key': tool_key(spec), 'tool': spec} for spec in tools]


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
    elif args.kind == 'attack':
        job.update(target='minecraft:pig', items=attack_items(args))
    elif args.kind == 'fuel':
        job.update(furnace='furnace', lit_furnace='minecraft:lit_furnace', input='cobblestone',
                   max_ticks=args.max_ticks,
                   items=[{'key': name, 'item': name} for name in item_names(args.items, SLICE_FUELS)])
    elif args.kind == 'blocks':
        names = scenario_names(args.scenarios)
        known = BLOCK_SCENARIOS + REDSTONE_SCENARIOS + UTILITY_SCENARIOS
        unknown = sorted(set(names) - set(known))
        if unknown:
            raise SystemExit(f'unknown blocks scenarios: {unknown}; choose from {known}, blocks, redstone, utility '
                             'or all')
        runs = [[int(speed), float(seconds)] for speed, seconds in
                (run.split(':') for run in args.growth_runs.split(','))]
        job['items'] = [{'key': name, **({'growth_runs': runs} if name == 'crops' else {})} for name in names]
        palette = set()
        if set(names) & set(REDSTONE_SCENARIOS):
            palette |= set(PALETTE_BLOCKS)
        if set(names) & set(UTILITY_SCENARIOS):
            palette |= set(PALETTE_BLOCKS) | set(UTILITY_PALETTE_BLOCKS)
        if palette:
            wanted = {f'minecraft:{name}' for name in palette}
            job['palette_states'] = [entry for entry in native.block_states() if entry['name'] in wanted]
    return job


def scenario_names(spec: str) -> list[str]:
    """Comma-separated scenario names and groups (`blocks`, `redstone`,
    `utility`, `all`), expanded in order."""
    groups = {'blocks': BLOCK_SCENARIOS, 'redstone': REDSTONE_SCENARIOS, 'utility': UTILITY_SCENARIOS,
              'all': BLOCK_SCENARIOS + REDSTONE_SCENARIOS + UTILITY_SCENARIOS}
    return [n for name in spec.split(',') for n in groups.get(name, (name,))]


def client_command(args, out: Path) -> list[str]:
    command = [str(args.client), '--name', PLAYER, '--addr', f'127.0.0.1:{args.port}']
    if args.kind == 'blocks':
        trace = BLOCK_TRACE + (UTILITY_TRACE if set(scenario_names(args.scenarios)) & set(UTILITY_SCENARIOS)
                               else ())
        command += ['--capture-dir', str(out / 'capture'), '--trace', ','.join(trace)]
    return command


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('kind', choices=['drops', 'mining', 'armor-slots', 'armor-values', 'attack', 'fuel',
                                         'blocks'])
    parser.add_argument('--out', type=Path)
    parser.add_argument('--blocks', default='slice', help="'slice', 'all' or comma-separated identifiers")
    parser.add_argument('--states', default='first', choices=['first', 'all'])
    parser.add_argument('--items', default='slice', help="'slice' or comma-separated identifiers")
    parser.add_argument('--samples', type=int, default=200)
    parser.add_argument('--max-ticks', type=int, default=24000, help='fuel: longest burn to wait for')
    parser.add_argument('--mine-ticks', type=int, default=600, help='mining: longest break to wait for')
    parser.add_argument('--client', type=Path, default=REPO / 'target' / 'debug' / 'bds-sweep-client.exe')
    parser.add_argument('--timeout', type=float, default=6 * 3600, help='seconds for the whole job')
    parser.add_argument('--port', type=int, default=19140,
                        help='BDS IPv4 UDP port (IPv6 uses the next one); 19132/19133 are refused')
    parser.add_argument('--scenarios', default=','.join(BLOCK_SCENARIOS), help='blocks: scenarios to run')
    parser.add_argument('--growth-runs', default='100:60,1:60',
                        help='blocks/crops: comma-separated randomtickspeed:seconds growth runs')
    parser.add_argument('--dry-run', action='store_true',
                        help='write job.json and print the client command; do not start BDS')
    args = parser.parse_args()
    if {args.port, args.port + 1} & {19132, 19133}:
        parser.error('ports 19132/19133 are reserved for the game server; pick another --port')
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
    command = client_command(args, out)
    if args.dry_run:
        print('client:', subprocess.list2cmdline(command))
        return 0

    world = bds_runtime.SERVER / 'worlds' / LEVEL
    if world.exists():
        shutil.rmtree(world)
    properties = bds_runtime.base_properties(
        LEVEL, level_type='FLAT', gamemode='creative', force_gamemode='true', difficulty='peaceful',
        default_player_permission_level='operator', view_distance=6, tick_distance=4,
        server_port=args.port, server_portv6=args.port + 1)
    env = {'BDS_SWEEP_JOB': str(job_path), 'CLIENT_DIFF_CAPTURE_DIR': str(out / 'client-diff-plugin')}
    started = time.monotonic()
    status = 'timeout'
    try:
        with bds_runtime.Bds(properties, out / 'bds.log', endstone=True, env=env):
            with (out / 'client.log').open('a', encoding='utf-8') as log:
                client = subprocess.Popen(command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT)
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
                                client = subprocess.Popen(command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT)
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
    capture = out / 'capture' / 'packets.jsonl'
    if args.kind == 'blocks' and capture.exists():
        written = scenarios.split(capture, out / 'scenarios')
        print(f'scenario captures: {", ".join(f"{k} ({v})" for k, v in written.items()) or "none"}')
    if status == 'error':
        print((out / 'error.txt').read_text(encoding='utf-8'))
    return 0 if status == 'done' else 1


if __name__ == '__main__':
    sys.exit(main())
