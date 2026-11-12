"""Round 4: piston, pressure plate, tripwire, target and TNT families for the
`scenarios` job, on the circuit layout (`circuits.LAYOUT`).

Each family builds a circuit in its cell, lets it settle and runs a
`timeline` that reads every listed position every tick; families with
actors (pushed mobs, primed TNT, items on plates) also read the actors in
the cell every tick (the timeline's `actors` box). The replay tests in
`crates/unastar/src/server/game/redstone/` and `.../pistons/` compare
unastar tick by tick.
"""

from __future__ import annotations

from circuits import AIR, DUST, LAMP, RB, STONE, at, circuit, dust, key, put, timeline, wait, xs

PISTON = 'minecraft:piston'
STICKY = 'minecraft:sticky_piston'
SLIME = 'minecraft:slime'
HONEY = 'minecraft:honey_block'
OBSIDIAN = 'minecraft:obsidian'
# The cell box actors are read in (cell offsets, inclusive).
BOX = {'box': [[-1, -1, -2], [20, 4, 2]]}


def piston(block: str = PISTON, facing: int = 0) -> tuple[str, dict]:
    return block, {'facing_direction': facing}


def run(fam: str, name, build: list, ticks: int, events: list, watch: list, settle: int = 10,
        actors: bool = False) -> dict:
    item = circuit(fam, name, build, ticks, events, watch, settle)
    if actors:
        item['steps'][-1].append(BOX)
    return item


def pistons_pilot() -> list[dict]:
    """First questions: which `facing_direction` points where, the push
    timing, what the moving blocks and the head look like each tick."""
    fam = 'pistons_pilot'
    items = []
    # A piston at x=2 with every facing, powered from the north (z=-1),
    # stones on the other horizontal sides and above.
    for block in (PISTON, STICKY):
        for facing in range(6):
            build = [(at(1), STONE, {}), (at(3), STONE, {}), (at(2, 0, 1), STONE, {}), (at(2, 1), STONE, {}),
                     (at(2), *piston(block, facing))]
            watch = [at(2), at(1), at(0), at(3), at(4), at(2, 0, 1), at(2, 0, 2), at(2, 1), at(2, 2), at(2, -1),
                     at(2, 0, -1)]
            for t0 in (4, 5):
                items.append(run(fam, ['facing', block, facing, t0], build, 50,
                                 [[t0, put(at(2, 0, -1), RB)], [t0 + 20, put(at(2, 0, -1), AIR)]], watch))
    return items


EAST = 4
WEST = 5
UP = 1
DOWN = 0
PARITIES = (4, 5)


def toggle(fam: str, name, build: list, watch: list, t0: int = 4, length: int = 20, ticks: int = 50,
           source=None, actors: bool = False) -> dict:
    """A redstone block at `source` (default x=0) from `t0` for `length`
    ticks."""
    src = source or at(0)
    return run(fam, name, build, ticks, [[t0, put(src, RB)], [t0 + length, put(src, AIR)]], watch,
               actors=actors)


def stones(x0: int, x1: int, y: int = 0) -> list:
    return [(at(x, y), STONE, {}) for x in range(x0, x1)]


def pistons_push() -> list[dict]:
    """Push limits and timing: a piston at x=1 facing east with n stones in
    front (x=2..n+1), n = 0..14, powered from x=0 for 20 ticks at both tick
    parities; a chain ended by obsidian; a chain ended by a torch."""
    fam = 'pistons_push'
    items = []
    watch = xs(17)
    for block in (PISTON, STICKY):
        for n in range(15):
            build = [(at(1), *piston(block, EAST))] + stones(2, n + 2)
            for t0 in PARITIES:
                items.append(toggle(fam, ['line', block, n, t0], build, watch, t0=t0))
        for n in (0, 1, 3):
            build = [(at(1), *piston(block, EAST))] + stones(2, n + 2) + [(at(n + 2), OBSIDIAN, {})]
            items.append(toggle(fam, ['obsidian_end', block, n], build, watch))
        for n in (1, 11, 12):
            build = ([(at(1), *piston(block, EAST))] + stones(2, n + 2)
                     + [(at(n + 2), 'minecraft:torch', {'torch_facing_direction': 'top'})])
            items.append(toggle(fam, ['torch_end', block, n], build, watch))
    return items


