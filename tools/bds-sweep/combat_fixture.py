"""Cut a combat replay fixture out of a bds-sweep-client capture of a script
played against BDS (`script.py`), for example `scripts/spears.txt`:

    python tools/bds-sweep/combat_fixture.py <out>/client-capture/packets.jsonl \
        crates/unastar/src/entity/mob/combat/hit/testdata/bds-spears.jsonl

With `--commands PREFIX` it keeps the CommandRequests whose command starts
with PREFIX and the CommandOutput BDS answered each with (BDS answers every
request once, in order), for example `--commands enchant`. Otherwise, inside
the windows (client input ticks, `--windows 40-165,170-280,...`), it keeps
the client packets that drive a replay (PlayerAuthInput, InventoryTransaction,
MobEquipment, Animate) and the server packets a replay compares: ActorEvent,
Animate, PlayerStartItemCooldown, LevelSoundEvent, the UpdateAttributes of
actors other than the player (their health) and the SetActorData of the
player (its flags). Every AddActor is kept, wherever it falls, so a replay can
map BDS runtime ids to the name tags its script gave. Each record carries
`input`, the client tick of the latest input sent before it; `--no-inputs`
drops the inputs themselves for replays that only tick the server. Item use
scenarios also keep CompletedUsingItem, MobEffect, InventorySlot, Text,
BlockActorData, UpdateBlock and AddItemActor.
"""
import argparse
import json

KEEP_C2S = {144: 'PlayerAuthInput', 30: 'InventoryTransaction', 31: 'MobEquipment', 44: 'Animate'}
KEEP_S2C = {27: 'ActorEvent', 44: 'Animate', 176: 'PlayerStartItemCooldown', 123: 'LevelSoundEvent',
            29: 'UpdateAttributes', 39: 'SetActorData', 142: 'CompletedUsingItem', 28: 'MobEffect',
            50: 'InventorySlot', 9: 'Text', 56: 'BlockActorData', 21: 'UpdateBlock', 15: 'AddItemActor',
            30: 'InventoryTransaction'}
ADD_ACTOR = 13
# Sounds of the world rather than of the scenario.
AMBIENT_SOUNDS = (b'pop', b'spawn', b'ambient', b'mob.warning')


def varint(b, i):
    r = s = 0
    while True:
        x = b[i]
        i += 1
        r |= (x & 0x7f) << s
        s += 7
        if not x & 0x80:
            return r, i


def input_tick(b):
    """The client tick of a PlayerAuthInput: after rotation (2 f32), position
    (3 f32), move vector (2 f32), head yaw (f32), the input data bitset (a
    byte count and the bytes), input mode, play mode and interaction model
    (varuints) and the interact rotation (2 f32)."""
    _, i = varint(b, 0)  # header
    i += 4 * 8
    count, i = varint(b, i)
    i += count
    for _ in range(3):
        _, i = varint(b, i)
    i += 4 * 2
    tick, _ = varint(b, i)
    return tick


def runtime_id(b):
    """The runtime id right after the header (ActorEvent, UpdateAttributes,
    SetActorData)."""
    _, i = varint(b, 0)
    rid, _ = varint(b, i)
    return rid


def keep_s2c(packet_id, b, player):
    if packet_id == 29:
        return runtime_id(b) != player
    if packet_id == 39:
        return runtime_id(b) == player
    if packet_id == 123:
        # Header byte, then the sound name (length, bytes).
        return not any(b[1] == len(sound) and b[2:2 + len(sound)] == sound for sound in AMBIENT_SOUNDS)
    return True


def windows(spec):
    out = []
    for part in spec.split(','):
        lo, hi = part.split('-')
        out.append((int(lo), int(hi)))
    return out


def cut(rows, windows=(), commands=None, player=1, inputs=True, also=()):
    """The kept records of a capture's rows (see the module docs)."""
    latest = None
    out = []
    # Whether each request still waiting for its output was kept.
    pending = []
    for row in rows:
        b = bytes.fromhex(row['hex'])
        inside = latest is not None and any(lo <= latest < hi for lo, hi in windows)
        if row['dir'] == 'c2s' and row['id'] == 144:
            latest = input_tick(b)
            keep = inputs and commands is None and any(lo <= latest < hi for lo, hi in windows)
        elif commands is not None:
            if row['name'] == 'CommandRequestPacket':
                # Header byte, then the command (length varint, text). Compared
                # without the slash, which Git Bash rewrites in arguments.
                length, i = varint(b, 1)
                keep = b[i:i + length].decode('utf-8').lstrip('/').startswith(commands.lstrip('/'))
                pending.append(keep)
            else:
                keep = row['name'] == 'CommandOutputPacket' and pending.pop(0)
        elif row['dir'] == 'c2s':
            keep = inside and row['id'] in KEEP_C2S
        elif row['id'] == ADD_ACTOR:
            keep = True
        else:
            keep = inside and ((row['id'] in KEEP_S2C and keep_s2c(row['id'], b, player))
                               or row['name'] in also)
        if keep:
            out.append({'seq': row['seq'], 't_ms': round(row['t_ms']), 'dir': row['dir'], 'input': latest,
                        'name': row['name'], 'hex': row['hex']})
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('source')
    parser.add_argument('target')
    parser.add_argument('--windows', type=windows, default=[])
    parser.add_argument('--commands', default=None,
                        help='keep only the commands starting with this text and their outputs')
    parser.add_argument('--player', type=int, default=1, help='runtime id BDS gave the scripted player')
    parser.add_argument('--also', type=lambda spec: {name + 'Packet' for name in spec.split(',')}, default=set(),
                        help='server packets to keep as well inside the windows (names without the Packet '
                             'suffix), e.g. InventoryContent')
    parser.add_argument('--no-inputs', action='store_true',
                        help='drop the PlayerAuthInputs themselves (records keep their input tick); for '
                             'replays that only need the server to tick')
    args = parser.parse_args()
    rows = [json.loads(line) for line in open(args.source, encoding='utf-8') if line.strip()]
    out = cut(rows, args.windows, args.commands, args.player, not args.no_inputs, args.also)
    with open(args.target, 'w', encoding='utf-8', newline='\n') as f:
        for record in out:
            f.write(json.dumps(record, separators=(',', ':')) + '\n')
    print(len(out), 'records,', sum(1 for r in out if r['dir'] == 'c2s'), 'client packets')


if __name__ == '__main__':
    main()
