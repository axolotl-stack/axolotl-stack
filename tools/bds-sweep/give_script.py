"""Print tools/bds-sweep/scripts/give.txt: `/give` of every item of the
native item registry to an empty inventory, then `/clear`, so a capture holds
the stack BDS puts in hotbar slot 0 for each (its `InventorySlot`, user data
included):

    python tools/bds-sweep/give_script.py > tools/bds-sweep/scripts/give.txt

`give_table.py` turns the capture into the committed table."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import native  # noqa: E402

sys.stdout.reconfigure(newline='\n')
# Client inputs between a /give and its /clear, and between items.
GAP = 3
FIRST = 40


def main() -> None:
    print('# Every item of the native registry given to hotbar slot 0 of an empty')
    print('# inventory, then cleared (printed by tools/bds-sweep/give_script.py).')
    print('# Play with `python tools/bds-sweep/script.py tools/bds-sweep/scripts/give.txt')
    print('# --out DIR`; cut with `give_table.py`. Ticks are the client\'s inputs.')
    print('20 /gamerule sendcommandfeedback false')
    print('21 /gamerule domobspawning false')
    print('22 /gamerule dodaylightcycle false')
    print('23 /gamemode survival @s')
    print('24 /clear @s')
    tick = FIRST
    for item in native.items():
        print(f'{tick} /give @s {item.removeprefix("minecraft:")}')
        print(f'{tick + GAP} /clear @s')
        tick += 2 * GAP
    print(f'{tick + 10} exit')


if __name__ == '__main__':
    main()