def pistons_blocks() -> list[dict]:
    """Every block's default state in front of a sticky piston (x=1 facing
    east, the block at x=2): pushed to x=3, broken, or holding the piston;
    pulled back or left when the power goes (20 ticks later)."""
    from circuits import conductor_candidates
    fam = 'pistons_blocks'
    items = []
    watch = [at(0), at(1), at(2), at(3), at(4), at(2, 1), at(3, 1), at(2, -1), at(3, -1)]
    for block, states in conductor_candidates():
        build = [(at(1), *piston(STICKY, EAST)), (at(2), block, states)]
        items.append(toggle(fam, ['block', block, states], build, watch))
    return items


def slime_case(name, extra: list, watch_extra: list, block: str = STICKY) -> dict:
    """A piston at (1,2,0) facing east, powered from (0,2,0), with the
    blocks `extra` in front; everything 2 above the floor."""
    build = [(at(1, 2), *piston(block, EAST))] + extra
    watch = [at(0, 2), at(1, 2)] + [at(x, 2) for x in range(2, 11)] + watch_extra
    return toggle('pistons_slime', name, build, watch, source=at(0, 2))


def pistons_slime() -> list[dict]:
    """Slime and honey: blocks stuck to them move along (push and pull),
    slime and honey do not stick to each other, glazed terracotta and
    immovable blocks, and the 12-block limit counted over a stuck
    structure."""
    items = []
    around = [at(2, 3), at(2, 1), at(2, 2, 1), at(2, 2, -1), at(3, 3), at(3, 1), at(3, 2, 1), at(3, 2, -1)]
    for glue in (SLIME, HONEY):
        g = glue.removeprefix('minecraft:')
        four = [(at(2, 3), STONE, {}), (at(2, 1), STONE, {}), (at(2, 2, 1), STONE, {}), (at(2, 2, -1), STONE, {})]
        for block in (STICKY, PISTON):
            items.append(slime_case([g, 'stuck', block], [(at(2, 2), glue, {})] + four, around, block))
        other = HONEY if glue == SLIME else SLIME
        items.append(slime_case([g, 'other_glue'], [(at(2, 2), glue, {}), (at(2, 2, 1), other, {})], around))
        items.append(slime_case([g, 'terracotta'], [(at(2, 2), glue, {}),
                                                    (at(2, 2, 1), 'minecraft:white_glazed_terracotta',
                                                     {'facing_direction': 2})], around))
        items.append(slime_case([g, 'obsidian'], [(at(2, 2), glue, {}), (at(2, 2, 1), OBSIDIAN, {})], around))
        items.append(slime_case([g, 'torch'], [(at(2, 2), glue, {}),
                                               (at(2, 3), 'minecraft:torch', {'torch_facing_direction': 'top'})],
                                around))
        # A line of k glue blocks with a stone on each: 2k blocks moved.
        for k in (5, 6, 7):
            extra = [(at(x, 2), glue, {}) for x in range(2, 2 + k)] + stones(2, 2 + k, y=3)
            items.append(slime_case([g, 'line', k], extra, [at(x, 3) for x in range(2, 10)]))
        # Glue behind a stone (pushed by it) with stones stuck to the glue.
        items.append(slime_case([g, 'behind'], [(at(2, 2), STONE, {}), (at(3, 2), glue, {}), (at(3, 3), STONE, {}),
                                                (at(3, 1), STONE, {})], around))
        # A stone stuck to the glue whose way is blocked by obsidian.
        items.append(slime_case([g, 'stuck_blocked'], [(at(2, 2), glue, {}), (at(2, 3), STONE, {}),
                                                       (at(3, 3), OBSIDIAN, {})], around))
        # Glue on the piston's own side.
        items.append(slime_case([g, 'piston_side'], [(at(2, 2), STONE, {}), (at(1, 3), glue, {}),
                                                     (at(2, 3), glue, {})], around + [at(1, 3)]))
    return items


