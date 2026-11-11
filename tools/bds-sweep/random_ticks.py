"""Random tick families for the `scenarios` sweep job.

Each item is a 16 x 16 field of identical cells (crops on farmland, sugar
cane, cacti, copper, leaves, bare farmland) built on the superflat floor;
the item sets `randomtickspeed` and then reads every cell every
`READ_EVERY` ticks for `READS` reads. The summary keeps, per read, how many
cells were in each state, and the replay test runs the same field in
unastar with a seeded random source and compares the histograms (chi-square)
with BDS's.

Fields are 16 blocks wide, 20 apart (`LAYOUT`), 16 per wave. Waves of one
family share one tick speed; the job runs families one wave after another,
so each wave's `cmd` step sets the speed of the whole world for its run.
"""

from __future__ import annotations

import json

LAYOUT = {'origin': [6, -60, -40], 'spacing': [20, 20], 'columns': 4, 'wave_size': 16, 'center': [8, 8],
          'clear_box': [[-1, -2, -1], [17, 4, 17]]}
PROPERTIES = {'view_distance': 10, 'tick_distance': 8}
READ_EVERY = 100
READS = 30


def key(family: str, *parts) -> str:
    return family + '|' + '|'.join(json.dumps(p, sort_keys=True, separators=(',', ':')) for p in parts)


def at(x: int, y: int = 0, z: int = 0) -> list[int]:
    return [x, y, z]


def field(family: str, name, speed: int, build: list, watch: list) -> dict:
    """`build` ([offset, block, states] set without block updates, so a
    field's blocks do not react to each other while it is laid out) and
    the reads of `watch` every `READ_EVERY` ticks."""
    steps = [['build', [[o, b, st, False] for o, b, st in build]],
             ['cmd', f'gamerule randomtickspeed {speed}', at(0)]]
    for read in range(1, READS + 1):
        steps += [['wait', READ_EVERY], ['read', f'r{read}', watch]]
    return {'key': key(family, name), 'family': family, 'steps': steps}


CROPS = ('wheat', 'carrots', 'potatoes', 'beetroot')


def random_crops() -> list[dict]:
    """Crops on a 9 x 9 farmland square: hydrated (a water source in the
    middle, farmland moist) or dry (no water, farmland dry); 80 crops each,
    at tick speed 128."""
    fam = 'random_crops'
    items = []
    for crop in CROPS:
        for hydrated in (True, False):
            build, watch = [], []
            for x in range(9):
                for z in range(9):
                    if (x, z) == (4, 4):
                        if hydrated:
                            build.append([at(x, -1, z), 'minecraft:water', {'liquid_depth': 0}])
                        else:
                            build.append([at(x, -1, z), 'minecraft:dirt', {'dirt_type': 'normal'}])
                        continue
                    build.append([at(x, -1, z), 'minecraft:farmland', {'moisturized_amount': 7 if hydrated else 0}])
                    build.append([at(x, 0, z), f'minecraft:{crop}', {'growth': 0}])
                    watch.append(at(x, 0, z))
            items.append(field(fam, [crop, 'hydrated' if hydrated else 'dry'], 128, build, watch))
    return items


def random_cane() -> list[dict]:
    """Sugar cane on sand along water channels (80 canes, one block tall)
    and cacti on sand two apart (64), at tick speed 32; the cell and the two
    blocks above are read."""
    fam = 'random_cane'
    build, watch = [], []
    for z in (1, 5, 9, 13):
        for x in range(16):
            build.append([at(x, -1, z), 'minecraft:water', {'liquid_depth': 0}])
            for side in (-1, 1):
                build.append([at(x, -1, z + side), 'minecraft:sand', {'sand_type': 'normal'}])
                if x % 2 == 0 or side == 1:
                    build.append([at(x, 0, z + side), 'minecraft:reeds', {'age': 0}])
                    watch += [at(x, 0, z + side), at(x, 1, z + side), at(x, 2, z + side)]
    items = [field(fam, ['sugar_cane'], 32, build, watch)]
    build, watch = [], []
    for x in range(0, 16, 2):
        for z in range(0, 16, 2):
            build.append([at(x, -1, z), 'minecraft:sand', {'sand_type': 'normal'}])
            build.append([at(x, 0, z), 'minecraft:cactus', {'age': 0}])
            watch += [at(x, 0, z), at(x, 1, z), at(x, 2, z)]
    items.append(field(fam, ['cactus'], 32, build, watch))
    return items


COPPER = ('copper_block', 'exposed_copper', 'weathered_copper', 'cut_copper', 'copper_grate',
          'chiseled_copper', 'copper_bulb')


def random_copper() -> list[dict]:
    """Copper oxidation at tick speed 256: 16 copper blocks five apart (no
    other copper within four) per field, one field per block kind; and a
    solid 4 x 4 x 2 copper slab of blocks whose neighbours are copper."""
    fam = 'random_copper'
    items = []
    for block in COPPER:
        states = {'lit': False, 'powered_bit': False} if block == 'copper_bulb' else {}
        build, watch = [], []
        for x in range(0, 16, 5):
            for z in range(0, 16, 5):
                build.append([at(x, 0, z), f'minecraft:{block}', states])
                watch.append(at(x, 0, z))
        items.append(field(fam, ['isolated', block], 256, build, watch))
    build, watch = [], []
    for x in range(4):
        for z in range(4):
            for y in range(2):
                build.append([at(x, y, z), 'minecraft:copper_block', {}])
                watch.append(at(x, y, z))
    items.append(field(fam, ['cluster', 'copper_block'], 256, build, watch))
    return items


