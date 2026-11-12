"""Liquid families for the `scenarios` sweep job (lane LIQUIDS).

Water and lava flow, sources, mixing and the waterlogged layer, read every
tick. Two layouts:

* `liquids` (`LAYOUT`): cells 18 blocks apart with a 17 x 17 box refilled
  after each wave, so water spreading 7 blocks from a cell's centre never
  reaches another cell. Most cells read the floor-level plane within 7 blocks
  of the centre (`plane`) every tick.
* `liquid_blocks` (`BLOCKS_LAYOUT`): one block per cell on the 6-block grid,
  walled in with a liquid source beside it, to see what water and lava do to
  every block (break and drop it, stay out, or fill its liquid layer).

Endstone reads only a block's first layer. The liquid layer (water inside a
waterlogged block) reaches the scripted player as `UpdateBlock` packets on
layer 1, so these layouts turn the client capture on and the timeline sends a
`bds-sweep:tick <wave> <tick>` chat marker before each tick's events: the
packets between two markers are what the player was sent for that tick.
"""

from __future__ import annotations

import json

LAYOUT = {'origin': [12, -60, -44], 'spacing': [18, 18], 'columns': 4, 'wave_size': 16, 'center': [0, 0],
          'clear_box': [[-8, -3, -8], [8, 5, 8]], 'tick_marks': True}
# Cells reach 66 blocks from the player: keep them ticking and sent.
PROPERTIES = {'view_distance': 10, 'tick_distance': 8}
BLOCKS_LAYOUT = {'origin': [8, -60, -24], 'spacing': 6, 'columns': 8, 'center': [0, 0],
                 'clear_box': [[-2, -1, -2], [3, 2, 2]], 'tick_marks': True}
BLOCKS_PROPERTIES = {'view_distance': 8, 'tick_distance': 6}
# The nether: cells high in the netherrack (cleared and floored by each
# item), kept loaded by a ticking area.
NETHER_LAYOUT = {'origin': [12, 100, -44], 'spacing': [18, 18], 'columns': 4, 'wave_size': 16, 'center': [0, 0],
                 'clear_box': [[-8, -1, -8], [8, 5, 8]], 'dimension': 'Nether'}

HERE = [0, 0, 0]
STONE = 'minecraft:stone'
AIR = 'minecraft:air'
WATER = 'minecraft:water'
LAVA = 'minecraft:lava'


def key(family: str, *parts) -> str:
    return family + '|' + '|'.join(json.dumps(p, sort_keys=True, separators=(',', ':')) for p in parts)


def at(x: int, y: int = 0, z: int = 0) -> list[int]:
    return [x, y, z]


def set_(offset, block, states=None, physics=True) -> list:
    return ['set', offset, block, states or {}, physics]


def source(block: str = WATER, depth: int = 0) -> tuple[str, dict]:
    return block, {'liquid_depth': depth}


def plane(radius: int = 7, y: int = 0) -> list[list[int]]:
    """Every position within `radius` (x and z) of the cell centre at `y`."""
    return [at(x, y, z) for z in range(-radius, radius + 1) for x in range(-radius, radius + 1)]


def timeline(ticks: int, events: list, watch: list, label: str = 'trace') -> list:
    return ['timeline', ticks, label, events, watch]


def item(family: str, name, steps: list) -> dict:
    return {'key': key(family, *name) if isinstance(name, tuple) else key(family, name),
            'family': family, 'steps': steps}


def build(blocks: list, physics: bool = False) -> list:
    """`[(offset, block, states)]` set in order (without block updates by
    default: the scene does not react while it is laid out)."""
    return ['build', [[offset, block, states, physics] for offset, block, states in blocks]]


def up(offset) -> list[int]:
    return [offset[0], offset[1] + 1, offset[2]]


def start(offset, block: str = WATER, depth: int = 0) -> list:
    """Set a liquid at `offset`, then a stone above it with a block update:
    BDS leaves a liquid set by `/setblock` or Endstone (with or without a
    block update) still until a neighbour changes."""
    return ['build', [[offset, block, {'liquid_depth': depth}, False], [up(offset), STONE, {}, True]]]


