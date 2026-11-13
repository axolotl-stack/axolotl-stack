"""Cut a committed breaking fixture out of a bds-sweep-client capture of
tools/bds-sweep/scripts/breaking.txt (`script.py` against BDS):

    python tools/bds-sweep/breaking_fixture.py <out>/client-capture/packets.jsonl \
        crates/unastar/src/server/game/blocks/testdata/bds-breaking.jsonl

It keeps every PlayerAuthInput inside the scenario windows (client input
ticks; the twelve of breaking.txt unless `--windows 80-110,120-130,...`) and
the server's LevelEvent, UpdateBlock, AddItemActor, InventorySlot,
LevelSoundEvent and PacketViolationWarning packets that arrived while the
latest input was inside a window. Each record also carries `input`, the
client tick of the latest input sent before it.

With `--observer <out>/observer-capture/packets.jsonl` (a run with
`script.py --observer`, as for scripts/breaking-2.txt) the second player's
LevelEvent, UpdateBlock, AddItemActor, TakeItemActor and LevelSoundEvent
packets are kept too, as `dir: "s2o"` records. The two captures count time
from their own starts; the observer's records are moved onto the player's
clock by the median offset between the UpdateBlock packets both received
(the same bytes, as many times in each), and labelled with the player's
latest input at that time.
"""
import argparse
import bisect
import json
import statistics
from collections import defaultdict

WINDOWS = [(60, 100), (120, 150), (160, 180), (190, 210), (220, 270), (280, 310), (320, 360), (370, 410),
           (420, 455), (470, 510), (510, 550), (570, 640)]
KEEP_S2C = {25: 'LevelEvent', 21: 'UpdateBlock', 15: 'AddItemActor', 50: 'InventorySlot', 123: 'LevelSoundEvent',
            156: 'PacketViolationWarning'}
KEEP_OBSERVER = {25: 'LevelEvent', 21: 'UpdateBlock', 15: 'AddItemActor', 17: 'TakeItemActor',
                 123: 'LevelSoundEvent'}
UPDATE_BLOCK = 21


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


def parse_windows(spec):
    return [tuple(int(v) for v in part.split('-')) for part in spec.split(',')]


def record(row, direction, latest, t_ms=None):
    return {'seq': row['seq'], 't_ms': round(row['t_ms'] if t_ms is None else t_ms), 'dir': direction,
            'input': latest, 'name': row['name'], 'hex': row['hex']}


def cut(rows, windows):
    """The player's kept records (see the module docs)."""
    latest = None
    out = []
    for row in rows:
        if row['dir'] == 'c2s' and row['id'] == 144:
            latest = input_tick(row['hex'])
            keep = any(lo <= latest < hi for lo, hi in windows)
        elif row['dir'] == 's2c' and row['id'] in KEEP_S2C:
            keep = latest is not None and any(lo <= latest < hi for lo, hi in windows)
        else:
            keep = False
        if keep:
            out.append(record(row, row['dir'], latest))
    return out


def clock_offset(rows, observer_rows):
    """Median of the player's minus the observer's time over the UpdateBlock
    packets both captures hold equally often."""
    def updates(captured):
        times = defaultdict(list)
        for row in captured:
            if row['dir'] == 's2c' and row['id'] == UPDATE_BLOCK:
                times[row['hex']].append(row['t_ms'])
        return times
    ours, theirs = updates(rows), updates(observer_rows)
    offsets = [mine - other for packet, times in ours.items()
               if len(times) == len(theirs.get(packet, ()))
               for mine, other in zip(times, theirs[packet])]
    if not offsets:
        raise SystemExit('no UpdateBlock packet in common: cannot align the observer')
    return statistics.median(offsets)


def cut_observer(rows, observer_rows, windows):
    """The observer's kept records, on the player's clock and inputs."""
    offset = clock_offset(rows, observer_rows)
    inputs = [(row['t_ms'], input_tick(row['hex'])) for row in rows if row['dir'] == 'c2s' and row['id'] == 144]
    times = [t for t, _ in inputs]
    out = []
    for row in observer_rows:
        if row['dir'] != 's2c' or row['id'] not in KEEP_OBSERVER:
            continue
        at = row['t_ms'] + offset
        index = bisect.bisect_right(times, at) - 1
        latest = inputs[index][1] if index >= 0 else None
        if latest is not None and any(lo <= latest < hi for lo, hi in windows):
            out.append(record(row, 's2o', latest, at))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('source')
    parser.add_argument('target')
    parser.add_argument('--windows', type=parse_windows, default=WINDOWS)
    parser.add_argument('--observer', help="the observer's capture of the same run")
    args = parser.parse_args()
    rows = [json.loads(line) for line in open(args.source, encoding='utf-8') if line.strip()]
    out = cut(rows, args.windows)
    if args.observer:
        observer_rows = [json.loads(line) for line in open(args.observer, encoding='utf-8') if line.strip()]
        out += cut_observer(rows, observer_rows, args.windows)
        out.sort(key=lambda r: (r['t_ms'], r['dir'] == 's2o'))
    with open(args.target, 'w', encoding='utf-8', newline='\n') as f:
        for kept in out:
            f.write(json.dumps(kept, separators=(',', ':')) + '\n')
    print(len(out), 'records,', sum(1 for r in out if r['dir'] == 'c2s'), 'inputs,',
          sum(1 for r in out if r['dir'] == 's2o'), 'observer packets')


if __name__ == '__main__':
    main()
