"""Redstone circuit families for the `scenarios` sweep job.

A circuit is built in a cell of its own (blocks set with the block update a
placement triggers, `physics`), left to settle, and then run for a
`timeline`: inputs change at known ticks (a redstone block set or removed, a
lever switched, a command) and every position of the circuit is read once
per server tick, so the trace says at which tick each block changed. The
replay test (`crates/unastar/src/server/game/redstone/tests.rs`) rebuilds
each circuit and compares its own trace tick by tick.

Cells are 22 x 6 blocks (`LAYOUT`): circuits run east along +x from the
cell origin on the superflat grass (offset y=-1 is the floor), at most 20
blocks long and 3 wide (z -1..1), with two empty rows between cells.
"""

from __future__ import annotations

import json

AIR = 'minecraft:air'
STONE = 'minecraft:stone'
DUST = 'minecraft:redstone_wire'
RB = 'minecraft:redstone_block'
LAMP = 'minecraft:redstone_lamp'

# Cells east of the player, inside the ticking range the layout's server
# properties give (tick distance 8).
# The witness (a redstone block and a dust west of the cells) gives every
# wave's redstone phase.
LAYOUT = {'origin': [6, -60, -45], 'spacing': [22, 6], 'columns': 4, 'wave_size': 64, 'center': [9, 0],
          'clear_box': [[-1, -1, -2], [20, 3, 2]], 'witness': [1, -60, -8]}
PROPERTIES = {'view_distance': 10, 'tick_distance': 8}


def key(family: str, *parts) -> str:
    return family + '|' + '|'.join(json.dumps(p, sort_keys=True, separators=(',', ':')) for p in parts)


def at(x: int, y: int = 0, z: int = 0) -> list[int]:
    return [x, y, z]


def put(offset, block: str, states: dict | None = None, physics: bool = True) -> list:
    return ['set', offset, block, states or {}, physics]


def wait(ticks: int) -> list:
    return ['wait', ticks]


def timeline(ticks: int, events: list, watch: list, label: str = 'trace') -> list:
    return ['timeline', ticks, label, events, watch]


def dust(signal: int = 0) -> tuple[str, dict]:
    return DUST, {'redstone_signal': signal}


def repeater(facing: str, delay: int = 0, powered: bool = False) -> tuple[str, dict]:
    return ('minecraft:powered_repeater' if powered else 'minecraft:unpowered_repeater',
            {'minecraft:cardinal_direction': facing, 'repeater_delay': delay})


def comparator(facing: str, subtract: bool = False) -> tuple[str, dict]:
    return 'minecraft:unpowered_comparator', {'minecraft:cardinal_direction': facing, 'output_lit_bit': False,
                                              'output_subtract_bit': subtract}


def torch(facing: str = 'top', lit: bool = True) -> tuple[str, dict]:
    return ('minecraft:redstone_torch' if lit else 'minecraft:unlit_redstone_torch',
            {'torch_facing_direction': facing})


def lever(direction: str = 'up_east_west', on: bool = False) -> tuple[str, dict]:
    return 'minecraft:lever', {'lever_direction': direction, 'open_bit': on}


def circuit(family: str, name, build: list, ticks: int, events: list, watch: list, settle: int = 10) -> dict:
    """`build`: [(offset, block, states)] set in order with block updates;
    then `settle` ticks; then the timeline."""
    steps = [['build', [[offset, block, states, True] for offset, block, states in build]]]
    steps += [wait(settle), timeline(ticks, events, watch)]
    return {'key': key(family, name), 'family': family, 'steps': steps}


def line(x0: int, x1: int, block: str, states: dict | None = None, y: int = 0, z: int = 0) -> list:
    return [(at(x, y, z), block, states or {}) for x in range(x0, x1 + 1)]


def pilot() -> list[dict]:
    """First questions: how fast dust, torches, repeaters, lamps and doors
    answer, whether set_data switches a lever like a click, which way a
    repeater's `cardinal_direction` points, strong and weak power, and
    whether the tick a change happens on matters (each input changes at an
    even and at an odd tick)."""
    fam = 'circuits_pilot'
    items = []
    for start in (4, 5):
        on, off = start, start + 30
        # Redstone block at x=0 feeding 17 dust: signal per distance and
        # the propagation timing both ways.
        watch = [at(x) for x in range(0, 18)]
        items.append(circuit(fam, ['dust_line', start], line(1, 17, *dust()), 70,
                             [[on, put(at(0), RB)], [off, put(at(0), AIR)]], watch))
        # Lever set by set_data, dust and a lamp.
        build = [(at(0), *lever()), *line(1, 3, *dust()), (at(4), LAMP, {})]
        items.append(circuit(fam, ['lever_set_data', start], build, 70,
                             [[on, put(at(0), *lever(on=True))], [off, put(at(0), *lever(on=False))]],
                             [at(x) for x in range(0, 5)]))
        # Lever set on from the start (the circuit is built powered).
        build = [(at(0), *lever(on=True)), *line(1, 3, *dust()), (at(4), LAMP, {})]
        items.append(circuit(fam, ['lever_built_on', start], build, 40,
                             [[on, put(at(0), *lever(on=False))]], [at(x) for x in range(0, 5)]))
        # Inverter: redstone block powers a stone; a torch on its east side
        # feeds dust and a lamp.
        build = [(at(1), STONE, {}), (at(2), *torch('west')), *line(3, 5, *dust()), (at(6), LAMP, {})]
        items.append(circuit(fam, ['inverter', start], build, 70,
                             [[on, put(at(0), RB)], [off, put(at(0), AIR)]], [at(x) for x in range(0, 7)]))
        # Repeaters of each delay and both facings between dust and a lamp.
        for facing in ('east', 'west'):
            for delay in range(4):
                build = [(at(1), *dust()), (at(2), *repeater(facing, delay)), (at(3), *dust()), (at(4), LAMP, {})]
                items.append(circuit(fam, ['repeater', facing, delay, start], build, 70,
                                     [[on, put(at(0), RB)], [off, put(at(0), AIR)]],
                                     [at(x) for x in range(0, 5)]))
        # Lamp directly next to a redstone block.
        items.append(circuit(fam, ['lamp', start], [(at(1), LAMP, {})], 60,
                             [[on, put(at(0), RB)], [off, put(at(0), AIR)]], [at(0), at(1)]))
        # Door (lower half at x=1, upper set too) next to a redstone block.
        door = {'door_hinge_bit': False, 'minecraft:cardinal_direction': 'east', 'open_bit': False}
        build = [(at(1), 'minecraft:wooden_door', {**door, 'upper_block_bit': False}),
                 (at(1, 1), 'minecraft:wooden_door', {**door, 'upper_block_bit': True})]
        items.append(circuit(fam, ['door', start], build, 60,
                             [[on, put(at(0), RB)], [off, put(at(0), AIR)]], [at(0), at(1), at(1, 1)]))
        # Strong power: a repeater into a stone powers dust beyond it; weak
        # power: dust into a stone does not.
        build = [(at(1), *repeater('east')), (at(2), STONE, {}), (at(3), *dust()), (at(4), LAMP, {})]
        items.append(circuit(fam, ['strong', start], build, 60,
                             [[on, put(at(0), RB)], [off, put(at(0), AIR)]], [at(x) for x in range(0, 5)]))
        build = [(at(1), *dust()), (at(2), STONE, {}), (at(3), *dust()), (at(4), LAMP, {}),
                 (at(2, 0, 1), LAMP, {})]
        items.append(circuit(fam, ['weak', start], build, 60,
                             [[on, put(at(0), RB)], [off, put(at(0), AIR)]],
                             [at(x) for x in range(0, 5)] + [at(2, 0, 1)]))
        # Comparator: rear from a redstone block, side from dust at 14.
        for facing, subtract in (('east', False), ('east', True), ('west', False)):
            build = [(at(1), *dust()), (at(2), *comparator(facing, subtract)), (at(3), *dust()),
                     (at(4), *dust()), (at(2, 0, 1), *dust()), (at(2, 0, 2), *dust())]
            items.append(circuit(fam, ['comparator', facing, subtract, start], build, 70,
                                 [[on, put(at(0), RB)], [on + 20, put(at(1, 0, 2), RB)],
                                  [off, put(at(0), AIR)], [off + 10, put(at(1, 0, 2), AIR)]],
                                 [at(x) for x in range(0, 5)] + [at(2, 0, 1), at(2, 0, 2)]))
        # Observer between a block that changes (x=0) and dust (x=2, 3).
        for facing in ('west', 'east'):
            build = [(at(1), 'minecraft:observer', {'minecraft:facing_direction': facing, 'powered_bit': False}),
                     (at(2), *dust()), (at(3), *dust())]
            items.append(circuit(fam, ['observer', facing, start], build, 40,
                                 [[on, put(at(0), STONE)]], [at(x) for x in range(0, 4)]))
        # A torch placed lit next to powered stone burns... or turns off.
        build = [(at(0), RB, {}), (at(1), STONE, {})]
        items.append(circuit(fam, ['torch_on_powered', start], build, 40,
                             [[on, put(at(2), *torch('west'))]], [at(x) for x in range(0, 3)]))
    return items


