"""Distil `sweep.py mooshroom --client-capture` runs into the committed
mooshroom fixture
(`crates/unastar/src/entity/mob/interaction/testdata/bds-1.26.51-mooshroom.json`).

    python tools/bds-sweep/mooshroom_fixture.py OUT.json SWEEP_DIR [SWEEP_DIR ...]

Decodes each run's `client/packets.jsonl` with `bds-extractor --decode`
(cargo build -p bds-extractor first). Per item it keeps the mooshroom's
feet and facing, the events run on it before the interactions, and for
every interaction that went through (BDS swung the player's arm): the
player's eyes and held item, the particle level event, the held slot
afterwards, the sounds, the mark variant actor data, the dropped items and
the actor the mooshroom turned into, each with its tick (`t`, the sample
whose `bds-sweep:mark` precedes it). Item ids are named through the
capture's item registry. Only observed values are kept: no packet bytes,
no login data.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
EXTRACTOR = REPO / 'target' / 'debug' / ('bds-extractor.exe' if sys.platform == 'win32' else 'bds-extractor')
PACKETS = ['TextPacket', 'ItemRegistryPacket', 'InventoryTransactionPacket', 'AnimatePacket', 'LevelEventPacket',
           'LevelSoundEventPacket', 'InventoryContentPacket', 'SetActorDataPacket', 'AddItemActorPacket',
           'RemoveActorPacket', 'AddActorPacket']
MARK_VARIANT = 43
# Ticks after a swing whose packets still belong to its interaction.
WINDOW = 3


def decoded(capture: Path) -> list[dict]:
    out = capture.with_name('decoded-mooshroom.jsonl')
    subprocess.run([str(EXTRACTOR), '--decode', str(capture), '--only', ','.join(PACKETS), '--output', str(out)],
                   check=True, capture_output=True)
    return [json.loads(line) for line in out.read_text(encoding='utf-8').splitlines() if line.strip()]


def vec(value: dict) -> list[float]:
    return [value['x'], value['y'], value['z']]


def marked(records: list[dict]) -> list[tuple[str, int, dict]]:
    """Every record after the first chat mark, with the mark's item and tick."""
    out, key, t = [], None, None
    for record in records:
        if record['name'] == 'TextPacket':
            message = json.dumps(record['packet'])
            if 'bds-sweep:mark ' in message:
                rest = message[message.index('bds-sweep:mark ') + len('bds-sweep:mark '):].split('"')[0]
                key, tick = rest.rsplit(' ', 1)
                t = int(tick)
            continue
        if key is not None:
            out.append((key, t, record))
    return out


def stack(item: dict, names: dict[int, str]) -> dict | None:
    if item['id'] == 0:
        return None
    return {'item': names[item['id']], 'aux': item['auxvalue'], 'count': item['stacksize']}


def distil(result: dict, rows: list[tuple[int, dict]], names: dict[int, str]) -> dict:
    mob_runtime = result['runtime_id']
    events = [line.split()[-1] for action in result['spec']['actions'] for line in action['commands']
              if line.startswith('event entity')]
    item = {'key': result['key'], 'at': result['spec']['at'], 'events': events, 'interactions': []}
    unique = None
    for t, record in rows:
        if record['name'] == 'AddActorPacket' and record['packet']['target_runtime_id']['actor_runtime_id'] == \
                mob_runtime:
            unique = record['packet']['target_actor_id']['actor_unique_id']
            break
    used = None
    current = None
    for t, record in rows:
        name, packet = record['name'], record['packet']
        if name == 'InventoryTransactionPacket' and record['dir'] == 'c2s':
            use = packet['transaction'].get('ItemUseOnActorInventoryTransaction')
            if use is not None:
                used = {'eyes': vec(use['from_position']), 'held': stack(use['item'], names)}
            continue
        if name == 'AnimatePacket' and packet.get('swing_source') == 'interact':
            current = {'t': t, **(used or {}), 'swing': True, 'particles': [], 'sounds': [], 'held_after': None,
                       'mark_variant': None, 'drops': [], 'transformed': None}
            item['interactions'].append(current)
            continue
        if current is None or t > current['t'] + WINDOW:
            continue
        if name == 'LevelSoundEventPacket' and packet['actor_unique_id'] == unique:
            current['sounds'].append({'t': t, 'sound': packet['sound_event'], 'position': vec(packet['position']),
                                      'baby': packet['is_baby']})
        elif name == 'InventoryContentPacket' and packet['container_id'] == 0:
            current['held_after'] = stack(packet['slots'][0], names) or {'item': None, 'aux': 0, 'count': 0}
        elif name == 'SetActorDataPacket' and packet['target_runtime_id']['actor_runtime_id'] == mob_runtime:
            for row in packet['actor_data']['data']:
                if row['id'] == MARK_VARIANT:
                    current['mark_variant'] = {'t': t, 'value': next(iter(row['payload'].values()))['value']}
        elif name == 'AddItemActorPacket':
            current['drops'].append({'t': t, **stack(packet['item'], names)})
        elif name == 'RemoveActorPacket' and packet['target_actor_id']['actor_unique_id'] == unique:
            current['transformed'] = {'t': t}
        elif name == 'AddActorPacket' and current['transformed'] is not None:
            current['transformed'].update(actor=packet['actor_type'], position=vec(packet['position']),
                                          yaw=packet['rotation']['y'])
    # A particle goes out just before its interaction's swing, in the same
    # batch.
    for t, record in rows:
        if record['name'] != 'LevelEventPacket' or record['packet']['event_id'] != 2009:
            continue
        owner = next((i for i in item['interactions'] if i['t'] == t), None)
        if owner is not None:
            owner['particles'].append({'t': t, 'data': record['packet']['data'],
                                       'position': vec(record['packet']['position'])})
    return item


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    out = Path(sys.argv[1])
    items = []
    for sweep in map(Path, sys.argv[2:]):
        records = decoded(sweep / 'client' / 'packets.jsonl')
        names = {}
        for record in records:
            if record['name'] == 'ItemRegistryPacket':
                names = {row['item_id']: row['item_name'] for row in record['packet']['item_data']}
        by_key: dict[str, list[tuple[int, dict]]] = {}
        for key, t, record in marked(records):
            by_key.setdefault(key, []).append((t, record))
        for line in (sweep / 'results.jsonl').read_text(encoding='utf-8').splitlines():
            result = json.loads(line)
            items.append(distil(result, by_key.get(result['key'], []), names))
    fixture = {
        'source': 'BDS 1.26.51, tools/bds-sweep/sweep.py mooshroom --client-capture (--items basic, geometry)',
        'items': items,
    }
    out.write_text(json.dumps(fixture, indent=1) + '\n', encoding='utf-8', newline='\n')
    print(f'{len(items)} items -> {out}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
