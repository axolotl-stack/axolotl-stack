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
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import native  # noqa: E402
import pvp_items  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
LEVEL = 'bds-sweep-1.26.51'
PLAYER = 'BDSSweep'
# The second client of `--observer` (a second viewer of the job's actors).
OBSERVER_NAME = 'BDSOther'
# The lockstep clients of `pvp` after the sweep player (B, then C).
PVP_OTHERS = ['BDSOther', 'BDSThird']
PVP_SETUP = ['gamerule pvp true', 'gamerule keepinventory true', 'gamerule naturalregeneration false',
             'gamerule doimmediaterespawn true', 'gamerule falldamage true']
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
    sets = {'slice': hits, 'air': air, 'far': far, 'cadence': cadence_items(), 'cadence2': cadence2_items(),
            'cadence3': cadence3_items(), 'all': hits + air + far}
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
    elif args.kind == 'walk':
        if not args.observer:
            raise SystemExit('walk needs --observer')
        job['items'] = walk_items(args)
    elif args.kind == 'range':
        if not args.observer:
            raise SystemExit('range needs --observer')
        job['kind'] = 'send_range'
        job['items'] = range_items(args)
    elif args.kind == 'pvp':
        job.update(others=PVP_OTHERS, pvp_setup=PVP_SETUP,
                   items=pvp_items.pvp_items(args.items, args.repeats, args.entity_interactions_strict))
    if args.observer:
        job['observer'] = OBSERVER_NAME
    return job


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('kind', choices=['drops', 'mining', 'armor-slots', 'armor-values', 'attack', 'fuel',
                                         'knockback', 'knockback-reload', 'range', 'bonus', 'walk', 'pvp'])
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
                        help='knockback, knockback-reload, bonus: the difficulty set before the job')
    parser.add_argument('--repeats', type=int, default=3, help='pvp: runs of each item')
    parser.add_argument('--entity-interactions-strict', action='store_true',
                        help='pvp: run BDS with server-authoritative-entity-interactions-strict=true')
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
    if args.kind == 'pvp':
        # Three lockstep clients; strict entity interactions on request.
        properties['max-players'] = 1 + len(PVP_OTHERS)
        properties['server-authoritative-entity-interactions-strict'] = str(
            args.entity_interactions_strict).lower()
    # The plugin's jobs come from this checkout, ahead of the editable
    # install in the shared venv (which may point at another worktree).
    env = {'BDS_SWEEP_JOB': str(job_path), 'CLIENT_DIFF_CAPTURE_DIR': str(out / 'client-diff-plugin'),
           'PYTHONPATH': str(REPO / 'tools' / 'bds-sweep' / 'endstone-sweep' / 'src')}
    started = time.monotonic()
    status = 'timeout'
    client_command = [str(args.client), '--addr', bds_runtime.ADDR, '--name', PLAYER]
    if args.client_capture:
        client_command += ['--capture-dir', str(out / 'client')]
    if args.kind == 'pvp':
        # One process for all three, so the plugin orders their packets.
        for other in PVP_OTHERS:
            client_command += ['--also', other]
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
