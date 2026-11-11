"""Summarises an `order_probe` capture (`events.jsonl`) trial by trial.

    python pvp_race_summary.py EVENTS [--json OUT]

For every trial mark (`case=… variant=… trial=…`) it lists, with the tick
relative to the race tick, what the server reported:

* `hurt`: hurt events (`ActorEvent` Hurt), from player C's view;
* `health`: health updates (`UpdateAttributes` minecraft:health) with the
  target's input tick the server stamped on them;
* `motion`: `SetActorMotion` (knockback) with its input tick;
* `crit`: critical-hit animations;
* `sound`: level sounds (attack.strong, attack.nodamage, …);
* `correct`: movement corrections each player received (own copies);
* `sprint`: changes of the sprinting flag in `SetActorData`;
* `query`: `/querytarget` answers (positions and yaw the server reported);
* `death`: death and respawn packets.

Then it groups trials by variant and prints the outcomes' distribution, so a
non-deterministic case shows up as more than one outcome.
"""

from __future__ import annotations

import argparse
import collections
import json
import re
import sys
from pathlib import Path

RID = {}
FLAG_SNEAKING = 1 << 1
FLAG_SPRINTING = 1 << 3

TICK = re.compile(r'inputtick: (\d+)')
TARGET = re.compile(r'target_runtime_id: ActorRuntimeId \{ actor_runtime_id: (\d+) \}')
VEC = r'Vec3 \{ x: ([-\d.e]+), y: ([-\d.e]+), z: ([-\d.e]+) \}'


def vec(match, start=1):
    return [round(float(match.group(start + i)), 4) for i in range(3)]


def name_of(rid: int) -> str:
    return RID.get(rid, str(rid))


# A gap this long between two server bursts inside a trial means BDS stalled
# (an autosave, say): every packet of the trial came late, so the trial says
# nothing about order and is reported apart.
STALL_US = 150_000

# Packets one player receives less than this far apart came in one server
# send burst (one server tick's flush); bursts are ~50 ms apart.
BURST_GAP_US = 12_000


def bursts(path: Path) -> dict[int, int]:
    """Record seq -> burst number, per receiving player."""
    last: dict[str, tuple[int, int]] = {}
    out = {}
    for line in path.open(encoding='utf-8'):
        record = json.loads(line)
        if record.get('dir') != 's2c':
            continue
        who, t = record['player'], record['t_us']
        prev_t, number = last.get(who, (-10**12, -1))
        if t - prev_t > BURST_GAP_US:
            number += 1
        last[who] = (t, number)
        out[record['seq']] = number
    return out


def parse(path: Path):
    burst_of = bursts(path)
    trials = []
    current = None
    # Every player's latest input so far: a trial's base starts from it,
    # because the race tick's idle inputs are sent before the trial mark.
    last_input = {}
    for line in path.open(encoding='utf-8'):
        record = json.loads(line)
        if record.get('dir') == 'c2s' and record['name'] == 'PlayerAuthInputPacket':
            last_input[record['player']] = int(TICK.search(record['debug']).group(1))
        if 'mark' in record:
            mark = record['mark']
            if mark.startswith('runtime id'):
                _, _, who, rid = mark.split()
                RID[int(rid)] = who
                continue
            fields = dict(part.split('=', 1) for part in mark.split())
            current = {'case': fields['case'], 'variant': fields['variant'],
                       'trial': int(fields['trial']), 'tick': record['tick'], 'events': [],
                       'sent': [], 'base': dict(last_input)}
            trials.append(current)
            continue
        if current is None:
            continue
        rel = record['tick'] - current['tick']
        if rel < -3:
            continue
        if rel > 38:
            continue
        if record['dir'] == 'c2s' and record['name'] == 'PlayerAuthInputPacket' and rel <= 0:
            current['base'][record['player']] = int(TICK.search(record['debug']).group(1))
        if record['dir'] == 'c2s':
            if -12 <= rel <= 13:
                text = summarise_sent(record, rel)
                plain_input = record['name'] == 'PlayerAuthInputPacket' and '[' not in text
                if rel == 0 or not plain_input:
                    current['sent'].append(text)
            continue
        if record['dir'] == 's2c' and record['player'] == 'C' and 0 <= rel <= OUTCOME_TICKS:
            last = current.get('last_rx')
            if last is not None and record['t_us'] - last > STALL_US:
                current['stalled'] = True
            current['last_rx'] = record['t_us']
        event = summarise(record, rel)
        if event:
            current['events'].append(event)
            current.setdefault('bursts', []).append((event[0], record['player'], burst_of[record['seq']], record['name']))
    return trials


