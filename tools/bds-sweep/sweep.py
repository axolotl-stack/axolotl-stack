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
so a sweep can be interrupted and resumed. To hand the BDS slot over
mid-sweep, create `<out>/stop.request`: the runner stops BDS gracefully
within a second and restores `server.properties`. Raw results stay out of
the repository; `summarize.py` turns them into measured data.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
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
# The second client of `--observer` (a second viewer of the job's actors).
OBSERVER_NAME = 'BDSOther'
STOP_REQUEST = 'stop.request'
# Superflat: grass top at y=-61, so the player stands at y=-60.
# `miner` = x, y, z, yaw, pitch: facing east (yaw -90) at the west face (4) of
# `mine_block`, which is at eye level (eyes at y + 1.62).
ARENA = {'player': [0.5, -60, 0.5], 'block': [3, -60, 0], 'furnaces': [-8, -60, 6],
         'miner': [0.5, -60, 0.5, -90, 0], 'mine_block': [1, -59, 0], 'mine_face': 4,
         'target': [2.0, -60, 0.5]}
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


def knockback_items(args) -> list[dict]:
    """`--items slice` (default): a cow and a zombie hit three times (at 0,
    11 and 30 ticks after a 40-tick settle) by the player standing two blocks
    west facing east; variants from the south-west (direction from positions
    differs from the player's facing), with the player's back to the mob,
    with a sword, and with the mob's AI left to move it (`natural`) instead
    of frozen. `--items air`: mobs moving through the air under their AI or
    hit there: a cow tempted by wheat over a one-block step (plain, Speed II,
    Slowness I), cows and zombies hit while falling, a panicking cow hit
    four times. `--items all`: both."""
    east = {'from': [-2.0, 0.0], 'yaw': -90}
    diagonal = {'from': [-1.5, -1.0], 'yaw': -90}
    away = {'from': [-2.0, 0.0], 'yaw': 90}
    trials = [
        ('cow', 'east', True, east, None),
        ('cow', 'diagonal', True, diagonal, None),
        ('cow', 'away', True, away, None),
        ('zombie', 'east', True, east, None),
        ('zombie', 'east-sword', True, east, {'item': 'minecraft:wooden_sword'}),
        ('cow', 'east', False, east, None),
        ('cow', 'diagonal', False, diagonal, None),
        ('zombie', 'east', False, east, None),
    ]
    hits = []
    for mob, geometry, frozen, hit, tool_spec in trials:
        hits.append({'key': f'{mob}|{geometry}|{"frozen" if frozen else "natural"}',
                     'mob': f'minecraft:{mob}', 'at': [2.5, -60, 0.5, 90], 'frozen': frozen,
                     'tool': tool_spec, 'settle': 40, 'tail': 40,
                     'hits': [{**hit, 'tick': tick} for tick in (0, 11, 30)]})
    step = 'fill 5 -60 -2 9 -60 3 {}'
    air = []
    for name, effects in (('plain', []), ('speed2', ['effect @e[type=minecraft:cow] speed 1000 1 true']),
                          ('slowness1', ['effect @e[type=minecraft:cow] slowness 1000 0 true'])):
        air.append({'key': f'cow|tempt-step-{name}|natural', 'mob': 'minecraft:cow', 'at': [1.5, -60, 0.5, -90],
                    'frozen': False, 'effects': effects, 'tool': {'item': 'minecraft:wheat'},
                    'player': [7.5, -59, 0.5, 90], 'setup': [step.format('stone')],
                    'cleanup': [step.format('air')], 'settle': 0, 'tail': 120, 'hits': []})
    for mob, key in (('cow', 'falling'), ('zombie', 'falling'), ('zombie', 'falling-2')):
        air.append({'key': f'{mob}|{key}|frozen', 'mob': f'minecraft:{mob}', 'at': [2.5, -56, 0.5, 90],
                    'frozen': True, 'tool': None, 'settle': 0, 'tail': 40, 'hits': [{**east, 'tick': 2}]})
    air.append({'key': 'cow|panic-run|natural', 'mob': 'minecraft:cow', 'at': [2.5, -60, 0.5, 90],
                'frozen': False, 'tool': None, 'settle': 20, 'tail': 40,
                'hits': [{**east, 'tick': tick} for tick in (0, 20, 40, 60)]})
    # Frozen cows falling 20 blocks at increasing horizontal distances from
    # the player: how BDS sends a moving actor's updates by distance.
    far = [{'key': f'cow|fall-{distance}|frozen', 'mob': 'minecraft:cow', 'at': [0.5 + distance, -40, 0.5, 90],
            'frozen': True, 'tool': None, 'player': [0.5, -60, 0.5, -90], 'settle': 0, 'tail': 45, 'hits': []}
           for distance in (4, 6, 7, 10, 16, 24, 32, 48)]
    # Mobs whose definitions give them a knockback resistance (zoglin and
    # hoglin 0.6, iron golem 1), hit like the frozen cows and zombies.
    resist = [{'key': f'{mob}|east|frozen', 'mob': f'minecraft:{mob}', 'at': [2.5, -60, 0.5, 90], 'frozen': True,
               'tool': None, 'settle': 40, 'tail': 40, 'hits': [{**east, 'tick': tick} for tick in (0, 11, 30)]}
              for mob in ('zoglin', 'hoglin', 'iron_golem')]
    sets = {'slice': hits, 'air': air, 'far': far, 'cadence': cadence_items(), 'cadence2': cadence2_items(),
            'cadence3': cadence3_items(), 'resist': resist, 'all': hits + air + far}
    if args.items not in sets:
        raise SystemExit(f"knockback --items must be one of {sorted(sets)}")
    return sets[args.items]


# The observer client of `--items cadence` stands 40 blocks north of the
# sweep player unless an item puts it elsewhere.
OBSERVER = [0.5, -60, -39.5, 180]


def cadence_items() -> list[dict]:
    """How often BDS sends a moving actor's position to each viewer, by
    distance and by the actor's `minecraft:conditional_bandwidth_optimization`
    values (run with `--observer --client-capture --view-distance 10
    --tick-distance 10`: two viewers per actor). Frozen cows falling 20
    blocks at 40-104 blocks (the empty component's defaults, beyond their
    80-block maximum); an armor stand and a frozen cow that land on a pillar,
    rest, and fall again when it is removed; one falling actor each with no
    component (goat), 10/80 with hints (enderman, xp orb), 7/100 (snowball),
    7/80 (egg, arrow) and the conditional minecart; and a frozen cow hit into
    a wall, whose x stops two ticks after the hit while y keeps changing,
    seen from 2 and 44 blocks."""
    player = [0.5, -60, 0.5, -90]
    items = []
    for distance in (40, 56, 72, 88, 104):
        items.append({'key': f'cow|fall-{distance}|frozen', 'mob': 'minecraft:cow',
                      'at': [0.5 + distance, -40, 0.5, 90], 'frozen': True, 'tool': None, 'player': player,
                      'observer': OBSERVER, 'settle': 0, 'tail': 45, 'hits': []})
    pillar = 'fill 40 -60 0 40 -50 0 {}'
    for mob, frozen in (('armor_stand', False), ('cow', True)):
        items.append({'key': f'{mob}|pause-40|{"frozen" if frozen else "natural"}', 'mob': f'minecraft:{mob}',
                      'at': [40.5, -48.5, 0.5, 90], 'frozen': frozen, 'tool': None, 'player': player,
                      'observer': OBSERVER, 'setup': [pillar.format('stone')], 'cleanup': [pillar.format('air')],
                      'actions': [{'t': 40, 'commands': [pillar.format('air')]}],
                      'settle': 0, 'tail': 40, 'hits': []})
    for mob, frozen, distance in (('goat', True, 32), ('enderman', True, 32), ('xp_orb', False, 32),
                                  ('snowball', False, 32), ('snowball', False, 56), ('egg', False, 32),
                                  ('arrow', False, 32), ('minecart', False, 32)):
        items.append({'key': f'{mob}|fall-{distance}|{"frozen" if frozen else "natural"}',
                      'mob': f'minecraft:{mob}', 'at': [0.5 + distance, -40, 0.5, 90], 'frozen': frozen,
                      'tool': None, 'player': player, 'observer': OBSERVER, 'settle': 0, 'tail': 50, 'hits': []})
    wall = 'fill 4 -60 -1 4 -58 2 {}'
    items.append({'key': 'cow|wall|frozen', 'mob': 'minecraft:cow', 'at': [3.0, -60, 0.5, 90], 'frozen': True,
                  'tool': None, 'observer': [-41.0, -60, 0.5, -90], 'setup': [wall.format('stone')],
                  'cleanup': [wall.format('air')], 'settle': 40, 'tail': 40,
                  'hits': [{'from': [-2.0, 0.0], 'yaw': -90, 'tick': 0}]})
    return items


