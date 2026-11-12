"""Block scenario families for the `scenarios` sweep job.

Each family returns items `{key, family, steps}` for the Endstone job of the
same name (`endstone-sweep/.../jobs.py`): blocks are set in a cell of their
own, with or without the block update a placement triggers, and read back
after a wait. Offsets are relative to the cell origin, which stands on the
superflat grass (the floor is offset y=-1).

Families:

* `pilot`: how BDS applies block updates (Endstone `set_data` with and
  without physics, `/setblock`) for stairs, fences, walls, torches, carpets,
  doors, flowers and lanterns.
"""

from __future__ import annotations

import json

EAST, WEST, SOUTH, NORTH = [1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]
UP, DOWN = [0, 1, 0], [0, -1, 0]
HERE = [0, 0, 0]


def key(family: str, *parts) -> str:
    return family + '|' + '|'.join(json.dumps(p, sort_keys=True, separators=(',', ':')) for p in parts)


def set_(offset, block: str, states: dict | None = None, physics: bool = True) -> list:
    return ['set', offset, block, states or {}, physics]


def setblock(offset, block: str, states: dict | None = None, mode: str = 'replace') -> list:
    return ['setblock', offset, block, states or {}, mode]


def read(label: str, *offsets) -> list:
    return ['read', label, list(offsets)]


def wait(ticks: int = 2) -> list:
    return ['wait', ticks]


def item(family: str, name: str, steps: list) -> dict:
    return {'key': key(family, name), 'family': family, 'steps': steps}


def pilot() -> list[dict]:
    """One cell per question; every item has its own step list, so each runs
    in a wave of its own (the waits differ in position)."""
    stairs_e = {'weirdo_direction': 0, 'upside_down_bit': False, 'minecraft:corner': 'none'}
    stairs_s = {'weirdo_direction': 2, 'upside_down_bit': False, 'minecraft:corner': 'none'}
    torch_w = {'torch_facing_direction': 'west'}
    items = []
    for how in ('physics', 'no_physics', 'setblock'):
        def put(offset, block, states=None):
            if how == 'setblock':
                return setblock(offset, block, states)
            return set_(offset, block, states, how == 'physics')

        items += [
            item('pilot', f'stairs_pair_{how}', [
                put(HERE, 'minecraft:oak_stairs', stairs_e), put(EAST, 'minecraft:oak_stairs', stairs_s),
                read('now', HERE, EAST), wait(), read('later', HERE, EAST)]),
            item('pilot', f'fence_after_stone_{how}', [
                put(EAST, 'minecraft:stone'), put(HERE, 'minecraft:oak_fence'),
                read('now', HERE), wait(), read('later', HERE)]),
            item('pilot', f'stone_after_fence_{how}', [
                put(HERE, 'minecraft:oak_fence'), put(EAST, 'minecraft:stone'),
                read('now', HERE), wait(), read('later', HERE)]),
            item('pilot', f'wall_between_walls_{how}', [
                put(EAST, 'minecraft:cobblestone_wall'), put(WEST, 'minecraft:cobblestone_wall'),
                put(HERE, 'minecraft:cobblestone_wall'), read('now', WEST, HERE, EAST), wait(),
                put(UP, 'minecraft:stone'), wait(), read('later', WEST, HERE, EAST)]),
            item('pilot', f'torch_loses_wall_{how}', [
                put(EAST, 'minecraft:stone'), put(HERE, 'minecraft:torch', torch_w), wait(),
                read('placed', HERE), put(EAST, 'minecraft:air'), read('now', HERE), wait(),
                read('later', HERE)]),
            item('pilot', f'carpet_loses_floor_{how}', [
                put(DOWN, 'minecraft:stone'), put(HERE, 'minecraft:white_carpet'), wait(),
                read('placed', HERE), put(DOWN, 'minecraft:air'), read('now', HERE), wait(),
                read('later', HERE)]),
            item('pilot', f'door_lower_{how}', [
                put(HERE, 'minecraft:wooden_door', {'upper_block_bit': False}), read('now', HERE, UP), wait(),
                read('later', HERE, UP), put(DOWN, 'minecraft:air'), wait(), read('unsupported', HERE, UP)]),
            item('pilot', f'flower_on_stone_{how}', [
                put(DOWN, 'minecraft:stone'), put(HERE, 'minecraft:dandelion'), read('now', HERE), wait(),
                read('later', HERE), put(EAST, 'minecraft:stone'), wait(), read('poked', HERE)]),
            item('pilot', f'lantern_loses_floor_{how}', [
                put(DOWN, 'minecraft:stone'), put(HERE, 'minecraft:lantern', {'hanging': False}), wait(),
                read('placed', HERE), put(DOWN, 'minecraft:air'), wait(), read('later', HERE)]),
        ]
    return items


