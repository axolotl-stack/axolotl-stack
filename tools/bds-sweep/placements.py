"""Survival block-click cases for the `placements` sweep job.

Each case: `setup` blocks `[dx, dy, dz, block, states]` set with block
updates, the `hand` stack in hotbar slot 0, the player's feet at
`player` `[dx, dy, dz, yaw, pitch]` (Bedrock yaw: 0 south, 90 west, 180
north, -90 east), a `click` `[dx, dy, dz, face, cx, cy, cz]` (face 0 down,
1 up, 2 north, 3 south, 4 west, 5 east; click position in thousandths of the
block) and the offsets to `read` back. Offsets are relative to the arena
origin, a block standing on the superflat grass. The scripted client records
both packet directions (`--capture-dir`), so BDS's answer to every click can
be compared with unastar's.
"""

from __future__ import annotations

STONE = ('minecraft:stone', {})
GRASS_TOP = [0, -1, 0]


def hand(item: str, count: int = 16) -> dict:
    return {'item': item, 'count': count}


def case(key, setup, held, player, click, read):
    return {'key': key, 'setup': setup, 'hand': held, 'player': player, 'click': click, 'read': read}


def block(dx, dy, dz, name, states=None) -> list:
    return [dx, dy, dz, name, states or {}]


# Standing two blocks north of the origin, looking south and down at it.
NORTH_OF = [0.5, 0, -1.5, 0, 35]
# Two blocks south-east, looking north-west at the origin's east face.
EAST_OF = [2.5, 0, 1.5, 135, 20]


