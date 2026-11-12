"""Round 4: TNT and explosion families for the `scenarios` job.

Explosions reach farther than a circuit cell, so these cells stand 16 blocks
apart with a 13 x 10 x 13 box refilled after each wave (`LAYOUT`); the
redstone witness gives each wave's update parity.

- `tnt_prime`: TNT primed by a redstone block (both parities) or a
  neighbouring explosion; the TNT block, the primed actor's path and the
  tick it explodes.
- `tnt_blast_<material>`: TNT in the middle of a 9 x 9 x 9 cube of one
  material, primed; after the explosion every cube position is read and the
  items left in the cell are counted. Many cells per material, compared by
  distribution.
- `tnt_actors`: armour stands and iron golems at distances 1-7 from a TNT,
  some behind a wall; their health and motion every tick (the golems
  wander: compare from their recorded positions).
- `tnt_hits`: iron golems and pigs held still with slowness, at known
  spots, alone and behind walls: damage and knockback.
"""

from __future__ import annotations

from circuits import AIR, RB, at, key, put, timeline, wait

LAYOUT = {'origin': [12, -60, 24], 'spacing': [16, 16], 'columns': 4, 'wave_size': 16, 'center': [0, 0],
          'clear_box': [[-6, -3, -6], [6, 6, 6]], 'witness': [2, -60, 18]}
PROPERTIES = {'view_distance': 10, 'tick_distance': 8}
TNT = 'minecraft:tnt'
BOX = {'box': [[-6, -3, -6], [6, 6, 6]]}
MATERIALS = ('dirt', 'oak_planks', 'cobblestone', 'glass')
SAMPLES = 64


def item(fam: str, name, build: list, ticks: int, events: list, watch: list, actors: bool = True,
         settle: int = 4) -> dict:
    steps = [['build', [[o, b, st, True] for o, b, st in build]], wait(settle), timeline(ticks, events, watch)]
    if actors:
        steps[-1].append(BOX)
    return {'key': key(fam, name), 'family': fam, 'steps': steps}


def tnt_prime() -> list[dict]:
    """TNT at the cell origin primed by a redstone block beside it set at
    t0 (both parities), eight cells each (the primed actor's first motion is
    random); two TNT side by side, the second primed by the first's
    explosion."""
    fam = 'tnt_prime'
    items = []
    watch = [at(0), at(1)]
    for t0 in (4, 5):
        for n in range(8):
            items.append(item(fam, ['redstone', t0, n], [(at(0), TNT, {'explode_bit': False})], 110,
                              [[t0, put(at(1), RB)]], watch))
    for n in range(8):
        build = [(at(0), TNT, {'explode_bit': False}), (at(-3), TNT, {'explode_bit': False})]
        items.append(item(fam, ['chain', n], build, 150, [[4, put(at(1), RB)]], watch + [at(-3)]))
    return items


def blast(material: str) -> list[dict]:
    """`SAMPLES` cells: a 9 x 9 x 9 cube of `material` centred one block
    above the floor (the floor's grass and dirt replaced), TNT in the
    middle, primed by a redstone block set in the cube's top face above a
    column of the material (the column carries no power: the block is set
    beside the TNT through a hole)."""
    fam = f'tnt_blast_{material}'
    items = []
    name = f'minecraft:{material}'
    cube = [['fill', at(-4, -3, -4), at(4, 5, 4), name], ['set', at(0, 1, 0), TNT, {'explode_bit': False}, False]]
    watch = [at(x, y, z) for x in range(-4, 5) for y in range(-3, 6) for z in range(-4, 5)]
    for n in range(SAMPLES):
        # The redstone block replaces the material beside the TNT.
        steps = [['build', []], *cube, wait(4),
                 timeline(110, [[4, put(at(1, 1, 0), RB)]], [at(0, 1, 0)]), ['read', 'after', watch]]
        items.append({'key': key(fam, n), 'family': fam, 'steps': steps})
    return items


def tnt_actors() -> list[dict]:
    """Armour stands and iron golems at distance d (1-7 blocks east of a
    TNT on the floor), alone and behind a one-block glass or stone wall;
    their health and motion every tick from priming to well after the
    explosion."""
    fam = 'tnt_actors'
    items = []
    for mob in ('armor_stand', 'iron_golem'):
        for d in range(1, 8):
            for wall in (None, 'glass', 'stone'):
                if wall and d < 3:
                    continue
                build = [(at(0), TNT, {'explode_bit': False})]
                if wall:
                    build += [(at(d - 1, y), f'minecraft:{wall}', {}) for y in range(0, 3)]
                events = [[1, ['cmd', f'summon minecraft:{mob} {{cx}} {{y}} {{cz}}', at(d)]],
                          [4, put(at(0, 0, -1), RB)]]
                items.append(item(fam, [mob, d, wall], build, 110, events, [at(0)]))
    return items


def tnt_hits() -> list[dict]:
    """Explosion damage and knockback on mobs held still with slowness:
    iron golems (100 health, no knockback) and pigs (knockback) at
    distances 1.5-7.5 east and on a diagonal, alone and behind a glass or
    stone wall half or fully covering them."""
    fam = 'tnt_hits'
    items = []
    spots = [(d, 0) for d in range(1, 8)] + [(d, d) for d in range(1, 5)] + [(d, 0, 'raised') for d in (2, 4)]
    for mob in ('iron_golem', 'pig'):
        for spot in spots:
            dx, dz = spot[0], spot[1]
            raised = len(spot) > 2
            for wall in (None, 'glass', 'stone', 'low_stone'):
                if wall and (dx < 3 or dz):
                    continue
                build = [(at(0), TNT, {'explode_bit': False})]
                if raised:
                    build.append((at(dx, 0, dz), 'minecraft:stone', {}))
                if wall == 'low_stone':
                    build.append((at(dx - 1, 0), 'minecraft:stone', {}))
                elif wall:
                    build += [(at(dx - 1, y), f'minecraft:{wall}', {}) for y in range(0, 4)]
                y = 1 if raised else 0
                events = [[1, ['cmd', f'summon minecraft:{mob} {{cx}} {{y}} {{cz}}', at(dx, y, dz)]],
                          [2, ['cmd', f'effect @e[type={mob},x={{cx}},y={{y}},z={{cz}},r=1] slowness 30 255 true',
                               at(dx, y, dz)]],
                          [4, put(at(0, 0, -1), RB)]]
                items.append(item(fam, [mob, list(spot), wall], build, 100, events, [at(0)]))
    return items


FAMILIES = {'tnt_prime': tnt_prime, 'tnt_actors': tnt_actors, 'tnt_hits': tnt_hits}
FAMILIES.update({f'tnt_blast_{m}': (lambda m=m: blast(m)) for m in MATERIALS})