def pistons_power() -> list[dict]:
    """How a piston is powered: quasi-connectivity (power next to the block
    above it, then a block update beside it), a lever on it, dust on top
    and beside, a powered block under it, a redstone block at its face."""
    fam = 'pistons_power'
    items = []
    base = [(at(1), *piston(PISTON, EAST)), (at(2), STONE, {})]
    watch = [at(0), at(1), at(2), at(3), at(1, 1), at(1, 2), at(0, 1), at(1, 0, 1)]
    nudge = put(at(1, 0, 1), STONE)
    for name, source in (('two_above', at(1, 2)), ('above_back', at(0, 1)), ('above_front', at(2, 1)),
                         ('above_side', at(1, 1, 1))):
        events = [[4, put(source, RB)], [14, nudge], [30, put(source, AIR)], [40, put(at(1, 0, 1), AIR)]]
        items.append(run(fam, ['qc', name], base, 60, events, watch + [source]))
    lever = {'lever_direction': 'up_east_west', 'open_bit': False}
    events = [[4, put(at(1, 1), 'minecraft:lever', {**lever, 'open_bit': True})],
              [24, put(at(1, 1), 'minecraft:lever', lever)]]
    items.append(run(fam, ['lever_on_top'], base + [(at(1, 1), 'minecraft:lever', lever)], 50, events, watch))
    items.append(toggle(fam, ['dust_on_top'], base + [(at(0), STONE, {}), (at(1, 1), *dust())], watch,
                        source=at(0, 1)))
    items.append(toggle(fam, ['dust_points'], [(at(1, 0, 1), *dust()), *base], watch + [at(1, 0, 2)],
                        source=at(1, 0, 2)))
    items.append(toggle(fam, ['dust_past'], [(at(0, 0, 1), *dust()), (at(1, 0, 1), *dust()), (at(2, 0, 1), *dust()),
                                             *base], watch + [at(-1, 0, 1)], source=at(-1, 0, 1)))
    raised = [(at(1, 1), *piston(PISTON, EAST)), (at(2, 1), STONE, {}), (at(1), STONE, {}),
              (at(0), 'minecraft:unpowered_repeater', {'minecraft:cardinal_direction': 'west', 'repeater_delay': 0})]
    items.append(toggle(fam, ['under_strong'], raised, [at(-1), at(0), at(1), at(1, 1), at(2, 1), at(3, 1)],
                        source=at(-1)))
    items.append(run(fam, ['at_face'], [(at(1), *piston(PISTON, EAST))], 40, [[4, put(at(2), RB)]], xs(5)))
    items.append(toggle(fam, ['front_block_powered'], base, watch + [at(2, 0, 1)], source=at(2, 0, 1)))
    items.append(run(fam, ['torch_beside'], base, 40,
                     [[4, put(at(1, 0, -1), 'minecraft:redstone_torch', {'torch_facing_direction': 'top'})]],
                     watch + [at(1, 0, -1)]))
    return items


def pistons_timing() -> list[dict]:
    """Short pulses (1-6 ticks) at both parities into a piston and a sticky
    piston pushing a stone; power back while retracting; a piston behind a
    repeater."""
    fam = 'pistons_timing'
    items = []
    for block in (PISTON, STICKY):
        build = [(at(1), *piston(block, EAST)), (at(2), STONE, {})]
        for length in (1, 2, 3, 4, 6):
            for t0 in PARITIES:
                items.append(toggle(fam, ['pulse', block, length, t0], build, xs(5), t0=t0, length=length,
                                    ticks=40))
        for gap in (1, 2, 3, 4):
            events = [[4, put(at(0), RB)], [24, put(at(0), AIR)], [24 + gap, put(at(0), RB)],
                      [44 + gap, put(at(0), AIR)]]
            items.append(run(fam, ['repower', block, gap], build, 70, events, xs(5)))
        for delay in (0, 3):
            rep = ('minecraft:unpowered_repeater', {'minecraft:cardinal_direction': 'west', 'repeater_delay': delay})
            build2 = [(at(1), *rep), (at(2), *piston(block, EAST)), (at(3), STONE, {})]
            items.append(toggle(fam, ['repeater', block, delay], build2, xs(6)))
    return items