SIDES = {'east': EAST, 'west': WEST, 'south': SOUTH, 'north': NORTH}
SHELL = {'east': EAST, 'west': WEST, 'south': SOUTH, 'north': NORTH, 'up': UP, 'down': DOWN}
STONE = 'minecraft:stone'
# Blocks a scenario cell cannot hold without disturbing its neighbours or the
# run: flowing liquids, fire and technical blocks.
UNSAFE = {
    'minecraft:air', 'minecraft:water', 'minecraft:flowing_water', 'minecraft:lava',
    'minecraft:flowing_lava', 'minecraft:bubble_column', 'minecraft:fire', 'minecraft:soul_fire',
    'minecraft:portal', 'minecraft:end_gateway', 'minecraft:moving_block', 'minecraft:piston_arm_collision',
    'minecraft:sticky_piston_arm_collision', 'minecraft:unknown',
}
# States that crashed Endstone's `set_data` in the drops sweep
# (`measured/block_drops`, fit `server_crash`).
CRASHING = {
    ('minecraft:bamboo_hanging_sign', (('attached_bit', True), ('facing_direction', 3),
                                       ('ground_sign_direction', 0), ('hanging', False))),
    ('minecraft:crimson_hanging_sign', (('attached_bit', True), ('facing_direction', 2),
                                        ('ground_sign_direction', 5), ('hanging', False))),
    ('minecraft:mangrove_hanging_sign', (('attached_bit', False), ('facing_direction', 1),
                                         ('ground_sign_direction', 8), ('hanging', False))),
    ('minecraft:prismarine_double_slab', (('minecraft:vertical_half', 'top'),)),
}


def _frozen(states: dict) -> tuple:
    return tuple(sorted(states.items()))


def palette() -> list[dict]:
    """Every canonical state (typed: byte properties are booleans), without
    unsafe blocks and crashing states."""
    import native
    return [e for e in native.block_states(typed=True)
            if e['name'] not in UNSAFE and (e['name'], _frozen(e['states'])) not in CRASHING]


def defaults() -> dict[str, dict]:
    first: dict[str, dict] = {}
    for entry in palette():
        first.setdefault(entry['name'], entry['states'])
    return first


def shape_variants() -> list[dict]:
    """One non-default state for every collision shape (protocolgen shape
    table, `crates/unastar-data/output/shapes.json`) that no block's default
    state has, first in palette order."""
    from pathlib import Path
    import native
    shapes = json.loads((Path(__file__).resolve().parents[2] / 'crates' / 'unastar-data' / 'output'
                         / 'shapes.json').read_text(encoding='utf-8'))['states']
    everything = native.block_states(typed=True)
    first = defaults()
    seen = {shapes[runtime_id][0] for runtime_id, entry in enumerate(everything)
            if first.get(entry['name']) == entry['states']}
    out = []
    for runtime_id, entry in enumerate(everything):
        name = entry['name']
        if name in UNSAFE or (name, _frozen(entry['states'])) in CRASHING:
            continue
        shape = shapes[runtime_id][0]
        if shape in seen:
            continue
        seen.add(shape)
        out.append(entry)
    return out


def stair(weirdo: int, upside: bool, block: str = 'minecraft:oak_stairs') -> tuple[str, dict]:
    return block, {'weirdo_direction': weirdo, 'upside_down_bit': upside, 'minecraft:corner': 'none'}


def stairs() -> list[dict]:
    """`minecraft:corner` of stairs next to stairs: every centre stair (4
    directions x 2 halves) with one neighbouring stair on each side in both
    placement orders, with a neighbour of another material, and with two
    neighbours on every pair of sides (neighbours placed first)."""
    variants = [(w, u) for w in range(4) for u in (False, True)]
    items = []
    for cw, cu in variants:
        centre = set_(HERE, *stair(cw, cu))
        for side, off in SIDES.items():
            for nw, nu in variants:
                neighbour = set_(off, *stair(nw, nu))
                for order, steps in (('neighbour_first', [neighbour, centre]),
                                     ('centre_first', [centre, neighbour])):
                    items.append({'key': key('stairs', 'one', [cw, cu], side, [nw, nu], order),
                                  'family': 'stairs',
                                  'steps': steps + [wait(1), read('after', HERE, off)]})
                other = set_(off, *stair(nw, nu, 'minecraft:stone_brick_stairs'))
                items.append({'key': key('stairs', 'material', [cw, cu], side, [nw, nu]),
                              'family': 'stairs',
                              'steps': [other, centre, wait(1), read('after', HERE, off)]})
    names = list(SIDES)
    for cw, cu in variants:
        centre = set_(HERE, *stair(cw, cu))
        for i, a in enumerate(names):
            for b in names[i + 1:]:
                for av in variants:
                    for bv in variants:
                        items.append({'key': key('stairs', 'two', [cw, cu], a, list(av), b, list(bv)),
                                      'family': 'stairs',
                                      'steps': [set_(SIDES[a], *stair(*av)), set_(SIDES[b], *stair(*bv)),
                                                centre, wait(1),
                                                read('after', HERE, SIDES[a], SIDES[b])]})
    return items


