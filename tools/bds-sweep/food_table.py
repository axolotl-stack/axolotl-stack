"""What eating each food of `scripts/foods.txt` did on BDS, from a capture of
the script played with `script.py`:

    bds-sweep-client --decode <out>/client-capture/packets.jsonl \\
        --only UpdateAttributes,MobEffect,CompletedUsingItem,InventorySlot,InventoryContent,LevelSoundEvent \\
        | python tools/bds-sweep/food_table.py tools/bds-sweep/scripts/foods.txt > table.json

A food runs from its `use` to the script's `/effect @s clear` after it; what
arrived in between (by the client tick of the latest input, the decoder's
first column; effects, completion, slot and sounds until 3 ticks before the
clear, whose removals land about a tick after it) belongs to it:
- `before`: the player's food level, saturation and exhaustion at the start
  of the tick the eat completed in (the values BDS last sent; natural
  regeneration while eating is already in them);
- `after`: its food level and saturation at the end of the tick the eat
  completed in (`null` when BDS did not let it be eaten), and `end`: the
  same at the script's closing `/effect @s clear` (what effects the food
  gave did with them since);
- `added`: effects BDS added `[id, amplifier, duration ticks, particles]`, and
  `removed`: effect ids it removed;
- `completed`: `CompletedUsingItem` `[item network id, use method]`;
- `slot`: the network id hotbar slot 0 held at the end (0 for nothing);
- `sounds`: the level sounds, without `pop` (picking up the given item).

Output: `{"script", "foods": [...]}`, one food per line, in script order.
"""
import json
import re
import sys

PLAYER = 1
# Ticks before the closing `/effect @s clear` that still belong to the eat.
CLEAR_MARGIN = 3
ATTRIBUTE = re.compile(r'current_value: ([-0-9.e]+), default_min_value: [-0-9.e]+, default_max_value: [-0-9.e]+, '
                       r'default_value: [-0-9.e]+, name: "minecraft:player\.(hunger|saturation|exhaustion)"')
ITEM_ID = re.compile(r'CerealizerNetworkItemStackDescriptorSerializedData \{ id: (-?\d+)')


def schedule(script_path):
    """`(use tick, end tick, item, data, after)` per food: `end` is the tick of
    the `/effect @s clear` after the use."""
    foods = []
    given = None
    pending = None
    for line in open(script_path, encoding='utf-8'):
        parts = line.split()
        if not parts or parts[0].startswith('#'):
            continue
        tick, command = int(parts[0]), parts[1:]
        if command[:2] == ['/give', '@s']:
            given = (command[2], int(command[4]))
        elif command == ['use'] and given:
            pending = (tick, given)
            given = None
        elif command == ['/effect', '@s', 'clear'] and pending:
            (use, (item, data)) = pending
            foods.append((use, tick, item, data))
            pending = None
    return foods


def main():
    sys.stdout.reconfigure(newline='\n')
    foods = schedule(sys.argv[1])
    rows = [{'item': item, 'data': data, 'before': None, 'after': None, 'end': None, 'added': [], 'removed': [],
             'completed': None, 'slot': None, 'sounds': []} for _, _, item, data in foods]
    state = {}
    index = 0
    # The tick the current food's eat completed in, until its values are read.
    completed_at = None
    # The player's values as they stood when the current tick began.
    tick_label, at_tick_start = None, {}
    for line in sys.stdin:
        parts = line.split(' ', 4)
        if len(parts) < 5 or not parts[0].isdigit() or parts[2] != 's2c':
            continue
        label, name, body = int(parts[0]), parts[3], parts[4]
        if label != tick_label:
            tick_label, at_tick_start = label, dict(state)
        # The eat's food level and saturation: as they stood at the end of the
        # tick it completed in.
        if completed_at is not None and label > completed_at:
            rows[index]['after'] = {key: state.get(key) for key in ('hunger', 'saturation')}
            completed_at = None
        # Past the end of the current food: close it.
        while index < len(foods) and label > foods[index][1]:
            rows[index]['end'] = {key: state.get(key) for key in ('hunger', 'saturation')}
            completed_at = None
            index += 1
        # The script's `/effect @s clear` lands about a tick after its own
        # tick; the eat's effects and sounds are the ones before it.
        inside = index < len(foods) and foods[index][0] < label <= foods[index][1] - CLEAR_MARGIN
        if name == 'UpdateAttributes' and f'actor_runtime_id: {PLAYER} }}' in body:
            for value, attribute in ATTRIBUTE.findall(body):
                state[attribute] = float(value)
        elif not inside:
            continue
        elif name == 'MobEffect':
            event = re.search(r'event_id: (\w+)', body).group(1)
            effect = int(re.search(r'effect_id: (-?\d+)', body).group(1))
            if event == 'Add':
                amplifier = int(re.search(r'effect_amplifier: (-?\d+)', body).group(1))
                duration = int(re.search(r'effect_duration_ticks: (-?\d+)', body).group(1))
                particles = re.search(r'show_particles: (\w+)', body).group(1) == 'true'
                rows[index]['added'].append([effect, amplifier, duration, particles])
            elif event == 'Remove':
                rows[index]['removed'].append(effect)
        elif name == 'CompletedUsingItem':
            item = int(re.search(r'item_id: (-?\d+)', body).group(1))
            method = int(re.search(r'item_use_method: (-?\d+)', body).group(1))
            rows[index]['completed'] = [item, method]
            rows[index]['before'] = at_tick_start
            completed_at = label
        elif name == 'InventorySlot' and 'container_id: 0, slot: 0,' in body:
            rows[index]['slot'] = int(ITEM_ID.search(body.split('item: ', 1)[1]).group(1))
        elif name == 'InventoryContent' and 'container_id: 0,' in body:
            rows[index]['slot'] = int(ITEM_ID.search(body).group(1))
        elif name == 'LevelSoundEvent':
            sound = re.search(r'sound_event: "([^"]*)"', body).group(1)
            if sound != 'pop':
                rows[index]['sounds'].append(sound)
    if completed_at is not None:
        rows[index]['after'] = {key: state.get(key) for key in ('hunger', 'saturation')}
    if index < len(foods):
        rows[index]['end'] = {key: state.get(key) for key in ('hunger', 'saturation')}
    lines = [json.dumps(row, separators=(',', ':')) for row in rows]
    script = json.dumps(sys.argv[1].replace('\\', '/'))
    sys.stdout.write('{"script":' + script + ',"foods":[\n' + ',\n'.join(lines) + '\n]}\n')


if __name__ == '__main__':
    main()
