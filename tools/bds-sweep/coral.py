"""Coral death families for the `scenarios` sweep job.

Live coral out of water turns dead some ticks after something wakes it.
These cells find what wakes it: being set with a block update, `/setblock`,
a neighbour changing, and the water beside a coral block going. Water a
cell brings (Endstone fills a set coral plant or fan with water, which then
flows) must not reach another cell, so cells stand 16 blocks apart with a
15 x 15 box refilled after each wave (`LAYOUT`); round 2's `coral_death`
family ran on the 6-block grid, where it did.

Every cell reads its coral every 10 ticks for 300 ticks (`READS`).
"""

from __future__ import annotations

import json

LAYOUT = {'origin': [12, -60, -44], 'spacing': [16, 16], 'columns': 4, 'wave_size': 16, 'center': [0, 0],
          'clear_box': [[-7, -1, -7], [7, 2, 7]]}
# Cells reach 60 blocks from the player: keep them ticking.
PROPERTIES = {'view_distance': 10, 'tick_distance': 8}
KINDS = ('brain', 'bubble', 'fire', 'horn', 'tube')
HERE = [0, 0, 0]
EAST = [1, 0, 0]
WEST = [-1, 0, 0]
STONE = 'minecraft:stone'


def key(family: str, *parts) -> str:
    return family + '|' + '|'.join(json.dumps(p, sort_keys=True, separators=(',', ':')) for p in parts)


def set_(offset, block, states=None, physics=True) -> list:
    return ['set', offset, block, states or {}, physics]


def cmd(text: str, offset=HERE) -> list:
    return ['cmd', text, offset]


def reads() -> list:
    """A read of the cell every 10 ticks for 300 ticks."""
    out = []
    for tick in range(10, 310, 10):
        out += [['wait', 10], ['read', f't{tick}', [HERE]]]
    return out


# A water source west of the cell, walled in so it cannot flow.
WALLED_WATER = [set_([-1, 0, -1], STONE, physics=False), set_([-1, 0, 1], STONE, physics=False),
                set_([-2, 0, 0], STONE, physics=False), set_(WEST, 'minecraft:water', {'liquid_depth': 0}, False)]


def coral_water() -> list[dict]:
    """Per coral kind: a coral block set with a block update, set by
    `/setblock`, set and then a stone placed beside it, set beside walled-in
    water that is then removed (or kept while a stone is placed); a coral
    plant and a floor fan set by `/setblock`, alone and with a stone placed
    beside them 40 ticks later."""
    fam = 'coral_water'
    items = []
    for kind in KINDS:
        block = f'minecraft:{kind}_coral_block'
        plant = f'minecraft:{kind}_coral'
        fan = f'minecraft:{kind}_coral_fan'
        wake = [['wait', 40], ['read', 'before', [HERE]], set_(EAST, STONE)]
        cells = {
            'block_set': ([set_(HERE, block)], 2),
            'block_setblock': ([cmd(f'setblock {{x}} {{y}} {{z}} {block}')], 2),
            'block_then_neighbour': ([set_(HERE, block)] + wake, 2),
            'block_water_goes': (WALLED_WATER + [set_(HERE, block), ['wait', 40], ['read', 'before', [HERE]],
                                                 set_(WEST, 'minecraft:air')], 2),
            'block_water_stays': (WALLED_WATER + [set_(HERE, block)] + wake, 1),
            'plant_setblock': ([cmd(f'setblock {{x}} {{y}} {{z}} {plant}')], 1),
            'plant_setblock_then_neighbour': ([cmd(f'setblock {{x}} {{y}} {{z}} {plant}')] + wake, 1),
            'fan_setblock': ([cmd(f'setblock {{x}} {{y}} {{z}} {fan} ["coral_fan_direction"=0]')], 1),
            'fan_setblock_then_neighbour': ([cmd(f'setblock {{x}} {{y}} {{z}} {fan} ["coral_fan_direction"=0]')]
                                            + wake, 1),
        }
        for name, (steps, repeats) in cells.items():
            for repeat in range(repeats):
                items.append({'key': key(fam, kind, name, repeat), 'family': fam, 'steps': steps + reads()})
    return items


def timeline(ticks: int, events: list, watch: list) -> list:
    return ['timeline', ticks, 'trace', events, watch]


def coral_timing() -> list[dict]:
    """The tick a live coral dies, read every tick: a coral block set with a
    block update or by `/setblock` in air, a coral block whose walled-in water
    is removed, a coral plant (`/setblock`) with a stone placed beside it, and
    a coral block placed by the scripted player's click (each click in its
    own wave: one player cannot click every cell of a wave)."""
    fam = 'coral_timing'
    items = []
    for kind in KINDS:
        block = f'minecraft:{kind}_coral_block'
        for repeat in range(2):
            items.append({'key': key(fam, kind, 'block_set', repeat), 'family': fam,
                          'steps': [timeline(70, [[0, set_(HERE, block)]], [HERE])]})
        for repeat in range(3):
            items.append({'key': key(fam, kind, 'block_water_goes', repeat), 'family': fam,
                          'steps': WALLED_WATER + [set_(HERE, block), ['wait', 20],
                                                   timeline(220, [[0, set_(WEST, 'minecraft:air')]], [HERE])]})
    for repeat in range(5):
        items.append({'key': key(fam, 'tube', 'block_setblock', repeat), 'family': fam,
                      'steps': [timeline(70, [[0, cmd('setblock {x} {y} {z} minecraft:tube_coral_block')]], [HERE])]})
        items.append({'key': key(fam, 'tube', 'plant_neighbour', repeat), 'family': fam,
                      'steps': [cmd('setblock {x} {y} {z} minecraft:tube_coral'), ['wait', 20],
                                timeline(40, [[0, set_(EAST, STONE)]], [HERE])]})
    for repeat in range(8):
        items.append({'key': key(fam, 'brain', 'block_player', repeat), 'family': fam,
                      'steps': [cmd('replaceitem entity {player} slot.hotbar 0 minecraft:brain_coral_block 16'),
                                cmd('tp {player} {x} {y} {z} 180 45', [0, 0, 2]),
                                cmd(f'say coral click {repeat}'), ['wait', 10],
                                timeline(90, [[0, ['use', [0, -1, 0], 1, [500, 1000, 500]]]], [HERE])]})
    return items


FAMILIES = {'coral_water': coral_water, 'coral_timing': coral_timing}
