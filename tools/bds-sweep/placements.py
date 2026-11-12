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


def case(key, setup, held, player, click, read, **extra):
    """`extra`: `after` steps, `read_later` [ticks, offsets], `watcher`
    [dx, dy, dz, yaw, pitch] and `watcher_hand` (see the plugin's
    `placements` job)."""
    return {'key': key, 'setup': setup, 'hand': held, 'player': player, 'click': click, 'read': read, **extra}


def block(dx, dy, dz, name, states=None) -> list:
    return [dx, dy, dz, name, states or {}]


# Standing two blocks north of the origin, looking south and down at it.
NORTH_OF = [0.5, 0, -1.5, 0, 35]
# Two blocks south-east, looking north-west at the origin's east face.
EAST_OF = [2.5, 0, 1.5, 135, 20]


def round_one() -> list[dict]:
    """The first set of clicks: placement rules, orientations, refusals."""
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


# ------------------------------------------------------------------ round 2

AIR_HAND = None
# Looking south at the origin from two blocks north (doors take two blocks).
SOUTHWARD = [0.5, 0, -2.5, 0, 30]
LOOKS = {'south': [0.5, 0, -1.5, 0, 35], 'west': [2.5, 0, 0.5, 90, 35], 'north': [0.5, 0, 2.5, 180, 35],
         'east': [-1.5, 0, 0.5, -90, 35]}
# A stone pillar two high at the origin, clicked on each side from two blocks
# away: face, player, the cell the click puts a block in, the click position.
PILLAR = [block(0, 0, 0, *STONE), block(0, 1, 0, *STONE)]
SIDE_FACES = {'north': (2, [0.5, 0, -2.5, 0, 0], [0, 1, -1], [500, 500, 0]),
              'south': (3, [0.5, 0, 3.5, 180, 0], [0, 1, 1], [500, 500, 1000]),
              'west': (4, [-2.5, 0, 0.5, -90, 0], [-1, 1, 0], [0, 500, 500]),
              'east': (5, [3.5, 0, 0.5, 90, 0], [1, 1, 0], [1000, 500, 500])}
TOP = [0, 0, 0, 1, 500, 1000, 500]
UNDER = [0, 2, 0, 0, 500, 0, 500]
UNDER_PLAYER = [0.5, 0, -1.5, 0, -20]


def faces_of(name: str, item: str, count: int = 16, **extra) -> list[dict]:
    """`item` clicked on each side of the pillar, on top of a stone and under
    a stone two blocks up."""
    out = []
    for side, (face, player, target, click) in SIDE_FACES.items():
        out.append(case(f'{name}_on_{side}_face', PILLAR, hand(item, count), player, [0, 1, 0, face, *click],
                        [target], **extra))
    out.append(case(f'{name}_on_top_face', [block(0, 0, 0, *STONE)], hand(item, count), NORTH_OF, TOP, [[0, 1, 0]],
                    **extra))
    out.append(case(f'{name}_under_block', [block(0, 2, 0, *STONE)], hand(item, count), UNDER_PLAYER, UNDER,
                    [[0, 1, 0]], **extra))
    return out


def door(dx, dy, dz, name='minecraft:wooden_door', facing='west', hinge=False, open_=False) -> list[list]:
    states = {'minecraft:cardinal_direction': facing, 'door_hinge_bit': hinge, 'open_bit': open_}
    return [block(dx, dy, dz, name, {**states, 'upper_block_bit': False}),
            block(dx, dy + 1, dz, name, {**states, 'upper_block_bit': True})]


def signs() -> list[dict]:
    """Sign editors and text: placing opens the editor for the placer; the
    clients then send text edits; using a sign opens it, dyes, glow ink and
    honeycomb change it. The watcher stands two blocks east of the sign."""
    stone = [block(0, 0, 0, *STONE)]
    near = [2.5, 0, 0.5, 90, 30]
    sign_at = [0, 1, 0]
    edit = lambda who, text, front=True: ['sign', who, sign_at, front, text]  # noqa: E731
    standing = lambda direction=8: block(0, 1, 0, 'minecraft:standing_sign',  # noqa: E731
                                         {'ground_sign_direction': direction})
    click_sign = [0, 1, 0, 2, 500, 500, 0]
    out = [
        case('sign_edit_front', stone, hand('minecraft:oak_sign'), NORTH_OF, TOP, [sign_at],
             after=[edit('player', 'Hello\nworld')], watcher=near),
        case('sign_edit_back_unopened', stone, hand('minecraft:oak_sign'), NORTH_OF, TOP, [sign_at],
             after=[edit('player', 'Back', front=False)], watcher=near),
        case('sign_edit_twice', stone, hand('minecraft:oak_sign'), NORTH_OF, TOP, [sign_at],
             after=[edit('player', 'One'), edit('player', 'Two')], watcher=near),
        case('sign_edit_by_watcher', stone, hand('minecraft:oak_sign'), NORTH_OF, TOP, [sign_at],
             after=[edit('watcher', 'Not mine')], watcher=near),
        case('sign_edit_long_text', stone, hand('minecraft:oak_sign'), NORTH_OF, TOP, [sign_at],
             after=[edit('player', '\n'.join(['L' + str(i) + ' ' + 'x' * 90 for i in range(6)]))], watcher=near),
        case('sign_edit_formatting', stone, hand('minecraft:oak_sign'), NORTH_OF, TOP, [sign_at],
             after=[edit('player', '§cRed §lbold')], watcher=near),
        case('wall_sign_edit', PILLAR, hand('minecraft:oak_sign'), SIDE_FACES['east'][1], [0, 1, 0, 5, 1000, 500, 500],
             [[1, 1, 0]], after=[['sign', 'player', [1, 1, 0], True, 'Wall']], watcher=[3.5, 0, -1.5, 45, 10]),
        case('hanging_sign_edit', [block(0, 2, 0, *STONE)], hand('minecraft:oak_hanging_sign'), UNDER_PLAYER, UNDER,
             [sign_at], after=[edit('player', 'Hanging')], watcher=near),
        case('spruce_sign_edit', stone, hand('minecraft:spruce_sign'), NORTH_OF, TOP, [sign_at],
             after=[edit('player', 'Spruce')], watcher=near),
    ]
    # The longest text BDS keeps: total characters and lines.
    for chars in (80, 128, 255, 256, 257, 400, 511, 512, 513, 600):
        out.append(case(f'sign_edit_chars_{chars}', stone, hand('minecraft:oak_sign'), NORTH_OF, TOP, [sign_at],
                        after=[edit('player', 'x' * chars)], watcher=near))
    for lines in (4, 5, 6, 10, 20):
        out.append(case(f'sign_edit_lines_{lines}', stone, hand('minecraft:oak_sign'), NORTH_OF, TOP, [sign_at],
                        after=[edit('player', '\n'.join('l' * 3 for _ in range(lines)))], watcher=near))
    out.append(case('sign_edit_line_100_chars_then_short', stone, hand('minecraft:oak_sign'), NORTH_OF, TOP,
                    [sign_at], after=[edit('player', 'y' * 100 + '\nshort')], watcher=near))
    # Using a sign: from the north and south, standing signs facing south (0)
    # and north (8).
    for direction in (0, 8):
        out.append(case(f'sign_use_from_north_dir_{direction}', stone + [standing(direction)], AIR_HAND, NORTH_OF,
                        click_sign, [sign_at], watcher=near))
        out.append(case(f'sign_use_from_south_dir_{direction}', stone + [standing(direction)], AIR_HAND,
                        [0.5, 0, 2.5, 180, 30], [0, 1, 0, 3, 500, 500, 1000], [sign_at], watcher=near))
    out += [
        case('sign_use_then_edit', stone + [standing(0)], AIR_HAND, NORTH_OF, click_sign, [sign_at],
             after=[edit('player', 'Used', front=False)], watcher=near),
        case('sign_dye_blank', stone + [standing(8)], hand('minecraft:red_dye'), NORTH_OF, click_sign, [sign_at],
             watcher=near),
        case('sign_dye_after_text', stone, hand('minecraft:oak_sign'), NORTH_OF, TOP, [sign_at],
             after=[edit('player', 'Dye me'), ['hand', 'player', 'minecraft:red_dye', 4],
                    ['use', 'player', click_sign]], watcher=near),
        case('sign_glow_after_text', stone, hand('minecraft:oak_sign'), NORTH_OF, TOP, [sign_at],
             after=[edit('player', 'Glow'), ['hand', 'player', 'minecraft:glow_ink_sac', 4],
                    ['use', 'player', click_sign], ['hand', 'player', 'minecraft:ink_sac', 4],
                    ['use', 'player', click_sign]], watcher=near),
        case('sign_honeycomb', stone + [standing(8)], hand('minecraft:honeycomb'), NORTH_OF, click_sign, [sign_at],
             after=[['hand', 'player', None, 0], ['use', 'player', click_sign], edit('player', 'Waxed')],
             watcher=near),
        case('sign_wall_use', [block(0, 1, -1, *STONE), block(0, 0, 0, *STONE),
                               block(0, 1, 0, 'minecraft:wall_sign', {'facing_direction': 3})],
             AIR_HAND, [0.5, 0, 2.5, 180, 20], [0, 1, 0, 3, 500, 500, 1000], [sign_at], watcher=near),
    ]
    return out