CONNECTION_CENTRES = ['minecraft:oak_fence', 'minecraft:nether_brick_fence', 'minecraft:crimson_fence',
                      'minecraft:glass_pane', 'minecraft:iron_bars', 'minecraft:copper_bars',
                      'minecraft:cobblestone_wall', 'minecraft:mud_brick_wall']
SHAPE_CENTRES = ['minecraft:oak_fence', 'minecraft:glass_pane', 'minecraft:cobblestone_wall']


def neighbour_states() -> list[tuple[str, dict, list[str]]]:
    """(block, states, centres): every block's default state against every
    connection centre, and each further collision shape of a block against
    one centre per family."""
    out = [('minecraft:air', {}, CONNECTION_CENTRES)]
    out += [(name, states, CONNECTION_CENTRES) for name, states in sorted(defaults().items())]
    out += [(e['name'], e['states'], SHAPE_CENTRES) for e in shape_variants()]
    return out


def connections() -> list[dict]:
    """Connection states of fences, panes, bars and walls with one neighbour
    to the east: the neighbour is placed first, then the centre (which
    computes its own connections), read after the walls' deferred update."""
    items = []
    for block, states, centres in neighbour_states():
        neighbour = set_(EAST, block, states)
        for centre in centres:
            items.append({'key': key('connections', centre, block, states), 'family': 'connections',
                          'steps': [neighbour, set_(HERE, centre), wait(3), read('after', HERE, EAST)]})
    return items


def wall_above() -> list[dict]:
    """Wall heights and post under every block (default state, and each
    further collision shape): a straight wall (walls east and west), a wall
    with one wall to the east, and a lone wall; the block above is placed
    last."""
    wall = 'minecraft:cobblestone_wall'
    aboves = [('minecraft:air', {})] + sorted(defaults().items())
    aboves += [(e['name'], e['states']) for e in shape_variants()]
    items = []
    for block, states in aboves:
        for shape, sides in (('straight', [EAST, WEST]), ('end', [EAST]), ('lone', [])):
            steps = [set_(off, wall) for off in sides] + [set_(HERE, wall), wait(3), set_(UP, block, states),
                                                          wait(3), read('after', HERE, UP)]
            items.append({'key': key('wall_above', shape, block, states), 'family': 'wall_above',
                          'steps': steps})
    return items


def gate_in_wall() -> list[dict]:
    """`in_wall_bit` of a fence gate (every direction, open and closed) with
    walls, fences, panes or stone beside it."""
    items = []
    for direction in ('south', 'west', 'north', 'east'):
        for open_ in (False, True):
            gate = set_(HERE, 'minecraft:fence_gate', {'minecraft:cardinal_direction': direction,
                                                       'open_bit': open_, 'in_wall_bit': False})
            for side_block in ('minecraft:cobblestone_wall', 'minecraft:oak_fence', 'minecraft:glass_pane',
                               STONE):
                for sides in (['east'], ['west'], ['south'], ['north'], ['east', 'west'], ['south', 'north']):
                    steps = [set_(SIDES[s], side_block) for s in sides]
                    steps += [gate, wait(3), read('after', HERE)]
                    items.append({'key': key('gate_in_wall', direction, open_, side_block, sides),
                                  'family': 'gate_in_wall', 'steps': steps})
    return items


def detection_states() -> list[tuple[str, dict]]:
    """The default state of every block and every single-property variation
    of it."""
    by_name: dict[str, list[dict]] = {}
    for entry in palette():
        by_name.setdefault(entry['name'], []).append(entry['states'])
    out = []
    for name, states_list in sorted(by_name.items()):
        default = states_list[0]
        out.append((name, default))
        present = {_frozen(s) for s in states_list}
        for prop in sorted(default):
            for value in sorted({s[prop] for s in states_list}, key=repr):
                if value == default[prop]:
                    continue
                variant = {**default, prop: value}
                if _frozen(variant) in present:
                    out.append((name, variant))
    return out


def support() -> list[dict]:
    """Which neighbours hold a block up: the block is placed inside a stone
    shell (all six neighbours), then the shell is removed one side at a
    time with block updates, forwards (east, west, south, north, up, down)
    and backwards; the block is read after each removal."""
    items = []
    for block, states in detection_states():
        for order in ('forward', 'backward'):
            sides = list(SHELL) if order == 'forward' else list(reversed(SHELL))
            steps = [set_(SHELL[s], STONE, physics=False) for s in SHELL]
            steps.append(set_(HERE, block, states))
            steps.append(wait(1))
            steps.append(read('placed', HERE))
            for side in sides:
                steps += [set_(SHELL[side], 'minecraft:air'), wait(2), read(f'without_{side}', HERE)]
            items.append({'key': key('support', order, block, states), 'family': 'support', 'steps': steps})
    return items


