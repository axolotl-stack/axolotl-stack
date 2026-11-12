"""Distil `sweep.py spider` / `sweep.py enderman` runs (`--client-capture`)
into a committed mob fixture.

    python tools/bds-sweep/mob_fixture.py OUT.json LABEL=SWEEP_DIR [LABEL=SWEEP_DIR ...]

`LABEL` names the run (its difficulty, say: `normal`); items are keyed
`<label>/<item>` and a later run's item replaces one of the same key.
Decodes each run's `client/packets.jsonl` with `bds-extractor --decode`
(cargo build -p bds-extractor first). Per item it keeps the spec (where the
mob and the player stood, whether the mob was frozen, the setup commands and
the timed actions), the mob's feet and ground contact on every tick it moved,
the player's feet whenever they changed, and the observed events, each with
its tick (`t`, the sample whose `bds-sweep:mark` precedes the packet): the
mob's `TARGET` (whether it is the player), every change of its flag bits and
of its other actor data, its sounds and actor events, its `RemoveActor`, the
player's `MobEffect`s, the block updates, the items that appeared, the level
events and the damage the job recorded. Only observed values are kept: no
packet bytes, no login data.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
EXTRACTOR = REPO / 'target' / 'debug' / ('bds-extractor.exe' if sys.platform == 'win32' else 'bds-extractor')
PACKETS = ['TextPacket', 'SetActorDataPacket', 'AddActorPacket', 'LevelSoundEventPacket', 'LevelEventPacket',
           'RemoveActorPacket', 'MobEffectPacket', 'ActorEventPacket', 'UpdateBlockPacket', 'AddItemActorPacket']
TARGET = 6
# Actor data that changes on its own every tick or two and says nothing here:
# the flag words (kept as bit changes), the air supply and the target.
SKIPPED_DATA = {0, 7, TARGET, 92}


def decoded(capture: Path) -> list[dict]:
    out = capture.with_name('decoded-mob.jsonl')
    subprocess.run([str(EXTRACTOR), '--decode', str(capture), '--only', ','.join(PACKETS), '--output', str(out)],
                   check=True, capture_output=True)
    return [json.loads(line) for line in out.read_text(encoding='utf-8').splitlines() if line.strip()]


def marked(records: list[dict]) -> dict[str, list[tuple[int, dict]]]:
    """Every record after the first chat mark, grouped by the mark's item."""
    out: dict[str, list[tuple[int, dict]]] = {}
    key, t = None, None
    for record in records:
        if record['name'] == 'TextPacket':
            message = json.dumps(record['packet'])
            if 'bds-sweep:mark ' in message:
                rest = message[message.index('bds-sweep:mark ') + len('bds-sweep:mark '):].split('"')[0]
                key, tick = rest.rsplit(' ', 1)
                t = int(tick)
            continue
        if key is not None:
            out.setdefault(key, []).append((t, record))
    return out


def data_items(entries: list[dict]) -> dict[int, object]:
    out = {}
    for entry in entries:
        (_, payload), = entry['payload'].items()
        out[entry['id']] = payload.get('value', payload)
    return out


def flag_word(items: dict[int, object]) -> int | None:
    if 0 not in items and 92 not in items:
        return None
    return (items.get(0, 0) & (2**64 - 1)) | ((items.get(92, 0) & (2**64 - 1)) << 64)


def rounded(values, digits=4):
    return [round(v, digits) for v in values]


