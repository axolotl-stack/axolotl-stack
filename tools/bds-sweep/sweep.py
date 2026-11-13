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
# The second scripted player of the `placements` job: it watches every click
# from outside the arena and records what another player is sent.
WATCHER = 'BDSWatch'
# The second client of `--observer` (a second viewer of the job's actors).
OBSERVER_NAME = 'BDSOther'
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
         # Survival block clicks: cleared box (offsets) around the origin; the
         # watcher stands west of the box, looking at it.
         'placements': {'origin': [4, -60, 4], 'clear': [[-3, -1, -4], [5, 4, 13]],
                        'watcher': [-6.5, 0, 2.5, -90, 10]}}
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
    shoots (run with `--client-capture`, once per `--difficulty`)."""
    target = f'"{PLAYER}"'
    protect = [f'gamemode survival {target}', f'effect {target} resistance 100000 4 true',
               f'effect {target} regeneration 100000 4 true']
    restore = [f'effect {target} clear', f'gamemode creative {target}', 'kill @e[type=minecraft:arrow]']
    items = []
    for mob, distance in (('skeleton', 8), ('skeleton', 15), ('stray', 8), ('bogged', 8), ('parched', 8)):
        items.append({'key': f'{mob}|shoot-{distance}', 'mob': f'minecraft:{mob}',
                      'at': [0.5 + distance, -60, 0.5, 90], 'player': [0.5, -60, 0.5, -90], 'frozen': True,
                      'ticks': 300, 'setup': protect, 'cleanup': restore, 'actions': []})
    return items


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
    elif args.kind == 'armor-wear':
        job.update(max_health=2000, amounts=ARMOR_WEAR_AMOUNTS, items=armor_wear_items(args))
    elif args.kind == 'attack':
        job.update(target='minecraft:pig', items=attack_items(args))
        if args.gamemode:
            job['gamemode'] = args.gamemode
    elif args.kind == 'scenarios':
        job['items'] = scenarios.items(args.family)
    elif args.kind == 'placements':
        job['items'] = placements.items(args.cases)
        if not args.no_watcher:
            job['watcher'] = WATCHER
    elif args.kind == 'item-blocks':
        job['items'] = [{'key': name.removeprefix('minecraft:'), 'item': name}
                        for name in item_names(args.items, native.items())]
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
    elif args.kind in ('ranged', 'mooshroom', 'melee-effects'):
        # The generic per-tick job of `effects`; hostile mobs need a
        # difficulty above peaceful and skeletons burn in daylight.
        job['kind'] = 'effects'
        job['setup_commands'] = SETUP + [f'difficulty {args.difficulty}', 'time set 18000']
        job['items'] = {'ranged': ranged_items, 'mooshroom': mooshroom_items,
                        'melee-effects': melee_effect_items}[args.kind](args)
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
    parser.add_argument('kind', choices=['drops', 'mining', 'armor-slots', 'armor-values', 'armor-wear', 'attack',
                                         'fuel', 'scenarios', 'placements', 'item-blocks', 'knockback',
                                         'knockback-reload', 'range', 'bonus', 'walk', 'effects', 'entity-loot',
                                         'ranged', 'mooshroom', 'melee-effects'])
    parser.add_argument('--out', type=Path)
    parser.add_argument('--blocks', default='slice', help="'slice', 'all' or comma-separated identifiers")
    parser.add_argument('--states', default='first', choices=['first', 'all'])
    parser.add_argument('--items', default='slice', help="'slice' or comma-separated identifiers")
    parser.add_argument('--samples', type=int, default=200)
    parser.add_argument('--family', default='pilot', help="scenarios: 'all' or comma-separated families")
    parser.add_argument('--cases', default='all',
                        help="placements: 'all' or comma-separated case groups (placements.GROUPS)")
    parser.add_argument('--no-watcher', action='store_true',
                        help='placements: run without the second (watching) scripted player')
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
    watcher_command = None
    if job.get('watcher'):
        watcher_command = [str(args.client), '--addr', bds_runtime.ADDR, '--name', job['watcher'],
                           '--capture-dir', str(out / 'watcher-capture')]
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
                # BDS refuses a second login from the same address right after
                # the first ("ip recently connected"): the watcher joins once
                # the player has spawned, and retries.
                watcher = None
                watcher_logins = 0
                watcher_next = 0.0
                watcher_log = (out / 'watcher.log').open('a', encoding='utf-8') if watcher_command else None
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
                        if watcher_command and (watcher is None or watcher.poll() is not None):
                            spawned = 'spawned as' in (out / 'client.log').read_text(encoding='utf-8',
                                                                                     errors='replace')
                            if watcher is not None and (watcher.returncode == 0 or watcher_logins >= 6):
                                status = f'watcher exited {watcher.returncode}'
                                break
                            if spawned and time.monotonic() >= watcher_next:
                                watcher_logins += 1
                                watcher_next = time.monotonic() + 15
                                watcher = subprocess.Popen(watcher_command, cwd=REPO, stdout=watcher_log,
                                                           stderr=subprocess.STDOUT)
                        time.sleep(1)
                finally:
                    for process in (client, observer, watcher):
                        if process is not None and process.poll() is None:
                            process.kill()
                    if watcher_log is not None:
                        watcher_log.close()
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