def pistons_chains() -> list[dict]:
    """Pistons pushing pistons: a retracted piston is pushed, an extended
    one (and its head) holds; an observer pushed in front of dust."""
    fam = 'pistons_chains'
    items = []
    for block in (PISTON, STICKY):
        build = [(at(1), *piston(block, EAST)), (at(2), *piston(PISTON, EAST))]
        items.append(toggle(fam, ['push_piston', block], build, xs(6)))
        build = [(at(1), *piston(block, EAST)), (at(2, -1), RB, {}), (at(2), *piston(PISTON, EAST)),
                 (at(4), STONE, {})]
        items.append(toggle(fam, ['push_extended', block], build, xs(6) + [at(2, -1)], t0=14))
        build = [(at(3, 0, -1), *piston(PISTON, 2)), (at(3, 0, -2), RB, {}), (at(1), *piston(block, EAST)),
                 (at(2), STONE, {})]
        items.append(toggle(fam, ['push_head', block], build, xs(6) + [at(3, 0, -1)], t0=14))
        build = [(at(1), *piston(block, EAST)),
                 (at(2), 'minecraft:observer', {'minecraft:facing_direction': 'east', 'powered_bit': False}),
                 (at(3), *dust()), (at(4), LAMP, {})]
        items.append(toggle(fam, ['observer', block], build, xs(7)))
    return items


def pistons_entities() -> list[dict]:
    """Actors pushed by a piston head and carried by moving blocks: an
    armour stand and an item in front of the head, on top of a pushed
    stone and beyond it; a piston facing up under a stone with an armour
    stand on it; a pig in front of the head."""
    fam = 'pistons_entities'
    items = []
    stand = 'summon minecraft:armor_stand {cx} {y} {cz}'

    def spawn(what: str, off):
        if what == 'item':
            return ['drop', off, 'minecraft:stone', 1]
        return ['cmd', stand, off]

    for block in (PISTON, STICKY):
        for what in ('armor_stand', 'item'):
            build = [(at(1), *piston(block, EAST))]
            items.append(run(fam, ['front', block, what], build, 50,
                             [[2, spawn(what, at(2))], [10, put(at(0), RB)], [30, put(at(0), AIR)]], xs(5),
                             actors=True))
            build = [(at(1), *piston(block, EAST)), (at(2), STONE, {})]
            items.append(run(fam, ['on_top', block, what], build, 50,
                             [[2, spawn(what, at(2, 1))], [10, put(at(0), RB)], [30, put(at(0), AIR)]], xs(5),
                             actors=True))
            items.append(run(fam, ['beyond', block, what], build, 50,
                             [[2, spawn(what, at(3))], [10, put(at(0), RB)], [30, put(at(0), AIR)]], xs(5),
                             actors=True))
        build = [(at(1), *piston(block, UP)), (at(1, 1), STONE, {})]
        items.append(run(fam, ['up', block], build, 60,
                         [[2, ['cmd', stand, at(1, 2)]], [10, put(at(0), RB)], [30, put(at(0), AIR)]],
                         [at(0), at(1), at(1, 1), at(1, 2), at(1, 3)], actors=True))
        items.append(run(fam, ['front_pig', block], [(at(1), *piston(block, EAST))], 50,
                         [[2, ['cmd', 'summon minecraft:pig {cx} {y} {cz}', at(2)]], [10, put(at(0), RB)],
                          [30, put(at(0), AIR)]], xs(5), actors=True))
    return items


FAMILIES = {'pistons_pilot': pistons_pilot, 'pistons_push': pistons_push, 'pistons_blocks': pistons_blocks,
            'pistons_slime': pistons_slime, 'pistons_power': pistons_power, 'pistons_timing': pistons_timing,
            'pistons_chains': pistons_chains, 'pistons_entities': pistons_entities}