def distil(result: dict, rows: list[tuple[int, dict]], label: str) -> dict:
    runtime_id = result['runtime_id']
    player_id = result['player_runtime_id']
    unique_id = None
    events: list[dict] = []
    flags = None
    data: dict[int, object] = {}
    for t, record in rows:
        packet, name = record['packet'], record['name']
        if name == 'AddActorPacket' and packet['target_runtime_id']['actor_runtime_id'] == runtime_id:
            unique_id = packet['target_actor_id']['actor_unique_id']
            items = data_items(packet['actor_data']['data'])
            flags = flag_word(items)
            data = {k: v for k, v in items.items() if k not in SKIPPED_DATA}
            events.append({'t': t, 'event': 'add'})
        elif name == 'SetActorDataPacket' and packet['target_runtime_id']['actor_runtime_id'] == runtime_id:
            items = data_items(packet['actor_data']['data'])
            if TARGET in items:
                events.append({'t': t, 'event': 'target', 'set': items[TARGET] != 0})
            word = flag_word(items)
            if word is not None and flags is not None:
                for bit in range(128):
                    old, new = flags >> bit & 1, word >> bit & 1
                    if old != new:
                        events.append({'t': t, 'event': 'flag', 'bit': bit, 'set': bool(new)})
                flags = word
            for key, value in items.items():
                if key not in SKIPPED_DATA and data.get(key) != value:
                    events.append({'t': t, 'event': 'data', 'id': key, 'value': value})
                    data[key] = value
        elif name == 'LevelSoundEventPacket' and unique_id is not None and packet['actor_unique_id'] == unique_id:
            events.append({'t': t, 'event': 'sound', 'sound': packet['sound_event'],
                           'at': rounded(packet['position'].values())})
        elif name == 'ActorEventPacket' and packet['target_runtime_id']['actor_runtime_id'] == runtime_id:
            events.append({'t': t, 'event': 'actor_event', 'id': packet['event_id'], 'data': packet['data']})
        elif name == 'RemoveActorPacket' and packet['target_actor_id']['actor_unique_id'] == unique_id:
            events.append({'t': t, 'event': 'removed'})
        elif name == 'MobEffectPacket' and packet['target_runtime_id']['actor_runtime_id'] == player_id:
            events.append({'t': t, 'event': 'effect', 'effect': packet['effect_id'], 'op': packet['event_id'],
                           'amplifier': packet['effect_amplifier'], 'duration': packet['effect_duration_ticks']})
        elif name == 'UpdateBlockPacket':
            pos = packet['block_position']
            events.append({'t': t, 'event': 'block', 'pos': [pos['x'], pos['y'], pos['z']],
                           'network_id': packet['block_runtime_id'], 'layer': packet['layer']})
        elif name == 'AddItemActorPacket':
            item = packet['item']
            events.append({'t': t, 'event': 'item', 'network_id': item.get('network_id'),
                           'count': item.get('count'), 'at': rounded(packet['position'].values(), 3)})
        elif name == 'LevelEventPacket':
            events.append({'t': t, 'event': 'level_event', 'id': packet['event_id'], 'data': packet['data'],
                           'at': rounded(packet['position'].values(), 3)})
    spec = result['spec']
    moves, last = [], None
    feet, last_feet = [], None
    for sample in result['samples']:
        mob = sample.get('mob')
        if mob is not None:
            here = rounded(mob['pos'], 5) + [mob['ground']]
            if here[:3] != (last or [None] * 3)[:3] or sample['t'] == 0:
                moves.append([sample['t']] + here)
            last = here
        player = rounded(sample['player']['pos'], 4)
        if player != last_feet:
            feet.append([sample['t']] + player)
            last_feet = player
    first = result['first_tick']
    return {
        'key': f'{label}/{result["key"].split("|")[1]}',
        'mob': result['mob'],
        'at': spec['at'],
        'player': spec['player'],
        'frozen': spec['frozen'],
        'setup': spec.get('setup', []),
        'actions': [{'t': a['t'], 'commands': a['commands'], 'order': a.get('order')}
                    for a in spec.get('actions', [])],
        'ticks': spec['ticks'],
        # [t, x, y, z, on_ground] on every tick the mob moved.
        'moves': moves,
        # [t, x, y, z] of the player's feet whenever they changed.
        'player_feet': feet,
        'damage': [{'t': d[0] - first, 'actor': d[1], 'amount': round(d[2], 4), 'cause': d[3]}
                   for d in result['damage']],
        'events': events,
    }


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    out = Path(sys.argv[1])
    items: dict[str, dict] = {}
    for arg in sys.argv[2:]:
        label, sweep = arg.split('=', 1)
        sweep = Path(sweep)
        rows = marked(decoded(sweep / 'client' / 'packets.jsonl'))
        for line in (sweep / 'results.jsonl').read_text(encoding='utf-8').splitlines():
            result = json.loads(line)
            item = distil(result, rows.get(result['key'], []), label)
            items[item['key']] = item
    fixture = {'source': 'BDS 1.26.51, tools/bds-sweep/sweep.py --client-capture, distilled by '
                         'tools/bds-sweep/mob_fixture.py',
               'items': list(items.values())}
    out.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(fixture, separators=(',', ':'))
    text = text.replace('{"key"', '\n{"key"').replace('{"t"', '\n{"t"').replace('],[', '],\n[')
    out.write_text(text + '\n', encoding='utf-8', newline='\n')
    return 0


if __name__ == '__main__':
    sys.exit(main())