def door_hinges() -> list[dict]:
    """Which side a placed door hinges on: neighbouring doors, blocks beside
    the door and the click position. The player looks south (the door faces
    west); its left is east."""
    floor_click = lambda x=500: [0, -1, 0, 1, x, 1000, 500]  # noqa: E731
    read = [[0, 0, 0], [0, 1, 0]]
    stone = lambda dx, dy: block(dx, dy, 0, *STONE)  # noqa: E731
    setups = {
        'nothing': [],
        'door_east': door(1, 0, 0), 'door_west': door(-1, 0, 0),
        'door_east_hinged': door(1, 0, 0, hinge=True), 'door_west_hinged': door(-1, 0, 0, hinge=True),
        'door_east_facing_north': door(1, 0, 0, facing='north'),
        'door_east_open': door(1, 0, 0, open_=True),
        'iron_door_east': door(1, 0, 0, 'minecraft:iron_door'),
        'spruce_door_east': door(1, 0, 0, 'minecraft:spruce_door'),
        'stone_east_lower': [stone(1, 0)], 'stone_east_upper': [stone(1, 1)],
        'stone_east_both': [stone(1, 0), stone(1, 1)], 'stone_west_lower': [stone(-1, 0)],
        'stone_west_upper': [stone(-1, 1)], 'stone_west_both': [stone(-1, 0), stone(-1, 1)],
        'stone_both_sides': [stone(1, 0), stone(1, 1), stone(-1, 0), stone(-1, 1)],
        'stone_east_both_west_lower': [stone(1, 0), stone(1, 1), stone(-1, 0)],
        'glass_east_both': [block(1, 0, 0, 'minecraft:glass'), block(1, 1, 0, 'minecraft:glass')],
        'slab_east_lower': [block(1, 0, 0, 'minecraft:oak_slab', {'minecraft:vertical_half': 'bottom'})],
        'fence_east_both': [block(1, 0, 0, 'minecraft:oak_fence'), block(1, 1, 0, 'minecraft:oak_fence')],
    }
    out = []
    for name, setup in setups.items():
        out.append(case(f'door_hinge_{name}', setup, hand('minecraft:wooden_door'), SOUTHWARD, floor_click(),
                        read + [[1, 0, 0], [1, 1, 0], [-1, 0, 0], [-1, 1, 0]]))
    for x in (100, 400, 600, 900):
        out.append(case(f'door_hinge_click_x{x}', [], hand('minecraft:wooden_door'), SOUTHWARD, floor_click(x), read))
        out.append(case(f'door_hinge_stone_both_sides_click_x{x}', setups['stone_both_sides'],
                        hand('minecraft:wooden_door'), SOUTHWARD, floor_click(x), read))
    # The same door on the player's left, looking each way.
    lefts = {'south': (1, 0, 'west', [0.5, 0, -2.5, 0, 30]), 'west': (0, 1, 'north', [2.5, 0, 0.5, 90, 30]),
             'north': (-1, 0, 'east', [0.5, 0, 2.5, 180, 30]), 'east': (0, -1, 'south', [-1.5, 0, 0.5, -90, 30])}
    for look, (dx, dz, facing, player) in lefts.items():
        out.append(case(f'door_hinge_look_{look}_door_left', door(dx, 0, dz, facing=facing),
                        hand('minecraft:wooden_door'), player, floor_click(), read))
        out.append(case(f'door_hinge_look_{look}_door_right', door(-dx, 0, -dz, facing=facing),
                        hand('minecraft:wooden_door'), player, floor_click(), read))
        out.append(case(f'door_hinge_look_{look}_stone_left', [block(dx, 0, dz, *STONE), block(dx, 1, dz, *STONE)],
                        hand('minecraft:wooden_door'), player, floor_click(), read))
    return out


