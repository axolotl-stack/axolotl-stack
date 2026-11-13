"""Cut the `/give` table out of a capture of `scripts/give.txt`:

    bds-sweep-client --decode DIR/client-capture/packets.jsonl --only CommandRequest,InventoryContent \\
        | python tools/bds-sweep/give_table.py > crates/unastar/src/command/exec/testdata/bds-give.json

For every `/give @s <item>` it keeps hotbar slot 0 of the first player
inventory (`InventoryContent` of container 0) BDS sent after it: item id,
count, aux value, block runtime id and the user data (hex). An item whose
slot stays empty is listed as `refused`."""
import ast
import json
import re
import sys

GIVE = re.compile(r'command: "/give @s (\S+)"')
SLOT = re.compile(r'slots: \[\w+ \{ id: (-?\d+), stacksize: (\d+), auxvalue: (\d+), net_id_variant: [^,]*, '
                  r'block_runtime_id: (\d+), user_data_buffer: (b"[^"]*") \}')


def table(lines):
    given, refused = {}, []
    current = None
    for line in lines:
        command = GIVE.search(line)
        if command:
            current = 'minecraft:' + command.group(1)
            continue
        if current is None or ' InventoryContent ' not in line or 'container_id: 0,' not in line:
            continue
        slot = SLOT.search(line)
        if not slot:
            continue
        item, count, aux, block, user_data = slot.groups()
        if item == '0':
            refused.append(current)
        else:
            given[current] = {'id': int(item), 'count': int(count), 'aux': int(aux), 'block': int(block),
                              'user_data': ast.literal_eval(user_data).hex()}
        current = None
    return given, sorted(refused)


def main():
    given, refused = table(sys.stdin)
    sys.stdout.reconfigure(newline='\n')
    doc = {
        '//': 'BDS 1.26.51: hotbar slot 0 of the inventory BDS resent after `/give @s <item>` for every item of '
              'the native registry (capture of tools/bds-sweep/scripts/give.txt, 2026-11-11, cut by '
              'tools/bds-sweep/give_table.py). `refused`: items /give does not give.',
        'given': dict(sorted(given.items())),
        'refused': refused,
    }
    print(json.dumps(doc, indent=0))


if __name__ == '__main__':
    main()
