"""Round 4: pressure plates and tripwires pressed by actors, for the
`scenarios` job on the circuit layout (`circuits.LAYOUT`).

A plate at x=1 feeds dust at x=2..3 and a lamp at x=4; actors are put on it
at a known tick (both parities), removed later, and the plate, the dust,
the lamp and the actors in the cell are read every tick.

- `plates_press`: every plate kind under an armour stand, a pig held still,
  one item and an item stack; each removed 30 ticks later with `/kill`.
- `plates_weighted`: the weighted plates under 1-64 separate items, and under
  one stack of 1-64 items (do stacks count by item or by actor?).
- `plates_tripwire`: two hooks facing each other with string between them;
  an armour stand set on the string and removed, an item dropped on it.
"""

from __future__ import annotations

from circuits import AIR, LAMP, STONE, at, dust, key, timeline, wait, xs

BOX = {'box': [[-1, -1, -2], [20, 4, 2]]}
PARITIES = (4, 5)
PLATES = ['minecraft:stone_pressure_plate', 'minecraft:wooden_pressure_plate', 'minecraft:spruce_pressure_plate',
          'minecraft:crimson_pressure_plate', 'minecraft:polished_blackstone_pressure_plate',
          'minecraft:light_weighted_pressure_plate', 'minecraft:heavy_weighted_pressure_plate']


def item(fam: str, name, build: list, ticks: int, events: list, watch: list) -> dict:
    steps = [['build', [[o, b, st, True] for o, b, st in build]], wait(10), timeline(ticks, events, watch)]
    steps[-1].append(BOX)
    return {'key': key(fam, name), 'family': fam, 'steps': steps}


def plate_cell(plate: str) -> list:
    return [(at(1), plate, {'redstone_signal': 0}), (at(2), *dust()), (at(3), *dust()), (at(4), LAMP, {})]


KILL = ['cmd', 'kill @e[x={cx},y={y},z={cz},r=0.9,type=!player]', at(1)]


def presser(what: str) -> list:
    """The events that put `what` on the plate at x=1 at tick 0 of the
    returned list (relative ticks)."""
    if what == 'armor_stand':
        return [[0, ['cmd', 'summon minecraft:armor_stand {cx} {y} {cz}', at(1)]]]
    if what == 'pig':
        return [[0, ['cmd', 'summon minecraft:pig {cx} {y} {cz}', at(1)]],
                [0, ['cmd', 'effect @e[type=pig,x={cx},y={y},z={cz},r=1] slowness 60 255 true', at(1)]]]
    if what == 'item':
        return [[0, ['drop', at(1), 'minecraft:stone', 1]]]
    if what == 'stack':
        return [[0, ['drop', at(1), 'minecraft:stone', 20]]]
    raise ValueError(what)


def shifted(events: list, t0: int) -> list:
    return [[t + t0, event] for t, event in events]


def plates_press() -> list[dict]:
    fam = 'plates_press'
    items = []
    for plate in PLATES:
        for what in ('armor_stand', 'pig', 'item', 'stack'):
            for t0 in PARITIES:
                events = shifted(presser(what), t0) + [[t0 + 30, KILL]]
                items.append(item(fam, [plate, what, t0], plate_cell(plate), 90, events, xs(5)))
    return items


def plates_weighted() -> list[dict]:
    fam = 'plates_weighted'
    items = []
    for plate in ('minecraft:light_weighted_pressure_plate', 'minecraft:heavy_weighted_pressure_plate'):
        for n in (1, 2, 3, 5, 9, 10, 11, 15, 16, 20, 31, 40, 64):
            drops = [[4, ['drop', at(1), 'minecraft:stone', 1]] for _ in range(n)]
            items.append(item(fam, [plate, 'items', n], plate_cell(plate), 60, drops + [[34, KILL]], xs(5)))
        for n in (1, 2, 15, 16, 64):
            items.append(item(fam, [plate, 'stack', n], plate_cell(plate), 60,
                              [[4, ['drop', at(1), 'minecraft:stone', n]], [34, KILL]], xs(5)))
    return items