def uses() -> list[dict]:
    """Using doors, trapdoors, fence gates, buttons and levers with an empty
    hand (every door, trapdoor and gate once), and a few variations."""
    import native
    names = sorted({e['name'] for e in native.block_states(typed=True)})
    out = []
    north_face = [0, 0, 0, 2, 500, 500, 0]
    read_door = [[0, 0, 0], [0, 1, 0]]
    for name in (n for n in names if n.endswith('_door')):
        short = name.removeprefix('minecraft:')
        out.append(case(f'use_{short}', door(0, 0, 0, name), AIR_HAND, SOUTHWARD, north_face, read_door))
    out += [
        case('use_door_upper_half', door(0, 0, 0), AIR_HAND, SOUTHWARD, [0, 1, 0, 2, 500, 500, 0], read_door),
        case('use_door_open_closes', door(0, 0, 0, open_=True), AIR_HAND, SOUTHWARD, north_face, read_door),
        case('use_door_holding_stone', door(0, 0, 0), hand('minecraft:stone'), SOUTHWARD, north_face,
             read_door + [[0, 0, -1]]),
        case('use_door_twice', door(0, 0, 0), AIR_HAND, SOUTHWARD, north_face, read_door,
             after=[['use', 'player', north_face]]),
        case('use_door_hinged', door(0, 0, 0, hinge=True), AIR_HAND, SOUTHWARD, north_face, read_door),
        case('use_door_facing_south', door(0, 0, 0, facing='south'), AIR_HAND, SOUTHWARD, north_face, read_door),
        case('use_door_by_watcher', door(0, 0, 0), AIR_HAND, SOUTHWARD, north_face, read_door,
             after=[['use', 'watcher', [0, 0, 0, 4, 0, 500, 500]]], watcher=[-1.5, 0, 0.5, -90, 20]),
    ]
    top = [0, 0, 0, 1, 500, 1000, 500]
    for name in (n for n in names if n.endswith('_trapdoor') or n == 'minecraft:trapdoor'):
        short = name.removeprefix('minecraft:')
        out.append(case(f'use_{short}', [block(0, 0, 0, name, {'direction': 0, 'open_bit': False,
                                                                'upside_down_bit': False})],
                        AIR_HAND, NORTH_OF, top, [[0, 0, 0]]))
    out += [
        case('use_trapdoor_open_closes', [block(0, 0, 0, 'minecraft:trapdoor', {'direction': 2, 'open_bit': True,
                                                                                'upside_down_bit': True})],
             AIR_HAND, NORTH_OF, north_face, [[0, 0, 0]]),
        case('use_trapdoor_holding_stone', [block(0, 0, 0, 'minecraft:trapdoor')], hand('minecraft:stone'),
             NORTH_OF, top, [[0, 0, 0], [0, 1, 0]]),
    ]
    gate = lambda name, facing='south', open_=False: [block(0, 0, 0, name, {  # noqa: E731
        'minecraft:cardinal_direction': facing, 'open_bit': open_, 'in_wall_bit': False})]
    for name in (n for n in names if n.endswith('fence_gate')):
        short = name.removeprefix('minecraft:')
        out.append(case(f'use_{short}', gate(name), AIR_HAND, NORTH_OF, north_face, [[0, 0, 0]]))
    south_face = [0, 0, 0, 3, 500, 500, 1000]
    from_south = [0.5, 0, 2.5, 180, 30]
    for facing in ('south', 'north', 'east', 'west'):
        out.append(case(f'use_gate_facing_{facing}_from_north', gate('minecraft:fence_gate', facing), AIR_HAND,
                        NORTH_OF, north_face, [[0, 0, 0]]))
        out.append(case(f'use_gate_facing_{facing}_from_south', gate('minecraft:fence_gate', facing), AIR_HAND,
                        from_south, south_face, [[0, 0, 0]]))
        out.append(case(f'use_gate_open_facing_{facing}_from_south', gate('minecraft:fence_gate', facing, True),
                        AIR_HAND, from_south, south_face, [[0, 0, 0]]))
    on_stone = lambda name, states: [block(0, 0, 0, *STONE), block(0, 1, 0, name, states)]  # noqa: E731
    button_click = [0, 1, 0, 1, 500, 1000, 500]
    for name in (n for n in names if n.endswith('_button')):
        short = name.removeprefix('minecraft:')
        out.append(case(f'use_{short}', on_stone(name, {'facing_direction': 1, 'button_pressed_bit': False}),
                        AIR_HAND, NORTH_OF, button_click, [[0, 1, 0]], read_later=[40, [[0, 1, 0]]]))
    out += [
        case('use_stone_button_timing', on_stone('minecraft:stone_button',
                                                 {'facing_direction': 1, 'button_pressed_bit': False}),
             AIR_HAND, NORTH_OF, button_click, [[0, 1, 0]], after=[['wait', 8]], read_later=[4, [[0, 1, 0]]]),
        case('use_wooden_button_timing', on_stone('minecraft:wooden_button',
                                                  {'facing_direction': 1, 'button_pressed_bit': False}),
             AIR_HAND, NORTH_OF, button_click, [[0, 1, 0]], after=[['wait', 18]], read_later=[4, [[0, 1, 0]]]),
        case('use_button_pressed', on_stone('minecraft:stone_button',
                                            {'facing_direction': 1, 'button_pressed_bit': True}),
             AIR_HAND, NORTH_OF, button_click, [[0, 1, 0]]),
        case('use_lever', on_stone('minecraft:lever', {'lever_direction': 'up_north_south', 'open_bit': False}),
             AIR_HAND, NORTH_OF, button_click, [[0, 1, 0]]),
        case('use_lever_twice', on_stone('minecraft:lever', {'lever_direction': 'up_north_south', 'open_bit': False}),
             AIR_HAND, NORTH_OF, button_click, [[0, 1, 0]], after=[['use', 'player', button_click]]),
        case('use_lever_holding_stone', on_stone('minecraft:lever', {'lever_direction': 'up_north_south',
                                                                     'open_bit': False}),
             hand('minecraft:stone'), NORTH_OF, button_click, [[0, 1, 0], [0, 2, 0]]),
    ]
    return out


CORALS = ('brain', 'bubble', 'fire', 'horn', 'tube')


def coral() -> list[dict]:
    """Coral plants and fans, live and dead, on top faces and walls; the
    live ones are read again 6 seconds later (out of water)."""
    out = []
    later = lambda target: [120, [target]]  # noqa: E731
    east = SIDE_FACES['east']
    for kind in CORALS:
        for dead in ('', 'dead_'):
            extra = {} if dead else {'read_later': later([0, 1, 0])}
            out.append(case(f'{dead}{kind}_coral_fan_on_top', [block(0, 0, 0, *STONE)],
                            hand(f'minecraft:{dead}{kind}_coral_fan'), NORTH_OF, TOP, [[0, 1, 0]], **extra))
            side_extra = {} if dead else {'read_later': later(east[2])}
            out.append(case(f'{dead}{kind}_coral_fan_on_east_face', PILLAR, hand(f'minecraft:{dead}{kind}_coral_fan'),
                            east[1], [0, 1, 0, east[0], *east[3]], [east[2]], **side_extra))
            out.append(case(f'{dead}{kind}_coral_on_top', [block(0, 0, 0, *STONE)],
                            hand(f'minecraft:{dead}{kind}_coral'), NORTH_OF, TOP, [[0, 1, 0]], **extra))
            block_extra = {} if dead else {'read_later': later([0, 1, 0])}
            out.append(case(f'{dead}{kind}_coral_block_on_top', [block(0, 0, 0, *STONE)],
                            hand(f'minecraft:{dead}{kind}_coral_block'), NORTH_OF, TOP, [[0, 1, 0]], **block_extra))
    for look, player in LOOKS.items():
        out.append(case(f'dead_brain_coral_fan_look_{look}', [block(0, 0, 0, *STONE)],
                        hand('minecraft:dead_brain_coral_fan'), player, TOP, [[0, 1, 0]]))
    out += [c for c in faces_of('dead_brain_coral_fan', 'minecraft:dead_brain_coral_fan')
            if not c['key'].endswith(('_on_east_face', '_on_top_face'))]
    out += [c for c in faces_of('dead_brain_coral', 'minecraft:dead_brain_coral') if 'top' not in c['key']]
    return out


