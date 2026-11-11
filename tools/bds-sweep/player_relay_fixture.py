"""Distil `sweep.py walk --observer --client-capture` runs into the committed
player-relay fixture (`crates/unastar/src/server/testdata/
bds-1.26.51-player-relay.json`).

    python tools/bds-sweep/player_relay_fixture.py OUT.json SWEEP_DIR [SWEEP_DIR ...]

Per item (a scripted walk of the sweep player, the observer 8 blocks away)
it keeps, for every input tick of the walk, the input the sweep player sent
(`PlayerAuthInput`: pitch, yaw, head yaw) and the `MovePlayer` the observer
got for it (position, pitch, yaw, head yaw, mode, ground, tick), plus the
`AddActor` rotation of the item's cow. Teleports between items are left out.
Only observed values are kept: no packet bytes, no login data.
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

    def f32(self) -> float:
        (value,) = struct.unpack_from('<f', self.data, self.at)
        self.at += 4
        return value

    def i32(self) -> int:
        (value,) = struct.unpack_from('<i', self.data, self.at)
        self.at += 4
        return value


def inputs(capture: Path) -> dict[int, dict]:
    """The sweep player's `PlayerAuthInput`s (144) by input tick."""
    out = {}
    for line in capture.read_text(encoding='utf-8').splitlines():
        record = json.loads(line)
        if record['dir'] != 'c2s' or record['id'] != 144:
            continue
        reader = Reader(bytes.fromhex(record['hex']))
        reader.varint()  # header
        pitch, yaw = reader.f32(), reader.f32()
        reader.at += 12 + 8  # position, move vector
        head = reader.f32()
        for _ in range(reader.varint()):  # input data
            reader.varint()
        reader.varint()  # input mode
        reader.varint()  # play mode
        reader.varint()  # interaction model
        reader.at += 8  # interact rotation
        tick = reader.varint()
        out[tick] = {'pitch': round(pitch, 4), 'yaw': round(yaw, 4), 'head': round(head, 4)}
    return out


def observed(capture: Path, runtime_id: int):
    """The observer's packets about the sweep player in arrival order, each
    with the last chat mark before it: `MovePlayer` (19) and the cows'
    `AddActor` (13)."""
    mark = None
    for line in capture.read_text(encoding='utf-8').splitlines():
        record = json.loads(line)
        if record['dir'] != 's2c':
            continue
        data = bytes.fromhex(record['hex'])
        if record['id'] == 9:
            found = MARK.search(data)
            if found:
                mark = found.group(1).decode()
            continue
        reader = Reader(data)
        reader.varint()
        if record['id'] == 19:
            if reader.varint() != runtime_id:
                continue
            pos = [round(reader.f32(), 4) for _ in range(3)]
            pitch, yaw, head = reader.f32(), reader.f32(), reader.f32()
            mode, ground = reader.u8(), reader.u8()
            reader.varint()  # riding
            if reader.u8():
                reader.i32()
                reader.i32()
            tick = reader.varint()
            yield mark, {'tick': tick, 'pos': pos, 'pitch': round(pitch, 4), 'yaw': round(yaw, 4),
                         'head': round(head, 4), 'mode': mode, 'ground': bool(ground)}
        elif record['id'] == 13:
            reader.varint()  # unique id (zigzag)
            reader.varint()  # runtime id
            size = reader.varint()
            kind = data[reader.at:reader.at + size].decode()
            reader.at += size
            if kind != 'minecraft:cow':
                continue
            reader.at += 24  # position, velocity
            rotation = [reader.f32() for _ in range(4)]
            yield mark, {'cow': [round(value, 5) for value in rotation]}


def distil(directory: Path) -> list[dict]:
    results = [json.loads(line) for line in (directory / 'results.jsonl').read_text(encoding='utf-8').splitlines()]
    sent = inputs(directory / 'client' / 'packets.jsonl')
    runtime_id = results[0]['player_runtime_id']
    seen = list(observed(directory / 'observer' / 'packets.jsonl', runtime_id))
    items = []
    for item in results:
        relays = [packet for mark, packet in seen if mark == item['key'] and 'tick' in packet]
        cows = [packet['cow'] for mark, packet in seen if 'cow' in packet]
        steps = []
        for relay in relays:
            if relay['mode'] != 0:
                break  # the teleport of the next item
            steps.append({'input': sent.get(relay['tick']), 'relay': relay})
        items.append({'key': item['key'], 'order': item['order'], 'cow_spawn_yaw': item['cow'] and 33.3,
                      'cow_add_actor_rotation': cows[0] if cows else None, 'steps': steps})
    return items


def main() -> int:
    out, dirs = Path(sys.argv[1]), [Path(arg) for arg in sys.argv[2:]]
    loaded = json.loads((dirs[0] / 'loaded.json').read_text(encoding='utf-8'))
    items = [item for directory in dirs for item in distil(directory)]
    fixture = {
        'bds_version': '1.26.51',
        'protocol': loaded['protocol_version'],
        'method': 'tools/bds-sweep/sweep.py walk --observer --client-capture: the sweep player driven by '
                  'bds-sweep:walk orders (turning, tilting, walking, stepping up) with the observer 8 blocks '
                  'south; its PlayerAuthInputs and the observer\'s MovePlayer for each input tick',
        'items': items,
    }
    out.write_text(json.dumps(fixture, separators=(',', ':')) + '\n', encoding='utf-8', newline='\n')
    steps = sum(len(item['steps']) for item in items)
    print(f'{len(items)} items, {steps} relayed inputs -> {out} ({out.stat().st_size} bytes)')
    return 0


if __name__ == '__main__':
    sys.exit(main())