def rep(delay: int = 0) -> tuple[str, dict]:
    """A repeater taking its input from the west (x-1) and powering east."""
    return repeater('west', delay)


def cmp_(subtract: bool = False) -> tuple[str, dict]:
    """A comparator with its rear to the west, output east."""
    return comparator('west', subtract)


def toggled(name, build: list, watch: list, ticks: int = 70, at_on: int = 4, gap: int = 30,
            source=None, fam: str = 'circuits_pilot2') -> dict:
    """A redstone block set at `at_on` and removed `gap` ticks later at
    `source` (default x=0)."""
    src = source or at(0)
    return circuit(fam, name, build, ticks, [[at_on, put(src, RB)], [at_on + gap, put(src, AIR)]], watch)


def xs(n: int, start: int = 0) -> list:
    return [at(x) for x in range(start, start + n)]


def pilot2() -> list[dict]:
    """Second questions: dust lag behind repeaters, repeater and comparator
    chains, torches, strong power, dust that does not point at a block,
    consumers, and clicks on levers and buttons."""
    items = []
    for t0 in (4, 5):
        items += [
            toggled(['lamp_end', t0], [*line(1, 5, *dust()), (at(6), LAMP, {})], xs(7), at_on=t0),
            toggled(['rep_direct', t0], [(at(1), *rep()), (at(2), *dust()), (at(3), LAMP, {})], xs(4), at_on=t0),
            toggled(['rep_lamp', t0], [(at(1), *dust()), (at(2), *rep()), (at(3), LAMP, {})], xs(4), at_on=t0),
            toggled(['rep_rep', t0], [(at(1), *rep()), (at(2), *rep()), (at(3), *dust()), (at(4), LAMP, {})],
                    xs(5), at_on=t0),
            toggled(['rep_dusts', t0], [(at(1), *dust()), (at(2), *rep()), *line(3, 5, *dust()), (at(6), LAMP, {})],
                    xs(7), at_on=t0),
            toggled(['cmp_direct', t0], [(at(1), *cmp_()), (at(2), *dust()), (at(3), LAMP, {})], xs(4), at_on=t0),
            toggled(['inverter', t0], [(at(1), *rep()), (at(2), STONE, {}), (at(3), *torch('west')),
                                       *line(4, 5, *dust()), (at(6), LAMP, {})], xs(7), at_on=t0),
            toggled(['strong', t0], [(at(1), *rep()), (at(2), STONE, {}), (at(3), *dust()), (at(4), LAMP, {}),
                                     (at(2, 0, 1), LAMP, {})], xs(5) + [at(2, 0, 1)], at_on=t0),
            # A torch on top of a powered stone, a lamp above the torch.
            toggled(['torch_up', t0], [(at(1), *rep()), (at(2), STONE, {}), (at(2, 1), *torch('top')),
                                       (at(2, 2), LAMP, {})], xs(3) + [at(2, 1), at(2, 2)], at_on=t0),
            # Dust running past a lamp (not pointing at it) and dust on a lamp.
            toggled(['dust_beside', t0], [*line(1, 4, *dust()), (at(2, 0, 1), LAMP, {})],
                    xs(5) + [at(2, 0, 1)], at_on=t0),
            toggled(['dust_on_lamp', t0], [(at(2, -1), LAMP, {}), *line(1, 4, *dust())], xs(5) + [at(2, -1)],
                    at_on=t0),
            # Dust turning a corner next to a lamp it points at and one it passes.
            toggled(['dust_corner', t0], [(at(1), *dust()), (at(2), *dust()), (at(2, 0, 1), *dust()),
                                          (at(3), LAMP, {}), (at(2, 0, -1), LAMP, {})],
                    xs(4) + [at(2, 0, 1), at(2, 0, -1)], at_on=t0),
        ]
    # Repeaters of each delay powered directly, on and off.
    for delay in range(4):
        items.append(toggled(['rep_delay', delay], [(at(1), *rep(delay)), (at(2), LAMP, {})], xs(3)))
    # Comparators: the rear at 15 (dust next to the block), the side from a
    # second block 1-3 dust away (side 15..13); compare and subtract.
    for subtract in (False, True):
        for side_len in (1, 2, 3):
            side = [(at(2, 0, z), *dust()) for z in range(1, side_len + 1)]
            build = [(at(1), *dust()), (at(2), *cmp_(subtract)), *line(3, 6, *dust()), *side]
            items.append(circuit('circuits_pilot2', ['cmp_side', subtract, side_len], build, 70,
                                 [[4, put(at(0), RB)], [14, put(at(2, 0, side_len + 1), RB)],
                                  [34, put(at(2, 0, side_len + 1), AIR)], [44, put(at(0), AIR)]],
                                 xs(7) + [at(2, 0, z) for z in range(1, side_len + 1)]))
    # Consumers next to a redstone block.
    door = {'door_hinge_bit': False, 'minecraft:cardinal_direction': 'east', 'open_bit': False}
    for block in ('minecraft:iron_door', 'minecraft:copper_door', 'minecraft:wooden_door'):
        build = [(at(1), block, {**door, 'upper_block_bit': False}),
                 (at(1, 1), block, {**door, 'upper_block_bit': True})]
        items.append(toggled(['consumer', block], build, [at(0), at(1), at(1, 1)], ticks=60))
    trap = {'direction': 0, 'open_bit': False, 'upside_down_bit': False}
    gate = {'minecraft:cardinal_direction': 'east', 'open_bit': False, 'in_wall_bit': False}
    for block, states in (('minecraft:iron_trapdoor', trap), ('minecraft:trapdoor', trap),
                          ('minecraft:copper_trapdoor', trap), ('minecraft:fence_gate', gate),
                          ('minecraft:copper_bulb', {'lit': False, 'powered_bit': False}),
                          ('minecraft:hopper', {'facing_direction': 0, 'toggle_bit': False}),
                          ('minecraft:dispenser', {'facing_direction': 3, 'triggered_bit': False}),
                          ('minecraft:dropper', {'facing_direction': 3, 'triggered_bit': False}),
                          ('minecraft:crafter', {'crafting': False, 'orientation': 'north_up', 'triggered_bit': False}),
                          ('minecraft:noteblock', {}), ('minecraft:tnt', {'explode_bit': False}),
                          ('minecraft:redstone_lamp', {})):
        items.append(toggled(['consumer', block], [(at(1), block, states)], [at(0), at(1)], ticks=60))
    # Powered and activator rails beside a redstone block, 10 long.
    for block in ('minecraft:golden_rail', 'minecraft:activator_rail'):
        build = line(1, 10, block, {'rail_data_bit': False, 'rail_direction': 1})
        items.append(toggled(['rails', block], build, xs(11), ticks=60))
    # Clicks: a lever (three toggles) and a button, feeding dust and a lamp.
    stand = ['cmd', 'tp {player} {x} {y} {z} 180 30', at(1, 0, 3)]
    build = [(at(0), *lever()), *line(1, 2, *dust()), (at(3), LAMP, {})]
    click = ['use', at(0), 1, [500, 100, 500]]
    items.append({'key': key('circuits_pilot2', 'click_lever'), 'family': 'circuits_pilot2',
                  'steps': [['build', [[o, b, st, True] for o, b, st in build]], stand, wait(10),
                            timeline(80, [[4, click], [30, click], [55, click]], xs(4))]})
    for button in ('minecraft:stone_button', 'minecraft:wooden_button'):
        build = [(at(0), button, {'facing_direction': 1, 'button_pressed_bit': False}), *line(1, 2, *dust()),
                 (at(3), LAMP, {})]
        items.append({'key': key('circuits_pilot2', 'click_button', button), 'family': 'circuits_pilot2',
                      'steps': [['build', [[o, b, st, True] for o, b, st in build]], stand, wait(10),
                                timeline(70, [[4, click]], xs(4))]})
    return items