def click(offset, face: int, item: str, stand, look=(180, 45)) -> tuple[list, list]:
    """Timeline prefix: the scripted player holds `item` in hotbar slot 0,
    stands at `stand` (offset) and the click is the event at tick 0."""
    return [['cmd', f'replaceitem entity {{player}} slot.hotbar 0 {item} 1', HERE],
            ['cmd', f'tp {{player}} {{cx}} {{y}} {{cz}} {look[0]} {look[1]}', stand],
            ['wait', 10]], [0, ['use', offset, face, [500, 1000, 500]]]


def liquids_pilot() -> list[dict]:
    """First questions: how far and how fast a water or lava source spreads
    once woken (a stone set on it with a block update), a source placed by
    the player's bucket, a source in the air, a spread receding after its
    source goes, the infinite-source rule, which way water turns toward a
    drop, and what water and lava make when they meet."""
    fam = 'liquids_pilot'
    items = []
    # A source woken at tick 0, and set at tick 0 but woken at tick 3.
    for repeat in range(2):
        items.append(item(fam, ('water_start', repeat), [timeline(60, [[0, start(HERE)]], plane())]))
    items.append(item(fam, 'water_set_wake_later', [timeline(60, [[0, set_(HERE, *source())],
                                                                  [3, set_(up(HERE), STONE)]], plane())]))
    # A bucket emptied on the floor by the scripted player (each click in
    # its own wave).
    for repeat in range(2):
        prefix, event = click(at(0, -1), 1, 'minecraft:water_bucket', at(0, 0, 3))
        items.append(item(fam, ('water_bucket', repeat), prefix + [timeline(60, [event], plane())]))
        prefix, event = click(at(0, -1), 1, 'minecraft:lava_bucket', at(0, 0, 3))
        items.append(item(fam, ('lava_bucket', repeat), prefix + [timeline(200, [event], plane(4))]))
    # A source in the air, 3 above the floor: the fall and the spread below.
    column = [at(0, y) for y in range(1, 4)]
    items.append(item(fam, 'water_fall', [timeline(60, [[0, start(at(0, 3))]],
                                                   plane() + column + plane(2, 3))]))
    # The spread receding after its source is removed.
    items.append(item(fam, 'water_remove', [start(HERE), ['wait', 60],
                                            timeline(60, [[0, set_(HERE, AIR)]], plane())]))
    # Infinite sources: two sources 2 apart (the middle between them), with
    # the floor under the middle and with air under it.
    two = [[0, start(at(-1))], [0, start(at(1))]]
    items.append(item(fam, 'infinite_floor', [timeline(60, two, plane(3) + [at(0, -1)])]))
    items.append(item(fam, 'infinite_over_air', [build([(at(0, -1), AIR, {}), (at(0, -2), STONE, {})]),
                                                 timeline(60, two, plane(3) + [at(0, -1), at(0, -2)])]))
    # A drop d blocks east of the source: a one-block hole in the floor.
    for d in range(1, 7):
        items.append(item(fam, ('water_drop', d), [build([(at(d, -1), AIR, {})]),
                                                   timeline(60, [[0, start(HERE)]], plane() + [at(d, -1)])]))
    # Lava: the spread is slow; read the plane within 4 for 200 ticks.
    items.append(item(fam, 'lava_start', [timeline(200, [[0, start(HERE, LAVA)]], plane(4))]))
    # Water spreading onto a still lava source (2 east): obsidian?
    items.append(item(fam, 'water_to_lava_source', [build([(at(2), *source(LAVA))]),
                                                    timeline(40, [[0, start(HERE)]], plane(3))]))
    # Lava spreading toward a still water source (3 east): cobblestone?
    items.append(item(fam, 'lava_to_water_source', [build([(at(3), *source())]),
                                                    timeline(120, [[0, start(HERE, LAVA)]], plane(4))]))
    # Lava falling onto a still water source: stone?
    items.append(item(fam, 'lava_onto_water', [build([(HERE, *source())]),
                                               timeline(120, [[0, start(at(0, 2), LAVA)]],
                                                        plane(3) + [at(0, 1), at(0, 2)])]))
    # Water falling onto a still lava source.
    items.append(item(fam, 'water_onto_lava', [build([(HERE, *source(LAVA))]),
                                               timeline(60, [[0, start(at(0, 2))]],
                                                        plane(3) + [at(0, 1), at(0, 2)])]))
    # Basalt: lava spreading over soul soil beside blue ice.
    items.append(item(fam, 'basalt', [build([(at(1, -1), 'minecraft:soul_soil', {}),
                                             (at(1, 0, 1), 'minecraft:blue_ice', {})]),
                                      timeline(120, [[0, start(HERE, LAVA)]], plane(3))]))
    return items