def cadence2_items() -> list[dict]:
    """Follow-ups to `cadence` (same flags): a frozen cow falling from 100
    blocks above the player, 103 blocks from it (beyond the 80-block maximum
    of its defaults, inside the 128 blocks beyond which a cow despawns at
    once); a frozen cow next to a wall pushed by
    `/damage` from the sweep player 44 blocks away while the observer is 56
    blocks away, so no viewer is near; and tempted mobs jumping a step under
    Slowness I (pig, carrot, speed multiplier 1.2) and Slowness II (cow)."""
    player = [0.5, -60, 0.5, -90]
    wall = 'fill 4 -60 -1 4 -58 2 {}'
    step = 'fill 5 -60 -2 9 -60 3 {}'
    items = [
        {'key': 'cow|cap|frozen', 'mob': 'minecraft:cow', 'at': [30.5, 40, 0.5, 90], 'frozen': True,
         'tool': None, 'player': player, 'observer': OBSERVER, 'settle': 0, 'tail': 80, 'hits': []},
        {'key': 'cow|wall-far|frozen', 'mob': 'minecraft:cow', 'at': [3.0, -60, 0.5, 90], 'frozen': True,
         'tool': None, 'player': [-41.0, -60, 0.5, -90], 'observer': [3.0, -60, -55.5, 0],
         'setup': [wall.format('stone')], 'cleanup': [wall.format('air')],
         'actions': [{'t': 40, 'commands': [f'damage @e[type=minecraft:cow] 1 entity_attack entity "{PLAYER}"']}],
         'settle': 0, 'tail': 40, 'hits': []},
    ]
    for mob, tool, name, effect in (('pig', 'minecraft:carrot', 'slowness1', 'slowness 1000 0'),
                                    ('cow', 'minecraft:wheat', 'slowness2', 'slowness 1000 1')):
        items.append({'key': f'{mob}|tempt-step-{name}|natural', 'mob': f'minecraft:{mob}',
                      'at': [1.5, -60, 0.5, -90], 'frozen': False,
                      'effects': [f'effect @e[type=minecraft:{mob}] {effect} true'], 'tool': {'item': tool},
                      'player': [7.5, -59, 0.5, 90], 'observer': OBSERVER, 'setup': [step.format('stone')],
                      'cleanup': [step.format('air')], 'settle': 0, 'tail': 120, 'hits': []})
    return items


def cadence3_items() -> list[dict]:
    """Follow-ups to `cadence2` (same flags): vehicles whose
    `conditional_bandwidth_optimization` switches to 0 / 0 while `is_moving`
    (a minecart and a boat resting on a pillar that is removed in two steps,
    so each falls, rests and falls again), and arrows and a snowball shot by
    a dispenser (`spawn_commands`) into a wall and onto the floor, for where a
    projectile stops."""
    player = [0.5, -60, 0.5, -90]
    items = []
    # Pillar top at y -54; the upper part goes at t 40, the rest at t 80.
    pillar = 'fill 32 -60 0 32 -55 0 {}'
    upper = 'fill 32 -56 0 32 -55 0 air'
    for mob in ('minecart', 'boat'):
        items.append({'key': f'{mob}|rest-fall|natural', 'mob': f'minecraft:{mob}', 'at': [32.5, -54, 0.5, 90],
                      'frozen': False, 'tool': None, 'player': player, 'observer': OBSERVER,
                      'setup': [pillar.format('stone')], 'cleanup': [pillar.format('air')],
                      'actions': [{'t': 40, 'commands': [upper]}, {'t': 80, 'commands': [pillar.format('air')]}],
                      'settle': 0, 'tail': 40, 'hits': []})

    def shot(key, mob, item, y, wall):
        """A dispenser facing east (+x) at (20, y, 0), powered from above."""
        dispenser = f'setblock 20 {y} 0 dispenser ["facing_direction"=5]'
        setup = [dispenser, f'replaceitem block 20 {y} 0 slot.container 0 {item} 1']
        cleanup = [f'setblock 20 {y + 1} 0 air', f'setblock 20 {y} 0 air']
        if y > -60:
            setup.insert(0, f'fill 20 -60 0 20 {y - 1} 0 stone')
            cleanup.append(f'fill 20 -60 0 20 {y - 1} 0 air')
        if wall:
            setup.append('fill 26 -60 -3 26 -54 3 stone')
            cleanup.append('fill 26 -60 -3 26 -54 3 air')
        return {'key': key, 'mob': f'minecraft:{mob}', 'at': [20.5, y, 0.5, 0], 'frozen': False, 'tool': None,
                'player': player, 'observer': OBSERVER, 'setup': setup, 'cleanup': cleanup,
                'spawn_commands': [f'setblock 20 {y + 1} 0 redstone_block'], 'settle': 0, 'tail': 40,
                'hits': []}

    items.append(shot('arrow|wall|natural', 'arrow', 'arrow', -60, True))
    items.append(shot('arrow|floor|natural', 'arrow', 'arrow', -55, False))
    items.append(shot('arrow|floor-2|natural', 'arrow', 'arrow', -57, False))
    items.append(shot('snowball|wall|natural', 'snowball', 'snowball', -60, True))
    return items


# `range`: where BDS stops sending actors. The sweep player stands at the
# middle of chunk (0, 0) among still markers of each kind; the observer walks
# away from it and back in one-block steps.
RANGE_CENTER = [8.5, 8.5]
RANGE_MARKERS = [
    {'type': 'minecraft:cow', 'at': [2.5, -60, 8.5, 90]},
    {'type': 'minecraft:cow', 'at': [14.5, -60, 8.5, 90]},
    {'type': 'minecraft:cow', 'at': [17.5, -60, 8.5, 90]},
    {'type': 'minecraft:armor_stand', 'at': [8.5, -60, 2.5, 0]},
    {'type': 'minecraft:item', 'at': [8.5, -60, 14.5]},
    {'type': 'minecraft:arrow', 'at': [5.5, -58, 5.5, 0]},
    {'type': 'minecraft:minecart', 'at': [11.5, -60, 2.5, 0]},
    {'type': 'minecraft:xp_orb', 'at': [8.5, -60, -3.5, 0]},
]


def range_items(args) -> list[dict]:
    """`--items all` (default): the observer walks away from the markers
    along +x, along the +x+z diagonal and along -x (negative chunk
    coordinates), 40 to 112 blocks from the sweep player in one-block steps,
    and back, ten ticks per step; then jumps between 40 and 112 blocks east
    three times, sixty ticks each, for how long an add or a remove takes.
    `--items pilot`: a short east leg."""
    cx, cz = RANGE_CENTER

    def leg(key, direction, near=40, far=112):
        dx, dz = direction
        path = [[round(cx + d * dx, 4), round(cz + d * dz, 4)] for d in range(near, far + 1)]
        return [{'key': f'{key}-out', 'center': RANGE_CENTER, 'markers': RANGE_MARKERS, 'path': path, 'ticks': 10},
                {'key': f'{key}-in', 'center': RANGE_CENTER, 'markers': RANGE_MARKERS, 'path': path[::-1],
                 'ticks': 10}]

    diagonal = 0.5 ** 0.5
    if args.items == 'pilot':
        return leg('pilot', (1, 0), 60, 70)[:1]
    if args.items not in ('all', 'slice'):
        raise SystemExit("range --items must be 'all' or 'pilot'")
    jump = [[cx + 40, cz], [cx + 112, cz]] * 3
    return (leg('east', (1, 0)) + leg('diagonal', (diagonal, diagonal)) + leg('west', (-1, 0))
            + [{'key': 'jump', 'center': RANGE_CENTER, 'markers': RANGE_MARKERS, 'path': jump, 'ticks': 60}])


def bonus_items(args) -> list[dict]:
    """`bonus`: many of each mob on a grid around the sweep player, left in
    the world (`--keep-world`), for the random spawn bonuses BDS saves in
    their attributes."""
    # No villagers, golems, wolves or illagers: they fight the others, and
    # only survivors are saved.
    counts = {'zombie': 300, 'husk': 60, 'drowned': 60, 'zombie_villager_v2': 60, 'zombie_pigman': 60,
              'skeleton': 40, 'stray': 30, 'creeper': 30, 'spider': 30, 'witch': 20, 'cow': 30, 'pig': 30,
              'chicken': 20}
    items = [{'key': mob, 'mob': f'minecraft:{mob}', 'count': count} for mob, count in counts.items()]
    items.append({'key': 'zombie-summon', 'mob': 'minecraft:zombie', 'count': 100, 'via': 'summon'})
    return items


def walk_items(args) -> list[dict]:
    """`walk`: the sweep player moves by scripted inputs (`bds-sweep:walk`
    DX DY DZ DYAW TICKS FLAGS, thousandths of a block / degree per input;
    FLAGS 1 = forward input, 2 = vertical collision) while the observer
    stands 8 blocks south; a frozen cow with yaw 33.3 is spawned each item
    for the rotation its `AddActor` carries."""
    start = [0.5, -60, 0.5, 0]
    observer = [0.5, -60, 8.5, 180]
    cow = [3.5, -60, -3.5, 33.3]
    orders = [
        ('still', '0 0 0 0 40 2', 40),
        ('turn', '0 0 0 9000 40 2', 50),
        ('walk-plain', '200 0 0 0 30 0', 45),
        ('walk-forward', '216 0 0 0 30 3', 45),
        ('walk-turn', '150 0 150 -7000 30 3', 45),
        ('step-up', '0 420 0 0 1 0', 20),
        ('tilt', '0 0 0 0 30 2 3000', 40),
        ('tilt-turn', '0 0 0 5000 30 2 -2000', 40),
    ]
    return [{'key': key, 'order': order, 'ticks': ticks, 'start': start, 'observer': observer, 'cow': cow}
            for key, order, ticks in orders]