SKULLS = ('skeleton_skull', 'wither_skeleton_skull', 'zombie_head', 'player_head', 'creeper_head', 'dragon_head',
          'piglin_head')


def skulls() -> list[dict]:
    """Skulls and heads: the facing from the clicked face and the rotation of
    a standing one from the look (its block actor)."""
    out = []
    for yaw in (0, 10, 30, 45, 60, 90, 135, 180, -90, -45, -150, 200):
        out.append(case(f'skull_on_top_yaw_{yaw}', [block(0, 0, 0, *STONE)], hand('minecraft:skeleton_skull'),
                        [0.5, 0, -1.5, yaw, 60], TOP, [[0, 1, 0]]))
    out += [c for c in faces_of('skull', 'minecraft:skeleton_skull') if 'top' not in c['key']]
    for name in SKULLS:
        out.append(case(f'{name}_on_top', [block(0, 0, 0, *STONE)], hand(f'minecraft:{name}'), NORTH_OF, TOP,
                        [[0, 1, 0]]))
        out.append(case(f'{name}_on_east_face', PILLAR, hand(f'minecraft:{name}'), SIDE_FACES['east'][1],
                        [0, 1, 0, 5, 1000, 500, 500], [[1, 1, 0]]))
    return out


def directions() -> list[dict]:
    """Blocks that take a direction from the look: each look on a top face,
    and pitch up and down for the crafter."""
    out = []
    for name in ('bee_nest', 'beehive', 'loom', 'chiseled_bookshelf', 'decorated_pot', 'crafter', 'grindstone',
                 'end_portal_frame', 'copper_golem_statue', 'shelf_mushroom', 'calibrated_sculk_sensor',
                 'oak_shelf'):
        for look, player in LOOKS.items():
            out.append(case(f'look_{name}_{look}', [block(0, 0, 0, *STONE)], hand(f'minecraft:{name}'), player, TOP,
                            [[0, 1, 0]]))
    for pitch in (-80, 80):
        out.append(case(f'look_crafter_pitch_{pitch}', [block(0, 0, 0, *STONE)], hand('minecraft:crafter'),
                        [0.5, 0, -1.5, 0, pitch], TOP, [[0, 1, 0]]))
    out += [c for c in faces_of('grindstone', 'minecraft:grindstone') if 'top' not in c['key']]
    out += [c for c in faces_of('shelf_mushroom', 'minecraft:shelf_mushroom') if 'top' not in c['key']]
    return out


def mushrooms() -> list[dict]:
    """Huge mushroom blocks: the faces a placed block shows, alone and next
    to other mushroom blocks."""
    out = []
    for name in ('brown_mushroom_block', 'red_mushroom_block', 'mushroom_stem'):
        out.append(case(f'{name}_on_stone', [block(0, 0, 0, *STONE)], hand(f'minecraft:{name}'), NORTH_OF, TOP,
                        [[0, 1, 0]]))
    bits = {'huge_mushroom_bits': 14}
    for neighbour in ('brown_mushroom_block', 'red_mushroom_block'):
        out.append(case(f'brown_mushroom_block_next_to_{neighbour}',
                        [block(0, 0, 0, *STONE), block(1, 1, 0, f'minecraft:{neighbour}', bits)],
                        hand('minecraft:brown_mushroom_block'), NORTH_OF, TOP, [[0, 1, 0], [1, 1, 0]]))
    out.append(case('brown_mushroom_block_on_brown', [block(0, 0, 0, 'minecraft:brown_mushroom_block', bits)],
                    hand('minecraft:brown_mushroom_block'), NORTH_OF, TOP, [[0, 1, 0], [0, 0, 0]]))
    return out


def bells() -> list[dict]:
    """Bells under a block, on each wall, and between two blocks."""
    out = [c for c in faces_of('bell', 'minecraft:bell') if 'top' not in c['key']]
    out.append(case('bell_between_blocks', PILLAR + [block(2, 1, 0, *STONE)], hand('minecraft:bell'),
                    [1.5, 0, 3.5, 180, 0], [0, 1, 0, 5, 1000, 500, 500], [[1, 1, 0]]))
    out.append(case('bell_between_blocks_north_south', [block(0, 0, 0, *STONE), block(0, 1, 0, *STONE),
                                                        block(0, 1, 2, *STONE)],
                    hand('minecraft:bell'), [-1.5, 0, 1.5, -90, 0], [0, 1, 0, 3, 500, 500, 1000], [[0, 1, 1]]))
    for look, player in LOOKS.items():
        out.append(case(f'bell_under_block_look_{look}', [block(0, 2, 0, *STONE)], hand('minecraft:bell'),
                        [player[0], 0, player[2], player[3], -20], UNDER, [[0, 1, 0]]))
    return out


def multiface() -> list[dict]:
    """Vines, glow lichen, sculk veins and resin clumps on every face, and a
    second face added to an existing one."""
    out = []
    for name in ('vine', 'glow_lichen', 'sculk_vein', 'resin_clump'):
        out += faces_of(name, f'minecraft:{name}')
        east = SIDE_FACES['east']
        # A second click on the block below the first one's cell.
        out.append(case(f'{name}_second_face', PILLAR + [block(1, 0, 0, *STONE)], hand(f'minecraft:{name}'),
                        east[1], [0, 1, 0, east[0], *east[3]], [east[2]],
                        after=[['use', 'player', [1, 0, 0, 1, 500, 1000, 500]]]))
    return out