# Blocks that need a neighbour, one per support rule seen in the `support`
# family: (block, states, support side, poke side). The poke (stone set with
# a block update) gives the block a neighbour update without touching the
# support.
ATTACHED = [
    ('minecraft:torch', {'torch_facing_direction': 'top'}, 'down', 'up'),
    ('minecraft:stone_button', {'facing_direction': 1, 'button_pressed_bit': False}, 'down', 'up'),
    ('minecraft:lever', {'lever_direction': 'up_east_west', 'open_bit': False}, 'down', 'up'),
    ('minecraft:stone_pressure_plate', {'redstone_signal': 0}, 'down', 'up'),
    ('minecraft:white_carpet', {}, 'down', 'up'),
    ('minecraft:rail', {'rail_direction': 0}, 'down', 'up'),
    ('minecraft:redstone_wire', {'redstone_signal': 0}, 'down', 'up'),
    ('minecraft:unpowered_repeater', {'minecraft:cardinal_direction': 'south', 'repeater_delay': 0}, 'down', 'up'),
    ('minecraft:standing_sign', {'ground_sign_direction': 0}, 'down', 'up'),
    ('minecraft:standing_banner', {'ground_sign_direction': 0}, 'down', 'up'),
    ('minecraft:cake', {'bite_counter': 0}, 'down', 'up'),
    ('minecraft:flower_pot', {'update_bit': False}, 'down', 'up'),
    ('minecraft:lantern', {'hanging': False}, 'down', 'up'),
    ('minecraft:small_amethyst_bud', {'minecraft:block_face': 'up'}, 'down', 'up'),
    ('minecraft:sea_pickle', {'cluster_count': 0, 'dead_bit': True}, 'down', 'up'),
    ('minecraft:snow_layer', {'height': 0, 'covered_bit': False}, 'down', 'up'),
    ('minecraft:dead_brain_coral_fan', {'coral_fan_direction': 0}, 'down', 'up'),
    ('minecraft:leaf_litter', {'growth': 0, 'minecraft:cardinal_direction': 'south'}, 'down', 'up'),
    ('minecraft:bell', {'attachment': 'standing', 'direction': 0, 'toggle_bit': False}, 'down', 'up'),
    ('minecraft:torch', {'torch_facing_direction': 'east'}, 'east', 'west'),
    ('minecraft:stone_button', {'facing_direction': 4, 'button_pressed_bit': False}, 'east', 'west'),
    ('minecraft:ladder', {'facing_direction': 4}, 'east', 'west'),
    ('minecraft:wall_sign', {'facing_direction': 4}, 'east', 'west'),
    ('minecraft:lever', {'lever_direction': 'west', 'open_bit': False}, 'east', 'west'),
    ('minecraft:frame', {'facing_direction': 4, 'item_frame_map_bit': False, 'item_frame_photo_bit': False},
     'east', 'west'),
    ('minecraft:wall_banner', {'facing_direction': 4}, 'east', 'west'),
    ('minecraft:tripwire_hook', {'direction': 1, 'attached_bit': False, 'powered_bit': False}, 'east', 'west'),
    ('minecraft:dead_brain_coral_wall_fan', {'coral_direction': 0}, 'east', 'west'),
    ('minecraft:small_amethyst_bud', {'minecraft:block_face': 'west'}, 'east', 'west'),
    ('minecraft:lantern', {'hanging': True}, 'up', 'down'),
    ('minecraft:stone_button', {'facing_direction': 0, 'button_pressed_bit': False}, 'up', 'down'),
    ('minecraft:hanging_roots', {}, 'up', 'down'),
    ('minecraft:spore_blossom', {}, 'up', 'down'),
    ('minecraft:oak_hanging_sign', {'attached_bit': False, 'facing_direction': 2, 'ground_sign_direction': 0,
                                    'hanging': True}, 'up', 'down'),
]
# Plants that cannot stay on stone: which blocks below they accept.
PLANTS = [
    ('minecraft:dandelion', {}), ('minecraft:short_grass', {}), ('minecraft:oak_sapling', {'age_bit': False}),
    ('minecraft:deadbush', {}), ('minecraft:bamboo', {'age_bit': False, 'bamboo_leaf_size': 'no_leaves',
                                                     'bamboo_stalk_thickness': 'thin'}),
    ('minecraft:nether_wart', {'age': 0}), ('minecraft:crimson_fungus', {}), ('minecraft:crimson_roots', {}),
    ('minecraft:sweet_berry_bush', {'growth': 0}), ('minecraft:wheat', {'growth': 0}),
    ('minecraft:pink_petals', {'growth': 0, 'minecraft:cardinal_direction': 'south'}), ('minecraft:azalea', {}),
    ('minecraft:torchflower', {}), ('minecraft:nether_sprouts', {}), ('minecraft:firefly_bush', {}),
    ('minecraft:short_dry_grass', {}), ('minecraft:bush', {}), ('minecraft:melon_stem', {'growth': 0,
                                                                                         'facing_direction': 0}),
    ('minecraft:mangrove_propagule', {'hanging': False, 'propagule_stage': 0}),
    ('minecraft:closed_eyeblossom', {}), ('minecraft:cactus', {'age': 0}),
]


