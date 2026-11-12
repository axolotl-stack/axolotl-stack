"""Print tools/bds-sweep/scripts/unbreaking.txt: wear with Unbreaking 0-III
sampled on BDS, for `unbreaking_table.py`:

    python tools/bds-sweep/unbreaking_script.py > tools/bds-sweep/scripts/unbreaking.txt
    python tools/bds-sweep/script.py tools/bds-sweep/scripts/unbreaking.txt --out DIR --timeout 1800

`--kinds armour,use` prints only those sections (the committed table's armour
and item use come from such a run; its mining from the full script, whose
mining section is the same).

Three kinds of wear, each at every level:

- mining: a netherite pickaxe with Efficiency V and Haste II breaks stone with
  every input, so the player keeps breaking one position and the stone is put
  back every other tick (tile drops off); the pickaxe's slot is resent when
  it wears;
- armour: four diamond pieces with the same level, each equipped from its own
  hotbar slot, take `/damage @s 8 entity_attack` (2 points of wear each) once
  every 11 ticks, outside the 10 invulnerable ticks; BDS sends the worn
  pieces' damage (`PlayerArmorDamage`);
- item use: flint and steel lights fire on netherrack, which is cleared after
  each use; BDS resends the inventory after each. A fresh one, with its
  marker, every 40 uses (it lasts 64).

The scripted client does not apply the inventory change `/enchant` makes,
so its next item use claims the old stack and BDS refuses it, resending the
inventory: each enchanted armour piece is used twice (the second use equips
it) and each flint and steel once in the air before its uses.

Each section begins with a `/say unbreaking KIND LEVEL` marker, which
`unbreaking_table.py` reads back from the capture."""
import argparse
import sys

sys.stdout.reconfigure(newline='\n')
ARMOUR = ['diamond_helmet', 'diamond_chestplate', 'diamond_leggings', 'diamond_boots']
ARMOUR_SLOTS = ['slot.armor.head', 'slot.armor.chest', 'slot.armor.legs', 'slot.armor.feet']

parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
parser.add_argument('--breaks', type=int, default=800, help='mining samples per level I-III (level 0: an eighth)')
parser.add_argument('--hits', type=int, default=250, help='armour hits per level I-III (four pieces each)')
parser.add_argument('--uses', type=int, default=400, help='flint and steel uses per level I-III')
parser.add_argument('--kinds', default='mining,armour,use', help='the sections to print')
args = parser.parse_args()
kinds = args.kinds.split(',')

lines = [
    '# Unbreaking sampled on BDS 1.26.51; printed by tools/bds-sweep/unbreaking_script.py (see there).',
    '# The player (feet (0.5,-60,0.5), facing east) breaks the west face (4) of (1,-59,0).',
]
tick = 20


def at(order, gap=1):
    global tick
    lines.append(f'{tick} {order}')
    tick += gap


for order in ['/gamerule sendcommandfeedback false', '/gamerule domobspawning false',
              '/gamerule dodaylightcycle false', '/gamerule randomtickspeed 0', '/gamerule dotiledrops false',
              '/time set 6000', '/fill -4 -61 -4 4 -61 4 stone', '/fill -4 -60 -4 4 -55 4 air',
              '/gamemode survival @s', '/clear @s', '/tp @s 0.5 -60 0.5 -90 0',
              '/effect @s resistance 100000 3 true', '/effect @s regeneration 100000 4 true',
              '/effect @s haste 100000 1 true']:
    at(order)
tick += 10

for level in range(4 if 'mining' in kinds else 0):
    breaks = args.breaks if level else max(args.breaks // 8, 1)
    at('/clear @s')
    at('/replaceitem entity @s slot.hotbar 0 netherite_pickaxe', 3)
    at('/enchant @s efficiency 5', 3)
    if level:
        at(f'/enchant @s unbreaking {level}', 3)
    at(f'/say unbreaking mining {level}', 3)
    at('/setblock 1 -59 0 stone', 2)
    at('mine 1 -59 0 4 0', 2)
    for _ in range(breaks - 1):
        at('/setblock 1 -59 0 stone', 2)
    at('stop', 5)

for level in range(1, 4 if 'armour' in kinds else 1):
    at('/clear @s')
    for slot in ARMOUR_SLOTS:
        at(f'/replaceitem entity @s {slot} 0 air')
    for index, piece in enumerate(ARMOUR):
        at(f'/replaceitem entity @s slot.hotbar {index + 1} {piece}', 2)
    for index in range(len(ARMOUR)):
        at(f'select {index + 1}', 3)
        at(f'/enchant @s unbreaking {level}', 3)
        at('use', 5)
        at('use', 5)
    at('select 0', 3)
    at(f'/say unbreaking armour {level}', 3)
    for _ in range(args.hits):
        at('/damage @s 8 entity_attack', 11)
    tick += 5

at('/clear @s')
for slot in ARMOUR_SLOTS:
    at(f'/replaceitem entity @s {slot} 0 air')
at('/setblock 2 -60 0 netherrack', 2)
for level in range(1, 4 if 'use' in kinds else 1):
    for use in range(args.uses):
        # A fresh one every 40 uses: flint and steel lasts 64.
        if use % 40 == 0:
            at('/clear @s')
            at('/replaceitem entity @s slot.hotbar 0 flint_and_steel', 3)
            at(f'/enchant @s unbreaking {level}', 3)
            at('use', 3)
            at(f'/say unbreaking use {level}', 3)
        at('use-on 2 -60 0 1', 2)
        at('/setblock 2 -59 0 air', 2)
    tick += 5

at('exit')
print('\n'.join(lines))