def candles() -> list[dict]:
    """Candles stacking, candles on cakes, lighting and putting them out,
    eating a candle cake and a cake."""
    stone = [block(0, 0, 0, *STONE)]
    on_top = [0, 1, 0, 1, 500, 500, 500]
    at = [[0, 1, 0], [0, 2, 0]]
    candle = lambda name='minecraft:candle', count=0, lit=False: block(0, 1, 0, name,  # noqa: E731
                                                                      {'candles': count, 'lit': lit})
    cake = lambda bites=0: block(0, 1, 0, 'minecraft:cake', {'bite_counter': bites})  # noqa: E731
    out = [
        case('candle_on_top', stone, hand('minecraft:candle'), NORTH_OF, TOP, at),
        case('candle_onto_candle', stone + [candle()], hand('minecraft:candle'), NORTH_OF, on_top, at),
        case('candle_onto_three_candles', stone + [candle(count=2)], hand('minecraft:candle'), NORTH_OF, on_top, at),
        case('candle_onto_four_candles', stone + [candle(count=3)], hand('minecraft:candle'), NORTH_OF, on_top, at),
        case('white_candle_onto_candle', stone + [candle()], hand('minecraft:white_candle'), NORTH_OF, on_top, at),
        case('candle_onto_lit_candle', stone + [candle(lit=True)], hand('minecraft:candle'), NORTH_OF, on_top, at),
        case('candle_onto_cake', stone + [cake()], hand('minecraft:candle'), NORTH_OF, on_top, at),
        case('candle_onto_bitten_cake', stone + [cake(1)], hand('minecraft:candle'), NORTH_OF, on_top, at),
        case('candle_onto_candle_cake', stone + [block(0, 1, 0, 'minecraft:candle_cake', {'lit': False})],
             hand('minecraft:candle'), NORTH_OF, on_top, at),
        case('lit_candle_empty_hand', stone + [candle(count=1, lit=True)], AIR_HAND, NORTH_OF, on_top, at),
        case('unlit_candle_empty_hand', stone + [candle()], AIR_HAND, NORTH_OF, on_top, at),
        case('lit_candle_cake_empty_hand', stone + [block(0, 1, 0, 'minecraft:candle_cake', {'lit': True})],
             AIR_HAND, NORTH_OF, on_top, at),
        case('candle_cake_empty_hand', stone + [block(0, 1, 0, 'minecraft:candle_cake', {'lit': False})],
             AIR_HAND, NORTH_OF, on_top, at),
        case('cake_empty_hand', stone + [cake()], AIR_HAND, NORTH_OF, on_top, at),
        case('flint_and_steel_on_candle', stone + [candle()], hand('minecraft:flint_and_steel', 1), NORTH_OF,
             on_top, at),
        case('flint_and_steel_on_candle_cake', stone + [block(0, 1, 0, 'minecraft:candle_cake', {'lit': False})],
             hand('minecraft:flint_and_steel', 1), NORTH_OF, on_top, at),
    ]
    import native
    names = sorted({e['name'] for e in native.block_states(typed=True)})
    for name in (n for n in names if n.endswith('_candle')):
        short = name.removeprefix('minecraft:')
        out.append(case(f'{short}_onto_cake', stone + [cake()], hand(name), NORTH_OF, on_top, at))
        out.append(case(f'{short}_onto_itself', stone + [candle(name)], hand(name), NORTH_OF, on_top, at))
    return out


def crops() -> list[dict]:
    """Items that plant something: seeds on farmland, berries, cocoa, nether
    wart, sugar cane, string, bamboo."""
    farmland = [block(0, 0, 0, 'minecraft:farmland', {'moisturized_amount': 7})]
    out = []
    for item in ('wheat_seeds', 'carrot', 'potato', 'beetroot_seeds', 'melon_seeds', 'pumpkin_seeds',
                 'torchflower_seeds', 'pitcher_pod'):
        out.append(case(f'{item}_on_farmland', farmland, hand(f'minecraft:{item}'), NORTH_OF, TOP,
                        [[0, 1, 0], [0, 2, 0], [0, 0, 0]]))
    out += [
        case('wheat_seeds_on_dirt', [block(0, 0, 0, 'minecraft:dirt')], hand('minecraft:wheat_seeds'), NORTH_OF, TOP,
             [[0, 1, 0]]),
        case('wheat_seeds_on_grass', [], hand('minecraft:wheat_seeds'), NORTH_OF, [0, -1, 0, 1, 500, 1000, 500],
             [[0, 0, 0]]),
        case('sweet_berries_on_grass', [], hand('minecraft:sweet_berries'), NORTH_OF, [0, -1, 0, 1, 500, 1000, 500],
             [[0, 0, 0]]),
        case('sweet_berries_on_stone', [block(0, 0, 0, *STONE)], hand('minecraft:sweet_berries'), NORTH_OF, TOP,
             [[0, 1, 0]]),
        case('glow_berries_under_stone', [block(0, 2, 0, *STONE)], hand('minecraft:glow_berries'), UNDER_PLAYER,
             UNDER, [[0, 1, 0]]),
        case('glow_berries_on_stone', [block(0, 0, 0, *STONE)], hand('minecraft:glow_berries'), NORTH_OF, TOP,
             [[0, 1, 0]]),
        case('nether_wart_on_soul_sand', [block(0, 0, 0, 'minecraft:soul_sand')], hand('minecraft:nether_wart'),
             NORTH_OF, TOP, [[0, 1, 0]]),
        case('nether_wart_on_stone', [block(0, 0, 0, *STONE)], hand('minecraft:nether_wart'), NORTH_OF, TOP,
             [[0, 1, 0]]),
        case('sugar_cane_by_water', [block(0, -1, 0, 'minecraft:sand'), block(1, -1, 0, 'minecraft:water',
                                                                            {'liquid_depth': 0})],
             hand('minecraft:sugar_cane'), NORTH_OF, [0, -1, 0, 1, 500, 1000, 500], [[0, 0, 0]]),
        case('sugar_cane_dry', [block(0, -1, 0, 'minecraft:sand')], hand('minecraft:sugar_cane'), NORTH_OF,
             [0, -1, 0, 1, 500, 1000, 500], [[0, 0, 0]]),
        case('string_on_stone', [block(0, 0, 0, *STONE)], hand('minecraft:string'), NORTH_OF, TOP, [[0, 1, 0]]),
        case('bamboo_on_grass', [], hand('minecraft:bamboo'), NORTH_OF, [0, -1, 0, 1, 500, 1000, 500], [[0, 0, 0]]),
        case('kelp_on_stone', [block(0, 0, 0, *STONE)], hand('minecraft:kelp'), NORTH_OF, TOP, [[0, 1, 0]]),
    ]
    jungle = [block(0, 0, 0, 'minecraft:jungle_log', {'pillar_axis': 'y'}),
              block(0, 1, 0, 'minecraft:jungle_log', {'pillar_axis': 'y'})]
    for side, (face, player, target, click) in SIDE_FACES.items():
        out.append(case(f'cocoa_beans_on_{side}_face', jungle, hand('minecraft:cocoa_beans'), player,
                        [0, 1, 0, face, *click], [target]))
    out.append(case('cocoa_beans_on_oak_log', [block(0, 0, 0, 'minecraft:oak_log', {'pillar_axis': 'y'}),
                                                block(0, 1, 0, 'minecraft:oak_log', {'pillar_axis': 'y'})],
                    hand('minecraft:cocoa_beans'), SIDE_FACES['east'][1], [0, 1, 0, 5, 1000, 500, 500], [[1, 1, 0]]))
    out.append(case('cocoa_beans_on_top', [block(0, 0, 0, 'minecraft:jungle_log', {'pillar_axis': 'y'})],
                    hand('minecraft:cocoa_beans'), NORTH_OF, TOP, [[0, 1, 0]]))
    return out