def surface_candidates() -> list[tuple[str, dict]]:
    """Every block's default state and each further collision shape."""
    out = [(name, states) for name, states in sorted(defaults().items())]
    out += [(e['name'], e['states']) for e in shape_variants()]
    return out


def surfaces(sides: tuple[str, ...] = ('down', 'east', 'up')) -> list[dict]:
    """Which blocks hold up each attached block: the candidate goes on the
    support side without a block update, the attached block next to it with
    one, then a poke on another side; both are read 2 ticks later."""
    items = []
    for candidate, cstates in surface_candidates():
        for block, states, side, poke in ATTACHED:
            if side not in sides:
                continue
            steps = [set_(SHELL[side], candidate, cstates, physics=False), set_(HERE, block, states),
                     set_(SHELL[poke], STONE), wait(2), read('after', HERE, SHELL[side])]
            items.append({'key': key('surfaces', block, states, side, candidate, cstates), 'family': 'surfaces',
                          'steps': steps})
    return items


def soils() -> list[dict]:
    """Which blocks each plant accepts below it: the candidate (default
    state) without a block update, the plant on it with one, then stone above
    the plant as a poke; both are read 2 ticks later."""
    items = []
    for candidate, cstates in sorted(defaults().items()):
        for block, states in PLANTS:
            steps = [set_(DOWN, candidate, cstates, physics=False), set_(HERE, block, states), set_(UP, STONE),
                     wait(2), read('after', HERE, DOWN)]
            items.append({'key': key('soils', block, states, candidate, cstates), 'family': 'soils',
                          'steps': steps})
    return items


def junctions() -> list[dict]:
    """Wall posts and arm heights at crosses and T junctions under every block
    (default state, and each further collision shape)."""
    wall = 'minecraft:cobblestone_wall'
    shapes = {'cross': [NORTH, EAST, SOUTH, WEST], 't': [NORTH, EAST, SOUTH]}
    items = []
    for block, states in [('minecraft:air', {})] + surface_candidates():
        for shape, sides in shapes.items():
            steps = [set_(off, wall) for off in sides] + [set_(HERE, wall), wait(3), set_(UP, block, states),
                                                          wait(3), read('after', HERE, UP)]
            items.append({'key': key('junctions', shape, block, states), 'family': 'junctions', 'steps': steps})
    return items


def halves() -> list[dict]:
    """Two-block structures: both halves set with block updates, then one
    half removed with a block update; the cell is read 2 ticks later."""
    pairs = [
        ('minecraft:wooden_door', 'upper_block_bit', {'door_hinge_bit': False, 'minecraft:cardinal_direction': 'south',
                                                      'open_bit': False}),
        ('minecraft:iron_door', 'upper_block_bit', {'door_hinge_bit': False, 'minecraft:cardinal_direction': 'south',
                                                    'open_bit': False}),
        ('minecraft:tall_grass', 'upper_block_bit', {}),
        ('minecraft:sunflower', 'upper_block_bit', {}),
        ('minecraft:lilac', 'upper_block_bit', {}),
    ]
    items = []
    for block, bit, states in pairs:
        lower = {**states, bit: False}
        upper = {**states, bit: True}
        for floor in ('minecraft:grass_block', STONE):
            for removed in ('lower', 'upper', 'floor'):
                target = {'lower': HERE, 'upper': UP, 'floor': DOWN}[removed]
                steps = [set_(DOWN, floor, physics=False), set_(HERE, block, lower), set_(UP, block, upper),
                         wait(2), read('placed', HERE, UP), set_(target, 'minecraft:air'), wait(2),
                         read('after', HERE, UP)]
                items.append({'key': key('halves', block, floor, removed), 'family': 'halves', 'steps': steps})
    return items


FAMILIES = {'pilot': pilot, 'stairs': stairs, 'connections': connections, 'wall_above': wall_above,
            'gate_in_wall': gate_in_wall, 'support': support, 'surfaces': surfaces,
            'surfaces_floor': lambda: surfaces(('down',)), 'surfaces_side': lambda: surfaces(('east', 'up')),
            'soils': soils,
            'junctions': junctions, 'halves': halves}