# `effects`: the mob stands in a barrier cell (interior x 3, y -60..-58, z 0)
# three blocks east of the sweep player.
EFFECT_CELL = 'fill 2 -61 -1 4 -57 1 {}'
EFFECT_CELL_SETUP = [EFFECT_CELL.format('barrier hollow')]
EFFECT_CELL_CLEANUP = [EFFECT_CELL.format('air'), 'fill 2 -61 -1 4 -61 1 grass']
EFFECT_PLAYER = [0.5, -60, 0.5, -90]
EFFECT_MOB_AT = [3.5, -60, 0.5, 90]


def effect_items(args) -> list[dict]:
    """`effects --items mobs|players|all` (default all): what effects do to
    a cow or zombie in a barrier cell and to the sweep player (survival),
    tick by tick: poison, regeneration, wither, instant health and damage
    (living and undead), resistance and fire resistance against `/damage`,
    health boost, absorption, speed, slowness, invisibility, hidden
    particles, several effects at once, a weaker or stronger effect over an
    active one, clearing, expiry, levitation and slow falling (cow out of the
    cell, frozen), hunger and saturation (player). Actions run at tick `t`
    after the spawn; run with `--client-capture`."""
    def cmd(target: str, text: str) -> str:
        return text.replace('@T', target)

    def mob_item(key, mob, actions, ticks=160, cell=True, at=None, frozen=False):
        target = f'@e[type=minecraft:{mob}]'
        return {'key': f'{mob}|{key}', 'mob': f'minecraft:{mob}', 'at': at or EFFECT_MOB_AT,
                'player': EFFECT_PLAYER, 'frozen': frozen, 'ticks': ticks,
                'setup': EFFECT_CELL_SETUP if cell else [], 'cleanup': EFFECT_CELL_CLEANUP if cell else [],
                'actions': [{'t': t, 'commands': [cmd(target, line) for line in lines]} for t, lines in actions]}

    def player_item(key, actions, ticks=160):
        target = f'"{PLAYER}"'
        return {'key': f'player|{key}', 'mob': None, 'player': EFFECT_PLAYER, 'ticks': ticks,
                'setup': [f'gamemode survival {target}'],
                # Back to full health and food, no effects, creative.
                # (`instant_health 1 20` heals nothing on BDS; amplifier 4 does.)
                'cleanup': [f'effect {target} clear', f'effect {target} instant_health 1 4 true',
                            f'effect {target} saturation 1 4 true', f'effect {target} clear',
                            f'gamemode creative {target}'],
                'actions': [{'t': t, 'commands': [cmd(target, line) for line in lines]} for t, lines in actions]}

    shared = [
        ('poison', [(10, ['effect @T poison 6 0'])]),
        ('poison-2', [(10, ['effect @T poison 6 1'])]),
        ('regeneration', [(5, ['damage @T 6']), (10, ['effect @T regeneration 6 0'])]),
        ('regeneration-2', [(5, ['damage @T 6']), (10, ['effect @T regeneration 6 1'])]),
        ('wither', [(10, ['effect @T wither 6 0'])]),
        ('instant-damage', [(10, ['effect @T instant_damage 1 0'])]),
        ('instant-health', [(5, ['damage @T 8']), (10, ['effect @T instant_health 1 0']),
                            (40, ['effect @T instant_health 1 1'])]),
        ('resistance', [(10, ['effect @T resistance 10 0']), (20, ['damage @T 5']),
                        (50, ['effect @T resistance 10 1']), (60, ['damage @T 5']),
                        (90, ['effect @T resistance 10 4']), (100, ['damage @T 5'])]),
        ('fire-resistance', [(10, ['effect @T fire_resistance 10 0']), (20, ['damage @T 3 fire']),
                             (40, ['damage @T 3 lava']), (60, ['damage @T 3 fire_tick']),
                             (80, ['damage @T 3 entity_attack'])]),
        ('health-boost', [(10, ['effect @T health_boost 5 0']), (40, ['effect @T health_boost 5 1'])]),
        ('absorption', [(10, ['effect @T absorption 5 0']), (30, ['damage @T 2']),
                        (60, ['effect @T absorption 5 1'])]),
        ('speed-slowness', [(10, ['effect @T speed 3 1']), (40, ['effect @T slowness 3 1']),
                            (70, ['effect @T clear'])]),
        ('invisibility', [(10, ['effect @T invisibility 3 0'])]),
        ('hidden', [(10, ['effect @T speed 5 0 true']), (40, ['effect @T poison 5 0'])]),
        ('several', [(10, ['effect @T speed 5 0']), (20, ['effect @T poison 5 0']),
                     (30, ['effect @T weakness 5 0']), (40, ['effect @T absorption 5 0'])]),
        ('stacking', [(10, ['effect @T poison 10 0']), (30, ['effect @T poison 4 1']),
                      (50, ['effect @T poison 20 0']), (70, ['effect @T poison 2 0'])]),
        ('clear', [(10, ['effect @T poison 10 0', 'effect @T speed 10 0']), (40, ['effect @T clear'])]),
        ('expiry', [(10, ['effect @T poison 1 0', 'effect @T speed 2 0'])]),
    ]
    mobs = [mob_item(key, 'cow', actions, ticks=300 if key == 'stacking' else 160) for key, actions in shared]
    for key, actions in shared:
        if key in ('poison', 'regeneration', 'wither', 'instant-damage', 'instant-health'):
            mobs.append(mob_item(key, 'zombie', actions))
    # Out of the cell, frozen so its AI cannot move it sideways.
    mobs.append(mob_item('levitation', 'cow', [(10, ['effect @T levitation 2 1'])], ticks=120, cell=False,
                         frozen=True))
    mobs.append(mob_item('slow-falling', 'cow', [(0, ['effect @T slow_falling 10 0'])], ticks=120, cell=False,
                         frozen=True, at=[3.5, -40, 0.5, 90]))
    players = [player_item(key, actions, ticks=300 if key == 'stacking' else 160) for key, actions in shared]
    players.append(player_item('hunger', [(10, ['effect @T hunger 5 1'])]))
    players.append(player_item('saturation', [(5, ['effect @T hunger 4 250']),
                                              (100, ['effect @T saturation 1 0'])], ticks=180))
    sets = {'mobs': mobs, 'players': players, 'all': mobs + players}
    choice = 'all' if args.items == 'slice' else args.items
    if choice not in sets:
        raise SystemExit(f'effects --items must be one of {sorted(sets)}')
    return sets[choice]