def summarise_sent(record, rel):
    d = record['debug']
    name = record['name'].replace('Packet', '')
    extra = ''
    if name == 'PlayerAuthInput':
        tick = TICK.search(d)
        flags = re.search(r'input_data: \[([^\]]*)\]', d)
        extra = f'#{tick.group(1)}' + (f' [{flags.group(1)}]' if flags and flags.group(1) else '')
    elif name == 'InventoryTransaction':
        target = re.search(r'runtime_id: ActorRuntimeId \{ actor_runtime_id: (\d+) \}, action_type', d)
        extra = f'attack {name_of(int(target.group(1)))}' if target else ''
    elif name == 'CommandRequest':
        extra = re.search(r'command: "([^"]*)"', d).group(1)
    return f"{rel:+d} {record['player']} b{record['batch']} {name} {extra}".rstrip()


def summarise(record, rel):
    d = record['debug']
    who = record['player']
    name = record['name']
    if name == 'ActorEventPacket' and who == 'C':
        target = int(TARGET.search(d).group(1))
        event = re.search(r'event_id: (\w+)', d).group(1)
        if event in ('Hurt', 'Death', 'DeathSmokeCloud'):
            data = re.search(r'data: (-?\d+)', d).group(1)
            return ('hurt' if event == 'Hurt' else 'death', rel, f'{name_of(target)} {event} data={data}')
    if name == 'UpdateAttributesPacket' and who == 'C' and 'minecraft:health' in d:
        target = int(TARGET.search(d).group(1))
        value = re.search(r'current_value: ([-\d.e]+), default_min_value: [-\d.e]+, '
                          r'default_max_value: [-\d.e]+, default_value: [-\d.e]+, name: "minecraft:health"', d)
        return ('health', rel, f'{name_of(target)} {float(value.group(1))} @in{TICK.search(d).group(1)}')
    if name == 'SetActorMotionPacket' and who == 'C':
        target = int(TARGET.search(d).group(1))
        motion = vec(re.search(VEC, d))
        if target not in RID:
            return None
        return ('motion', rel, f'{name_of(target)} {motion} @in{TICK.search(d).group(1)}')
    if name == 'AnimatePacket' and who == 'C' and 'Criticalhit' in d:
        target = re.search(r'target_actor_runtime_id: ActorRuntimeId \{ actor_runtime_id: (\d+) \}', d)
        return ('crit', rel, f'{name_of(int(target.group(1)))}')
    if name == 'LevelSoundEventPacket' and who == 'C':
        sound = re.search(r'sound_event: "([^"]*)"', d).group(1)
        if sound.startswith('attack') or sound in ('hurt', 'death'):
            return ('sound', rel, sound)
    if name == 'CorrectPlayerMovePredictionPacket':
        pos = vec(re.search(r'pos: ' + VEC, d))
        delta = vec(re.search(r'pos_delta: ' + VEC, d))
        return ('correct', rel, f'{who} pos={pos} delta={delta} @in{TICK.search(d).group(1)}')
    if name == 'SetActorDataPacket' and who == 'C':
        flags = re.search(r'DataItemEntry \{ id: 0, payload: DataItemInt64Payload\('
                          r'DataItemInt64Payload \{ type_: Int64, value: (-?\d+) \}', d)
        if flags:
            target = int(TARGET.search(d).group(1))
            value = int(flags.group(1))
            state = ('sprint' if value & FLAG_SPRINTING else '') + ('sneak' if value & FLAG_SNEAKING else '')
            return ('flags', rel, (name_of(target), state or 'none', TICK.search(d).group(1)))
    if name in ('CommandOutputPacket', 'TextPacket') and 'querytarget' in d.lower() or (
            name == 'CommandOutputPacket' and 'yRot' in d):
        return ('query', rel, f'{who} ' + d[:600])
    if name == 'MovePlayerPacket' and 'position_mode: Normal' not in d:
        rid = int(re.search(r'player_runtime_id: ActorRuntimeId \{ actor_runtime_id: (\d+) \}', d).group(1))
        if RID.get(rid) == who:
            yaw = re.search(r'rotation: Vec2 \{ x: [-\d.e]+, y: ([-\d.e]+) \}', d).group(1)
            pos = vec(re.search(r'position: ' + VEC, d))
            return ('tp', rel, f'{who} pos={pos} yaw={float(yaw)}')
    if name == 'AddActorPacket' and who == 'C' and 'snowball' in d:
        return ('spawn', rel, 'snowball')
    if name == 'ContainerOpenPacket' and who == 'A':
        return ('open', rel, 'inventory')
    if name == 'TextPacket' and who == 'C' and 'order-probe' in d:
        return ('chat', rel, 'echo')
    if name == 'AnimatePacket' and who == 'C' and 'action: Swing' in d:
        return ('swing', rel, 'relayed')
    if name == 'RespawnPacket':
        state = re.search(r'state: (\w+)', d).group(1)
        return ('death', rel, f'{who} respawn {state}')
    return None