def signature(item: dict) -> list[str]:
    """The step signature the job groups waves by (`jobs.step_signature`):
    an item whose timeline has a click (`use`) runs alone."""
    out = []
    for step in item['steps']:
        if step[0] == 'timeline' and any(event[0] == 'use' for _, event in step[3]):
            out.append(f'timeline{step[1]}:{item["key"]}')
        elif step[0] in ('wait', 'timeline'):
            out.append(f'{step[0]}{step[1]}')
        elif step[0] == 'cmd':
            # A command (a game rule) applies to the whole wave.
            out.append(f'cmd:{step[1]}')
        else:
            out.append(step[0])
    return out


def items(families: str) -> list[dict]:
    names = list(FAMILIES) if families == 'all' else families.split(',')
    out = []
    for name in names:
        if name not in FAMILIES:
            raise SystemExit(f'unknown scenario family {name!r}; known: {", ".join(FAMILIES)}')
        family = FAMILIES[name]()
        # Items with the same step signature share waves.
        out += sorted(family, key=signature)
    return out


# ------------------------------------------------------------------ round 2

def named(*suffixes: str, exclude: tuple[str, ...] = ()) -> list[str]:
    """Every block whose identifier ends with one of `suffixes`."""
    import native
    names = sorted({e['name'] for e in native.block_states(typed=True)})
    return [n for n in names if n.endswith(suffixes) and not any(part in n for part in exclude)]


def stair_blocks() -> list[str]:
    return named('_stairs')


def fence_blocks() -> list[str]:
    return named('_fence')


def pane_blocks() -> list[str]:
    return named('_pane', '_bars')


def wall_blocks() -> list[str]:
    return named('_wall', exclude=('sign', 'banner', 'fan', 'torch'))


def stairs_blocks() -> list[dict]:
    """Every stair block but oak (round 1): the centre in each of its eight
    orientations with one stair of the same block beside it on each side in
    each orientation, the neighbour placed first."""
    variants = [(w, u) for w in range(4) for u in (False, True)]
    items = []
    for block in stair_blocks():
        if block == 'minecraft:oak_stairs':
            continue
        for cw, cu in variants:
            centre = set_(HERE, *stair(cw, cu, block))
            for side, off in SIDES.items():
                for nw, nu in variants:
                    items.append({'key': key('stairs_blocks', block, [cw, cu], side, [nw, nu]),
                                  'family': 'stairs_blocks',
                                  'steps': [set_(off, *stair(nw, nu, block)), centre, wait(1),
                                            read('after', HERE, off)]})
    return items


# Neighbours for every fence, pane, bars and wall centre: one block of each
# kind round 1 found (full faces, partial faces, the blocks that never or
# always connect) and every fence, pane, bars and wall.
CURATED_NEIGHBOURS = [
    ('minecraft:air', {}), (STONE, {}), ('minecraft:glass', {}), ('minecraft:dirt', {}), ('minecraft:oak_planks', {}),
    ('minecraft:oak_leaves', {'persistent_bit': True, 'update_bit': False}), ('minecraft:azalea', {}),
    ('minecraft:white_shulker_box', {}), ('minecraft:barrier', {}), ('minecraft:carved_pumpkin', {}),
    ('minecraft:pumpkin', {}), ('minecraft:melon_block', {}), ('minecraft:cauldron', {}), ('minecraft:dragon_egg', {}),
    ('minecraft:honey_block', {}), ('minecraft:mud', {}), ('minecraft:soul_sand', {}), ('minecraft:structure_void', {}),
    ('minecraft:oak_slab', {'minecraft:vertical_half': 'bottom'}),
    ('minecraft:oak_slab', {'minecraft:vertical_half': 'top'}),
    ('minecraft:trapdoor', {'direction': 0, 'open_bit': False, 'upside_down_bit': False}),
    ('minecraft:trapdoor', {'direction': 1, 'open_bit': True, 'upside_down_bit': False}),
    ('minecraft:chest', {}), ('minecraft:enchanting_table', {}), ('minecraft:white_carpet', {}),
    ('minecraft:torch', {'torch_facing_direction': 'top'}), ('minecraft:lantern', {'hanging': False}),
    ('minecraft:snow_layer', {'height': 0, 'covered_bit': False}),
    ('minecraft:snow_layer', {'height': 7, 'covered_bit': False}), ('minecraft:end_rod', {}),
    ('minecraft:iron_chain', {}), ('minecraft:cactus', {}), ('minecraft:anvil', {}), ('minecraft:hopper', {}),
    ('minecraft:farmland', {}), ('minecraft:grass_path', {}), ('minecraft:cake', {}), ('minecraft:lectern', {}),
    ('minecraft:border_block', {}), ('minecraft:scaffolding', {}), ('minecraft:daylight_detector', {}),
]