CHUNK_LAYOUT = {'origin': [-40, -60, -40], 'spacing': [32, 32], 'columns': 4, 'wave_size': 16, 'center': [0, 0],
                'clear_box': [[-8, -2, -8], [8, 3, 8]], 'tick_marks': True}


def liquids_chunks() -> list[dict]:
    """Ticks crossing a chunk border: cells sit at chunk centres (32 apart,
    the player still), a source at a chunk corner (local 15, 15 or 0, 0) so
    its first spread enters two neighbouring chunks; four repeats, in
    separate waves (the repeat's wait is part of the wave signature)."""
    fam = 'liquids_chunks'
    items = []
    for repeat in range(4):
        for corner, offset in (('high', at(7, 0, 7)), ('low', at(-8, 0, -8))):
            watch = [[offset[0] + x, 0, offset[2] + z] for z in range(-2, 3) for x in range(-2, 3)]
            for kind in range(8):
                steps = [['wait', 1 + repeat], timeline(16, [[0, start(offset)]], watch)]
                items.append(item(fam, (corner, kind, repeat), steps))
    return items


def liquids_order() -> list[dict]:
    """Ticks due in the same game tick: two sources 2 apart make a source
    between them, and whether the blocks beside the middle get depth 1 or 2
    shows which of them ticked first. Along x and along z, at several
    chunk-local spots, with the two sources woken in either order."""
    fam = 'liquids_order'
    items = []
    for axis in ('x', 'z'):
        a, b = (at(-1), at(1)) if axis == 'x' else (at(0, 0, -1), at(0, 0, 1))
        for shift in (0, 1, 2, 3):
            base = at(shift, 0, shift)
            pa = [a[0] + shift, 0, a[2] + shift]
            pb = [b[0] + shift, 0, b[2] + shift]
            watch = [[base[0] + x, 0, base[2] + z] for z in range(-3, 4) for x in range(-3, 4)]
            for first in ('a', 'b'):
                order = [pa, pb] if first == 'a' else [pb, pa]
                events = [[0, start(order[0])], [0, start(order[1])]]
                items.append(item(fam, (axis, shift, first), [timeline(40, events, watch)]))
    return items


def hole(x: int, z: int = 0, depth: int = 1) -> tuple:
    return at(x, -depth, z), AIR, {}


