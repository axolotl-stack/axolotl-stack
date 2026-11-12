"""Cut the committed breaking fixture out of a bds-sweep-client capture of
tools/bds-sweep/scripts/breaking.txt (`script.py` against BDS):

    python tools/bds-sweep/breaking_fixture.py <out>/client-capture/packets.jsonl \
        crates/unastar/src/server/game/blocks/testdata/bds-breaking.jsonl

It keeps every PlayerAuthInput inside the twelve scenario windows (client input
ticks) and the server's LevelEvent, UpdateBlock, AddItemActor, InventorySlot
and LevelSoundEvent packets that arrived while the latest input was inside a
window. Each record also carries `input`, the client tick of the latest input
sent before it.
"""
import json
import sys

WINDOWS = [(60, 100), (120, 150), (160, 180), (190, 210), (220, 270), (280, 310), (320, 360), (370, 410),
           (420, 455), (470, 510), (510, 550), (570, 640)]
KEEP_S2C = {25: 'LevelEvent', 21: 'UpdateBlock', 15: 'AddItemActor', 50: 'InventorySlot', 123: 'LevelSoundEvent'}


def varint(b, i):
    r = s = 0
    while True:
        x = b[i]
        i += 1
        r |= (x & 0x7f) << s
        s += 7
        if not x & 0x80:
            return r, i


def input_tick(hexstr):
    """The client tick of a PlayerAuthInput: after rotation (2 f32), position
    (3 f32), move vector (2 f32), head yaw (f32), the input data bitset (a
    byte count and the bytes), input mode, play mode and interaction model
    (varuints) and the interact rotation (2 f32)."""
    b = bytes.fromhex(hexstr)
    _, i = varint(b, 0)  # header
    i += 4 * 8
    count, i = varint(b, i)
    i += count
    for _ in range(3):
        _, i = varint(b, i)
    i += 4 * 2
    tick, _ = varint(b, i)
    return tick


def main():
    source, target = sys.argv[1], sys.argv[2]
    rows = [json.loads(line) for line in open(source, encoding='utf-8') if line.strip()]
    latest = None
    out = []
    for row in rows:
        if row['dir'] == 'c2s' and row['id'] == 144:
            latest = input_tick(row['hex'])
            keep = any(lo <= latest < hi for lo, hi in WINDOWS)
        elif row['dir'] == 's2c' and row['id'] in KEEP_S2C:
            keep = latest is not None and any(lo <= latest < hi for lo, hi in WINDOWS)
        else:
            keep = False
        if keep:
            out.append({'seq': row['seq'], 't_ms': round(row['t_ms']), 'dir': row['dir'], 'input': latest,
                        'name': row['name'], 'hex': row['hex']})
    with open(target, 'w', encoding='utf-8', newline='\n') as f:
        for record in out:
            f.write(json.dumps(record, separators=(',', ':')) + '\n')
    print(len(out), 'records,', sum(1 for r in out if r['dir'] == 'c2s'), 'inputs')


main()