def connection_neighbours() -> list[tuple[str, dict]]:
    gates = [('minecraft:fence_gate', {'minecraft:cardinal_direction': d, 'open_bit': o, 'in_wall_bit': False})
             for d in ('south', 'east') for o in (False, True)]
    stairs_ = [stair(w, u) for w in range(4) for u in (False, True)]
    families = [(name, {}) for name in fence_blocks() + pane_blocks() + wall_blocks()]
    return CURATED_NEIGHBOURS + gates + stairs_ + families


def connections_blocks() -> list[dict]:
    """Connections of every fence, pane, bars and wall: each centre with
    one neighbour east (the curated kinds and every connecting block), the
    neighbour placed first, read after the walls' deferred update."""
    items = []
    for centre in fence_blocks() + pane_blocks() + wall_blocks():
        for block, states in connection_neighbours():
            items.append({'key': key('connections_blocks', centre, block, states), 'family': 'connections_blocks',
                          'steps': [set_(EAST, block, states), set_(HERE, centre), wait(3),
                                    read('after', HERE, EAST)]})
    return items


WALL_ABOVES = [
    ('minecraft:air', {}), (STONE, {}), ('minecraft:stone_pressure_plate', {}), ('minecraft:white_carpet', {}),
    ('minecraft:torch', {'torch_facing_direction': 'top'}), ('minecraft:oak_slab', {'minecraft:vertical_half': 'bottom'}),
    ('minecraft:oak_slab', {'minecraft:vertical_half': 'top'}), ('minecraft:lantern', {'hanging': False}),
    ('minecraft:cobblestone_wall', {}), ('minecraft:oak_fence', {}), ('minecraft:glass_pane', {}),
    ('minecraft:chest', {}), ('minecraft:cake', {}), ('minecraft:candle', {}), ('minecraft:end_rod', {}),
    ('minecraft:flower_pot', {}), ('minecraft:standing_sign', {}),
]


def wall_heights_blocks() -> list[dict]:
    """Every wall's arm heights and post under a set of blocks: straight,
    end and lone walls of the same block, the block above placed last."""
    items = []
    for wall in wall_blocks():
        for block, states in WALL_ABOVES:
            for shape, sides in (('straight', [EAST, WEST]), ('end', [EAST]), ('lone', [])):
                steps = [set_(off, wall) for off in sides] + [set_(HERE, wall), wait(3),
                                                              set_(UP, block, states), wait(3),
                                                              read('after', HERE, UP)]
                items.append({'key': key('wall_heights_blocks', wall, shape, block, states),
                              'family': 'wall_heights_blocks', 'steps': steps})
    return items


def multiface_support() -> list[dict]:
    """Vines, glow lichen, sculk veins and resin clumps in every state inside
    a stone shell; the shell is removed one side at a time (both orders)
    and the whole state is read after each removal."""
    import native
    items = []
    for entry in native.block_states(typed=True):
        if entry['name'] not in ('minecraft:vine', 'minecraft:glow_lichen', 'minecraft:sculk_vein',
                                 'minecraft:resin_clump'):
            continue
        block, states = entry['name'], entry['states']
        for order in ('forward', 'backward'):
            sides = list(SHELL) if order == 'forward' else list(reversed(SHELL))
            steps = [set_(SHELL[s], STONE, physics=False) for s in SHELL]
            steps += [set_(HERE, block, states), wait(1), read('placed', HERE)]
            for side in sides:
                steps += [set_(SHELL[side], 'minecraft:air'), wait(2), read(f'without_{side}', HERE)]
            items.append({'key': key('multiface_support', order, block, states), 'family': 'multiface_support',
                          'steps': steps})
    return items


def rails() -> list[dict]:
    """Rail shapes from neighbouring rails: a rail (or powered, detector,
    activator rail) at the centre with no, a flat or a raised rail (on a
    stone, one block up) on each side, neighbours first or the centre
    first."""
    import itertools
    centres = ['minecraft:rail', 'minecraft:golden_rail', 'minecraft:detector_rail', 'minecraft:activator_rail']
    names = ['north', 'east', 'south', 'west']
    items = []
    for centre in centres:
        for config in itertools.product(('none', 'flat', 'raised'), repeat=4):
            neighbour_steps = []
            reads = [HERE]
            for side, kind in zip(names, config):
                off = SIDES[side]
                if kind == 'flat':
                    neighbour_steps.append(set_(off, 'minecraft:rail'))
                    reads.append(off)
                elif kind == 'raised':
                    neighbour_steps.append(set_(off, STONE, physics=False))
                    up = [off[0], 1, off[2]]
                    neighbour_steps.append(set_(up, 'minecraft:rail'))
                    reads.append(up)
            for order in ('neighbours_first', 'centre_first'):
                centre_step = set_(HERE, centre)
                steps = (neighbour_steps + [centre_step] if order == 'neighbours_first'
                         else [centre_step] + neighbour_steps)
                items.append({'key': key('rails', centre, list(config), order), 'family': 'rails',
                              'steps': steps + [wait(2), read('after', *reads)]})
    return items


