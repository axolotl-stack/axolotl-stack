"""Distil `sweep.py range --observer` runs into the committed send-range
fixture (`crates/unastar/src/entity/testdata/bds-1.26.51-send-range.json`).

    python tools/bds-sweep/range_fixture.py OUT.json VIEW,SIM:SWEEP_DIR [VIEW,SIM:SWEEP_DIR ...]

`VIEW,SIM` are the view and simulation distances the run used
(`--view-distance`, `--tick-distance`). Per item it keeps each marker's kind
and position (the sweep player at the centre is a `minecraft:player`
marker), the observer's position at each step, and every add and remove
the observer got for a marker: the step and the tick within the step (from
the `bds-sweep:mark` chat line it followed). The removals of the item's end
(the markers are despawned after its last mark) are left out. Only observed values are kept.
"""

from __future__ import annotations

import json
import re
import struct
import sys
from pathlib import Path

MARK = re.compile(rb'bds-sweep:mark (\S+) (\d+) (\d+)')


class Reader:
    def __init__(self, data: bytes) -> None:
        self.data, self.at = data, 0

    def u8(self) -> int:
        value = self.data[self.at]
        self.at += 1
        return value

    def varint(self) -> int:
        value = shift = 0
        while True:
            byte = self.u8()
            value |= (byte & 0x7F) << shift
            shift += 7
            if not byte & 0x80:
                return value

    def zigzag(self) -> int:
        value = self.varint()
        return (value >> 1) ^ -(value & 1)


def actor_events(capture: Path):
    """(mark, kind, unique id or runtime id) for the observer's AddPlayer (12),
    AddActor (13), RemoveActor (14) and AddItemActor (15), in arrival order;
    `mark` is the last `(item, step, tick)` chat mark before the packet."""
    mark = None
    for line in capture.read_text(encoding='utf-8').splitlines():
        record = json.loads(line)
        if record['dir'] != 's2c':
            continue
        data = bytes.fromhex(record['hex'])
        if record['id'] == 9:
            found = MARK.search(data)
            if found:
                mark = (found.group(1).decode(), int(found.group(2)), int(found.group(3)))
            continue
        if record['id'] not in (12, 13, 14, 15) or mark is None:
            continue
        reader = Reader(data)
        reader.varint()  # header
        if record['id'] == 12:
            reader.at += 16  # uuid
            size = reader.varint()
            reader.at += size  # name
            yield mark, 'add', ('runtime', reader.varint())
        elif record['id'] == 14:
            yield mark, 'remove', ('unique', reader.zigzag())
        else:
            yield mark, 'add', ('unique', reader.zigzag())


def distil(directory: Path, view: int, simulation: int) -> dict:
    results = [json.loads(line) for line in (directory / 'results.jsonl').read_text(encoding='utf-8').splitlines()]
    events = list(actor_events(directory / 'observer' / 'packets.jsonl'))
    items = []
    for item in results:
        markers = [{'type': marker['type'], 'pos': [round(v, 4) for v in marker['state']['pos']]}
                   for marker in item['markers']]
        markers.append({'type': 'minecraft:player', 'pos': [round(v, 4) for v in item['player']['pos']]})
        by_id = {('unique', marker['unique_id']): index for index, marker in enumerate(item['markers'])}
        player = len(markers) - 1
        by_id[('runtime', item['player_runtime_id'])] = player
        first_tick = {step['step']: step['tick'] for step in item['steps']}
        last_step = len(item['steps']) - 1
        found = []
        for mark, kind, ident in events:
            if mark[0] != item['key']:
                continue
            index = by_id.get(ident)
            if index is None and kind == 'remove' and ident[0] == 'unique':
                # The sweep player's unique id is not recorded; it is the only
                # other actor the observer is ever told to remove.
                if not any(key == ident for key in by_id):
                    index = player
            if index is None:
                continue
            step, tick = mark[1], mark[2] - first_tick[mark[1]]
            if step == last_step and kind == 'remove' and tick == item['ticks'] - 1:
                continue  # the item's end despawns its markers after the last mark
            found.append({'step': step, 'tick': tick, 'marker': index, 'kind': kind})
        items.append({
            'key': item['key'],
            'ticks_per_step': item['ticks'],
            'markers': markers,
            'path': [[round(step['observer']['pos'][0], 4), round(step['observer']['pos'][2], 4)]
                     for step in item['steps']],
            'events': found,
        })
    return {'view_distance': view, 'simulation_distance': simulation, 'items': items}


def main() -> int:
    out = Path(sys.argv[1])
    runs, sources = [], []
    for arg in sys.argv[2:]:
        config, directory = arg.split(':', 1)
        view, simulation = (int(part) for part in config.split(','))
        directory = Path(directory)
        loaded = json.loads((directory / 'loaded.json').read_text(encoding='utf-8'))
        sources.append({'sweep': directory.name, **loaded})
        runs.append(distil(directory, view, simulation))
    fixture = {
        'bds_version': '1.26.51',
        'protocol': sources[0]['protocol_version'],
        'sources': sources,
        'method': 'tools/bds-sweep/sweep.py range --observer --client-capture --view-distance V --tick-distance S: '
                  'still markers around the sweep player in chunk (0, 0), the observer walking away and back in '
                  'one-block steps of ten ticks; the observer capture tied to per-tick chat marks',
        'runs': runs,
    }
    out.write_text(json.dumps(fixture, separators=(',', ':')) + '\n', encoding='utf-8', newline='\n')
    events = sum(len(item['events']) for run in runs for item in run['items'])
    print(f'{len(runs)} runs, {events} events -> {out} ({out.stat().st_size} bytes)')
    return 0


if __name__ == '__main__':
    sys.exit(main())
