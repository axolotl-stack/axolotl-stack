"""Cut the committed command/input order fixture out of a bds-sweep-client
capture of tools/bds-sweep/scripts/command-order.txt played against plain BDS:

    python tools/bds-sweep/command_order_fixture.py <out>/client-capture/packets.jsonl \
        crates/unastar/src/server/game/testdata/bds-command-order.jsonl

It keeps the client's CommandRequest and PlayerAuthInput packets and the
server's crack-start LevelEvents (3600) whose latest client input tick lies
inside a scenario window, in capture order. Each record also carries `input`,
the client tick of the latest input sent before it (a command sent in tick 40
goes out before that tick's input, so it carries 39).
"""
import json
import sys

# Scenarios A-D of the script, by latest client input tick.
WINDOWS = [(39, 47), (59, 68), (79, 87), (99, 118)]
COMMAND_REQUEST, PLAYER_AUTH_INPUT, LEVEL_EVENT = 77, 144, 25
CRACK_START = 3600


def varint(b, i):
    r = s = 0
    while True:
        x = b[i]
        i += 1
        r |= (x & 0x7f) << s
        s += 7
        if not x & 0x80:
            return r, i


def zigzag(v):
    return (v >> 1) ^ -(v & 1)


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


def level_event_id(hexstr):
    b = bytes.fromhex(hexstr)
    _, i = varint(b, 0)  # header
    event, _ = varint(b, i)
    return zigzag(event)


def main():
    source, target = sys.argv[1], sys.argv[2]
    rows = [json.loads(line) for line in open(source, encoding='utf-8') if line.strip()]
    latest = None
    out = []
    for row in rows:
        if row['dir'] == 'c2s' and row['id'] == PLAYER_AUTH_INPUT:
            latest = input_tick(row['hex'])
        elif row['dir'] == 'c2s' and row['id'] == COMMAND_REQUEST:
            pass
        elif row['dir'] == 's2c' and row['id'] == LEVEL_EVENT and level_event_id(row['hex']) == CRACK_START:
            pass
        else:
            continue
        if latest is not None and any(lo <= latest < hi for lo, hi in WINDOWS):
            out.append({'seq': row['seq'], 't_ms': round(row['t_ms']), 'dir': row['dir'], 'input': latest,
                        'name': row['name'], 'hex': row['hex']})
    with open(target, 'w', encoding='utf-8', newline='\n') as f:
        for record in out:
            f.write(json.dumps(record, separators=(',', ':')) + '\n')
    print(len(out), 'records,', sum(1 for r in out if r['name'] == 'CommandRequestPacket'), 'commands')


if __name__ == '__main__':
    main()