SOIL_PLANTS = [('minecraft:waterlily', {}), ('minecraft:wither_rose', {}), ('minecraft:red_shrub', {}),
               ('minecraft:brown_mushroom', {}), ('minecraft:red_mushroom', {}), ('minecraft:cactus_flower', {}),
               ('minecraft:frog_spawn', {})]
WATERY = [('minecraft:water', {'liquid_depth': 0}), ('minecraft:flowing_water', {'liquid_depth': 0}),
          ('minecraft:water', {'liquid_depth': 3}), ('minecraft:lava', {'liquid_depth': 0})]


def soils_more() -> list[dict]:
    """The plants round 1 did not sweep, on every block's default state and
    on water and lava below (the liquid replaces the floor, so it cannot
    flow): set as in `soils`."""
    items = []
    candidates = sorted(defaults().items()) + WATERY
    for candidate, cstates in candidates:
        for block, states in SOIL_PLANTS:
            steps = [set_(DOWN, candidate, cstates, physics=False), set_(HERE, block, states), set_(UP, STONE),
                     wait(2), read('after', HERE, DOWN)]
            items.append({'key': key('soils_more', block, states, candidate, cstates), 'family': 'soils_more',
                          'steps': steps})
    return items


def coral_death() -> list[dict]:
    """Live coral set (with a block update) in air, read every 10 ticks for
    300 ticks: blocks, plants, floor fans and wall fans (on a stone east of
    them), three cells each; and with water below them (the water replaces
    the floor)."""
    items = []
    reads = []
    for tick in range(10, 310, 10):
        reads += [wait(10), read(f't{tick}', HERE)]
    kinds = ('brain', 'bubble', 'fire', 'horn', 'tube')
    for kind in kinds:
        cells = [('block', [set_(HERE, f'minecraft:{kind}_coral_block')]),
                 ('plant', [set_(HERE, f'minecraft:{kind}_coral')]),
                 ('fan', [set_(HERE, f'minecraft:{kind}_coral_fan', {'coral_fan_direction': 0})]),
                 ('wall_fan', [set_(EAST, STONE, physics=False),
                               set_(HERE, f'minecraft:{kind}_coral_wall_fan', {'coral_direction': 0})])]
        for name, steps in cells:
            padded = steps if len(steps) == 2 else [set_(WEST, 'minecraft:air', physics=False)] + steps
            for repeat in range(3):
                items.append({'key': key('coral_death', kind, name, repeat), 'family': 'coral_death',
                              'steps': padded + reads})
            water = [set_(DOWN, 'minecraft:water', {'liquid_depth': 0}, physics=False)] + padded[1:]
            items.append({'key': key('coral_death', kind, f'{name}_over_water', 0), 'family': 'coral_death',
                          'steps': [set_(WEST, 'minecraft:air', physics=False)] + water + reads})
    return items


ROUND_TWO = {'stairs_blocks': stairs_blocks, 'connections_blocks': connections_blocks,
             'wall_heights_blocks': wall_heights_blocks, 'multiface_support': multiface_support, 'rails': rails,
             'soils_more': soils_more, 'coral_death': coral_death}
FAMILIES.update(ROUND_TWO)


# ------------------------------------------------------------------ round 3

def _circuit_families() -> dict:
    import circuits
    return circuits.FAMILIES


def _random_tick_families() -> dict:
    import random_ticks
    return random_ticks.FAMILIES


def _coral_families() -> dict:
    import coral
    return coral.FAMILIES


def _piston_families() -> dict:
    import pistons
    return pistons.FAMILIES


def _tnt_families() -> dict:
    import tnt
    return tnt.FAMILIES


ROUND_THREE = _circuit_families()
ROUND_FOUR = _piston_families()
TNT = _tnt_families()
RANDOM_TICKS = _random_tick_families()
CORAL = _coral_families()
FAMILIES.update(ROUND_THREE)
FAMILIES.update(RANDOM_TICKS)
FAMILIES.update(CORAL)
FAMILIES.update(ROUND_FOUR)
FAMILIES.update(TNT)
# Families that need their own cell layout (name in sweep.ARENAS) and server
# properties; every family in one run must share a layout.
LAYOUTS = ({name: 'circuits' for name in ROUND_THREE | ROUND_FOUR} | {name: 'fields' for name in RANDOM_TICKS}
           | {name: 'coral' for name in CORAL} | {'coral_death': 'coral'} | {name: 'blast' for name in TNT})


def layout(families: str) -> str:
    names = list(FAMILIES) if families == 'all' else families.split(',')
    found = {LAYOUTS.get(name, 'cells') for name in names}
    if len(found) != 1:
        raise SystemExit(f'families {families!r} mix cell layouts {sorted(found)}; run them separately')
    return found.pop()