def random_leaves() -> list[dict]:
    """Leaves with no log near them (64 per field, two apart, waiting for
    their update) and player-placed (persistent) ones, at tick speed 8."""
    fam = 'random_leaves'
    items = []
    for block in ('oak_leaves', 'birch_leaves', 'azalea_leaves'):
        for persistent in (False, True):
            build, watch = [], []
            for x in range(0, 16, 2):
                for z in range(0, 16, 2):
                    build.append([at(x, 1, z), f'minecraft:{block}', {'persistent_bit': persistent,
                                                                      'update_bit': True}])
                    watch.append(at(x, 1, z))
            items.append(field(fam, [block, 'persistent' if persistent else 'natural'], 8, build, watch))
    return items


def random_farmland() -> list[dict]:
    """Bare farmland without water: an 8 x 8 square at each moisture 7 and
    0, at tick speed 32."""
    fam = 'random_farmland'
    items = []
    for moisture in (7, 0):
        build, watch = [], []
        for x in range(8):
            for z in range(8):
                build.append([at(x, -1, z), 'minecraft:farmland', {'moisturized_amount': moisture}])
                watch.append(at(x, -1, z))
        items.append(field(fam, ['bare', moisture], 32, build, watch))
    return items


FAMILIES = {'random_crops': random_crops, 'random_cane': random_cane, 'random_copper': random_copper,
            'random_leaves': random_leaves, 'random_farmland': random_farmland}


def random_leaves_logs() -> list[dict]:
    """Which leaves near a log decay, at tick speed 8: an oak log in the
    middle of the field (a 3-high trunk) with leaves (natural, waiting for
    their update) along lines from it, touching each other (`line`) or one
    alone at each distance (`apart`, air between)."""
    fam = 'random_leaves_logs'
    items = []
    for shape in ('line', 'apart'):
        build, watch = [], []
        for y in range(3):
            build.append([at(8, y, 8), 'minecraft:oak_log', {'pillar_axis': 'y'}])
        leaf = {'persistent_bit': False, 'update_bit': True}
        for dx, dz in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            for d in range(1, 8):
                if shape == 'apart' and (dx, dz) != (1, 0):
                    continue
                pos = at(8 + dx * d, 1, 8 + dz * d)
                if shape == 'apart':
                    pos = at(8 + d, 1 + (d % 2), 8 + 2 * (d % 3) - 2)
                build.append([pos, 'minecraft:oak_leaves', leaf])
                watch.append(pos)
        items.append(field(fam, ['oak', shape], 8, build, watch))
    return items


def random_grass() -> list[dict]:
    """Grass spreading onto dirt at tick speed 32: a 15 x 15 dirt square
    (on stone, the floor grass replaced) with one grass block in the
    middle."""
    fam = 'random_grass'
    build, watch = [], []
    for x in range(15):
        for z in range(15):
            build.append([at(x, -2, z), 'minecraft:stone', {}])
            block = 'minecraft:grass_block' if (x, z) == (7, 7) else 'minecraft:dirt'
            states = {} if block == 'minecraft:grass_block' else {'dirt_type': 'normal'}
            build.append([at(x, -1, z), block, states])
            watch.append(at(x, -1, z))
    # A stone ring keeps the floor's grass from reaching the square.
    for i in range(-1, 16):
        for x, z in ((i, -1), (i, 15), (-1, i), (15, i)):
            build.append([at(x, -1, z), 'minecraft:stone', {}])
    return [field(fam, ['dirt_square'], 32, build, watch)]


FAMILIES.update({'random_leaves_logs': random_leaves_logs, 'random_grass': random_grass})


COPPER_MORE = [
    ('cut_copper_slab', {'minecraft:vertical_half': 'bottom'}),
    ('double_cut_copper_slab', {'minecraft:vertical_half': 'bottom'}),
    ('cut_copper_stairs', {'minecraft:corner': 'none', 'upside_down_bit': False, 'weirdo_direction': 0}),
    ('copper_trapdoor', {'direction': 0, 'open_bit': False, 'upside_down_bit': False}),
    ('copper_bars', {'minecraft:connection_east': False, 'minecraft:connection_north': False,
                     'minecraft:connection_south': False, 'minecraft:connection_west': False}),
    ('copper_chain', {'pillar_axis': 'y'}),
    ('copper_lantern', {'hanging': False}),
    ('copper_chest', {'minecraft:cardinal_direction': 'south'}),
    ('copper_golem_statue', {'minecraft:cardinal_direction': 'south'}),
    ('lightning_rod', {'facing_direction': 1, 'powered_bit': False}),
]


def random_copper_more() -> list[dict]:
    """The other copper blocks, 16 isolated per field at tick speed 256,
    and copper doors (both halves)."""
    fam = 'random_copper_more'
    items = []
    for block, states in COPPER_MORE:
        build, watch = [], []
        for x in range(0, 16, 5):
            for z in range(0, 16, 5):
                build.append([at(x, 0, z), f'minecraft:{block}', states])
                watch.append(at(x, 0, z))
        items.append(field(fam, ['isolated', block], 256, build, watch))
    build, watch = [], []
    door = {'door_hinge_bit': False, 'minecraft:cardinal_direction': 'south', 'open_bit': False}
    for x in range(0, 16, 5):
        for z in range(0, 16, 5):
            build.append([at(x, 0, z), 'minecraft:copper_door', {**door, 'upper_block_bit': False}])
            build.append([at(x, 1, z), 'minecraft:copper_door', {**door, 'upper_block_bit': True}])
            watch += [at(x, 0, z), at(x, 1, z)]
    items.append(field(fam, ['isolated', 'copper_door'], 256, build, watch))
    return items


FAMILIES['random_copper_more'] = random_copper_more
