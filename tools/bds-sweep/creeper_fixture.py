"""Distil `sweep.py creeper --client-capture` runs into the committed creeper
fixture (`crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-creeper.json`).

    python tools/bds-sweep/creeper_fixture.py OUT.json SWEEP_DIR [SWEEP_DIR ...]

Decodes each run's `client/packets.jsonl` with `bds-extractor --decode`
(cargo build -p bds-extractor first); an item of a later sweep replaces one
of the same name from an earlier sweep. Per item it keeps the creeper's and
the player's feet on every tick, whether the player was a target
(survival), the commands and orders run and their ticks, and the
creeper's observed events, each with its tick (`t`, the sample whose
`bds-sweep:mark` precedes the packet): its `TARGET` actor data, every
change of its `IGNITED`, `POWERED` and `MOVING` flags, every `FUSE_TIME`
value, its sounds (`mob.warning`, `ignite`, `fuse`), the explosion's level
event (2025, with its data) and its `RemoveActor`. Only observed values are
kept: no packet bytes, no login data.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
EXTRACTOR = REPO / 'target' / 'debug' / ('bds-extractor.exe' if sys.platform == 'win32' else 'bds-extractor')
PACKETS = ['TextPacket', 'SetActorDataPacket', 'AddActorPacket', 'LevelSoundEventPacket', 'LevelEventPacket',
           'RemoveActorPacket']
FLAGS = {'ignited': 10, 'powered': 9, 'moving': 34}
TARGET = 6
FUSE_TIME = 55
EXPLOSION = 2025
SOUNDS = ('mob.warning', 'ignite', 'fuse', 'explode')


def decoded(capture: Path) -> list[dict]:
    out = capture.with_name('decoded-creeper.jsonl')
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
        out[entry['id']] = payload['value']
    return out


def distil(result: dict, rows: list[tuple[int, dict]]) -> dict:
    runtime_id = result['runtime_id']
    unique_id = None
    events: list[dict] = []
    flags = None
    for t, record in rows:
        packet, name = record['packet'], record['name']
        if name == 'AddActorPacket' and packet['target_runtime_id']['actor_runtime_id'] == runtime_id:
            unique_id = packet['target_actor_id']['actor_unique_id']
            flags = data_items(packet['actor_data']['data']).get(0)
            events.append({'t': t, 'event': 'add', 'powered': bool(flags >> FLAGS['powered'] & 1)})
        elif name == 'SetActorDataPacket' and packet['target_runtime_id']['actor_runtime_id'] == runtime_id:
            items = data_items(packet['actor_data']['data'])
            if TARGET in items:
                events.append({'t': t, 'event': 'target', 'set': items[TARGET] != 0})
            if 0 in items and flags is not None:
                for flag, bit in FLAGS.items():
                    old, new = flags >> bit & 1, items[0] >> bit & 1
                    if old != new:
                        events.append({'t': t, 'event': flag, 'set': bool(new)})
                flags = items[0]
            if FUSE_TIME in items:
                events.append({'t': t, 'event': 'fuse_time', 'value': items[FUSE_TIME]})
        elif name == 'LevelSoundEventPacket' and packet['sound_event'] in SOUNDS \
                and packet['actor_unique_id'] in (unique_id, -1):
            pos = packet['position']
            events.append({'t': t, 'event': 'sound', 'sound': packet['sound_event'],
                           'at': [pos['x'], pos['y'], pos['z']]})
        elif name == 'LevelEventPacket' and packet['event_id'] == EXPLOSION:
            pos = packet['position']
            events.append({'t': t, 'event': 'explosion', 'data': packet['data'],
                           'at': [pos['x'], pos['y'], pos['z']]})
        elif name == 'RemoveActorPacket' and packet['target_actor_id']['actor_unique_id'] == unique_id:
            events.append({'t': t, 'event': 'removed'})
    spec = result['spec']
    return {
        'key': result['key'].split('|')[1],
        'frozen': spec['frozen'],
        'target': any('gamemode survival' in line for line in spec['setup']),
        'griefing': not any('mobgriefing false' in line for line in spec['setup']),
        'actions': [{'t': a['t'], 'commands': a['commands'], 'order': a.get('order')} for a in spec['actions']],
        'ticks': [{'t': s['t'], 'creeper': s['mob']['pos'] if s.get('mob') else None, 'player': s['player']['pos']}
                  for s in result['samples']],
        'events': events,
    }


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    out = Path(sys.argv[1])
    fixture = {'source': 'BDS 1.26.51, tools/bds-sweep/sweep.py creeper --client-capture, distilled by '
                         'tools/bds-sweep/creeper_fixture.py',
               'items': []}
    # A later sweep's item replaces an earlier one of the same name (a rerun).
    items: dict[str, dict] = {}
    for sweep in map(Path, sys.argv[2:]):
        rows = marked(decoded(sweep / 'client' / 'packets.jsonl'))
        for line in (sweep / 'results.jsonl').read_text(encoding='utf-8').splitlines():
            result = json.loads(line)
            item = distil(result, rows.get(result['key'], []))
            items[item['key']] = item
    fixture['items'] = list(items.values())
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(fixture, separators=(',', ':')).replace('{"t"', '\n{"t"') + '\n', encoding='utf-8',
                   newline='\n')
    return 0


if __name__ == '__main__':
    sys.exit(main())