def ranged_items(args) -> list[dict]:
    """`ranged`: a frozen skeleton (stray, bogged, parched) 8 or 15 blocks
    east of the sweep player, who stands in survival with Resistance V (no
    damage) and Regeneration so it lives through every shot; the mob is
    sampled every tick for 300 ticks and the capture holds every arrow it
    shoots (run with `--client-capture`, once per `--difficulty`).
    `--items geometry` moves the shooter and the player off the axis and up
    and down, `spread` keeps the player in a barrier pen for 1200 ticks of
    shots, `damage` drops the Resistance (Health Boost keeps the player
    alive) and `avoid` leaves an unfrozen skeleton next to a frozen wolf."""
    target = f'"{PLAYER}"'
    protect = [f'gamemode survival {target}', f'effect {target} resistance 100000 4 true',
               f'effect {target} regeneration 100000 4 true']
    restore = [f'effect {target} clear', f'gamemode creative {target}', 'kill @e[type=minecraft:arrow]']
    choice = args.items
    if choice == 'slice':
        items = []
        for mob, distance in (('skeleton', 8), ('skeleton', 15), ('stray', 8), ('bogged', 8), ('parched', 8)):
            items.append({'key': f'{mob}|shoot-{distance}', 'mob': f'minecraft:{mob}',
                          'at': [0.5 + distance, -60, 0.5, 90], 'player': [0.5, -60, 0.5, -90], 'frozen': True,
                          'ticks': 300, 'setup': protect, 'cleanup': restore, 'actions': []})
        return items
    # A barrier wall west, north and south of the player keeps it in place
    # when an arrow knocks it back; the east side, towards the mobs, is open.
    pen = ['fill -1 -60 -1 -1 -58 1 barrier', 'fill 0 -60 -1 1 -58 -1 barrier', 'fill 0 -60 1 1 -58 1 barrier']
    unpen = [line.replace(' barrier', ' air') for line in pen]
    if choice == 'geometry':
        # Off-axis and raised shooters and targets pin where the arrow
        # spawns (relative to the look or to the target) and how it aims.
        rows = [
            ('skeleton|off-axis', [8.5, -60, 3.5, 90], [0.5, -60, 0.5, -90], [], []),
            ('skeleton|diagonal', [5.5, -60, 5.5, 135], [0.5, -60, 0.5, -45], [], []),
            ('skeleton|player-raised', [8.5, -60, 0.5, 90], [0.5, -57, 0.5, -90],
             ['fill 0 -60 0 0 -58 0 stone'], ['fill 0 -60 0 0 -58 0 air']),
            ('skeleton|mob-raised', [8.5, -56, 2.5, 90], [0.5, -60, 0.5, -90],
             ['fill 8 -60 2 8 -57 2 stone'], ['fill 8 -60 2 8 -57 2 air']),
            ('stray|off-axis', [6.5, -60, -4.5, 45], [0.5, -60, 0.5, -90], [], []),
        ]
        return [{'key': key, 'mob': mob_of(key), 'at': at, 'player': player, 'frozen': True, 'ticks': 200,
                 'setup': setup + protect, 'cleanup': restore + cleanup, 'actions': []}
                for key, at, player, setup, cleanup in rows]
    if choice == 'spread':
        # Many shots for the spread statistics: the player is held in place
        # by the barrier pen so every shot has the same geometry.
        return [{'key': f'skeleton|spread-{n}', 'mob': 'minecraft:skeleton', 'at': [8.5, -60, 0.5, 90],
                 'player': [0.5, -60, 0.5, -90], 'frozen': True, 'ticks': 1200,
                 'setup': pen + protect, 'cleanup': restore + unpen, 'actions': []} for n in range(2)]
    if choice in ('damage', 'damage-long'):
        # The damage an arrow deals: no Resistance, the player lives on
        # Health Boost X, a full heal and Regeneration V. `damage-long`
        # repeats the skeleton for the damage statistics.
        alive = [f'gamemode survival {target}', f'effect {target} health_boost 100000 9 true',
                 f'effect {target} regeneration 100000 4 true']
        heal = [{'t': 0, 'commands': [f'effect {target} instant_health 1 9 true']}]
        cases = ([('skeleton', ''), ('stray', ''), ('bogged', '')] if choice == 'damage'
                 else [('skeleton', f'-{n}') for n in range(4)])
        return [{'key': f'{mob}|damage{suffix}', 'mob': f'minecraft:{mob}', 'at': [8.5, -60, 0.5, 90],
                 'player': [0.5, -60, 0.5, -90], 'frozen': True, 'ticks': 900 if choice == 'damage' else 1200,
                 'setup': pen + alive, 'cleanup': restore + unpen, 'actions': heal}
                for mob, suffix in cases]
    if choice in ('avoid', 'avoid-many'):
        # `behavior.avoid_mob_type`: a skeleton two blocks from a frozen wolf
        # with no target (the player is in creative) runs away from it;
        # `avoid-many` repeats it 20 times for where the flight ends.
        wolf = ['summon minecraft:wolf 2.5 -60 6.5', 'effect @e[type=minecraft:wolf] slowness 100000 255 true',
                'effect @e[type=minecraft:wolf] resistance 100000 4 true']
        return [{'key': f'skeleton|avoid-{n}', 'mob': 'minecraft:skeleton', 'at': [4.5, -60, 6.5, 0],
                 'player': [0.5, -60, -12.5, 0], 'frozen': False, 'ticks': 200 if choice == 'avoid' else 120,
                 'setup': wolf, 'cleanup': ['kill @e[type=minecraft:wolf]', 'kill @e[type=minecraft:arrow]'],
                 'actions': []}
                for n in range(3 if choice == 'avoid' else 20)]
    if choice == 'weapon':
        # `has_ranged_weapon`: which held items keep a skeleton shooting.
        # The frozen skeleton's hand is replaced on tick 5; one that loses
        # its ranged weapon turns to melee and never shoots.
        rows = []
        for weapon in ('bow', 'crossbow', 'trident', 'snowball', 'stick', 'air'):
            hand = f'replaceitem entity @e[type=minecraft:skeleton] slot.weapon.mainhand 0 minecraft:{weapon}'
            rows.append({'key': f'skeleton|hold-{weapon}', 'mob': 'minecraft:skeleton', 'at': [8.5, -60, 0.5, 90],
                         'player': [0.5, -60, 0.5, -90], 'frozen': True, 'ticks': 160, 'setup': pen + protect,
                         'cleanup': restore + unpen, 'actions': [{'t': 5, 'commands': [hand]}]})
        return rows
    if choice == 'aim':
        # How the shot aims: the player pinned in a barrier pen at several
        # heights and distances from the frozen skeleton (run on hard, the
        # smallest spread). `(distance, player raise, mob raise)`.
        rows = []
        for distance, up, down in ((4, 0, 0), (8, 0, 0), (12, 0, 0), (8, 3, 0), (8, 6, 0), (8, 0, 3),
                                   (8, 0, 6)):
            py, my = -60 + up, -60 + down
            setup = [f'fill -1 {py} -1 -1 {py + 2} 1 barrier', f'fill 0 {py} -1 1 {py + 2} -1 barrier',
                     f'fill 0 {py} 1 1 {py + 2} 1 barrier']
            if up:
                setup.append(f'fill 0 -60 0 0 {py - 1} 0 stone')
            if down:
                setup.append(f'fill {distance} -60 0 {distance} {my - 1} 0 stone')
            cleanup = [line.replace(' barrier', ' air').replace(' stone', ' air') for line in setup]
            rows.append({'key': f'skeleton|aim-{distance}-up{up}-down{down}', 'mob': 'minecraft:skeleton',
                         'at': [distance + 0.5, my, 0.5, 90], 'player': [0.5, py, 0.5, -90], 'frozen': True,
                         'ticks': 400, 'setup': setup + protect, 'cleanup': restore + cleanup, 'actions': []})
        return rows
    if choice == 'pickup':
        # Arrow pickup: a dispenser facing down at (1, -56, 0) shoots an
        # arrow into the floor next to the survival player (`near`), or
        # while the player stands 6 blocks away and is moved next to it 60
        # ticks later (`late`); the capture holds the `TakeItemActor`.
        dispenser = ['setblock 1 -56 0 dispenser ["facing_direction"=0]',
                     'replaceitem block 1 -56 0 slot.container 0 arrow 1']
        fire = {'t': 5, 'commands': ['setblock 1 -55 0 redstone_block']}
        undo = ['setblock 1 -55 0 air', 'setblock 1 -56 0 air', 'kill @e[type=minecraft:arrow]',
                f'clear {target}', f'gamemode creative {target}']
        rows = []
        for n in range(3):
            rows.append({'key': f'arrow|pickup-near-{n}', 'mob': None, 'at': None,
                         'player': [0.5, -60, 0.5, -90], 'frozen': False, 'ticks': 100,
                         'setup': [f'gamemode survival {target}'] + dispenser,
                         'cleanup': undo, 'actions': [fire]})
            rows.append({'key': f'arrow|pickup-late-{n}', 'mob': None, 'at': None,
                         'player': [0.5, -60, 6.5, -90], 'frozen': False, 'ticks': 120,
                         'setup': [f'gamemode survival {target}'] + dispenser,
                         'cleanup': undo,
                         'actions': [fire, {'t': 65, 'commands': [f'tp {target} 0.5 -60 0.5 -90 0']}]})
        return rows
    if choice == 'filters':
        # `is_underground`: a stray given `change_to_skeleton` turns into a
        # skeleton when underground, and one time in five anyway; four
        # repeats per roof. `in_block`: a skeleton with powder snow at its
        # feet or only at its head shakes (`is_shaking`) when it is in it.
        rows = []
        roofs = {'open': None, 'stone-3': ('stone', 3), 'stone-12': ('stone', 12), 'glass-3': ('glass', 3),
                 'leaves-3': ('oak_leaves', 3)}
        for name, roof in roofs.items():
            setup = [] if roof is None else [f'setblock 4 {-60 + roof[1]} 0 {roof[0]}']
            cleanup = [] if roof is None else [f'setblock 4 {-60 + roof[1]} 0 air']
            for n in range(4):
                rows.append({'key': f'stray|underground-{name}-{n}', 'mob': 'minecraft:stray',
                             'at': [4.5, -60, 0.5, 90], 'player': [0.5, -60, -12.5, 0], 'frozen': True,
                             'ticks': 40, 'setup': setup, 'cleanup': cleanup + ['kill @e[type=minecraft:skeleton]',
                                                                               'kill @e[type=minecraft:stray]'],
                             'actions': [{'t': 5, 'commands': [
                                 'event entity @e[type=minecraft:stray] change_to_skeleton']}]})
        # The skeleton's `minecraft:lightning_immune` damage sensor filters
        # on `other_with_families`, which is no filter key: with and
        # without `spring_trap`, how much of a hit gets through.
        for trap in (False, True):
            hurt = ['damage @e[type=minecraft:skeleton] 4 entity_attack']
            actions = ([{'t': 5, 'commands': ['event entity @e[type=minecraft:skeleton] minecraft:spring_trap']}]
                       if trap else []) + [{'t': 15, 'commands': hurt},
                                           {'t': 40, 'commands': ['damage @e[type=minecraft:skeleton] 3 fall']}]
            rows.append({'key': f'skeleton|spring-trap-{str(trap).lower()}', 'mob': 'minecraft:skeleton',
                         'at': [4.5, -60, 0.5, 90], 'player': [0.5, -60, -12.5, 0], 'frozen': True,
                         'ticks': 60, 'setup': [], 'cleanup': [], 'actions': actions})
        for name, y in (('feet', -60), ('head', -59)):
            rows.append({'key': f'skeleton|powder-{name}', 'mob': 'minecraft:skeleton', 'at': [4.5, -60, 0.5, 90],
                         'player': [0.5, -60, -12.5, 0], 'frozen': True, 'ticks': 60,
                         'setup': [f'setblock 4 {y} 0 powder_snow'], 'cleanup': [f'setblock 4 {y} 0 air'],
                         'actions': []})
        return rows
    raise SystemExit("ranged --items must be one of slice, geometry, spread, damage, damage-long, avoid, "
                     "avoid-many, weapon, aim, filters, pickup")


def mob_of(key: str) -> str:
    """The mob identifier of a `<mob>|<case>` item key."""
    return f'minecraft:{key.split("|")[0]}'


