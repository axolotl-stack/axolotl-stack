"""Round 5: infested blocks broken by the scripted survival player, for the
`scenarios` job on the circuit layout (`circuits.LAYOUT`).

The player stands two blocks south of an infested block at x=1, looking at
it, and breaks it (`mine`, the order the `mining` job gives) with a bare hand
or a tool; the block and the actors in the cell (the silverfish it lets out)
are read every tick.

- `infested_break`: every infested block by hand; infested stone with a
  wooden pickaxe, with a silk touch pickaxe, and with `doTileDrops` off.
"""

from __future__ import annotations

from circuits import at, key, timeline, wait

BOX = {'box': [[-1, -1, -2], [20, 4, 3]]}
BLOCKS = ['minecraft:infested_stone', 'minecraft:infested_cobblestone', 'minecraft:infested_stone_bricks',
          'minecraft:infested_mossy_stone_bricks', 'minecraft:infested_cracked_stone_bricks',
          'minecraft:infested_chiseled_stone_bricks', 'minecraft:infested_deepslate']
WOODEN = 'minecraft:wooden_pickaxe'
SILK = 'minecraft:diamond_pickaxe'


def broken(fam: str, name, block: str, hand: str | None, before: list | None = None, ench: str = '') -> dict:
    give = (f'replaceitem entity {{player}} slot.weapon.mainhand 0 {hand} 1' if hand
            else 'replaceitem entity {player} slot.weapon.mainhand 0 minecraft:air')
    steps = [['build', [[at(1), block, {}, True]]],
             ['cmd', 'gamemode survival {player}', at(1)],
             ['cmd', give, at(1)]]
    if ench:
        steps.append(['cmd', f'enchant {{player}} {ench}', at(1)])
    steps += [['cmd', step, at(1)] for step in before or []]
    steps += [['cmd', 'tp {player} {cx} {y} {cz} 180 25', at(1, 0, 2)], wait(10)]
    line = timeline(160, [[2, ['mine', at(1), 3]], [120, ['stop']]], [at(1)])
    line.append(BOX)
    steps.append(line)
    if before:
        steps.append(['cmd', 'gamerule dotiledrops true', at(1)])
    return {'key': key(fam, name), 'family': fam, 'steps': steps}


def infested_break() -> list[dict]:
    fam = 'infested_break'
    items = [broken(fam, [block, 'hand'], block, None) for block in BLOCKS]
    items.append(broken(fam, ['minecraft:infested_stone', 'wooden_pickaxe'], 'minecraft:infested_stone', WOODEN))
    items.append(broken(fam, ['minecraft:infested_stone', 'silk_touch'], 'minecraft:infested_stone', SILK,
                        ench='silk_touch 1'))
    items.append(broken(fam, ['minecraft:infested_stone', 'no_tile_drops'], 'minecraft:infested_stone', None,
                        before=['gamerule dotiledrops false']))
    return items


FAMILIES = {'infested_break': infested_break}