def plants_and_stacks() -> list[dict]:
    """Leaves placed by a player, lily pads on water, wither roses and
    mushrooms on their soils, and blocks that stack when placed again."""
    import native
    names = sorted({e['name'] for e in native.block_states(typed=True)})
    stone = [block(0, 0, 0, *STONE)]
    water_floor = [block(0, -1, 0, 'minecraft:water', {'liquid_depth': 0})]
    floor_click = [0, -1, 0, 1, 500, 1000, 500]
    on_top = [0, 1, 0, 1, 500, 500, 500]
    out = []
    for name in (n for n in names if n.endswith('_leaves') or n == 'minecraft:azalea_leaves_flowered'):
        out.append(case(f'{name.removeprefix("minecraft:")}_placed', stone, hand(name), NORTH_OF, TOP, [[0, 1, 0]]))
    out += [
        case('waterlily_on_water', water_floor, hand('minecraft:waterlily'), NORTH_OF, floor_click, [[0, 0, 0]]),
        case('waterlily_on_stone', stone, hand('minecraft:waterlily'), NORTH_OF, TOP, [[0, 1, 0]]),
        case('waterlily_on_ice', [block(0, 0, 0, 'minecraft:ice')], hand('minecraft:waterlily'), NORTH_OF, TOP,
             [[0, 1, 0]]),
        case('frog_spawn_on_water', water_floor, hand('minecraft:frog_spawn'), NORTH_OF, floor_click, [[0, 0, 0]]),
    ]
    for soil in ('grass_block', 'dirt', 'netherrack', 'soul_sand', 'soul_soil', 'stone', 'moss_block', 'farmland',
                 'mycelium', 'podzol', 'mud', 'sand'):
        setup = [block(0, 0, 0, f'minecraft:{soil}')]
        out.append(case(f'wither_rose_on_{soil}', setup, hand('minecraft:wither_rose'), NORTH_OF, TOP, [[0, 1, 0]]))
        out.append(case(f'brown_mushroom_on_{soil}', setup, hand('minecraft:brown_mushroom'), NORTH_OF, TOP,
                        [[0, 1, 0]]))
    out += [
        case('snow_layer_onto_snow_layer', stone + [block(0, 1, 0, 'minecraft:snow_layer', {'height': 0,
                                                                                            'covered_bit': False})],
             hand('minecraft:snow_layer'), NORTH_OF, on_top, [[0, 1, 0], [0, 2, 0]]),
        case('snow_layer_onto_side_of_snow_layer', stone + [block(0, 1, 0, 'minecraft:snow_layer',
                                                                 {'height': 2, 'covered_bit': False})],
             hand('minecraft:snow_layer'), SIDE_FACES['east'][1], [0, 1, 0, 5, 1000, 100, 500], [[0, 1, 0], [1, 1, 0]]),
        case('sea_pickle_onto_sea_pickle', stone + [block(0, 1, 0, 'minecraft:sea_pickle', {'cluster_count': 0,
                                                                                            'dead_bit': True})],
             hand('minecraft:sea_pickle'), NORTH_OF, on_top, [[0, 1, 0], [0, 2, 0]]),
        case('sea_pickle_on_stone', stone, hand('minecraft:sea_pickle'), NORTH_OF, TOP, [[0, 1, 0]]),
        case('turtle_egg_onto_turtle_egg', [block(0, 0, 0, 'minecraft:sand'),
                                            block(0, 1, 0, 'minecraft:turtle_egg', {'cracked_state': 'no_cracks',
                                                                                     'turtle_egg_count': 'one_egg'})],
             hand('minecraft:turtle_egg'), NORTH_OF, on_top, [[0, 1, 0], [0, 2, 0]]),
        case('turtle_egg_on_stone', stone, hand('minecraft:turtle_egg'), NORTH_OF, TOP, [[0, 1, 0]]),
    ]
    for name in ('pink_petals', 'wildflowers', 'leaf_litter'):
        states = {'growth': 0, 'minecraft:cardinal_direction': 'south'}
        out.append(case(f'{name}_onto_itself',
                        [block(0, 0, 0, 'minecraft:grass_block'), block(0, 1, 0, f'minecraft:{name}', states)],
                        hand(f'minecraft:{name}'), NORTH_OF, on_top, [[0, 1, 0], [0, 2, 0]]))
        for look, player in LOOKS.items():
            out.append(case(f'look_{name}_{look}', [block(0, 0, 0, 'minecraft:grass_block')],
                            hand(f'minecraft:{name}'), player, TOP, [[0, 1, 0]]))
    return out


def actors() -> list[dict]:
    """Blocks with a block actor placed by their item: banners, flower pots
    with a plant, beds."""
    stone = [block(0, 0, 0, *STONE)]
    pot = stone + [block(0, 1, 0, 'minecraft:flower_pot', {'update_bit': False})]
    pot_click = [0, 1, 0, 1, 500, 500, 500]
    out = [case(f'banner_on_top_{look}', stone, hand('minecraft:banner'), player, TOP, [[0, 1, 0], [0, 2, 0]])
           for look, player in LOOKS.items()]
    out += [c for c in faces_of('banner', 'minecraft:banner') if 'top' not in c['key']]
    out += [
        case('flower_pot_with_dandelion', pot, hand('minecraft:dandelion'), NORTH_OF, pot_click, [[0, 1, 0]]),
        case('flower_pot_with_cactus', pot, hand('minecraft:cactus'), NORTH_OF, pot_click, [[0, 1, 0]]),
        case('flower_pot_with_stone', pot, hand('minecraft:stone'), NORTH_OF, pot_click, [[0, 1, 0], [0, 2, 0]]),
        case('flower_pot_with_dandelion_then_empty_hand', pot, hand('minecraft:dandelion'), NORTH_OF, pot_click,
             [[0, 1, 0]], after=[['hand', 'player', None, 0], ['use', 'player', pot_click]]),
    ]
    return out


GROUPS = {'blocks': round_one, 'signs': signs, 'door_hinges': door_hinges, 'uses': uses, 'coral': coral,
          'skulls': skulls, 'directions': directions, 'mushrooms': mushrooms, 'bells': bells,
          'multiface': multiface, 'candles': candles, 'crops': crops, 'plants_and_stacks': plants_and_stacks,
          'actors': actors}


def items(groups: str = 'all') -> list[dict]:
    names = list(GROUPS) if groups == 'all' else groups.split(',')
    out = []
    for name in names:
        if name not in GROUPS:
            raise SystemExit(f'unknown placement group {name!r}; known: {", ".join(GROUPS)}')
        for item in GROUPS[name]():
            out.append({**item, 'group': name})
    keys = [item['key'] for item in out]
    duplicates = {key for key in keys if keys.count(key) > 1}
    if duplicates:
        raise SystemExit(f'duplicate placement keys: {sorted(duplicates)}')
    return out