def silverfish_items(args) -> list[dict]:
    """`silverfish`: `--items merge` leaves a silverfish (no target: the
    player is creative and far) in a 3 × 3 pit dug into stone for 600 ticks,
    ten times, for when and where it merges into the stone
    (`behavior.silverfish_merge_with_stone`; the capture holds the block
    updates). `--items wake` puts infested stone at several distances from
    a frozen silverfish and hurts it on tick 20, three times, for which
    blocks `behavior.silverfish_wake_up_friends` breaks and when."""
    far = [0.5, -60, -24.5, 0]
    if args.items in ('slice', 'merge'):
        pit = ['fill 2 -61 -2 8 -58 4 stone', 'fill 4 -60 0 6 -58 2 air']
        undo = ['fill 2 -61 -2 8 -58 4 air', 'fill 2 -61 -2 8 -61 4 grass_block',
                'kill @e[type=minecraft:silverfish]']
        return [{'key': f'silverfish|merge-{n}', 'mob': 'minecraft:silverfish', 'at': [5.5, -60, 1.5, 0],
                 'player': far, 'frozen': False, 'ticks': 600, 'setup': pit, 'cleanup': undo, 'actions': []}
                for n in range(10)]
    if args.items in ('wake', 'wake-nogrief'):
        # Infested stone along +x at 1..12 blocks, along +z at 1..6 and
        # 2..6 blocks up; stone underneath so nothing falls.
        cells = ([(x, -60, 0) for x in range(1, 13)] + [(0, -60, z) for z in range(1, 7)]
                 + [(0, y, 0) for y in range(-58, -53)])
        place = [f'setblock {x + 4} {y} {z} infested_stone' for x, y, z in cells]
        undo = [f'setblock {x + 4} {y} {z} air' for x, y, z in cells] + ['kill @e[type=minecraft:silverfish]']
        # The silverfish only turns angry with a target: the survival player
        # 4 blocks west, protected, then hits it through `/damage`.
        target = f'"{PLAYER}"'
        protect = [f'gamemode survival {target}', f'effect {target} resistance 100000 4 true',
                   f'effect {target} regeneration 100000 4 true']
        restore = [f'effect {target} clear', f'gamemode creative {target}']
        hurt = {'t': 40, 'commands': [f'damage @e[type=minecraft:silverfish] 1 entity_attack entity {target}']}
        # `wake-nogrief`: the same with mobGriefing off.
        rules = ['gamerule mobgriefing false'] if args.items == 'wake-nogrief' else []
        return [{'key': f'silverfish|{args.items}-{n}', 'mob': 'minecraft:silverfish', 'at': [4.5, -60, 0.5, 90],
                 'player': [0.5, -60, 0.5, -90], 'frozen': True, 'ticks': 300, 'setup': rules + place + protect,
                 'cleanup': restore + undo + ['gamerule mobgriefing true'], 'actions': [hurt]}
                for n in range(3)]
    if args.items in ('pit', 'blocks', 'griefing', 'ab'):
        # A 1 x 1 pit two blocks deep in a 3 x 3 x 4 block of one kind: the
        # silverfish's feet block has five neighbours of that kind and
        # nowhere to walk. `pit` repeats stone 20 times for when it merges,
        # `blocks` tries each kind three times for which ones it merges
        # into (and what they become), `griefing` turns mobGriefing off and
        # `ab` interleaves stone and cobblestone pits.
        def pit(block: str, n: int, ticks: int, rules=()) -> dict:
            cut = ['fill 4 -61 0 6 -58 2 ' + block, 'fill 5 -60 1 5 -59 1 air']
            undo = ['fill 4 -61 0 6 -58 2 air', 'fill 4 -61 0 6 -61 2 grass_block',
                    'kill @e[type=minecraft:silverfish]', 'gamerule mobgriefing true']
            # `deepslate ["pillar_axis"="x"]` -> `deepslate-x`.
            name = '-'.join([block.split(' [')[0]] + re.findall(r'="([^"]+)"', block))
            return {'key': f'silverfish|{args.items}-{name}-{n}', 'mob': 'minecraft:silverfish',
                    'at': [5.5, -60, 1.5, 0], 'player': far, 'frozen': False, 'ticks': ticks,
                    'setup': list(rules) + cut, 'cleanup': undo, 'actions': []}
        if args.items == 'pit':
            return [pit('stone', n, 400) for n in range(20)]
        if args.items == 'ab':
            # Stone and cobblestone pits interleaved, 20 each: whether the
            # host block changes how soon the silverfish merges.
            return [pit(block, n, 300) for n in range(20) for block in ('stone', 'cobblestone')]
        if args.items == 'griefing':
            return [pit('stone', n, 400, ['gamerule mobgriefing false']) for n in range(3)]
        kinds = ['cobblestone', 'stone_bricks', 'mossy_stone_bricks', 'cracked_stone_bricks',
                 'chiseled_stone_bricks', 'deepslate', 'deepslate ["pillar_axis"="x"]', 'cobbled_deepslate',
                 'andesite', 'granite', 'diorite', 'tuff', 'smooth_stone', 'infested_stone']
        return [pit(kind, n, 300) for kind in kinds for n in range(3)]
    raise SystemExit('silverfish --items must be merge, wake, wake-nogrief, pit, blocks, griefing or ab')


def creeper_items(args) -> list[dict]:
    """`creeper`: a creeper and the survival sweep player (Resistance V, so
    the blast hurts nothing; a barrier pen holds it in place), sampled every
    tick; run with `--client-capture` for the creeper's flags, fuse actor
    data, sounds and the explosion's packets. `--items slice` (the default,
    or a comma-separated list of the item names below):
    `approach` (an unfrozen creeper walks in from 6 blocks, three times),
    `cancel` (a frozen creeper 2 blocks away lights; the player leaves to 10
    blocks on tick 15 and comes back on tick 60), `sight` (the player hides
    behind a stone wall on tick 15), `flint` (a frozen creeper 4 blocks
    away, outside the sensor, lit with flint and steel on tick 20),
    `charged` (`minecraft:become_charged` on tick 0, then lit),
    `no-griefing` (mobGriefing false), `flint-alone` (lit by a creative
    player, so without a target) and `gap` / `gap-far` (the player steps back
    from a lit creeper, no pen). Every item rebuilds the floor the blast
    broke."""
    target = f'"{PLAYER}"'
    protect = [f'gamemode survival {target}', f'effect {target} resistance 100000 4 true']
    pen = ['fill -1 -60 -1 -1 -58 1 barrier', 'fill 0 -60 -1 1 -58 -1 barrier', 'fill 0 -60 1 1 -58 1 barrier']
    floor = ['fill -8 -60 -8 14 -50 8 air', 'fill -8 -63 -8 14 -62 8 dirt', 'fill -8 -61 -8 14 -61 8 grass_block',
             'kill @e[type=minecraft:item]', 'kill @e[type=minecraft:creeper]']
    restore = [f'effect {target} clear', f'gamemode creative {target}', 'gamerule mobgriefing true'] + floor
    home = [0.5, -60, 0.5, -90]
    away = f'tp {target} -9.5 -60 0.5 -90 0'
    back = f'tp {target} 0.5 -60 0.5 -90 0'

    def item(key, at, frozen, ticks, actions, setup=(), hold=None, target=True):
        row = {'key': f'creeper|{key}', 'mob': 'minecraft:creeper', 'at': at, 'player': home, 'frozen': frozen,
               'ticks': ticks, 'setup': list(setup) + (protect if target else []), 'cleanup': restore,
               'actions': actions}
        if hold is not None:
            row['hold'] = hold
        return row

    near = [2.5, -60, 0.5, 90]
    # Stone, not barrier, for the wall that hides the player.
    wall = 'fill 1 -60 -1 1 -58 1 stone'
    items = [item(f'approach-{n}', [6.5, -60, 0.5, 90], False, 160, []) for n in range(3)]
    items += [
        item('cancel', near, True, 140, [{'t': 15, 'commands': [away]}, {'t': 60, 'commands': [back]}]),
        item('sight', near, True, 140, [{'t': 15, 'commands': [wall]}]),
        # The first use after the hand changed is refused (it resyncs the
        # slot), so the player uses the flint and steel twice.
        item('flint', [4.5, -60, 0.5, 90], True, 80, [{'t': 10, 'commands': [], 'order': 'interact'},
                                                      {'t': 20, 'commands': [], 'order': 'interact'}],
             hold={'item': 'minecraft:flint_and_steel'}),
        item('charged', near, True, 80, [{'t': 0, 'commands': ['event entity @e[type=minecraft:creeper] '
                                                               'minecraft:become_charged']}]),
        item('no-griefing', near, True, 80, [], setup=['gamerule mobgriefing false']),
        # No target: the creative player lights a frozen creeper 3 blocks away.
        item('flint-alone', [3.5, -60, 0.5, 90], True, 80, [{'t': 10, 'commands': [], 'order': 'interact'},
                                                            {'t': 20, 'commands': [], 'order': 'interact'}],
             hold={'item': 'minecraft:flint_and_steel'}, target=False),
    ]
    for row in items:
        row['setup'] = pen + row['setup']
    # Lit by the sensor, then the player steps back on tick 40 to 4.9 and
    # 6.4 blocks between centres (4.3 and 5.8 between the boxes): whether the
    # lit creeper follows, and which distance `outside_range` measures.
    for key, x in (('gap', -1.5), ('gap-far', -3.0)):
        items.append(item(key, [6.5, -60, 0.5, 90], False, 120,
                          [{'t': 40, 'commands': [f'tp {target} {x} -60 0.5 -90 0']}]))
    if args.items in ('slice', 'all'):
        return items
    # A comma-separated list of item names (`flint,charged`) reruns those.
    chosen = [row for row in items if row['key'].split('|')[1] in args.items.split(',')]
    if not chosen:
        raise SystemExit(f'creeper --items must be slice or names among {[row["key"] for row in items]}')
    return chosen