def items() -> list[dict]:
    out = [
        case('stone_on_top', [block(0, 0, 0, *STONE)], hand('minecraft:stone'), NORTH_OF,
             [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]),
        case('stone_on_side', [block(0, 0, 0, *STONE)], hand('minecraft:stone'), EAST_OF,
             [0, 0, 0, 5, 1000, 500, 500], [[1, 0, 0]]),
        case('last_stone_in_hand', [block(0, 0, 0, *STONE)], hand('minecraft:stone', 1), NORTH_OF,
             [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]),
        case('onto_floor_grass', [], hand('minecraft:stone'), NORTH_OF,
             [0, -1, 0, 1, 500, 1000, 500], [[0, 0, 0]]),
        case('replaces_short_grass', [block(0, 0, 0, 'minecraft:short_grass')], hand('minecraft:stone'), NORTH_OF,
             [0, 0, 0, 1, 500, 500, 500], [[0, 0, 0], [0, 1, 0]]),
        case('replaces_thin_snow', [block(0, 0, 0, 'minecraft:snow_layer', {'height': 0, 'covered_bit': False})],
             hand('minecraft:stone'), NORTH_OF, [0, 0, 0, 1, 500, 125, 500], [[0, 0, 0], [0, 1, 0]]),
        case('on_full_snow', [block(0, 0, 0, 'minecraft:snow_layer', {'height': 7, 'covered_bit': False})],
             hand('minecraft:stone'), NORTH_OF, [0, 0, 0, 1, 500, 1000, 500], [[0, 0, 0], [0, 1, 0]]),
        case('into_water',
             [block(1, 0, 0, *STONE), block(-1, 0, 0, *STONE), block(0, 0, 1, *STONE), block(0, 0, -1, *STONE),
              block(0, 0, 0, 'minecraft:water', {'liquid_depth': 0})],
             hand('minecraft:stone'), [0.5, 0, -2.5, 0, 45], [0, -1, 0, 1, 500, 1000, 500], [[0, 0, 0]]),
        case('blocked_by_player', [block(0, 0, 0, *STONE), block(-1, 1, 0, *STONE)], hand('minecraft:stone'),
             [0.5, 1, 0.5, 90, 10], [-1, 1, 0, 5, 1000, 500, 500], [[0, 1, 0]]),
        case('click_on_air', [], hand('minecraft:stone'), NORTH_OF, [0, 2, 0, 1, 500, 1000, 500],
             [[0, 2, 0], [0, 3, 0]]),
        case('out_of_reach', [block(0, 0, 10, *STONE)], hand('minecraft:stone'), [0.5, 0, -2.5, 0, 5],
             [0, 0, 10, 1, 500, 1000, 500], [[0, 1, 10]]),
        case('stick_on_stone', [block(0, 0, 0, *STONE)], hand('minecraft:stick'), NORTH_OF,
             [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]),
        case('stairs_looking_east', [block(0, 0, 0, *STONE)], hand('minecraft:oak_stairs'), [-1.5, 0, 0.5, -90, 30],
             [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]),
        case('stairs_upper_half', [block(0, 0, 0, *STONE)], hand('minecraft:oak_stairs'), EAST_OF,
             [0, 0, 0, 5, 1000, 750, 500], [[1, 0, 0]]),
        case('slab_lower_half', [block(0, 0, 0, *STONE)], hand('minecraft:oak_slab'), EAST_OF,
             [0, 0, 0, 5, 1000, 250, 500], [[1, 0, 0]]),
        case('slab_upper_half', [block(0, 0, 0, *STONE)], hand('minecraft:oak_slab'), EAST_OF,
             [0, 0, 0, 5, 1000, 750, 500], [[1, 0, 0]]),
        case('slab_merge', [block(0, 0, 0, 'minecraft:oak_slab', {'minecraft:vertical_half': 'bottom'})],
             hand('minecraft:oak_slab'), NORTH_OF, [0, 0, 0, 1, 500, 500, 500], [[0, 0, 0], [0, 1, 0]]),
        case('log_on_side', [block(0, 0, 0, *STONE)], hand('minecraft:oak_log'), EAST_OF,
             [0, 0, 0, 5, 1000, 500, 500], [[1, 0, 0]]),
        case('torch_on_side', [block(0, 0, 0, *STONE)], hand('minecraft:torch'), EAST_OF,
             [0, 0, 0, 5, 1000, 500, 500], [[1, 0, 0]]),
        case('torch_on_top', [block(0, 0, 0, *STONE)], hand('minecraft:torch'), NORTH_OF,
             [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]),
        case('torch_under', [block(0, 2, 0, *STONE)], hand('minecraft:torch'), [0.5, 0, -1.5, 0, -20],
             [0, 2, 0, 0, 500, 0, 500], [[0, 1, 0]]),
        case('lantern_under', [block(0, 2, 0, *STONE)], hand('minecraft:lantern'), [0.5, 0, -1.5, 0, -20],
             [0, 2, 0, 0, 500, 0, 500], [[0, 1, 0]]),
        case('glazed_terracotta_looking_south', [block(0, 0, 0, *STONE)], hand('minecraft:white_glazed_terracotta'),
             NORTH_OF, [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]),
        case('glazed_terracotta_looking_east', [block(0, 0, 0, *STONE)], hand('minecraft:white_glazed_terracotta'),
             [-1.5, 0, 0.5, -90, 30], [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]),
        case('furnace_looking_south', [block(0, 0, 0, *STONE)], hand('minecraft:furnace'), NORTH_OF,
             [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]),
        case('door_looking_south', [], hand('minecraft:wooden_door'), [0.5, 0, -2.5, 0, 30],
             [0, -1, 0, 1, 500, 1000, 500], [[0, 0, 0], [0, 1, 0]]),
        case('door_blocked_above', [block(0, 1, 0, *STONE)], hand('minecraft:wooden_door'), [0.5, 0, -2.5, 0, 30],
             [0, -1, 0, 1, 500, 1000, 500], [[0, 0, 0], [0, 1, 0]]),
        case('trapdoor_upper_half', [block(0, 0, 0, *STONE)], hand('minecraft:trapdoor'), EAST_OF,
             [0, 0, 0, 5, 1000, 750, 500], [[1, 0, 0]]),
        case('button_on_side', [block(0, 0, 0, *STONE)], hand('minecraft:stone_button'), EAST_OF,
             [0, 0, 0, 5, 1000, 500, 500], [[1, 0, 0]]),
        case('ladder_on_side', [block(0, 0, 0, *STONE)], hand('minecraft:ladder'), EAST_OF,
             [0, 0, 0, 5, 1000, 500, 500], [[1, 0, 0]]),
        case('carpet_on_stone', [block(0, 0, 0, *STONE)], hand('minecraft:white_carpet'), NORTH_OF,
             [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]),
        case('sapling_on_stone', [block(0, 0, 0, *STONE)], hand('minecraft:oak_sapling'), NORTH_OF,
             [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]),
        case('dandelion_on_grass', [], hand('minecraft:dandelion'), NORTH_OF,
             [0, -1, 0, 1, 500, 1000, 500], [[0, 0, 0]]),
        case('fence_next_to_stone', [block(1, 0, 0, *STONE)], hand('minecraft:oak_fence'), NORTH_OF,
             [0, -1, 0, 1, 500, 1000, 500], [[0, 0, 0]]),
    ]
    # Orientation from the look direction: the same top-face click looking
    # each way (yaw 0 south, 90 west, 180 north, -90 east), and from the
    # clicked face.
    looks = {'south': [0.5, 0, -1.5, 0, 35], 'west': [2.5, 0, 0.5, 90, 35], 'north': [0.5, 0, 2.5, 180, 35],
             'east': [-1.5, 0, 0.5, -90, 35]}
    for name, item in (('trapdoor', 'minecraft:trapdoor'), ('glazed', 'minecraft:white_glazed_terracotta'),
                       ('chest', 'minecraft:chest'), ('observer', 'minecraft:observer'),
                       ('piston', 'minecraft:piston'), ('hopper', 'minecraft:hopper'),
                       ('end_rod', 'minecraft:end_rod'), ('sign', 'minecraft:oak_sign'),
                       ('door', 'minecraft:wooden_door'), ('stairs', 'minecraft:oak_stairs'),
                       ('anvil', 'minecraft:anvil'), ('lectern', 'minecraft:lectern'),
                       ('campfire', 'minecraft:campfire'), ('bell', 'minecraft:bell'),
                       ('pumpkin', 'minecraft:carved_pumpkin'), ('bed', 'minecraft:bed')):
        for look, player in looks.items():
            out.append(case(f'look_{name}_{look}', [block(0, 0, 0, *STONE)], hand(item, 1 if item == 'minecraft:bed' else 16),
                            player, [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0], [0, 2, 0]]))
    # Clicked faces: a stone pillar two high at the origin, clicked on each
    # side from two blocks away and underneath from below.
    pillar = [block(0, 0, 0, *STONE), block(0, 1, 0, *STONE)]
    faces = {'north': (2, [0.5, 0, -2.5, 0, 0], [0, 1, -1]), 'south': (3, [0.5, 0, 3.5, 180, 0], [0, 1, 1]),
             'west': (4, [-2.5, 0, 0.5, -90, 0], [-1, 1, 0]), 'east': (5, [3.5, 0, 0.5, 90, 0], [1, 1, 0])}
    for name, item in (('button', 'minecraft:stone_button'), ('trapdoor', 'minecraft:trapdoor'),
                       ('torch', 'minecraft:torch'), ('ladder', 'minecraft:ladder'), ('lever', 'minecraft:lever'),
                       ('end_rod', 'minecraft:end_rod'), ('sign', 'minecraft:oak_sign'),
                       ('hook', 'minecraft:tripwire_hook'), ('bud', 'minecraft:small_amethyst_bud'),
                       ('hanging_sign', 'minecraft:oak_hanging_sign'), ('rod', 'minecraft:lightning_rod'),
                       ('frame', 'minecraft:frame')):
        for side, (face, player, target) in faces.items():
            click = {2: [500, 500, 0], 3: [500, 500, 1000], 4: [0, 500, 500], 5: [1000, 500, 500]}[face]
            out.append(case(f'{name}_on_{side}_face', pillar, hand(item), player, [0, 1, 0, face, *click], [target]))
        out.append(case(f'{name}_on_top_face', [block(0, 0, 0, *STONE)], hand(item), NORTH_OF,
                        [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]))
        out.append(case(f'{name}_under_block', [block(0, 2, 0, *STONE)], hand(item), [0.5, 0, -1.5, 0, -20],
                        [0, 2, 0, 0, 500, 0, 500], [[0, 1, 0]]))
    # A lever on a top or bottom face looking east (the axis of the look).
    out.append(case('lever_on_top_face_looking_east', [block(0, 0, 0, *STONE)], hand('minecraft:lever'),
                    looks['east'], [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]))
    out.append(case('lever_under_block_looking_east', [block(0, 2, 0, *STONE)], hand('minecraft:lever'),
                    [-1.5, 0, 0.5, -90, -20], [0, 2, 0, 0, 500, 0, 500], [[0, 1, 0]]))
    # Beds (and straw beds) with stone under both cells next to the clicked one.
    for look, player in looks.items():
        dx, dz = {'south': (0, 1), 'west': (-1, 0), 'north': (0, -1), 'east': (1, 0)}[look]
        for name in ('bed', 'straw_bed'):
            out.append(case(f'{name}_on_platform_{look}',
                            [block(0, 0, 0, *STONE), block(dx, 0, dz, *STONE), block(-dx, 0, -dz, *STONE)],
                            hand(f'minecraft:{name}', 1), player, [0, 0, 0, 1, 500, 1000, 500],
                            [[0, 1, 0], [dx, 1, dz], [-dx, 1, -dz]]))
    # Items that place a block of their own name without being block items,
    # and items that place a block of another name, looking each way.
    for name in ('cake', 'flower_pot', 'cauldron', 'brewing_stand', 'iron_door', 'redstone'):
        out.append(case(f'{name}_on_top_face', [block(0, 0, 0, *STONE)], hand(f'minecraft:{name}'), NORTH_OF,
                        [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0], [0, 2, 0]]))
    for name in ('repeater', 'comparator'):
        for look, player in looks.items():
            out.append(case(f'look_{name}_{look}', [block(0, 0, 0, *STONE)], hand(f'minecraft:{name}'), player,
                            [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]))
    # What a bed's head may stand on (looking south: the head goes to z+1).
    floors = {'bottom_slab': ('minecraft:oak_slab', {'minecraft:vertical_half': 'bottom'}),
              'top_slab': ('minecraft:oak_slab', {'minecraft:vertical_half': 'top'}),
              'glass': ('minecraft:glass', {}), 'fence': ('minecraft:oak_fence', {}),
              'carpet': ('minecraft:white_carpet', {}), 'short_grass': ('minecraft:short_grass', {})}
    for floor, (name, states) in floors.items():
        out.append(case(f'bed_head_over_{floor}', [block(0, 0, 0, *STONE), block(0, 0, 1, name, states)],
                        hand('minecraft:bed', 1), looks['south'], [0, 0, 0, 1, 500, 1000, 500],
                        [[0, 1, 0], [0, 1, 1]]))
    # Off-axis looks: a hanging sign on a block's north face and a standing
    # sign on a top face, looking 30 degrees either side of south.
    for yaw in (30, -30):
        out.append(case(f'hanging_sign_on_north_face_yaw_{yaw}', pillar, hand('minecraft:oak_hanging_sign'),
                        [0.5, 0, -2.5, yaw, 0], [0, 1, 0, 2, 500, 500, 0], [[0, 1, -1]]))
        out.append(case(f'sign_on_top_face_yaw_{yaw}', [block(0, 0, 0, *STONE)], hand('minecraft:oak_sign'),
                        [0.5, 0, -1.5, yaw, 35], [0, 0, 0, 1, 500, 1000, 500], [[0, 1, 0]]))
    out.append(case('hanging_sign_under_fence', [block(0, 2, 0, 'minecraft:oak_fence')],
                    hand('minecraft:oak_hanging_sign'), [0.5, 0, -1.5, 0, -20], [0, 2, 0, 0, 500, 0, 500],
                    [[0, 1, 0]]))
    return out