def followup() -> list[dict]:
    """Round 2 follow-up: which blocks count for a door's hinge, the floor
    coral fan's direction over many looks, when a live coral block dies and
    every dye's sign text colour."""
    out = []
    floor_click = [0, -1, 0, 1, 500, 1000, 500]
    read = [[0, 0, 0], [0, 1, 0]]
    right = {'glass': ('minecraft:glass', {}), 'bottom_slab': ('minecraft:oak_slab', {'minecraft:vertical_half': 'bottom'}),
             'top_slab': ('minecraft:oak_slab', {'minecraft:vertical_half': 'top'}),
             'fence': ('minecraft:oak_fence', {}), 'leaves': ('minecraft:oak_leaves', {'persistent_bit': True,
                                                                                     'update_bit': False}),
             'chest': ('minecraft:chest', {}), 'stairs': ('minecraft:oak_stairs', {}),
             'trapdoor': ('minecraft:trapdoor', {}), 'glowstone': ('minecraft:glowstone', {}),
             'ice': ('minecraft:ice', {}), 'scaffolding': ('minecraft:scaffolding', {}),
             'cobblestone_wall': ('minecraft:cobblestone_wall', {})}
    for name, (block_name, states) in right.items():
        out.append(case(f'door_hinge_right_{name}', [block(-1, 0, 0, block_name, states), block(-1, 1, 0, block_name,
                                                                                                    states)],
                        hand('minecraft:wooden_door'), SOUTHWARD, floor_click, read))
    out += [
        case('door_hinge_door_left_stone_right', door(1, 0, 0) + [block(-1, 0, 0, *STONE), block(-1, 1, 0, *STONE)],
             hand('minecraft:wooden_door'), SOUTHWARD, floor_click, read),
        case('door_hinge_door_right_stone_left', door(-1, 0, 0) + [block(1, 0, 0, *STONE), block(1, 1, 0, *STONE)],
             hand('minecraft:wooden_door'), SOUTHWARD, floor_click, read),
        case('door_hinge_doors_both_sides', door(1, 0, 0) + door(-1, 0, 0), hand('minecraft:wooden_door'), SOUTHWARD,
             floor_click, read),
    ]
    for yaw in (0, 10, 30, 44, 46, 80, 100, 135, 170, 180, 190, 225, 260, 280, 315, 350, -10, -100):
        out.append(case(f'coral_fan_yaw_{yaw}', [block(0, 0, 0, *STONE)], hand('minecraft:dead_brain_coral_fan'),
                        [0.5, 0, -1.5, yaw, 35], TOP, [[0, 1, 0]]))
    for wait in (20, 40, 60, 80, 100, 120):
        for repeat in range(2):
            out.append(case(f'coral_block_dies_{wait}_{repeat}', [block(0, 0, 0, *STONE)],
                            hand('minecraft:brain_coral_block'), NORTH_OF, TOP, [[0, 1, 0]],
                            read_later=[wait - 10, [[0, 1, 0]]]))
    stone = [block(0, 0, 0, *STONE)]
    click_sign = [0, 1, 0, 2, 500, 500, 0]
    for colour in ('white', 'orange', 'magenta', 'light_blue', 'yellow', 'lime', 'pink', 'gray', 'light_gray', 'cyan',
                   'purple', 'blue', 'brown', 'green', 'red', 'black'):
        out.append(case(f'sign_dye_{colour}', stone, hand('minecraft:oak_sign'), NORTH_OF, TOP, [[0, 1, 0]],
                        after=[['sign', 'player', [0, 1, 0], True, 'Dye'], ['hand', 'player', f'minecraft:{colour}_dye', 4],
                               ['use', 'player', click_sign]]))
    return out


GROUPS['followup'] = followup


# Plants and other items tried in an empty flower pot (round 3).
POT_ITEMS = ('dandelion', 'poppy', 'blue_orchid', 'allium', 'azure_bluet', 'red_tulip', 'orange_tulip',
             'white_tulip', 'pink_tulip', 'oxeye_daisy', 'cornflower', 'lily_of_the_valley', 'wither_rose',
             'torchflower', 'open_eyeblossom', 'closed_eyeblossom', 'cactus_flower', 'oak_sapling', 'spruce_sapling',
             'birch_sapling', 'jungle_sapling', 'acacia_sapling', 'dark_oak_sapling', 'cherry_sapling',
             'pale_oak_sapling', 'mangrove_propagule', 'azalea', 'flowering_azalea', 'brown_mushroom', 'red_mushroom',
             'crimson_fungus', 'warped_fungus', 'crimson_roots', 'warped_roots', 'fern', 'short_grass', 'deadbush',
             'cactus', 'bamboo', 'bush', 'firefly_bush', 'short_dry_grass', 'tall_dry_grass', 'large_fern',
             'tall_grass', 'sunflower', 'lilac', 'rose_bush', 'peony', 'pitcher_plant', 'pink_petals', 'wildflowers',
             'leaf_litter', 'sweet_berries', 'glow_berries', 'sugar_cane', 'kelp', 'seagrass', 'sea_pickle',
             'nether_sprouts', 'nether_wart', 'hanging_roots', 'spore_blossom', 'moss_carpet', 'pale_moss_carpet',
             'pale_hanging_moss', 'big_dripleaf', 'small_dripleaf_block', 'twisting_vines', 'weeping_vines', 'vine',
             'glow_lichen', 'waterlily', 'chorus_flower', 'wheat_seeds', 'torchflower_seeds', 'pitcher_pod',
             'oak_leaves', 'dirt', 'stone')