STAMP = re.compile(r'^(\S+) (.*) @in(\d+)$')
# Outcomes stop before the next trial's reset (heals land from +20 on).
OUTCOME_TICKS = 19


def relative_stamp(trial, text: str) -> str:
    """`B ... @in1234` becomes `B ... @B+k`: the input number relative to the
    last input that player sent on or before the race tick."""
    match = STAMP.match(text)
    if not match:
        return text
    who, rest, tick = match.groups()
    base = trial['base'].get(who)
    if base is None:
        return text
    return f'{who} {rest} @{who}{int(tick) - base:+d}'


def outcome(trial) -> str:
    """The trial's result without timing noise: the ordered list of hurts,
    health values, motions, crits and corrections (with relative ticks)."""
    if trial.get('stalled'):
        return 'STALLED (BDS sent nothing for over 150 ms inside the trial; excluded)'
    parts = []
    last_sprint = {}
    for kind, rel, text in trial['events']:
        if rel > OUTCOME_TICKS:
            continue
        if kind == 'flags':
            who, sprinting, tick = text
            if last_sprint.get(who) != sprinting:
                if who in last_sprint:
                    parts.append(f'{rel:+d} flags ' + relative_stamp(trial, f'{who} {sprinting} @in{tick}'))
                last_sprint[who] = sprinting
            continue
        if kind == 'query':
            yrot = re.findall(r'yRot\\+"?:\s*([-\d.]+)', text)
            pos = re.findall(r'\\"x\\":([-\d.]+)', text)
            parts.append(f'{rel:+d} query {text.split()[0]} yRot={yrot} x={pos}')
            continue
        if kind == 'tp' and not text.startswith('C'):
            continue
        if kind in ('hurt', 'health', 'motion', 'crit', 'correct', 'death', 'sound', 'tp', 'chat', 'swing',
                    'spawn', 'open'):
            parts.append(f'{rel:+d} {kind} ' + relative_stamp(trial, text))
    return '\n      '.join(parts)


def output_timing(group) -> str:
    """For a variant: in which burst, relative to the burst that brought
    the first hurt event to each receiver, each kind of packet arrived."""
    counts = collections.Counter()
    for trial in group:
        rows = trial.get('bursts', [])
        hurt = {who: b for kind, who, b, _ in rows if kind == 'hurt'}
        hurt.update({who: b for kind, who, b, _ in rows if kind == 'hurt' and who not in hurt})
        first = {}
        for kind, who, b, name in rows:
            if kind == 'hurt' and who not in first:
                first[who] = b
        for kind, who, b, name in rows:
            if who in first and kind in ('hurt', 'health', 'motion', 'sound', 'crit', 'correct', 'flags', 'chat', 'swing'):
                offset = b - first[who]
                if -3 <= offset <= 3:
                    counts[(kind, offset)] += 1
    return ', '.join(f'{kind}@burst{offset:+d}x{n}' for (kind, offset), n in sorted(counts.items()))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('events', type=Path)
    parser.add_argument('--json', type=Path)
    parser.add_argument('--trials', action='store_true', help='print every trial')
    args = parser.parse_args()
    trials = parse(args.events)
    groups = collections.defaultdict(list)
    for trial in trials:
        groups[(trial['case'], trial['variant'])].append(trial)
    summary = []
    for (case, variant), group in groups.items():
        counts = collections.Counter(outcome(t) for t in group)
        print(f'== {case} {variant}: {len(group)} trials, {len(counts)} outcome(s)')
        print('   sent: ' + '\n         '.join(group[0]['sent']))
        for text, n in counts.most_common():
            print(f'   {n}x  {text}')
        summary.append({'case': case, 'variant': variant, 'trials': len(group),
                        'outcomes': [{'count': n, 'events': text.split('\n      ')}
                                     for text, n in counts.most_common()],
                        'sent': group[0]['sent']})
        timing = output_timing(group)
        if timing:
            print('   output bursts (receiver C, relative to the hurt burst): ' + timing)
        if args.trials:
            for t in group:
                print(f"   trial {t['trial']}: " + outcome(t).replace('\n', ' |'))
    if args.json:
        args.json.write_text(json.dumps(summary, indent=1) + '\n', encoding='utf-8')
    return 0


if __name__ == '__main__':
    sys.exit(main())