def plates_tripwire() -> list[dict]:
    """Tripwire hooks hanging on stones at x=0 and x=6, facing each other,
    with string at x=2..4 and a lamp beside the west hook: an armour stand
    set on the string and removed, an item dropped on it, and the string
    cut. Watch the hooks, the string and the lamp."""
    fam = 'plates_tripwire'
    items = []
    hook = 'minecraft:tripwire_hook'
    build = [(at(0), STONE, {}), (at(6), STONE, {}),
             (at(1), hook, {'attached_bit': False, 'direction': 3, 'powered_bit': False}),
             (at(5), hook, {'attached_bit': False, 'direction': 1, 'powered_bit': False}),
             *[(at(x), 'minecraft:trip_wire', {}) for x in range(2, 5)],
             (at(1, 0, 1), LAMP, {})]
    watch = xs(7) + [at(1, 0, 1)]
    for what in ('armor_stand', 'item'):
        for t0 in PARITIES:
            spawn = (['cmd', 'summon minecraft:armor_stand {cx} {y} {cz}', at(3)] if what == 'armor_stand'
                     else ['drop', at(3), 'minecraft:stone', 1])
            kill = ['cmd', 'kill @e[x={cx},y={y},z={cz},r=0.9,type=!player]', at(3)]
            items.append(item(fam, [what, t0], build, 90, [[t0, spawn], [t0 + 30, kill]], watch))
    # Breaking the string with a block update (set to air) while armed.
    items.append(item(fam, ['cut'], build, 40, [[4, ['set', at(3), AIR, {}, True]]], watch))
    return items


# Supports beside the string at (2, 0, 1) an armour stand stands on with
# its box over the string (round 5: how high the string reaches).
SUPPORTS = {
    'floor': None,
    'carpet': ('minecraft:white_carpet', {}),
    'repeater': ('minecraft:unpowered_repeater', {'minecraft:cardinal_direction': 'north', 'repeater_delay': 0}),
    'trapdoor': ('minecraft:trapdoor', {'direction': 0, 'open_bit': False, 'upside_down_bit': False}),
    'snow2': ('minecraft:snow_layer', {'covered_bit': False, 'height': 1}),
    'slab': ('minecraft:smooth_stone_slab', {'minecraft:vertical_half': 'bottom'}),
}


def plates_tripwire_more() -> list[dict]:
    """Round 5: the string between hooks at x=1 and x=4 (stones at x=0 and
    x=5, string at x=2..3, a lamp beside the west hook).

    - `height`: an armour stand set above a support south of the string at
      x=2 and moved so its box reaches over the string; it falls onto the
      support (or the floor): which heights press the string.
    - `hooks_last`: the string set before the hooks.
    - `no_hooks`, `one_hook`: an armour stand on string with no hook or one.
    - `cut_then_step`: the string at x=3 set to air, then an armour stand on
      the string at x=2 (does the hook still power?).
    """
    fam = 'plates_tripwire_more'
    hook = 'minecraft:tripwire_hook'
    west = (at(1), hook, {'attached_bit': False, 'direction': 3, 'powered_bit': False})
    east = (at(4), hook, {'attached_bit': False, 'direction': 1, 'powered_bit': False})
    wire = [(at(x), 'minecraft:trip_wire', {}) for x in (2, 3)]
    ends = [(at(0), STONE, {}), (at(5), STONE, {})]
    lamp = (at(1, 0, 1), LAMP, {})
    watch = xs(6) + [at(1, 0, 1), at(2, 0, 1)]
    kill = ['cmd', 'kill @e[x={cx},y={y},z={cz},r=1.5,type=!player]', at(2)]
    stand = ['cmd', 'summon minecraft:armor_stand {cx} {y} {cz}', at(2)]
    items = []
    for name, support in SUPPORTS.items():
        build = ends + [west, east] + wire + [lamp]
        if support:
            build.append((at(2, 0, 1), *support))
        events = [[4, ['cmd', 'summon minecraft:armor_stand {cx} {y} {cz}', at(2, 1, 1)]],
                  [5, ['cmd', 'tp @e[type=armor_stand,x={cx},y={y},z={cz},r=0.9] ~ ~ ~-0.6', at(2, 1, 1)]],
                  [40, kill]]
        items.append(item(fam, ['height', name], build, 60, events, watch))
    items.append(item(fam, ['hooks_last'], ends + wire + [west, east, lamp], 60, [[4, stand], [34, kill]], watch))
    items.append(item(fam, ['no_hooks'], ends + wire + [lamp], 60, [[4, stand], [34, kill]], watch))
    items.append(item(fam, ['one_hook'], ends + [west] + wire + [lamp], 60, [[4, stand], [34, kill]], watch))
    items.append(item(fam, ['cut_then_step'], ends + [west, east] + wire + [lamp], 70,
                      [[4, ['set', at(3), AIR, {}, True]], [10, stand], [40, kill]], watch))
    return items


FAMILIES = {'plates_press': plates_press, 'plates_weighted': plates_weighted,
            'plates_tripwire': plates_tripwire, 'plates_tripwire_more': plates_tripwire_more}