SLIMES = ('slime', 'magma_cube')
SLIME_SIZES = ('small', 'medium', 'large')


def slime_items(args) -> list[dict]:
    """`slime`: the slime and the magma cube, summoned at a size with
    `summon <mob> x y z 0 0 spawn_<size>`; every slime and magma cube is
    sampled every tick (`track`), and `--client-capture` holds their
    AddActors, motion and the damage they deal. `--items` picks a set:
    `hop` (a calm mob of each size, no target: the creative player stands
    24 blocks away, 600 ticks, three times), `attack` (the survival
    player, kept alive by Health Boost X and Regeneration V in a barrier
    wall behind it, and the mob 5 blocks east, 400 ticks), `split`
    (a medium or large mob killed by `/damage` from the player on tick 10,
    twelve times, and by `/kill` three times; 80 ticks), `float` (a slime
    in water, a magma cube in lava, 200 ticks) or `all`."""
    target = f'"{PLAYER}"'
    track = [f'minecraft:{mob}' for mob in SLIMES]
    # The job removes the tracked mobs without killing them (a killed slime
    # splits into the next item).
    clean = ['kill @e[type=minecraft:item]', 'kill @e[type=minecraft:xp_orb]']

    def summon(mob, size, at):
        return f'summon minecraft:{mob} {at[0]} {at[1]} {at[2]} {at[3]} 0 spawn_{size}'

    def row(key, mob, size, at, player, ticks, setup=(), cleanup=(), actions=()):
        return {'key': f'{mob}|{key}', 'mob': None, 'at': None, 'player': player, 'frozen': False,
                'ticks': ticks, 'setup': list(setup) + [summon(mob, size, at)],
                'cleanup': list(cleanup) + clean, 'actions': list(actions), 'track': track}

    far = [0.5, -60, -24.5, 0]
    sets = {}
    sets['hop'] = [row(f'hop-{size}-{n}', mob, size, [6.5, -60, 6.5, 0], far, 600)
                   for mob in SLIMES for size in SLIME_SIZES for n in range(3)]
    # A barrier wall behind the player (west) keeps the slime's knockback
    # from pushing it away; nothing at the sides, so a big slime can touch.
    pen = ['fill -1 -60 -3 -1 -57 3 barrier']
    unpen = [line.replace(' barrier', ' air') for line in pen]
    alive = [f'gamemode survival {target}', f'effect {target} health_boost 100000 9 true',
             f'effect {target} regeneration 100000 4 true']
    heal = {'t': 0, 'commands': [f'effect {target} instant_health 1 9 true']}
    sets['attack'] = [row(f'attack-{size}', mob, size, [5.5, -60, 0.5, 90], [0.5, -60, 0.5, -90], 400,
                          setup=pen + alive, cleanup=[f'effect {target} clear', f'gamemode creative {target}'] + unpen,
                          actions=[heal])
                      for mob in SLIMES for size in SLIME_SIZES]
    split = []
    for mob in SLIMES:
        for size in ('medium', 'large'):
            hit = f'damage @e[type=minecraft:{mob}] 100 entity_attack entity {target}'
            split += [row(f'split-{size}-{n}', mob, size, [4.5, -60, 0.5, 30], far, 80,
                          actions=[{'t': 10, 'commands': [hit]}]) for n in range(12)]
            split += [row(f'kill-{size}-{n}', mob, size, [4.5, -60, 0.5, 30], far, 80,
                          actions=[{'t': 10, 'commands': [f'kill @e[type=minecraft:{mob}]']}]) for n in range(3)]
    sets['split'] = split
    pool = {'slime': 'water', 'magma_cube': 'lava'}
    sets['float'] = [row(f'float-{size}', mob, size, [6.5, -62, 0.5, 0], far, 200,
                         setup=[f'fill 3 -63 -3 9 -61 3 {pool[mob]}'],
                         cleanup=['fill 3 -63 -3 9 -62 3 dirt', 'fill 3 -61 -3 9 -61 3 grass_block'])
                     for mob in SLIMES for size in ('small', 'large')]
    if args.items == 'all':
        return [item for name in ('hop', 'attack', 'split', 'float') for item in sets[name]]
    if args.items not in sets:
        raise SystemExit(f'slime --items must be all or one of {sorted(sets)}')
    return sets[args.items]


def slime_spawn_items(args) -> list[dict]:
    """`slime-spawn`: natural spawning on the superflat floor (y -61, below
    the slime-chunk height) on normal difficulty at midnight: the creative
    player stands still at the origin and every slime that appears is
    recorded and removed (`spawn_watch`), every other monster removed; two
    items of `--samples` × 100 ticks."""
    monsters = ['zombie', 'zombie_villager', 'husk', 'skeleton', 'stray', 'bogged', 'creeper', 'spider',
                'cave_spider', 'enderman', 'witch', 'phantom', 'drowned', 'parched']
    setup = ['difficulty normal', 'time set 18000', 'gamerule domobspawning true']
    cleanup = ['gamerule domobspawning false', 'kill @e[type=minecraft:slime]']
    return [{'key': f'slime|spawn-{n}', 'player': [0.5, -60, 0.5, 0], 'setup': setup, 'cleanup': cleanup,
             'ticks': args.samples * 100, 'every': 5, 'watch': ['minecraft:slime'],
             'clear': [f'minecraft:{mob}' for mob in monsters]} for n in range(2)]


# A glass pool east of the player, water x 3..19, y -60..-54, z -7..7 (the
# surface at y -53), and glass land east of it, level with the surface.
POOL = ['fill 2 -61 -8 30 -54 8 glass', 'fill 3 -60 -7 19 -54 7 water']
UNPOOL = ['fill 2 -61 -8 30 -50 8 air', 'fill 2 -63 -8 30 -62 8 dirt', 'fill 2 -61 -8 30 -61 8 grass_block']


def drowned_items(args) -> list[dict]:
    """`drowned`: the drowned in a glass pool (`POOL`), every drowned,
    zombie, husk and thrown trident sampled every tick (`track`), summoned
    with a spawn event (`summon … minecraft:as_adult`). `--items` picks a
    set: `sink` (no target: the creative player stands outside), `hunt`
    (the survival player on the pool floor, on a pillar at the surface, or
    on the land east of the pool), `trident` (a ranged drowned and the
    player in the water or on the land, no Resistance), `day` (noon, the
    player in the water or on the land), `equipment` (200 drowned summoned
    with their natural spawn event, on land), `convert` (a zombie, a baby
    zombie and a husk held under water until they turn) or `all`."""
    target = f'"{PLAYER}"'
    track = ['minecraft:drowned', 'minecraft:zombie', 'minecraft:husk', 'minecraft:thrown_trident']
    clean = ['kill @e[type=minecraft:item]', 'kill @e[type=minecraft:xp_orb]',
             'kill @e[type=minecraft:thrown_trident]']
    alive = [f'gamemode survival {target}', f'effect {target} health_boost 100000 9 true',
             f'effect {target} regeneration 100000 4 true', f'effect {target} water_breathing 100000 0 true']
    tough = alive + [f'effect {target} resistance 100000 4 true']
    restore = [f'effect {target} clear', f'gamemode creative {target}']
    heal = {'t': 0, 'commands': [f'effect {target} instant_health 1 9 true']}
    far = [0.5, -60, -24.5, 0]
    floor = [4.5, -60, 0.5, -90]
    surface = [4.5, -54, 0.5, -90]
    land = [25.5, -53, 0.5, 90]
    pillar = ['fill 4 -60 0 4 -55 0 glass']

    def summon(mob, event, at):
        return f'summon minecraft:{mob} {at[0]} {at[1]} {at[2]} {at[3]} 0 minecraft:{event}'

    def row(key, mob, event, at, player, ticks, setup=(), cleanup=(), actions=()):
        return {'key': f'drowned|{key}', 'mob': None, 'at': None, 'player': player, 'frozen': False,
                'ticks': ticks, 'setup': POOL + list(setup) + [summon(mob, event, at)],
                'cleanup': clean + list(cleanup) + UNPOOL, 'actions': list(actions), 'track': track}

    sets = {}
    sets['sink'] = [row(f'sink-{n}', 'drowned', 'as_adult', [11.5, -56, 0.5, 90], far, 400) for n in range(3)]
    hunt = []
    for n in range(3):
        hunt.append(row(f'hunt-floor-{n}', 'drowned', 'as_adult', [16.5, -60, 0.5, 90], floor, 300,
                        setup=tough, cleanup=restore, actions=[heal]))
        hunt.append(row(f'hunt-surface-{n}', 'drowned', 'as_adult', [16.5, -60, 0.5, 90], surface, 300,
                        setup=pillar + tough, cleanup=restore, actions=[heal]))
        hunt.append(row(f'hunt-land-{n}', 'drowned', 'as_adult', [8.5, -60, 0.5, 270], land, 400,
                        setup=tough, cleanup=restore, actions=[heal]))
    sets['hunt'] = hunt
    trident = []
    for n in range(4):
        trident.append(row(f'trident-water-{n}', 'drowned', 'as_ranged_adult', [14.5, -60, 0.5, 90], floor, 600,
                           setup=alive, cleanup=restore, actions=[heal]))
    for n in range(2):
        trident.append(row(f'trident-land-{n}', 'drowned', 'as_ranged_adult', [10.5, -60, 0.5, 270], land, 600,
                           setup=alive, cleanup=restore, actions=[heal]))
    sets['trident'] = trident
    noon = ['time set 6000']
    night = ['time set 18000']
    sets['day'] = [row(f'day-{where}-{n}', 'drowned', 'as_adult', [16.5, -60, 0.5, 90], spot, 300,
                       setup=noon + tough, cleanup=night + restore,
                       actions=[heal])
                   for n in range(2) for where, spot in (('water', floor), ('land', land))]
    # 50 drowned a batch on the land west of the player, each with its
    # natural spawn event; the capture holds their equipment.
    batch = [f'summon minecraft:drowned {-6.5 - (k % 10) * 2} -60 {-8.5 + (k // 10) * 4} 0 0'
             for k in range(50)]
    sets['equipment'] = [{'key': f'drowned|equipment-{n}', 'mob': None, 'at': None, 'player': far,
                          'frozen': False, 'ticks': 20, 'setup': batch, 'cleanup': clean, 'actions': [],
                          'track': ['minecraft:drowned']} for n in range(4)]
    gear = ['replaceitem entity @e[type=minecraft:zombie] slot.weapon.mainhand 0 minecraft:iron_sword',
            'replaceitem entity @e[type=minecraft:zombie] slot.armor.head 0 minecraft:golden_helmet']
    convert = [row(f'convert-zombie-{n}', 'zombie', 'as_adult', [11.5, -60, 0.5, 90], far, 1100, setup=[],
                   actions=[{'t': 1, 'commands': gear}]) for n in range(3)]
    convert.append(row('convert-baby-0', 'zombie', 'as_baby', [11.5, -60, 0.5, 90], far, 1100))
    convert += [row(f'convert-husk-{n}', 'husk', 'as_adult', [11.5, -60, 0.5, 90], far, 1100) for n in range(2)]
    sets['convert'] = convert
    if args.items == 'all':
        return [item for name in ('sink', 'hunt', 'trident', 'day', 'equipment', 'convert') for item in sets[name]]
    if args.items not in sets:
        raise SystemExit(f'drowned --items must be all or one of {sorted(sets)}')
    return sets[args.items]


