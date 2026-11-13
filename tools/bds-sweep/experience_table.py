"""Experience dropped per block break, from a capture of `scripts/ore-xp.txt` or
`scripts/ore-xp-2.txt` played against BDS (`script.py`):

    bds-sweep-dump <out>/client-capture/packets.jsonl --input-tick --name AddActor \\
        | python tools/bds-sweep/experience_table.py tools/bds-sweep/scripts/ore-xp.txt > table.json

Each `/setblock 1 -59 0 <block>` of the script starts one break; every
`minecraft:xp_orb` the server added before the next one (by the client tick of
the latest input, the dump's first column) belongs to it. The tool is
read from the script's `/clear`, `/give` and `/enchant` commands before the
break. Output: `{"script", "breaks": [{"block", "tool", "orbs": [values]}]}`
in script order.
"""
import bisect
import json
import re
import sys


def schedule(script_path):
    """(tick, block, tool) of every break in the script."""
    breaks = []
    item, enchants = None, []
    for line in open(script_path, encoding='utf-8'):
        parts = line.split()
        if not parts or parts[0].startswith('#'):
            continue
        tick, command = int(parts[0]), parts[1:]
        if command[:1] == ['/clear']:
            item, enchants = None, []
        elif command[:2] == ['/give', '@s']:
            item, enchants = command[2], []
        elif command[:2] == ['/enchant', '@s']:
            enchants.append(f'{command[2]}{command[3]}')
        elif command[:1] == ['/setblock'] and command[1:4] == ['1', '-59', '0']:
            tool = '+'.join([item or 'hand'] + sorted(enchants))
            breaks.append((tick, f'minecraft:{command[4]}', tool))
    return breaks


def main():
    # The table is committed with LF line ends, also from Windows.
    sys.stdout.reconfigure(newline='\n')
    breaks = schedule(sys.argv[1])
    ticks = [tick for tick, _, _ in breaks]
    orbs = [[] for _ in breaks]
    for line in sys.stdin:
        if '"minecraft:xp_orb"' not in line:
            continue
        label = int(line.split()[0])
        value = re.search(r'DataItemEntry \{ id: 15, payload: DataItemIntPayload\(DataItemIntPayload '
                          r'\{ type_: Int, value: (-?\d+)', line)
        index = bisect.bisect_right(ticks, label) - 1
        if index >= 0 and value:
            orbs[index].append(int(value.group(1)))
    # One break per line, so the committed table diffs readably.
    rows = [json.dumps({'block': block, 'tool': tool, 'orbs': values}, separators=(',', ':'))
            for (_, block, tool), values in zip(breaks, orbs)]
    script = json.dumps(sys.argv[1].replace('\\', '/'))
    sys.stdout.write('{"script":' + script + ',"breaks":[\n' + ',\n'.join(rows) + '\n]}\n')


if __name__ == '__main__':
    main()