def clicked(fam: str, name, build: list, clicks: list, watch: list, ticks: int, stand=None) -> dict:
    """The scripted player stands south of the cell (default x=1) and clicks
    `clicks` = [[tick, offset, face], ...] (click in the middle of the face's
    lower part)."""
    stand = stand or at(1, 0, 3)
    steps = [['build', [[o, b, st, True] for o, b, st in build]],
             ['cmd', 'tp {player} {x} {y} {z} 180 30', stand], wait(10),
             timeline(ticks, [[t, ['use', off, face, [500, 100, 500]]] for t, off, face in clicks], watch)]
    return {'key': key(fam, name), 'family': fam, 'steps': steps}


BUTTONS = ['minecraft:stone_button', 'minecraft:polished_blackstone_button', 'minecraft:wooden_button',
           'minecraft:spruce_button', 'minecraft:birch_button', 'minecraft:jungle_button', 'minecraft:acacia_button',
           'minecraft:dark_oak_button', 'minecraft:mangrove_button', 'minecraft:cherry_button',
           'minecraft:bamboo_button', 'minecraft:pale_oak_button', 'minecraft:crimson_button',
           'minecraft:warped_button']
PLATES = ['minecraft:stone_pressure_plate', 'minecraft:wooden_pressure_plate',
          'minecraft:light_weighted_pressure_plate', 'minecraft:heavy_weighted_pressure_plate']