def witch_items(args) -> list[dict]:
    """`witch`: a frozen witch (hidden Slowness 255) and the survival player
    in a barrier pen open towards it, kept alive by Health Boost X and
    Regeneration (no Resistance, so the potions land); the witch, the
    splash potions it throws and the player are sampled every tick.
    `--items throw` (the player 3, 6, 9 and 10 blocks away, twice),
    `drink` (the witch under water, standing in fire, hurt by `/damage`, and
    with its target moved from 6 to 14 blocks), `resist` (`/damage` of
    several causes and instant damage on a witch with no target) or `all`."""
    target = f'"{PLAYER}"'
    track = ['minecraft:witch', 'minecraft:splash_potion']
    alive = [f'gamemode survival {target}', f'effect {target} health_boost 100000 9 true',
             f'effect {target} regeneration 100000 4 true']
    restore = [f'effect {target} clear', f'gamemode creative {target}']
    clean = ['kill @e[type=minecraft:splash_potion]', 'kill @e[type=minecraft:item]',
             'kill @e[type=minecraft:xp_orb]']
    pen = ['fill -1 -60 -1 -1 -58 1 barrier', 'fill 0 -60 -1 1 -58 -1 barrier', 'fill 0 -60 1 1 -58 1 barrier']
    unpen = [line.replace(' barrier', ' air') for line in pen]
    heal = {'t': 0, 'commands': [f'effect {target} instant_health 1 9 true']}
    player = [0.5, -60, 0.5, -90]
    far = [0.5, -60, -24.5, 0]

    def row(key, at, spot, ticks, setup=(), cleanup=(), actions=(), frozen=True):
        return {'key': f'witch|{key}', 'mob': 'minecraft:witch', 'at': at, 'player': spot, 'frozen': frozen,
                'ticks': ticks, 'setup': list(setup), 'cleanup': clean + list(cleanup),
                'actions': list(actions), 'track': track}

    sets = {}
    sets['throw'] = [row(f'throw-{d}-{n}', [0.5 + d, -60, 0.5, 90], player, 600, setup=pen + alive,
                         cleanup=restore + unpen, actions=[heal])
                     for d in (3, 6, 9, 10) for n in range(2)]
    fire = ['setblock 6 -60 0 fire']
    hurt = 'damage @e[type=minecraft:witch] 12 entity_attack'
    move = f'tp {target} -7.5 -60 0.5 -90'
    sets['drink'] = (
        [row(f'drink-water-{n}', [11.5, -58, 0.5, 90], far, 400, setup=POOL, cleanup=UNPOOL) for n in range(2)]
        + [row(f'drink-fire-{n}', [6.5, -60, 0.5, 90], far, 300, setup=fire, cleanup=['setblock 6 -60 0 air'])
           for n in range(2)]
        + [row(f'drink-heal-{n}', [6.5, -60, 0.5, 90], far, 400, actions=[{'t': 20, 'commands': [hurt]}])
           for n in range(2)]
        + [row(f'drink-swift-{n}', [6.5, -60, 0.5, 90], player, 400, setup=alive, cleanup=restore,
               actions=[heal, {'t': 80, 'commands': [move]}]) for n in range(2)])
    who = '@e[type=minecraft:witch]'
    sets['resist'] = [row('resist-0', [6.5, -60, 0.5, 90], far, 200, actions=[
        {'t': 10, 'commands': [f'damage {who} 20 magic']},
        {'t': 40, 'commands': [f'effect {who} instant_damage 1 1 true']},
        {'t': 70, 'commands': [f'damage {who} 5 entity_attack']},
        {'t': 100, 'commands': [f'damage {who} 10 wither']},
        {'t': 130, 'commands': [f'effect {who} poison 4 1 true']}])]
    if args.items == 'all':
        return [item for name in ('throw', 'drink', 'resist') for item in sets[name]]
    if args.items not in sets:
        raise SystemExit(f'witch --items must be all or one of {sorted(sets)}')
    return sets[args.items]


def melee_effect_items(args) -> list[dict]:
    """`melee-effects`: a husk spawned two blocks east of the sweep player
    (survival) walks up and hits it for 200 ticks; the player lives through
    the hits on Health Boost X, a full heal and Regeneration V (no
    Resistance, which would change the hits). Run once per `--difficulty`
    with `--client-capture` for the hunger `MobEffect` each hit sends."""
    target = f'"{PLAYER}"'
    protect = [f'gamemode survival {target}', f'effect {target} health_boost 1000 9 true',
               f'effect {target} regeneration 1000 4 true']
    restore = [f'effect {target} clear', f'gamemode creative {target}']
    return [{'key': 'husk|hits', 'mob': 'minecraft:husk', 'at': [2.5, -60, 0.5, 90],
             'player': [0.5, -60, 0.5, -90], 'frozen': False, 'ticks': 200, 'setup': protect,
             'cleanup': restore, 'actions': [{'t': 0, 'commands': [f'effect {target} instant_health 1 9 true']}]}]


def mooshroom_items(args) -> list[dict]:
    """`mooshroom`: the sweep player (survival) interacts with a frozen
    mooshroom two blocks east of it holding a bowl (red mooshroom: stew into
    the hand), an allium (brown mooshroom: eats it, smoke particles) and
    shears (red mooshroom: sheared into a cow, mushrooms dropped). The
    `geometry` items (`--items geometry`) move the particles: the player on a
    block north-west of the mooshroom, a baby mooshroom, and a brown
    mooshroom milked with a bowl after its allium (suspicious stew). One
    mooshroom in 20 spawns a baby, which has no interactions, so the
    geometry items that need an adult grow theirs up first."""
    target = f'"{PLAYER}"'
    setup = [f'gamemode survival {target}']
    restore = [f'gamemode creative {target}', 'kill @e[type=minecraft:item]']
    brown = 'event entity @e[type=minecraft:mooshroom] minecraft:become_brown'
    born = 'event entity @e[type=minecraft:mooshroom] minecraft:entity_born'
    grown = 'event entity @e[type=minecraft:mooshroom] minecraft:ageable_grow_up'
    bowl = f'replaceitem entity {target} slot.weapon.mainhand 0 minecraft:bowl'

    # The first use of an item the plugin put in the hand is refused (the
    # client still holds the old stack) and makes BDS resend the slot, so
    # every item interacts twice, 20 ticks apart.
    def item(key, hold, actions, at=(2.5, -60, 0.5, 90), player=(0.5, -60, 0.5, -90), ticks=80,
             extra_setup=(), extra_cleanup=()):
        uses = [{'t': 10, 'commands': [], 'order': 'interact'}, {'t': 30, 'commands': [], 'order': 'interact'}]
        return {'key': f'mooshroom|{key}', 'mob': 'minecraft:mooshroom', 'at': list(at),
                'player': list(player), 'frozen': True, 'ticks': ticks, 'setup': setup + list(extra_setup),
                'cleanup': list(extra_cleanup) + restore, 'actions': actions + uses, 'hold': hold}

    allium = {'item': 'minecraft:allium'}
    basic = [
        item('bowl', {'item': 'minecraft:bowl'}, []),
        item('allium', allium, [{'t': 5, 'commands': [brown]}]),
        item('shears', {'item': 'minecraft:shears'}, []),
    ]
    geometry = [
        item('allium-raised', allium, [{'t': 3, 'commands': [grown]}, {'t': 5, 'commands': [brown]}],
             at=(2.5, -60, 1.5, 90),
             player=(0.5, -59, 3.5, -135), extra_setup=['setblock 0 -60 3 stone'],
             extra_cleanup=['setblock 0 -60 3 air']),
        item('allium-baby', allium, [{'t': 3, 'commands': [born]}, {'t': 5, 'commands': [brown]}]),
        item('suspicious-stew', allium, [{'t': 3, 'commands': [grown]}, {'t': 5, 'commands': [brown]},
                                         {'t': 40, 'commands': [bowl]},
                                         {'t': 50, 'commands': [], 'order': 'interact'},
                                         {'t': 60, 'commands': [], 'order': 'interact'}], ticks=90),
    ]
    sets = {'basic': basic, 'geometry': geometry, 'all': basic + geometry}
    choice = 'basic' if args.items == 'slice' else args.items
    if choice not in sets:
        raise SystemExit(f'mooshroom --items must be one of {sorted(sets)}')
    return sets[choice]