def liquids_flow() -> list[dict]:
    """Flow direction and spread details: drops beside, diagonal, behind a
    wall and two at once; a two-deep drop; lava's drop search; lava receding
    after its source goes (random: compared by distribution); the middle
    of two sources over a still source; a wall removed beside a still
    source; water reaching a still source; a source in the air."""
    fam = 'liquids_flow'
    items = []
    water = source()
    cases = {
        'drop_diagonal': [hole(2, 1)],
        'drop_two_sides': [hole(3), hole(-3)],
        'drop_near_far': [hole(2), hole(-4)],
        'drop_behind_wall': [hole(3), (at(1), STONE, {}), (at(1, 0, -1), STONE, {})],
        'drop_two_deep': [hole(2), hole(2, 0, 2)],
        'drop_corner': [hole(2, 2)],
        'wall_east': [(at(1), STONE, {})],
        'pocket': [(at(1), STONE, {}), (at(-1), STONE, {}), (at(0, 0, -1), STONE, {})],
    }
    for name, blocks in cases.items():
        watch = plane() + [b[0] for b in blocks if b[0][1] < 0]
        items.append(item(fam, ('water', name), [build(blocks), timeline(60, [[0, start(HERE)]], watch)]))
    for d in range(1, 6):
        items.append(item(fam, ('lava_drop', d), [build([hole(d)]),
                                                 timeline(200, [[0, start(HERE, LAVA)]], plane(4) + [at(d, -1)])]))
    for repeat in range(8):
        items.append(item(fam, ('lava_recede', repeat), [start(HERE, LAVA), ['wait', 120],
                                                         timeline(260, [[0, set_(HERE, AIR)]], plane(4))]))
    items.append(item(fam, 'infinite_over_source', [build([(at(0, -1), *water), (at(0, -2), STONE, {})]),
                                                    timeline(40, [[0, start(at(-1))], [0, start(at(1))]],
                                                             plane(3) + [at(0, -1)])]))
    items.append(item(fam, 'wall_removed', [build([(HERE, *water), (at(1), STONE, {})]),
                                            timeline(40, [[0, set_(at(1), AIR)]], plane(3))]))
    items.append(item(fam, 'reach_still_source', [build([(at(3), *water)]),
                                                  timeline(60, [[0, start(HERE)]], plane(5))]))
    items.append(item(fam, 'air_source_side_wake', [timeline(40, [[0, set_(at(0, 2), *water, physics=False)],
                                                                  [0, set_(at(1, 2), STONE)]],
                                                             plane(2, 2) + [at(0, 1), at(0, 0)])]))
    items.append(item(fam, 'lava_fall', [timeline(200, [[0, start(at(0, 3), LAVA)]],
                                                  plane(4) + [at(0, y) for y in range(1, 4)] + plane(1, 3))]))
    return items


def liquids_logged() -> list[dict]:
    """The liquid layer: a fence set into a source (Endstone with a block
    update, `/setblock`), water spreading against blocks that may hold
    water, bucket clicks on them, emptying a waterlogged fence with a
    bucket, removing a waterlogged fence, and water inside coral, seagrass
    and kelp."""
    fam = 'liquids_logged'
    items = []
    fence = ('minecraft:oak_fence', {})
    items.append(item(fam, 'fence_set_in_source', [build([(HERE, *source())]),
                                                   timeline(40, [[0, set_(HERE, *fence)], [5, set_(at(0, 1), STONE)]],
                                                            plane(3))]))
    items.append(item(fam, 'fence_setblock_in_source', [build([(HERE, *source())]),
                                                        timeline(40, [[0, ['setblock', HERE, fence[0], {}, 'replace']],
                                                                      [5, set_(at(0, 1), STONE)]], plane(3))]))
    holders = (('fence', fence), ('slab', ('minecraft:oak_slab', {'minecraft:vertical_half': 'bottom'})),
               ('stairs', ('minecraft:oak_stairs', {'upside_down_bit': False, 'weirdo_direction': 0})),
               ('chest', ('minecraft:chest', {'minecraft:cardinal_direction': 'north'})),
               ('wall', ('minecraft:cobblestone_wall', {})), ('glass_pane', ('minecraft:glass_pane', {})),
               ('ladder', ('minecraft:ladder', {'facing_direction': 2})),
               ('trapdoor', ('minecraft:trapdoor', {})), ('leaves', ('minecraft:oak_leaves', {})))
    for name, block in holders:
        items.append(item(fam, ('flow_against', name), [build([(at(1), *block)]),
                                                        timeline(40, [[0, start(HERE)]], plane(3))]))
        prefix, event = click(HERE, 1, 'minecraft:water_bucket', at(0, 0, 3))
        items.append(item(fam, ('bucket_on', name), [build([(HERE, *block)])] + prefix
                          + [timeline(40, [event], plane(3))]))
    prefix, event = click(HERE, 1, 'minecraft:water_bucket', at(0, 0, 3))
    items.append(item(fam, 'bucket_fill_back', [build([(HERE, *fence)])] + prefix + [
        timeline(60, [event, [10, ['cmd', 'replaceitem entity {player} slot.hotbar 0 minecraft:bucket 1', HERE]],
                      [20, ['use', HERE, 1, [500, 1000, 500]]]], plane(3))]))
    for mode in ('replace', 'destroy'):
        prefix, event = click(HERE, 1, 'minecraft:water_bucket', at(0, 0, 3))
        items.append(item(fam, ('bucket_then_remove', mode), [build([(HERE, *fence)])] + prefix + [
            timeline(60, [event, [20, ['setblock', HERE, AIR, {}, mode]]], plane(3))]))
    for name, block in (('coral', ('minecraft:tube_coral', {})), ('seagrass', ('minecraft:seagrass', {})),
                        ('kelp', ('minecraft:kelp', {}))):
        items.append(item(fam, ('self_watered', name), [timeline(40, [[0, set_(HERE, *block)],
                                                                      [5, set_(at(0, 1), STONE)]], plane(3))]))
    return items