def pilot3() -> list[dict]:
    """Third questions: every button's press length, lever parity, pressure
    plates under mobs and items, daylight detectors, observers on dust and
    in chains, a dust dot, dust climbing blocks, repeater locking,
    comparators reading blocks, a torch clock burning out, pistons and the
    copper bulb toggling."""
    fam = 'circuits_pilot3'
    items = []
    lamp_line = [*line(1, 2, *dust()), (at(3), LAMP, {})]
    for button in BUTTONS:
        build = [(at(0), button, {'facing_direction': 1, 'button_pressed_bit': False}), *lamp_line]
        items.append(clicked(fam, ['button', button], build, [[4, at(0), 1]], xs(4), 60))
    for t0 in (4, 5):
        build = [(at(0), *lever()), *lamp_line]
        items.append(clicked(fam, ['lever', t0], build, [[t0, at(0), 1], [t0 + 21, at(0), 1]], xs(4), 50))
    # Pressure plates at x=1 with dust east of them, pressed by a pig, an
    # armour stand, one item or 20 items for 40 ticks.
    for plate in PLATES:
        build = [(at(1), plate, {'redstone_signal': 0}), (at(2), *dust()), (at(3), *dust()), (at(4), LAMP, {})]
        presses = {'pig': ['cmd', 'summon minecraft:pig {cx} {y} {cz}', at(1)],
                   'armor_stand': ['cmd', 'summon minecraft:armor_stand {cx} {y} {cz}', at(1)],
                   'item1': ['drop', at(1), 'minecraft:stone', 1],
                   'item20': ['drop', at(1), 'minecraft:stone', 20]}
        for what, event in presses.items():
            remove = ['cmd', 'kill @e[x={cx},y={y},z={cz},r=1.5,type=!player]', at(1)]
            items.append(circuit(fam, ['plate', plate, what], build, 90, [[4, event], [44, remove]], xs(5)))
    # Daylight detectors (normal and inverted) under a changing time of day.
    for block in ('minecraft:daylight_detector', 'minecraft:daylight_detector_inverted'):
        build = [(at(1), block, {'redstone_signal': 0}), (at(2), *dust()), (at(3), LAMP, {})]
        events = [[4 + 25 * i, ['cmd', f'time set {time}', at(0)]]
                  for i, time in enumerate((0, 1000, 3000, 6000, 9000, 11500, 12500, 13500, 18000, 23000))]
        items.append(circuit(fam, ['daylight', block], build, 260, events, xs(4)))
    # Observers: watching a dust that changes, watching another observer.
    obs = 'minecraft:observer'
    build = [(at(1), *dust()), (at(1, 0, 1), obs, {'minecraft:facing_direction': 'north', 'powered_bit': False}),
             (at(1, 0, 2), *dust()), (at(2, 0, 2), LAMP, {})]
    items.append(toggled(['observer_dust'], build, xs(2) + [at(1, 0, 1), at(1, 0, 2), at(2, 0, 2)], fam=fam))
    build = [(at(1), obs, {'minecraft:facing_direction': 'west', 'powered_bit': False}),
             (at(2), obs, {'minecraft:facing_direction': 'west', 'powered_bit': False}),
             (at(3), *dust()), (at(4), LAMP, {})]
    items.append(circuit(fam, ['observer_chain'], build, 50, [[4, put(at(0), STONE)], [24, put(at(0), AIR)]],
                         xs(5)))
    # A dust dot on a stone strongly powered from the side below it, lamps
    # on three sides of the dot.
    build = [(at(1, -1), *rep()), (at(2, -1), STONE, {}), (at(2), *dust()), (at(3), LAMP, {}),
             (at(2, 0, 1), LAMP, {}), (at(2, 0, -1), LAMP, {})]
    items.append(toggled(['dust_dot'], build, [at(0, -1), at(1, -1), at(2), at(3), at(2, 0, 1), at(2, 0, -1)],
                         source=at(0, -1), fam=fam))
    # Dust climbing a stone (x=2) and coming down the far side; with and
    # without a stone above the lower dust cutting the climb.
    for cut in (False, True):
        build = [(at(1), *dust()), (at(2), STONE, {}), (at(2, 1), *dust()), (at(3), *dust()), (at(4), LAMP, {})]
        if cut:
            build.append((at(1, 1), STONE, {}))
        items.append(toggled(['dust_climb', cut], build, [at(0), at(1), at(2, 1), at(3), at(4)], fam=fam))
    # Repeater locking: the main repeater (x=2) with a side repeater at
    # (2,0,1) facing north into it, fed from (2,0,2).
    for order in ('lock_first', 'main_first'):
        lock_at, main_at = (4, 14) if order == 'lock_first' else (14, 4)
        build = [(at(1), *dust()), (at(2), *rep()), (at(3), *dust()), (at(4), LAMP, {}),
                 (at(2, 0, 1), *repeater('south'))]
        events = [[main_at, put(at(0), RB)], [lock_at, put(at(2, 0, 2), RB)], [34, put(at(0), AIR)],
                  [54, put(at(2, 0, 2), AIR)]]
        items.append(circuit(fam, ['locking', order], build, 80, events,
                             xs(5) + [at(2, 0, 1), at(2, 0, 2)]))
    # Comparators reading blocks behind them (x=0), output dust east.
    read_line = [(at(1), *cmp_()), *line(2, 17, *dust())]
    readers = [('composter', {'composter_fill_level': n}) for n in range(9)]
    readers += [('cake', {'bite_counter': n}) for n in range(7)]
    readers += [('cauldron', {'cauldron_liquid': 'water', 'fill_level': n}) for n in range(7)]
    readers += [('end_portal_frame', {'end_portal_eye_bit': b, 'minecraft:cardinal_direction': 'south'})
                for b in (False, True)]
    readers += [('respawn_anchor', {'respawn_anchor_charge': n}) for n in range(5)]
    readers += [('beehive', {'direction': 0, 'honey_level': n}) for n in range(6)]
    for block, states in readers:
        items.append(circuit(fam, ['read', block, states], [(at(0), f'minecraft:{block}', states), *read_line],
                             20, [], xs(18), settle=10))
    for count in (0, 1, 32, 64, 128, 27 * 64):
        events = [[4, ['cmd', f'replaceitem block {{x}} {{y}} {{z}} slot.container {slot} minecraft:stone '
                       f'{min(64, count - 64 * slot)}', at(0)]] for slot in range((count + 63) // 64)]
        items.append(circuit(fam, ['read_chest', count], [(at(0), 'minecraft:chest', {}), *read_line], 30, events,
                             xs(18)))
    # A torch clock: torch (2,0,0) on stone (1,0,0) feeds a repeater south,
    # dust and a repeater back north into the stone.
    build = [(at(1), STONE, {}), (at(2), *torch('west')), (at(2, 0, 1), *repeater('north')),
             (at(2, 0, 2), *dust()), (at(1, 0, 2), *dust()), (at(1, 0, 1), *repeater('south'))]
    items.append(circuit(fam, ['torch_clock'], build, 300, [], [at(2), at(2, 0, 1), at(2, 0, 2), at(1, 0, 1)],
                         settle=1))
    # Pistons facing east at x=1 with a stone in front, powered from x=0.
    for piston in ('minecraft:piston', 'minecraft:sticky_piston'):
        build = [(at(1), piston, {'facing_direction': 5}), (at(2), STONE, {})]
        items.append(toggled(['piston', piston], build, xs(5), ticks=60, fam=fam))
    # The copper bulb toggles on each rising edge.
    items.append(circuit(fam, ['bulb_toggle'], [(at(1), 'minecraft:copper_bulb', {'lit': False, 'powered_bit': False})],
                         70, [[4, put(at(0), RB)], [20, put(at(0), AIR)], [36, put(at(0), RB)], [52, put(at(0), AIR)]],
                         xs(2)))
    return items


FAMILIES = {'circuits_pilot': pilot, 'circuits_pilot2': pilot2, 'circuits_pilot3': pilot3}


# ------------------------------------------------------------ measured families
#
# The committed families (`redstone_*`). Each circuit names its blocks by
# offset; a redstone block (`RB`) at the source position is set at `t0` and
# removed later unless a family says otherwise. Both tick parities (t0 = 4
# and 5) are measured where timing is the question.

PARITIES = (4, 5)
WOOD = ('oak', 'spruce', 'birch', 'jungle', 'acacia', 'dark_oak', 'mangrove', 'cherry', 'bamboo', 'pale_oak',
        'crimson', 'warped')
COPPER_STAGES = ('', 'exposed_', 'weathered_', 'oxidized_')


def block_ids() -> set[str]:
    import native
    return {e['name'] for e in native.block_states(typed=True)}


def pulse(fam: str, name, build: list, watch: list, t0: int, length: int = 24, ticks: int = 50,
          source=None) -> dict:
    """A redstone block at `source` (default x=0) from `t0` for `length`
    ticks."""
    src = source or at(0)
    return circuit(fam, name, build, ticks, [[t0, put(src, RB)], [t0 + length, put(src, AIR)]], watch)


def redstone_dust() -> list[dict]:
    """Dust: signal over distance, which neighbours a dust powers for each
    shape (line end, corner, T, cross, dot), climbing and coming down
    blocks (and the blocks that cut it), dust on and beside lamps."""
    fam = 'redstone_dust'
    items = []
    for t0 in PARITIES:
        for n in range(1, 16):
            items.append(pulse(fam, ['line', n, t0], [*line(1, n, *dust()), (at(n + 1), LAMP, {})], xs(n + 2), t0))
        # The centre dust (3,0,0) is fed from the west; other dust on its
        # north, south and east sides as listed, lamps on the rest.
        sides = {'n': at(3, 0, -1), 's': at(3, 0, 1), 'e': at(4)}
        for config in ('', 'n', 's', 'e', 'ns', 'ne', 'se', 'nse'):
            build = [(at(1), *dust()), (at(2), *dust()), (at(3), *dust())]
            for side, off in sides.items():
                build.append((off, *(dust() if side in config else (LAMP, {}))))
            build.append((at(3, -1), LAMP, {}))
            watch = xs(4) + list(sides.values()) + [at(3, -1)]
            items.append(pulse(fam, ['shape', config, t0], build, watch, t0))
    # A dust dot on a stone strongly powered from beside it below.
    build = [(at(1, -1), *rep()), (at(2, -1), STONE, {}), (at(2), *dust()), (at(3), LAMP, {}),
             (at(2, 0, 1), LAMP, {}), (at(2, 0, -1), LAMP, {}), (at(1), LAMP, {})]
    for t0 in PARITIES:
        items.append(pulse(fam, ['dot', t0], build, [at(0, -1), at(1, -1), at(2), at(3), at(2, 0, 1), at(2, 0, -1),
                                                    at(1)], t0, source=at(0, -1)))
    # Climbing onto a block x=2 and down its far side, cut by a block above
    # the lower dust on either side.
    climbs = [(STONE, {}), ('minecraft:glass', {}), ('minecraft:oak_slab', {'minecraft:vertical_half': 'bottom'}),
              ('minecraft:oak_slab', {'minecraft:vertical_half': 'top'}), ('minecraft:redstone_lamp', {}),
              ('minecraft:glowstone', {}), ('minecraft:oak_leaves', {'persistent_bit': True, 'update_bit': False}),
              ('minecraft:hopper', {'facing_direction': 0, 'toggle_bit': False})]
    for block, states in climbs:
        for cut in ('', 'west', 'east'):
            build = [(at(1), *dust()), (at(2), block, states), (at(2, 1), *dust()), (at(3), *dust()),
                     (at(4), LAMP, {})]
            if cut == 'west':
                build.append((at(1, 1), STONE, {}))
            elif cut == 'east':
                build.append((at(3, 1), STONE, {}))
            items.append(pulse(fam, ['climb', block, states, cut], build,
                               [at(0), at(1), at(2), at(2, 1), at(3), at(4)], 4))
    # A dust line over lamps: the dust powers the block under it.
    for t0 in PARITIES:
        build = [(at(2, -1), LAMP, {}), (at(4, -1), LAMP, {}), *line(1, 4, *dust())]
        items.append(pulse(fam, ['over_lamps', t0], build, xs(5) + [at(2, -1), at(4, -1)], t0))
    return items


def redstone_power() -> list[dict]:
    """Strong and weak power: each source powering a stone S at x=2;
    probes are a lamp east of S, a dust south of S and a lamp north of it
    (a dust next to S only takes strong power)."""
    fam = 'redstone_power'
    probes = [(at(3), LAMP, {}), (at(2, 0, 1), *dust()), (at(2, 0, -1), LAMP, {})]
    watch = xs(4) + [at(2, 0, 1), at(2, 0, -1)]
    sources = {
        'repeater': [(at(1), *rep())],
        'comparator': [(at(1), *cmp_())],
        'dust': [(at(1), *dust())],
        'redstone_block': [],
        'observer': [(at(1), 'minecraft:observer', {'minecraft:facing_direction': 'west', 'powered_bit': False})],
        'torch_beside': [(at(1), *torch('east'))],
    }
    items = []
    for t0 in PARITIES:
        for name, build in sources.items():
            if name == 'redstone_block':
                items.append(pulse(fam, ['source', name, t0], [(at(2), STONE, {}), *probes], watch, t0,
                                   source=at(1)))
            elif name == 'observer':
                events = [[t0, put(at(0), STONE)], [t0 + 24, put(at(0), AIR)]]
                items.append(circuit(fam, ['source', name, t0], [*build, (at(2), STONE, {}), *probes], 50, events,
                                     watch))
            elif name == 'torch_beside':
                # A torch standing west of S on a stone, switched off by a
                # redstone block under... placed and removed.
                events = [[t0, put(at(1), *torch('top'))], [t0 + 24, put(at(1), AIR)]]
                items.append(circuit(fam, ['source', name, t0], [(at(2), STONE, {}), *probes], 50, events, watch))
            else:
                items.append(pulse(fam, ['source', name, t0], [*build, (at(2), STONE, {}), *probes], watch, t0))
        # A torch under S (S one block up, probes around it).
        build = [(at(2, 1), STONE, {}), (at(3, 1), LAMP, {}), (at(2, 0, 1), STONE, {}), (at(2, 1, 1), *dust()),
                 (at(2, 1, -1), LAMP, {})]
        events = [[t0, put(at(2), *torch('top'))], [t0 + 24, put(at(2), AIR)]]
        items.append(circuit(fam, ['torch_under', t0], build, 50, events,
                             [at(2), at(2, 1), at(3, 1), at(2, 1, 1), at(2, 1, -1)]))
    # A lever and a button on S's west face, clicked by the player.
    for name, block, states in (('lever', 'minecraft:lever', {'lever_direction': 'west', 'open_bit': False}),
                                ('stone_button', 'minecraft:stone_button',
                                 {'facing_direction': 4, 'button_pressed_bit': False})):
        build = [(at(2), STONE, {}), (at(1), block, states), *probes]
        clicks = [[4, at(1), 4]] + ([[30, at(1), 4]] if name == 'lever' else [])
        items.append(clicked(fam, ['attached', name], build, clicks, watch, 60))
    return items


def redstone_repeaters() -> list[dict]:
    """Repeaters: each delay on and off, short pulses into each delay,
    chains, locking by a repeater or a comparator from either side."""
    fam = 'redstone_repeaters'
    items = []
    for t0 in PARITIES:
        for delay in range(4):
            build = [(at(1), *dust()), (at(2), *rep(delay)), (at(3), *dust()), (at(4), LAMP, {})]
            items.append(pulse(fam, ['delay', delay, t0], build, xs(5), t0))
    for delay in range(4):
        for length in range(1, 7):
            build = [(at(1), *rep(delay)), (at(2), *dust()), (at(3), LAMP, {})]
            items.append(pulse(fam, ['pulse', delay, length], build, xs(4), 4, length=length))
    for delays in ((0, 0, 0), (1, 2, 3), (3, 0, 1)):
        build = [(at(1 + i), *rep(d)) for i, d in enumerate(delays)] + [(at(4), *dust()), (at(5), LAMP, {})]
        items.append(pulse(fam, ['chain', list(delays)], build, xs(6), 4))
    for locker in ('repeater', 'comparator'):
        for side in ('south', 'north'):
            dz = 1 if side == 'south' else -1
            facing = 'south' if side == 'south' else 'north'
            lock = repeater(facing) if locker == 'repeater' else comparator(facing)
            for order in ('lock_first', 'main_first'):
                lock_at, main_at = (4, 14) if order == 'lock_first' else (14, 4)
                build = [(at(1), *dust()), (at(2), *rep(1)), (at(3), *dust()), (at(4), LAMP, {}),
                         (at(2, 0, dz), *lock)]
                events = [[main_at, put(at(0), RB)], [lock_at, put(at(2, 0, 2 * dz), RB)], [34, put(at(0), AIR)],
                          [54, put(at(2, 0, 2 * dz), AIR)]]
                items.append(circuit(fam, ['locking', locker, side, order], build, 80, events,
                                     xs(5) + [at(2, 0, dz), at(2, 0, 2 * dz)]))
    # Inputs a repeater takes from behind: a weakly powered stone, a
    # strongly powered stone, a lamp, a redstone block, dust beside.
    for name, build in (('weak_stone', [(at(1), *dust()), (at(2), STONE, {})]),
                        ('strong_stone', [(at(1), *rep()), (at(2), STONE, {})]),
                        ('dust_beside', [(at(1), *dust()), (at(2), *dust()), (at(2, 0, 1), *dust())])):
        rest = [(at(3), *rep()), (at(4), *dust()), (at(5), LAMP, {})]
        items.append(pulse(fam, ['input', name], build + rest, xs(6) + [at(2, 0, 1)], 4))
    # The repeater at (2,0,1) faces south: dust west of x=2 runs past it.
    return items


def redstone_comparators() -> list[dict]:
    """Comparators: compare and subtract with rear and side signals, side
    inputs from dust, a repeater and a redstone block, reading blocks and
    containers behind them (also through a stone), and the timing of a
    container change."""
    fam = 'redstone_comparators'
    items = []
    out = line(3, 17, *dust())
    for subtract in (False, True):
        for rear in (1, 2, 3):
            # The side source stays within z <= 3, clear of the next cell.
            for side in (1, 2):
                # Rear source at x=1-rear .. dust up to x=1; side dust from
                # (2,0,side+1) towards the comparator.
                build = [*line(2 - rear, 1, *dust()), (at(2), *cmp_(subtract)), *out,
                         *[(at(2, 0, z), *dust()) for z in range(1, side + 1)]]
                events = [[4, put(at(1 - rear), RB)], [14, put(at(2, 0, side + 1), RB)],
                          [34, put(at(2, 0, side + 1), AIR)], [44, put(at(1 - rear), AIR)]]
                items.append(circuit(fam, ['compare', subtract, rear, side], build, 60, events,
                                     xs(18, 1 - rear) + [at(2, 0, z) for z in range(1, side + 2)]))
    for t0 in PARITIES:
        build = [(at(1), *dust()), (at(2), *cmp_()), (at(3), *dust()), (at(4), LAMP, {})]
        items.append(pulse(fam, ['timing', t0], build, xs(5), t0))
        for subtract in (False, True):
            for name, side in (('repeater', [(at(2, 0, 1), *repeater('south'))]),
                               ('redstone_block', [])):
                build = [(at(1), *dust()), (at(2), *cmp_(subtract)), *line(3, 6, *dust()), *side]
                src = at(2, 0, 2) if name == 'repeater' else at(2, 0, 1)
                events = [[4, put(at(0), RB)], [t0 + 10, put(src, RB)], [t0 + 30, put(src, AIR)], [50, put(at(0), AIR)]]
                items.append(circuit(fam, ['side_from', name, subtract, t0], build, 60, events,
                                     xs(7) + [at(2, 0, 1), at(2, 0, 2)]))
    readers = [('composter', {'composter_fill_level': n}) for n in range(9)]
    readers += [('cake', {'bite_counter': n}) for n in range(7)]
    readers += [('cauldron', {'cauldron_liquid': liquid, 'fill_level': n}) for liquid in ('water', 'lava')
                for n in range(7)]
    readers += [('end_portal_frame', {'end_portal_eye_bit': b, 'minecraft:cardinal_direction': 'south'})
                for b in (False, True)]
    readers += [('respawn_anchor', {'respawn_anchor_charge': n}) for n in range(5)]
    readers += [(hive, {'direction': 0, 'honey_level': n}) for hive in ('beehive', 'bee_nest') for n in range(6)]
    readers += [('chiseled_bookshelf', {'books_stored': n, 'direction': 0}) for n in (0, 1, 3, 63)]
    for block, states in readers:
        for through in (False, True):
            build = [(at(0), f'minecraft:{block}', states), (at(1), *cmp_()), *line(2, 17, *dust())]
            if through:
                build = [(at(-1), f'minecraft:{block}', states), (at(0), STONE, {}), (at(1), *cmp_()),
                         *line(2, 17, *dust())]
            items.append(circuit(fam, ['read', block, states, through], build, 30, [], xs(18, -1)))
    containers = {'chest': 27, 'barrel': 27, 'hopper': 5, 'dispenser': 9, 'dropper': 9, 'furnace': 3}
    for block, slots in containers.items():
        for count in (0, 1, 64, 128, slots * 64):
            events = [[4, ['cmd', f'replaceitem block {{x}} {{y}} {{z}} slot.container {slot} minecraft:stone '
                           f'{min(64, count - 64 * slot)}', at(0)]] for slot in range((count + 63) // 64)]
            states = {'facing_direction': 0, 'toggle_bit': False} if block == 'hopper' else {}
            items.append(circuit(fam, ['container', block, count], [(at(0), f'minecraft:{block}', states),
                                                                     (at(1), *cmp_()), *line(2, 17, *dust())],
                                 40, events, xs(18)))
    for t0 in PARITIES:
        events = [[t0, ['cmd', 'replaceitem block {x} {y} {z} slot.container 0 minecraft:stone 64', at(0)]],
                  [t0 + 20, ['cmd', 'replaceitem block {x} {y} {z} slot.container 0 minecraft:air 1', at(0)]]]
        items.append(circuit(fam, ['container_timing', t0], [(at(0), 'minecraft:chest', {}), (at(1), *cmp_()),
                                                             (at(2), *dust()), (at(3), LAMP, {})], 50, events, xs(4)))
    return items


def redstone_torches() -> list[dict]:
    """Torches: inverters on a side and on top, a torch strongly powering
    the block above it, and a torch that turns itself off every update
    (burnout)."""
    fam = 'redstone_torches'
    items = []
    for t0 in PARITIES:
        build = [(at(1), *rep()), (at(2), STONE, {}), (at(3), *torch('west')), *line(4, 5, *dust()),
                 (at(6), LAMP, {})]
        items.append(pulse(fam, ['inverter_side', t0], build, xs(7), t0))
        build = [(at(1), *rep()), (at(2), STONE, {}), (at(2, 1), *torch('top')), (at(2, 2), STONE, {}),
                 (at(3, 2), LAMP, {}), (at(1, 2), LAMP, {})]
        items.append(pulse(fam, ['inverter_top', t0], build, xs(3) + [at(2, 1), at(2, 2), at(3, 2), at(1, 2)], t0))
    # Burnout: the torch T (2,0,0) on stone S (1,0,0) strongly powers the
    # stone U above it; the dust on top of S next to U powers S.
    # The loop closes when the dust is set at tick 4.
    build = [(at(1), STONE, {}), (at(2, 1), STONE, {}), (at(2), *torch('west'))]
    items.append(circuit(fam, ['burnout'], build, 400, [[4, put(at(1, 1), *dust())]], [at(2), at(1, 1)]))
    # The same loop broken after 200 ticks: does the torch come back?
    items.append(circuit(fam, ['burnout_recover'], build, 400,
                         [[4, put(at(1, 1), *dust())], [200, put(at(1, 1), AIR)]], [at(2), at(1, 1)]))
    return items


def door_blocks() -> list[str]:
    ids = block_ids()
    return sorted(n for n in ids if n.endswith('_door') or n == 'minecraft:wooden_door')


def trapdoor_blocks() -> list[str]:
    ids = block_ids()
    return sorted(n for n in ids if n.endswith('_trapdoor') or n == 'minecraft:trapdoor')


def gate_blocks() -> list[str]:
    ids = block_ids()
    return sorted(n for n in ids if n.endswith('fence_gate'))


def bulb_blocks() -> list[str]:
    ids = block_ids()
    return sorted(n for n in ids if n.endswith('copper_bulb'))


def redstone_consumers() -> list[dict]:
    """Every door, trapdoor, fence gate and copper bulb, hoppers, droppers,
    dispensers, crafters, TNT, lamps, note blocks and powered and activator
    rails next to a redstone block; doors powered at their upper half;
    consumers fed by dust and by a weakly powered block."""
    fam = 'redstone_consumers'
    items = []
    door = {'door_hinge_bit': False, 'minecraft:cardinal_direction': 'east', 'open_bit': False}
    trap = {'direction': 0, 'open_bit': False, 'upside_down_bit': False}
    gate = {'minecraft:cardinal_direction': 'east', 'open_bit': False, 'in_wall_bit': False}
    singles = [(b, trap) for b in trapdoor_blocks()] + [(b, gate) for b in gate_blocks()]
    singles += [(b, {'lit': False, 'powered_bit': False}) for b in bulb_blocks()]
    singles += [('minecraft:hopper', {'facing_direction': 0, 'toggle_bit': False}),
                ('minecraft:dispenser', {'facing_direction': 3, 'triggered_bit': False}),
                ('minecraft:dropper', {'facing_direction': 3, 'triggered_bit': False}),
                ('minecraft:crafter', {'crafting': False, 'orientation': 'north_up', 'triggered_bit': False}),
                ('minecraft:tnt', {'explode_bit': False}), (LAMP, {}), ('minecraft:noteblock', {})]
    for t0 in PARITIES:
        for block in door_blocks():
            build = [(at(1), block, {**door, 'upper_block_bit': False}),
                     (at(1, 1), block, {**door, 'upper_block_bit': True})]
            items.append(pulse(fam, ['door', block, t0], build, [at(0), at(1), at(1, 1)], t0))
        for block, states in singles:
            items.append(pulse(fam, ['beside', block, t0], [(at(1), block, states)], [at(0), at(1)], t0))
    for block in ('minecraft:wooden_door', 'minecraft:iron_door'):
        build = [(at(1), block, {**door, 'upper_block_bit': False}),
                 (at(1, 1), block, {**door, 'upper_block_bit': True})]
        items.append(pulse(fam, ['door_upper', block], build, [at(0, 1), at(1), at(1, 1)], 4, source=at(0, 1)))
    # Rails 10 long beside a redstone block, and on a slope.
    for block in ('minecraft:golden_rail', 'minecraft:activator_rail'):
        for t0 in PARITIES:
            items.append(pulse(fam, ['rails', block, t0], line(1, 10, block, {'rail_data_bit': False,
                                                                                'rail_direction': 1}), xs(11), t0))
    # Consumers at the end of a dust line and beside a weakly powered stone.
    for block, states in (('minecraft:iron_door', None), ('minecraft:iron_trapdoor', trap),
                          ('minecraft:hopper', {'facing_direction': 0, 'toggle_bit': False}), (LAMP, {}),
                          ('minecraft:copper_bulb', {'lit': False, 'powered_bit': False}),
                          ('minecraft:dropper', {'facing_direction': 3, 'triggered_bit': False})):
        if states is None:
            target = [(at(3), block, {**door, 'upper_block_bit': False}),
                      (at(3, 1), block, {**door, 'upper_block_bit': True})]
            watch = xs(4) + [at(3, 1)]
        else:
            target = [(at(3), block, states)]
            watch = xs(4)
        items.append(pulse(fam, ['dust_fed', block], [*line(1, 2, *dust()), *target], watch, 4))
        weak = [(at(1), *dust()), (at(2), STONE, {})]
        items.append(pulse(fam, ['weak_fed', block], weak + target, watch, 4))
    return items


def redstone_observers() -> list[dict]:
    """Observers watching a block set and removed, a dust whose signal
    changes, a lamp, a door and a repeater; observers watching observers;
    observers facing up and down."""
    fam = 'redstone_observers'
    obs = 'minecraft:observer'
    items = []
    for t0 in PARITIES:
        build = [(at(1), obs, {'minecraft:facing_direction': 'west', 'powered_bit': False}), (at(2), *dust()),
                 (at(3), LAMP, {})]
        items.append(circuit(fam, ['block', t0], build, 50, [[t0, put(at(0), STONE)], [t0 + 24, put(at(0), AIR)]],
                             xs(4)))
        build = [(at(1), *dust()), (at(1, 0, 1), obs, {'minecraft:facing_direction': 'north', 'powered_bit': False}),
                 (at(1, 0, 2), *dust()), (at(2, 0, 2), LAMP, {})]
        items.append(pulse(fam, ['dust', t0], build, xs(2) + [at(1, 0, 1), at(1, 0, 2), at(2, 0, 2)], t0))
        build = [(at(1), LAMP, {}), (at(1, 0, 1), obs, {'minecraft:facing_direction': 'north', 'powered_bit': False}),
                 (at(1, 0, 2), *dust()), (at(2, 0, 2), LAMP, {})]
        items.append(pulse(fam, ['lamp', t0], build, xs(2) + [at(1, 0, 1), at(1, 0, 2), at(2, 0, 2)], t0))
        build = [(at(1), *rep()), (at(1, 0, 1), obs, {'minecraft:facing_direction': 'north', 'powered_bit': False}),
                 (at(1, 0, 2), *dust()), (at(2, 0, 2), LAMP, {})]
        items.append(pulse(fam, ['repeater', t0], build, xs(2) + [at(1, 0, 1), at(1, 0, 2), at(2, 0, 2)], t0))
        build = [(at(1), obs, {'minecraft:facing_direction': 'west', 'powered_bit': False}),
                 (at(2), obs, {'minecraft:facing_direction': 'west', 'powered_bit': False}),
                 (at(3), obs, {'minecraft:facing_direction': 'west', 'powered_bit': False}),
                 (at(4), *dust()), (at(5), LAMP, {})]
        items.append(circuit(fam, ['chain', t0], build, 50, [[t0, put(at(0), STONE)], [t0 + 24, put(at(0), AIR)]],
                             xs(6)))
        # Facing down at the floor block under it (replaced), output up.
        build = [(at(1, 1), obs, {'minecraft:facing_direction': 'down', 'powered_bit': False}),
                 (at(1, 2), LAMP, {})]
        items.append(circuit(fam, ['down', t0], build, 50, [[t0, put(at(1), STONE)], [t0 + 24, put(at(1), AIR)]],
                             [at(1), at(1, 1), at(1, 2)]))
    return items


# Blocks Endstone sets with water around them (live coral fans, water
# plants, underwater torches and TNT): the water flows into the cells beside
# them, so the per-block families leave them out.
WATERY = ('_coral_fan', '_coral_wall_fan', 'minecraft:kelp', 'minecraft:seagrass', 'minecraft:sea_pickle',
          'minecraft:underwater_torch', 'minecraft:underwater_tnt')


def watery(block: str) -> bool:
    return not block.startswith('minecraft:dead_') and any(
        block == w or (w.startswith('_') and block.endswith(w)) for w in WATERY)


def conductor_candidates() -> list[tuple[str, dict]]:
    """Every block's default state that a scenario cell can hold, without
    the blocks that bring water."""
    return [(name, states) for name, states in sorted(defaults().items()) if not watery(name)]


def redstone_conductors() -> list[dict]:
    """Which blocks a repeater powers through: the block B at x=2 with a
    repeater into it from x=1 and a lamp beyond it at x=3."""
    fam = 'redstone_conductors'
    items = []
    for block, states in conductor_candidates():
        build = [(at(1), *rep()), (at(2), block, states), (at(3), LAMP, {})]
        items.append(circuit(fam, ['block', block, states], build, 20,
                             [[2, put(at(0), RB)]], xs(4)))
    return items


def defaults() -> dict:
    import scenarios
    return scenarios.defaults()


MEASURED = {'redstone_dust': redstone_dust, 'redstone_power': redstone_power,
            'redstone_repeaters': redstone_repeaters, 'redstone_comparators': redstone_comparators,
            'redstone_torches': redstone_torches, 'redstone_consumers': redstone_consumers,
            'redstone_observers': redstone_observers, 'redstone_conductors': redstone_conductors}
FAMILIES.update(MEASURED)


def redstone_dust_blocks() -> list[dict]:
    """Dust climbing every block (default state, and each further collision
    shape): dust on the floor west of the block X (x=2), dust on top of X,
    dust on the floor east of it and a lamp: whether the signal goes up
    onto X and down its far side, and whether X holds the dust at all."""
    import scenarios
    fam = 'redstone_dust_blocks'
    candidates = conductor_candidates()
    candidates += [(e['name'], e['states']) for e in scenarios.shape_variants() if not watery(e['name'])]
    items = []
    for block, states in candidates:
        build = [(at(1), *dust()), (at(2), block, states), (at(2, 1), *dust()), (at(3), *dust()), (at(4), LAMP, {})]
        items.append(circuit(fam, ['climb', block, states], build, 20, [[2, put(at(0), RB)]],
                             [at(0), at(1), at(2), at(2, 1), at(3), at(4)]))
    return items


def redstone_dust_cut() -> list[dict]:
    """Which blocks above the lower dust cut a climb: dust x=1, stone x=2
    with dust on top, the candidate above x=1."""
    fam = 'redstone_dust_cut'
    items = []
    for block, states in conductor_candidates():
        build = [(at(1), *dust()), (at(2), STONE, {}), (at(2, 1), *dust()), (at(1, 1), block, states)]
        items.append(circuit(fam, ['cut', block, states], build, 20, [[2, put(at(0), RB)]],
                             [at(0), at(1), at(2, 1), at(1, 1)]))
    return items


MEASURED.update({'redstone_dust_blocks': redstone_dust_blocks, 'redstone_dust_cut': redstone_dust_cut})
FAMILIES.update(MEASURED)


def redstone_observer_timing() -> list[dict]:
    """When an observer pulses after the block it watches changes, for each
    kind of change: dust, a repeater, a comparator, a torch, a lamp, a door,
    a trapdoor, a hopper, a copper bulb, a powered rail, a dispenser, and a
    block set or removed from outside. The observer at (1,0,1) faces north
    at (1,0,0); its output runs through a dust at (1,0,2) to a lamp at
    (2,0,2). A redstone block at the source (default x=0) is set at t0 and
    removed 24 ticks later."""
    fam = 'redstone_observer_timing'
    obs = ('minecraft:observer', {'minecraft:facing_direction': 'north', 'powered_bit': False})
    out = [(at(1, 0, 1), *obs), (at(1, 0, 2), *dust()), (at(2, 0, 2), LAMP, {})]
    out_watch = [at(1, 0, 1), at(1, 0, 2), at(2, 0, 2)]
    trap = {'direction': 0, 'open_bit': False, 'upside_down_bit': False}
    door = {'door_hinge_bit': False, 'minecraft:cardinal_direction': 'east', 'open_bit': False}
    watched = {
        'dust': [(at(1), *dust())],
        'repeater': [(at(1), *rep())],
        'repeater_slow': [(at(1), *rep(2))],
        'comparator': [(at(1), *cmp_())],
        'lamp': [(at(1), LAMP, {})],
        'iron_door': [(at(1), 'minecraft:iron_door', {**door, 'upper_block_bit': False}),
                      (at(1, 1), 'minecraft:iron_door', {**door, 'upper_block_bit': True})],
        'iron_trapdoor': [(at(1), 'minecraft:iron_trapdoor', trap)],
        'hopper': [(at(1), 'minecraft:hopper', {'facing_direction': 0, 'toggle_bit': False})],
        'copper_bulb': [(at(1), 'minecraft:copper_bulb', {'lit': False, 'powered_bit': False})],
        'golden_rail': [(at(1), 'minecraft:golden_rail', {'rail_data_bit': False, 'rail_direction': 1})],
        'dispenser': [(at(1), 'minecraft:dispenser', {'facing_direction': 2, 'triggered_bit': False})],
    }
    items = []
    for t0 in PARITIES:
        for name, build in watched.items():
            items.append(pulse(fam, [name, t0], build + out, [at(0), at(1)] + out_watch, t0))
        # A torch on a stone strongly powered by a repeater below and west.
        build = [(at(0, -1), *rep()), (at(1, -1), STONE, {}), (at(1), *torch('top'))] + out
        items.append(pulse(fam, ['torch', t0], build, [at(-1, -1), at(1)] + out_watch, t0, source=at(-1, -1)))
        # The watched block itself set and removed from outside.
        items.append(circuit(fam, ['outside', t0], out, 50,
                             [[t0, put(at(1), STONE)], [t0 + 24, put(at(1), AIR)]], [at(1)] + out_watch))
    return items


def redstone_burnout() -> list[dict]:
    """Torch clocks of different periods (a torch on stone S at x=1 feeding
    a chain of repeaters back into S): which burn out, and when."""
    fam = 'redstone_burnout'
    items = []
    for delays in ((0,), (1,), (2,), (3,), (0, 0), (1, 1), (3, 3)):
        # T at (2,0,0) on S (1,0,0); repeaters south from T at (2,0,1..),
        # then dust west and a repeater north into S from (1,0,1).
        build = [(at(1), STONE, {}), (at(2), *torch('west'))]
        z = 1
        for d in delays:
            build.append((at(2, 0, z), *repeater('north', d)))
            z += 1
        build += [(at(2, 0, z), *dust()), (at(1, 0, z), *dust())]
        build += [(at(1, 0, zz), *dust()) for zz in range(2, z)]
        watch = [at(2), at(1, 0, 1)]
        items.append(circuit(fam, ['clock', list(delays)], build, 300,
                             [[4, put(at(1, 0, 1), *repeater('south'))]], watch))
    return items


MEASURED.update({'redstone_observer_timing': redstone_observer_timing, 'redstone_burnout': redstone_burnout})
FAMILIES.update(MEASURED)


def redstone_switches() -> list[dict]:
    """Every button and the lever, clicked by the player, powering a dust
    line and a lamp: when the power arrives and, for buttons, how long it
    stays (the lever is clicked on and off at each tick parity)."""
    fam = 'redstone_switches'
    items = []
    lamp_line = [*line(1, 2, *dust()), (at(3), LAMP, {})]
    for button in BUTTONS:
        build = [(at(0), button, {'facing_direction': 1, 'button_pressed_bit': False}), *lamp_line]
        items.append(clicked(fam, ['button', button], build, [[4, at(0), 1]], xs(4), 60))
    for t0 in PARITIES:
        build = [(at(0), *lever()), *lamp_line]
        items.append(clicked(fam, ['lever', t0], build, [[t0, at(0), 1], [t0 + 21, at(0), 1]], xs(4), 50))
    return items


MEASURED['redstone_switches'] = redstone_switches
FAMILIES.update(MEASURED)