def leftovers() -> list[dict]:
    """Round 3 follow-up: which items go into an empty flower pot (and the
    actor a potted plant gets), a second click on a potted plant, a placed
    cave vine's age over many placements, cake eaten slice by slice, string
    on other floors and beside a tripwire, and last a player eating cake on
    normal difficulty, full and hungry (these cases keep the player's
    attribute packets)."""
    stone = [block(0, 0, 0, *STONE)]
    pot = stone + [block(0, 1, 0, 'minecraft:flower_pot', {'update_bit': False})]
    pot_click = [0, 1, 0, 1, 500, 500, 500]
    out = [case(f'pot_{item}', pot, hand(f'minecraft:{item}'), NORTH_OF, pot_click, [[0, 1, 0], [0, 2, 0]])
           for item in POT_ITEMS]
    for second in ('poppy', 'stone', 'dandelion'):
        out.append(case(f'pot_dandelion_then_{second}', pot, hand('minecraft:dandelion'), NORTH_OF, pot_click,
                        [[0, 1, 0], [0, 2, 0]],
                        after=[['hand', 'player', f'minecraft:{second}', 16], ['use', 'player', pot_click]]))
    for repeat in range(100):
        out.append(case(f'cave_vines_age_{repeat}', [block(0, 2, 0, *STONE)], hand('minecraft:glow_berries'),
                        UNDER_PLAYER, UNDER, [[0, 1, 0]]))
    vine = [block(0, 3, 0, *STONE), block(0, 2, 0, 'minecraft:cave_vines', {'growing_plant_age': 10})]
    for repeat in range(10):
        out.append(case(f'cave_vines_under_vine_{repeat}', vine, hand('minecraft:glow_berries'), UNDER_PLAYER,
                        [0, 2, 0, 0, 500, 0, 500], [[0, 1, 0], [0, 2, 0]]))
    on_top = [0, 1, 0, 1, 500, 500, 500]
    at = [[0, 1, 0], [0, 2, 0]]
    cake = lambda bites=0: block(0, 1, 0, 'minecraft:cake', {'bite_counter': bites})  # noqa: E731
    for bites in range(1, 7):
        out.append(case(f'cake_bite_{bites}_empty_hand', stone + [cake(bites)], AIR_HAND, NORTH_OF, on_top, at))
    out += [
        case('cake_eaten_twice', stone + [cake()], AIR_HAND, NORTH_OF, on_top, at,
             after=[['use', 'player', on_top]]),
        case('stone_onto_cake', stone + [cake()], hand('minecraft:stone'), NORTH_OF, on_top, at),
        case('lit_candle_cake_with_candle', stone + [block(0, 1, 0, 'minecraft:candle_cake', {'lit': True})],
             hand('minecraft:candle'), NORTH_OF, on_top, at),
    ]
    string = hand('minecraft:string')
    out += [
        case('string_on_glass', [block(0, 0, 0, 'minecraft:glass')], string, NORTH_OF, TOP, [[0, 1, 0]]),
        case('string_on_bottom_slab', [block(0, 0, 0, 'minecraft:oak_slab', {'minecraft:vertical_half': 'bottom'})],
             string, NORTH_OF, [0, 0, 0, 1, 500, 500, 500], [[0, 1, 0]]),
        case('string_on_grass', [], string, NORTH_OF, [0, -1, 0, 1, 500, 1000, 500], [[0, 0, 0]]),
        case('string_on_east_face', PILLAR, string, SIDE_FACES['east'][1], [0, 1, 0, 5, 1000, 500, 500],
             [[1, 1, 0], [1, 0, 0]]),
        case('string_next_to_tripwire', stone + [block(1, 0, 0, *STONE), block(1, 1, 0, 'minecraft:trip_wire')],
             string, NORTH_OF, TOP, [[0, 1, 0], [1, 1, 0]]),
        case('string_between_tripwires', stone + [block(1, 0, 0, *STONE), block(-1, 0, 0, *STONE),
                                                  block(1, 1, 0, 'minecraft:trip_wire'),
                                                  block(-1, 1, 0, 'minecraft:trip_wire')],
             string, NORTH_OF, TOP, [[-1, 1, 0], [0, 1, 0], [1, 1, 0]]),
    ]
    # Last: the world stays on normal difficulty afterwards.
    normal = [['cmd', 'difficulty normal']]
    hungry = normal + [['cmd', 'effect {player} hunger 3 255 true'], ['wait', 100]]
    out += [
        case('cake_full_normal', stone + [cake()], AIR_HAND, NORTH_OF, on_top, at, before=normal),
        case('cake_hungry_normal', stone + [cake()], AIR_HAND, NORTH_OF, on_top, at, before=hungry,
             after=[['use', 'player', on_top]]),
    ]
    return out


GROUPS['leftovers'] = leftovers


def pistons() -> list[dict]:
    """Round 4: what a client is sent while a piston moves. The player
    switches a lever beside a piston (east of the lever, facing east) on and
    off again 30 ticks later; the watcher records every packet."""
    lever = block(0, 0, 0, 'minecraft:lever', {'lever_direction': 'up_east_west', 'open_bit': False})
    click = [0, 0, 0, 1, 500, 100, 500]
    after = [['wait', 30], ['use', 'player', click], ['wait', 30]]
    read = [[x, 0, 0] for x in range(0, 6)]
    east = {'facing_direction': 4}
    chest = block(3, 0, 0, 'minecraft:chest', {'minecraft:cardinal_direction': 'north'})
    raised_lever = block(0, 1, 0, 'minecraft:lever', {'lever_direction': 'up_east_west', 'open_bit': False})
    raised_click = [0, 1, 0, 1, 500, 100, 500]
    return [
        case('piston_stone', [lever, block(1, 0, 0, 'minecraft:piston', east), block(2, 0, 0, *STONE)],
             AIR_HAND, NORTH_OF, click, read, after=after),
        case('sticky_piston_stone_chest', [lever, block(1, 0, 0, 'minecraft:sticky_piston', east),
                                           block(2, 0, 0, *STONE), chest],
             AIR_HAND, NORTH_OF, click, read, after=after),
        # Raised one block so the slime does not stick to the floor.
        case('sticky_piston_slime', [block(0, 0, 0, *STONE), raised_lever,
                                     block(1, 1, 0, 'minecraft:sticky_piston', east), block(2, 1, 0, 'minecraft:slime'),
                                     block(2, 2, 0, *STONE)],
             AIR_HAND, NORTH_OF, raised_click, [[x, 1, 0] for x in range(0, 5)] + [[2, 2, 0], [3, 2, 0]],
             after=[['wait', 30], ['use', 'player', raised_click], ['wait', 30]]),
        case('piston_breaks_torch', [lever, block(1, 0, 0, 'minecraft:piston', east), block(2, 0, 0, *STONE),
                                     block(3, 0, 0, 'minecraft:torch', {'torch_facing_direction': 'top'})],
             AIR_HAND, NORTH_OF, click, read, after=after),
        case('piston_blocked', [lever, block(1, 0, 0, 'minecraft:piston', east), block(2, 0, 0, 'minecraft:obsidian')],
             AIR_HAND, NORTH_OF, click, read, after=after),
    ]


GROUPS['pistons'] = pistons


def tnt() -> list[dict]:
    """Round 4: what a client is sent when TNT is primed and explodes. The
    player switches a lever beside a TNT block (three stones and a dirt
    block around it to break) and the watcher records every packet for 100
    ticks."""
    lever = block(0, 0, 0, 'minecraft:lever', {'lever_direction': 'up_east_west', 'open_bit': False})
    click = [0, 0, 0, 1, 500, 100, 500]
    setup = [lever, block(1, 0, 0, 'minecraft:tnt', {'explode_bit': False}), block(2, 0, 0, *STONE),
             block(1, 0, 1, 'minecraft:dirt'), block(1, 1, 0, 'minecraft:dirt')]
    return [case('tnt_lever', setup, AIR_HAND, [0.5, 0, -6.5, 0, 10], click, [[x, 0, 0] for x in range(0, 4)],
                 after=[['wait', 100]])]


GROUPS['tnt'] = tnt