_TYPED: dict[str, dict] = {}


def typed_states(name: str) -> dict:
    """The first palette state of `name` in command form (bytes as bools)."""
    if not _TYPED:
        import native
        for entry in native.block_states(typed=True):
            _TYPED.setdefault(entry['name'], entry['states'])
    return _TYPED[name]


def liquid_blocks_items(liquid: str, ticks: int) -> list[dict]:
    """Per block (first palette state, set without a block update): a cell
    walled in with stone so the liquid source beside the block (woken at
    tick 0) can only reach the block's position."""
    typed_states('minecraft:air')
    fam = f'liquid_blocks_{liquid.removeprefix("minecraft:")}'
    skip = {'minecraft:air', WATER, LAVA, 'minecraft:flowing_water', 'minecraft:flowing_lava'}
    walls = [(at(-1), STONE, {}), (at(0, 0, -1), STONE, {}), (at(0, 0, 1), STONE, {}), (at(1, 0, -1), STONE, {}),
             (at(1, 0, 1), STONE, {}), (at(2), STONE, {})]
    items = []
    for name in sorted(_TYPED):
        if name in skip:
            continue
        states = _TYPED[name]
        steps = [build(walls + [(at(1), name, states)]), timeline(ticks, [[0, start(HERE, liquid)]], [HERE, at(1)])]
        items.append({'key': key(fam, name), 'family': fam, 'steps': steps})
    return items


def liquid_blocks_water() -> list[dict]:
    return liquid_blocks_items(WATER, 12)


def liquid_blocks_lava() -> list[dict]:
    return liquid_blocks_items(LAVA, 40)


def liquids_nether() -> list[dict]:
    """Lava and water in the nether: spread speed and reach, lava's drop
    search, lava receding. Each cell is cleared and floored with stone
    first."""
    fam = 'liquids_nether'
    prepare = [['cmd', 'execute in nether positioned {x} {y} {z} run fill ~-8 ~ ~-8 ~8 ~4 ~8 air', HERE],
               ['cmd', 'execute in nether positioned {x} {y} {z} run fill ~-8 ~-1 ~-8 ~8 ~-1 ~8 stone', HERE],
               ['wait', 5]]
    items = []
    for repeat in range(2):
        items.append(item(fam, ('lava_start', repeat), prepare + [timeline(100, [[0, start(HERE, LAVA)]], plane())]))
        items.append(item(fam, ('water_start', repeat), prepare + [timeline(60, [[0, start(HERE)]], plane())]))
    for d in range(1, 7):
        items.append(item(fam, ('lava_drop', d), prepare + [build([hole(d)]),
                                                            timeline(100, [[0, start(HERE, LAVA)]],
                                                                     plane() + [at(d, -1)])]))
    for repeat in range(4):
        items.append(item(fam, ('lava_recede', repeat), prepare + [start(HERE, LAVA), ['wait', 80],
                                                                   timeline(160, [[0, set_(HERE, AIR)]], plane())]))
    return items


NETHER_LAYOUT['setup'] = ['execute in nether run tickingarea add 0 100 -56 80 100 24 liquids']
# A natural world: what BDS does by itself with freshly generated liquids.
WORLD_LAYOUT = {'origin': [0, 80, 0], 'spacing': 6, 'columns': 1, 'center': [0, 0], 'tick_marks': True}
WORLD_PROPERTIES = {'view_distance': 8, 'tick_distance': 4, 'level_type': 'DEFAULT', 'level_seed': 12345}


def liquids_world() -> list[dict]:
    """No action at all: 1200 ticks of a fresh default world (seed 12345)
    with the player standing at its spawn; the capture shows every liquid
    change BDS makes on its own."""
    return [item('liquids_world', 'idle', [['wait', 100], timeline(1200, [], [HERE])])]