def reload_items(args) -> list[dict]:
    """`knockback-reload --items spawn` (three zombies, each hit once and left
    in the world) or `--items reload` (the same zombies, loaded from the first
    run's world, hit again). They stand 200 blocks from the world spawn: BDS
    loads the chunks around the spawn while the player is still joining, and a
    zombie loaded there despawns at once (no player counts as near yet)."""
    if args.items not in ('spawn', 'reload'):
        raise SystemExit("knockback-reload --items must be 'spawn' or 'reload'")
    return [{'key': f'zombie-{index}', 'mob': 'minecraft:zombie', 'at': [200.5, -60, 0.5 + 6 * index, 90],
             'from': [-2.0, 0.0], 'yaw': -90, 'phase': args.items} for index in range(3)]


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
    elif args.kind in ('knockback', 'knockback-reload', 'bonus'):
        # Zombies need a difficulty above peaceful and burn in daylight.
        job['setup_commands'] = SETUP + [f'difficulty {args.difficulty}', 'time set 18000']
        job['items'] = {'knockback': knockback_items, 'knockback-reload': reload_items,
                        'bonus': bonus_items}[args.kind](args)
        if args.kind == 'bonus':
            job['kind'] = 'spawn_bonus'
    elif args.kind == 'effects':
        # Zombies need a difficulty above peaceful and burn in daylight.
        job['setup_commands'] = SETUP + [f'difficulty {args.difficulty}', 'time set 18000']
        job['items'] = effect_items(args)
    elif args.kind in ('ranged', 'mooshroom', 'melee-effects', 'silverfish', 'creeper', 'slime', 'drowned',
                       'witch'):
        # The generic per-tick job of `effects`; hostile mobs need a
        # difficulty above peaceful and skeletons burn in daylight.
        job['kind'] = 'effects'
        job['setup_commands'] = SETUP + [f'difficulty {args.difficulty}', 'time set 18000']
        job['items'] = {'ranged': ranged_items, 'mooshroom': mooshroom_items,
                        'melee-effects': melee_effect_items, 'silverfish': silverfish_items,
                        'creeper': creeper_items, 'slime': slime_items, 'drowned': drowned_items,
                        'witch': witch_items}[args.kind](args)
    elif args.kind == 'slime-spawn':
        job['kind'] = 'spawn_watch'
        job['items'] = slime_spawn_items(args)
    elif args.kind == 'entity-loot':
        # Hostile mobs need a difficulty above peaceful.
        job['setup_commands'] = SETUP + [f'difficulty {args.difficulty}', 'time set 18000']
        mobs = ['wither_skeleton'] if args.items == 'slice' else args.items.split(',')
        job['items'] = [{'key': mob, 'mob': f'minecraft:{mob}', 'at': [3.5, -60, 0.5], 'drop': [0, -58, 4],
                         'samples': args.samples} for mob in mobs]
    elif args.kind == 'walk':
        if not args.observer:
            raise SystemExit('walk needs --observer')
        job['items'] = walk_items(args)
    elif args.kind == 'range':
        if not args.observer:
            raise SystemExit('range needs --observer')
        job['kind'] = 'send_range'
        job['items'] = range_items(args)
    if args.observer:
        job['observer'] = OBSERVER_NAME
    return job


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('kind', choices=['drops', 'mining', 'armor-slots', 'armor-values', 'attack', 'fuel',
                                         'knockback', 'knockback-reload', 'range', 'bonus', 'walk',
                                         'effects', 'entity-loot', 'ranged', 'mooshroom', 'melee-effects',
                                         'silverfish', 'creeper', 'slime', 'slime-spawn', 'drowned', 'witch'])
    parser.add_argument('--out', type=Path)
    parser.add_argument('--blocks', default='slice', help="'slice', 'all' or comma-separated identifiers")
    parser.add_argument('--states', default='first', choices=['first', 'all'])
    parser.add_argument('--items', default='slice', help="'slice' or comma-separated identifiers")
    parser.add_argument('--samples', type=int, default=200)
    parser.add_argument('--max-ticks', type=int, default=24000, help='fuel: longest burn to wait for')
    parser.add_argument('--mine-ticks', type=int, default=600, help='mining: longest break to wait for')
    parser.add_argument('--client', type=Path, default=REPO / 'target' / 'debug' / 'bds-sweep-client.exe')
    parser.add_argument('--timeout', type=float, default=6 * 3600, help='seconds for the whole job')
    parser.add_argument('--client-capture', action='store_true',
                        help="record the sweep player's packets to <out>/client/packets.jsonl")
    parser.add_argument('--observer', action='store_true',
                        help=f'add a second client, {OBSERVER_NAME}, recording to <out>/observer/packets.jsonl')
    parser.add_argument('--view-distance', type=int, default=6, help='server view distance in chunks')
    parser.add_argument('--tick-distance', type=int, default=4, help='server simulation distance in chunks (4-12)')
    parser.add_argument('--difficulty', default='easy', choices=['easy', 'normal', 'hard'],
                        help='knockback, knockback-reload, bonus, effects, entity-loot: the difficulty set '
                             'before the job')
    parser.add_argument('--keep-world', action='store_true',
                        help='copy the world to <out>/world after BDS stops (it is deleted otherwise)')
    parser.add_argument('--world-from', type=Path,
                        help='start from a copy of this world folder (a --keep-world copy) instead of a fresh one')
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
    if args.world_from:
        shutil.copytree(args.world_from, world)
    # A world with zombies in it must load above peaceful, which removes them.
    difficulty = args.difficulty if args.kind in ('knockback-reload', 'bonus') else 'peaceful'
    properties = bds_runtime.base_properties(
        LEVEL, level_type='FLAT', gamemode='creative', force_gamemode='true', difficulty=difficulty,
        default_player_permission_level='operator', view_distance=args.view_distance,
        tick_distance=args.tick_distance)
    # The plugin's jobs come from this checkout, ahead of the editable
    # install in the shared venv (which may point at another worktree).
    env = {'BDS_SWEEP_JOB': str(job_path), 'CLIENT_DIFF_CAPTURE_DIR': str(out / 'client-diff-plugin'),
           'PYTHONPATH': str(REPO / 'tools' / 'bds-sweep' / 'endstone-sweep' / 'src')}
    started = time.monotonic()
    status = 'timeout'
    client_command = [str(args.client), '--addr', bds_runtime.ADDR, '--name', PLAYER]
    if args.client_capture:
        client_command += ['--capture-dir', str(out / 'client')]
    observer_command = [str(args.client), '--addr', bds_runtime.ADDR, '--name', OBSERVER_NAME,
                        '--capture-dir', str(out / 'observer')]
    observer = None
    observer_logins = 0
    observer_due = None
    try:
        with bds_runtime.Bds(properties, out / 'bds.log', endstone=True, env=env):
            with ((out / 'client.log').open('a', encoding='utf-8') as log,
                  (out / 'observer.log').open('a', encoding='utf-8') as observer_log):
                client = subprocess.Popen(client_command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT)
                logins = 1
                try:
                    while time.monotonic() - started < args.timeout:
                        # The observer logs in after the sweep player has spawned (its join
                        # starts the job), a few seconds apart: BDS refuses a second loopback
                        # login right after the first ("ip recently connected").
                        if args.observer and (observer is None or observer.poll() not in (None, 0)):
                            if observer_due is None and 'spawned as' in (out / 'client.log').read_text(
                                    encoding='utf-8', errors='replace'):
                                observer_due = time.monotonic() + 6
                            if observer_due is not None and time.monotonic() >= observer_due:
                                if observer_logins >= 5:
                                    status = f'observer exited {observer.returncode}'
                                    break
                                observer_logins += 1
                                observer = subprocess.Popen(observer_command, cwd=REPO, stdout=observer_log,
                                                            stderr=subprocess.STDOUT)
                                observer_due = time.monotonic() + 8
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
                    for process in (client, observer):
                        if process is not None and process.poll() is None:
                            process.kill()
    finally:
        if world.exists():
            if args.keep_world:
                kept = out / 'world'
                if kept.exists():
                    shutil.rmtree(kept)
                shutil.copytree(world, kept)
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