FAMILIES = {'liquids_pilot': liquids_pilot, 'liquids_chunks': liquids_chunks, 'liquids_order': liquids_order,
            'liquids_flow': liquids_flow, 'liquids_logged': liquids_logged,
            'liquid_blocks_water': liquid_blocks_water, 'liquid_blocks_lava': liquid_blocks_lava,
            'liquids_nether': liquids_nether, 'liquids_world': liquids_world}
LAYOUTS = {'liquids_pilot': 'liquids', 'liquids_chunks': 'liquids_chunks', 'liquids_order': 'liquids',
           'liquids_flow': 'liquids', 'liquids_logged': 'liquids', 'liquid_blocks_water': 'liquid_blocks',
           'liquid_blocks_lava': 'liquid_blocks', 'liquids_nether': 'liquids_nether',
           'liquids_world': 'liquids_world'}


def liquids_order2() -> list[dict]:
    """Ticks due in the same game tick, inside one chunk (cells at chunk
    centres): two sources 2 apart along x or z woken in either order, at the
    same tick or one tick apart, at two chunk-local spots; a lone source; a
    spread receding after its source goes. Twice, in separate waves."""
    fam = 'liquids_order2'
    items = []
    for repeat in range(2):
        for spot in (at(0), at(3, 0, 3)):
            for axis in ('x', 'z'):
                a = at(spot[0] - 1, 0, spot[2]) if axis == 'x' else at(spot[0], 0, spot[2] - 1)
                b = at(spot[0] + 1, 0, spot[2]) if axis == 'x' else at(spot[0], 0, spot[2] + 1)
                watch = [[spot[0] + x, 0, spot[2] + z] for z in range(-4, 5) for x in range(-4, 5)]
                for name, events in (('ab', [[0, start(a)], [0, start(b)]]), ('ba', [[0, start(b)], [0, start(a)]])):
                    items.append(item(fam, (axis, spot, name, repeat), [['wait', 1 + repeat],
                                                                        timeline(40, events, watch)]))
        for axis in ('x', 'z'):
            a = at(-1) if axis == 'x' else at(0, 0, -1)
            b = at(1) if axis == 'x' else at(0, 0, 1)
            watch = [[x, 0, z] for z in range(-4, 5) for x in range(-4, 5)]
            items.append(item(fam, (axis, 'a_then_b', repeat), [['wait', 1 + repeat],
                                                                timeline(40, [[0, start(a)], [1, start(b)]], watch)]))
        items.append(item(fam, ('remove', repeat), [['wait', 1 + repeat], start(HERE), ['wait', 50],
                                                    timeline(60, [[0, set_(HERE, AIR)]], plane())]))
        items.append(item(fam, ('remove_lava', repeat), [['wait', 1 + repeat], start(HERE, LAVA), ['wait', 120],
                                                         timeline(120, [[0, set_(HERE, AIR)]], plane(4))]))
    return items


FAMILIES['liquids_order2'] = liquids_order2
LAYOUTS['liquids_order2'] = 'liquids_chunks'


def liquid_holders() -> list[dict]:
    """Which blocks keep the water they are set into, and through which
    sides it flows out: per block (first palette state), a still water
    source, the block set into it with a block update at tick 0, a stone set
    on it at tick 5 (waking the water), and the four sides read every tick."""
    fam = 'liquid_holders'
    typed_states('minecraft:air')
    skip = {'minecraft:air', WATER, LAVA, 'minecraft:flowing_water', 'minecraft:flowing_lava'}
    sides = [at(-1), at(1), at(0, 0, -1), at(0, 0, 1)]
    items = []
    for name in sorted(_TYPED):
        if name in skip:
            continue
        events = [[0, set_(HERE, name, _TYPED[name])], [5, set_(at(0, 1), STONE)]]
        items.append({'key': key(fam, name), 'family': fam,
                      'steps': [build([(HERE, *source())]), timeline(14, events, [HERE] + sides)]})
    return items


FAMILIES['liquid_holders'] = liquid_holders
LAYOUTS['liquid_holders'] = 'liquid_blocks'
