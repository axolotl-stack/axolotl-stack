"""Distils `sweep.py pvp` runs into the committed PvP fixture.

    python tools/bds-sweep/pvp_fixture.py OUT.json RUN_DIR [RUN_DIR ...]
    python tools/bds-sweep/pvp_fixture.py --summary RUN_DIR [RUN_DIR ...]

A run's `results.jsonl` holds, per trial, the job spec, a sample of every
player each lockstep tick, and one ordered event list: the tick lines, the
packets the players sent as BDS handled them (`in`), selected server packets
to them with their payload (`out`), and damage events. This keeps only
values: per trial the roles (A attacker, B victim, C second attacker), the
setup and each player's starting health, every hit BDS applied (damage,
cause, the knockback event's velocity change), every attack in the order
and tick BDS applied it (`attempts`: arrival tick, held slot, both players'
state at that moment, the victim's reported velocity, the attacker's
sprint), and the decoded `SetActorMotion`, `ActorEvent` and health
`UpdateAttributes` a player got about itself plus the hit feedback anyone
got (`Animate`, `SetLastHurtBy`, attack sounds). Ticks count from the
trial's first lockstep tick. `--summary` prints a readable digest instead.
"""

from __future__ import annotations

import argparse
import json
import re
import struct
import subprocess
from pathlib import Path

MARK_STEP = 0.01


class Reader:
    def __init__(self, data: bytes) -> None:
        self.data = data
        self.at = 0

    def varint(self) -> int:
        out = shift = 0
        while True:
            byte = self.data[self.at]
            self.at += 1
            out |= (byte & 0x7F) << shift
            shift += 7
            if not byte & 0x80:
                return out

    def zigzag(self) -> int:
        value = self.varint()
        return (value >> 1) ^ -(value & 1)

    def f32(self) -> float:
        value = struct.unpack_from('<f', self.data, self.at)[0]
        self.at += 4
        return value

    def u8(self) -> int:
        self.at += 1
        return self.data[self.at - 1]

    def string(self) -> str:
        length = self.varint()
        self.at += length
        return self.data[self.at - length:self.at].decode('utf-8')


def f32_list(values) -> list[float]:
    """Floats as the f32 they were, printed shortest."""
    return [float(f'{struct.unpack("<f", struct.pack("<f", v))[0]:.9g}') for v in values]


def decode_out(packet_id: int, payload: bytes) -> dict | None:
    """The fields of a recorded server packet this fixture keeps."""
    r = Reader(payload)
    if packet_id == 40:  # SetActorMotion
        about = r.varint()
        motion = [r.f32(), r.f32(), r.f32()]
        return {'packet': 'SetActorMotion', 'about': about, 'motion': f32_list(motion), 'tick': r.varint()}
    if packet_id == 27:  # ActorEvent
        about = r.varint()
        event = r.u8()
        data = r.zigzag()
        return {'packet': 'ActorEvent', 'about': about, 'event': event, 'data': data}
    if packet_id == 19:  # MovePlayer
        about = r.varint()
        position = [r.f32(), r.f32(), r.f32()]
        pitch, yaw, head_yaw = r.f32(), r.f32(), r.f32()
        mode = r.u8()
        ground = bool(r.u8())
        r.varint()
        if mode == 2:
            r.at += 8
        return {'packet': 'MovePlayer', 'about': about, 'position': f32_list(position),
                'rotation': f32_list([pitch, yaw, head_yaw]), 'mode': mode, 'ground': ground,
                'tick': r.varint()}
    if packet_id == 29:  # UpdateAttributes: keep health only
        about = r.varint()
        for _ in range(r.varint()):
            values = [r.f32() for _ in range(6)]
            name = r.string()
            for _ in range(r.varint()):
                # id, name, amount, operation and operand (4-byte ints),
                # serializable.
                r.string(), r.string()
                r.at += 4 + 4 + 4 + 1
            if name == 'minecraft:health':
                return {'packet': 'Health', 'about': about, 'value': f32_list([values[2]])[0]}
        return None
    if packet_id == 44:  # Animate: action 4 critical hit, 5 magic critical hit
        action = r.varint()
        about = r.varint()
        return {'packet': 'Animate', 'about': about, 'action': action, 'data': f32_list([r.f32()])[0]}
    if packet_id == 96:  # SetLastHurtBy: the hurting actor's type id
        return {'packet': 'SetLastHurtBy', 'actor_type': r.zigzag()}
    if packet_id == 123:  # LevelSoundEvent: the sound and where
        sound = r.string()
        position = [r.f32(), r.f32(), r.f32()]
        return {'packet': 'LevelSoundEvent', 'sound': sound, 'position': f32_list(position)}
    if packet_id == 161:  # CorrectPlayerMovePrediction: kept as a marker
        return {'packet': 'CorrectPlayerMovePrediction'}
    return None


def consumed_lines(result: dict) -> dict[str, list[int]]:
    """Per role, for every sample, the index of the last line whose input
    BDS had consumed (from the yaw marks; -1 before the first)."""
    roles = dict(zip(result['names'], 'ABC'))
    out = {role: [] for role in roles.values()}
    for name, spot in zip(result['names'], result['spec']['at']):
        for sample in result['samples']:
            state = sample['players'].get(name)
            if state is None:
                out[roles[name]].append(None)
                continue
            turned = (state['rot'][1] - spot[3] + 180.0) % 360.0 - 180.0
            out[roles[name]].append(round(turned / MARK_STEP) - 1)
    return out


def consumption_ticks(marks: list[int | None]) -> dict[int, int]:
    """Line index -> trial tick in which BDS consumed that line's input.
    A sample at trial tick t shows what the ticks before it consumed."""
    out = {}
    last = -1
    for t, mark in enumerate(marks):
        if mark is None:
            continue
        for line in range(last + 1, mark + 1):
            out[line] = t - 1
        last = max(last, mark)
    return out


def player_runtime_ids(events: list[dict], roles: dict[str, str]) -> dict[int, str]:
    """Runtime id -> role: BDS relays every other player's moves to a
    player, never its own, so a player's id is the one missing from the
    relays it got."""
    relayed: dict[str, set[int]] = {name: set() for name in roles}
    for event in events:
        if event['event'] == 'out' and event['id'] == 19 and event['player'] in relayed:
            relayed[event['player']].add(Reader(bytes.fromhex(event['payload'])).varint())
    every = set().union(*relayed.values())
    out = {}
    for name, seen in relayed.items():
        missing = every - seen
        if len(missing) == 1:
            out[missing.pop()] = roles[name]
    return out


def state(raw: dict | None) -> dict | None:
    if raw is None:
        return None
    return {'pos': [round(v, 6) for v in raw['pos']], 'vel': [round(v, 6) for v in raw['vel']],
            'ground': raw['ground']}


CAPTURES = {'A': 'client/packets.jsonl', 'B': 'client/BDSOther/packets.jsonl', 'C': 'client/BDSThird/packets.jsonl'}
INPUT_LINE = re.compile(r'player_rotation: Vec2 \{ x: [^,]+, y: ([^ ]+) \}.*?'
                        r'input_data: \[([^\]]*)\].*?'
                        r'client_tick: PlayerInputTick \{ inputtick: (\d+) \}, '
                        r'pos_delta: Vec3 \{ x: ([^,]+), y: ([^,]+), z: ([^ ]+) \}')


class Inputs:
    """One client's inputs in send order: yaw, input flags, input tick and
    reported velocity (`pos_delta`), from its own capture. Trials take
    their inputs in order (`take`)."""

    def __init__(self, rows: list[tuple]) -> None:
        self.rows = rows
        self.by_tick = {row[2]: row for row in rows}
        self.cursor = 0

    def take(self, base_yaw: float, lines: int) -> dict[int, tuple]:
        """The inputs of a trial whose line `n` turned the client to
        `base_yaw + MARK_STEP * (n + 1)`: line -> row, from the first input
        after the previous trial's that carries line 0's mark."""
        def line_of(yaw: float) -> int | None:
            turned = (yaw - base_yaw + 180.0) % 360.0 - 180.0
            line = round(turned / MARK_STEP) - 1
            return line if abs(turned - MARK_STEP * (line + 1)) < 2e-3 and 0 <= line < lines else None

        start = next((i for i in range(self.cursor, len(self.rows)) if line_of(self.rows[i][0]) == 0), None)
        if start is None:
            return {}
        out = {}
        index = start
        while index < len(self.rows):
            line = line_of(self.rows[index][0])
            if line is None or line < max(out, default=-1):
                break
            out.setdefault(line, self.rows[index])
            index += 1
        self.cursor = index
        return out


def reported_inputs(run: Path, dump: Path) -> dict[str, Inputs]:
    """Per role, its client's inputs (from the client's own capture)."""
    out = {}
    for role, capture in CAPTURES.items():
        path = run / capture
        if not path.exists():
            continue
        text = subprocess.run([str(dump), str(path), '--name', 'PlayerAuthInputPacket', '--dir', 'c2s'],
                              capture_output=True, text=True, check=True).stdout
        rows = []
        for line in text.splitlines():
            m = INPUT_LINE.search(line)
            if m:
                rows.append((float(m.group(1)), [f.strip() for f in m.group(2).split(',') if f.strip()],
                             int(m.group(3)), f32_list(float(v) for v in m.group(4, 5, 6))))
        out[role] = Inputs(rows)
    return out


# When nothing landed to show it: the order BDS applied different players'
# attacks in within one tick in most trials (the player that joined last
# first: C, then B, then A), whatever order the packets came in. An input
# consumed a tick late went before that tick's own inputs instead.
APPLY_RANK = {'C': 0, 'B': 1, 'A': 2}


def steps_of(text: str):
    """The steps of a tick line in order, a batch's steps flattened."""
    for word in text.split():
        yield from word.strip('[]').split(',')


def attempts_of(result: dict, consumption: dict, hits: list[dict]) -> list[dict]:
    """Every attack of the trial, in the order and tick BDS applied it.

    An attack joins the attacker's next input after it: that line's input
    when the input follows the attack in the line, else the next line's.
    A hit that landed is applied in its damage event's tick; another one
    in its input's consumption tick, shifted by the offset the trial's
    landed hits of that attacker show (or any attacker's, else 1). Within a
    tick the landed hits go in the order BDS applied them, then the other
    attacks in `APPLY_RANK` and send order (one that did not land was
    either refused or ignored by an earlier hit's window)."""
    spec = result['spec']
    lines = {int(t): steps for t, steps in (spec.get('script') or {}).items()}
    for fired in result.get('fired', []):
        lines[fired['t']] = fired['steps']
    slots = {role: 0 for role in 'ABC'}
    claims = {role: [0.0, 0.0, 0.0] for role in 'ABC'}
    claim_slots = {role: None for role in 'ABC'}
    attempts = []
    for line in sorted(lines):
        steps = list(steps_of(lines[line]))
        for index, step in enumerate(steps):
            if step.startswith('o') and '=slot:' in step:
                slots['ABC'[int(step[1])]] = int(step.split(':')[1])
            if step.startswith('o') and '=claim:' in step:
                claims['ABC'[int(step[1])]] = [float(v) for v in step.split(':')[1:4]]
            if step.startswith('o') and '=claim-slot:' in step:
                value = step.split(':')[1]
                claim_slots['ABC'[int(step[1])]] = None if value == 'off' else int(value)
            if not step.startswith('a'):
                continue
            attacker, victim = ('ABC'[int(n)] for n in step[1:].split('>'))
            own_input = f'i{"ABC".index(attacker)}'
            joins = line if own_input in steps[index + 1:] else line + 1
            attempts.append({'line': line, 'joins': joins, 'order': len(attempts), 'attacker': attacker,
                             'victim': victim, 'slot': slots[attacker], 'claim_offset': claims[attacker],
                             'claim_slot': claim_slots[attacker],
                             'consumed': consumption[attacker].get(joins)})
    # The tick each attack's packet arrived in: an attacker's attacks are
    # its only inventory transactions of the trial, in send order.
    first = result['first_tick']
    for role in 'ABC':
        name = result['names']['ABC'.index(role)]
        arrivals = [e['tick'] - first for e in result['events']
                    if e['event'] == 'in' and e['id'] == 30 and e['player'] == name]
        mine = [a for a in attempts if a['attacker'] == role]
        for attempt, arrived in zip(mine, arrivals):
            attempt['arrived'] = arrived
    for attempt in attempts:
        attempt.setdefault('arrived', None)
    # Landed hits, matched to attempts of the same attacker and victim in
    # send order.
    pending = list(hits)
    for attempt in attempts:
        attempt['landed'] = None
    for hit_index, hit in enumerate(hits):
        for attempt in attempts:
            if (attempt['landed'] is None and attempt['attacker'] == hit['attacker']
                    and attempt['victim'] == hit['victim']
                    and (attempt['consumed'] is None or attempt['consumed'] >= hit['t'])
                    and all(a['landed'] is not None or a['attacker'] != hit['attacker']
                            or a['consumed'] is None or a['consumed'] < hit['t'] or a is attempt
                            for a in attempts if a['order'] < attempt['order'])):
                attempt['landed'] = hit_index
                break
    del pending
    offsets = {}
    for attempt in attempts:
        if attempt['landed'] is not None and attempt['consumed'] is not None:
            offsets.setdefault(attempt['attacker'], attempt['consumed'] - hits[attempt['landed']]['t'])
    # An attacker without a landed hit: the offset of any landed hit.
    fallback = next(iter(offsets.values()), 1)
    for attempt in attempts:
        if attempt['landed'] is not None:
            attempt['t'] = hits[attempt['landed']]['t']
        elif attempt['consumed'] is not None:
            attempt['t'] = attempt['consumed'] - offsets.get(attempt['attacker'], fallback)
        else:
            attempt['t'] = None
    attempts.sort(key=lambda a: (a['t'] if a['t'] is not None else 10 ** 6,
                                 0 if a['landed'] is not None else 1,
                                 a['landed'] if a['landed'] is not None else APPLY_RANK[a['attacker']],
                                 a['order']))
    return attempts


def sample_state(result: dict, t: int | None, role: str) -> dict | None:
    """A player's sampled state at the start of trial tick `t`."""
    name = result['names']['ABC'.index(role)]
    for sample in result['samples']:
        if sample['t'] == t:
            raw = sample['players'].get(name)
            if raw is None:
                return None
            return {**state(raw), 'sprinting': raw['sprinting'], 'health': raw['health']}
    return None


def distil(result: dict, reported: dict[str, Inputs] | None = None, feedback: bool = False) -> dict:
    names = result['names']
    roles = dict(zip(names, 'ABC'))
    first = result['first_tick']
    # Runtime ids from the knockback events and the hurt events' targets are
    # resolved through the names: each player's own MovePlayer relay to
    # another client names it (A's relay to B carries A's id).
    marks = consumed_lines(result)
    lines = len(result['samples'])
    inputs = {role: reported[role].take(spot[3], lines) for role, spot in zip('ABC', result['spec']['at'])
              if reported and role in reported}
    consumption = {role: consumption_ticks(m) for role, m in marks.items()}
    spec = result['spec']
    runtime_ids = player_runtime_ids(result['events'], roles)
    hits = []
    knockbacks = {kb['seq']: kb for kb in result['knockbacks'] if kb.get('seq') is not None}
    events = result['events']
    for i, event in enumerate(events):
        if event['event'] != 'damage':
            continue
        # The knockback of this hit follows it before the next damage event.
        kb = None
        for seq in sorted(knockbacks):
            if seq > event['seq'] and all(not (e['event'] == 'damage' and event['seq'] < e['seq'] < seq)
                                          for e in events):
                kb = knockbacks[seq]
                break
        victim_role = roles.get(event['victim'])
        # The victim's input tick the push is stamped with is its last
        # consumed input, whose reported velocity the push keeps half of.
        motion = next((e for e in events[i + 1:] if e['event'] == 'out' and e['id'] == 40
                       and e['player'] == event['victim']), None)
        input_tick = decode_out(40, bytes.fromhex(motion['payload']))['tick'] if motion else None
        hits.append({
            't': event['tick'] - first,
            'victim_input_tick': input_tick,
            'victim_reported_velocity': (reported[victim_role].by_tick.get(input_tick, (None,) * 4)[3]
                                         if reported and victim_role in reported else None),
            'victim': roles.get(event['victim']),
            'attacker': roles.get(event['attacker']),
            'damage': event['damage'],
            'cause': event['cause'],
            'health_after': event.get('health_after', event.get('health_before')),
            'knockback': None if kb is None else {
                'change': f32_list(kb['knockback']), 'victim': state(kb['state']),
                'attacker': state(kb['source'])},
        })
    sent = []
    for event in events:
        t = event['tick'] - first
        if event['event'] == 'out':
            decoded = decode_out(event['id'], bytes.fromhex(event['payload']))
            if decoded is None or decoded['packet'] == 'MovePlayer':
                continue
            about = decoded.pop('about', None)
            decoded['about'] = runtime_ids.get(about, about)
            to = roles.get(event['player'])
            # A player's packets about itself, and the hit feedback anyone
            # gets (animation, attack sound, last hurt by).
            if decoded['about'] != to and decoded['packet'] not in ('Animate', 'LevelSoundEvent',
                                                                      'SetLastHurtBy'):
                continue
            if decoded['packet'] == 'LevelSoundEvent' and not decoded['sound'].startswith('attack.'):
                continue
            sent.append({'t': t, 'to': to, **decoded})
    attempts = attempts_of(result, consumption, hits)
    for attempt in attempts:
        hit = hits[attempt['landed']] if attempt['landed'] is not None else None
        kb = hit['knockback'] if hit else None
        attempt['attacker_state'] = sample_state(result, attempt['t'], attempt['attacker'])
        attempt['victim_state'] = sample_state(result, attempt['t'], attempt['victim'])
        attempt['victim_reported_velocity'] = hit['victim_reported_velocity'] if hit else None
        # The attacker's sprint before the input the attack joins: the
        # `Sprinting` flag of its input of the line before.
        before = inputs.get(attempt['attacker'], {}).get(attempt['joins'] - 1)
        attempt['attacker_sprinting'] = 'Sprinting' in before[1] if before else None
        if kb is not None:
            # The moment of the hit, from the knockback event.
            attempt['attacker_state'] = {**(attempt['attacker_state'] or {}), **kb['attacker']}
            attempt['victim_state'] = {**(attempt['victim_state'] or {}), **kb['victim']}
        for kept in (attempt['attacker_state'], attempt['victim_state']):
            for diagnostic in ('health', 'sprinting'):
                (kept or {}).pop(diagnostic, None)
        for diagnostic in ('order', 'line', 'joins', 'consumed'):
            del attempt[diagnostic]
    for hit in hits:
        if hit['knockback'] is not None:
            # The states are the attempts'; the hit keeps the change.
            hit['knockback'] = hit['knockback']['change']
    # Each player's health when the trial started (a `/damage` of the setup
    # can fail, right after a respawn).
    start = result['samples'][0]['players'] if result['samples'] else {}
    start_health = {role: start[name]['health'] for name, role in roles.items() if start.get(name)}
    key, _, repeat = result['key'].rpartition('#')
    return {
        'key': key,
        'repeat': int(repeat),
        'setup': {k: spec[k] for k in ('at', 'held', 'hotbar', 'armour', 'modes', 'difficulty', 'effects',
                                         'setup', 'damage_before', 'script', 'when') if k in spec},
        'fired': [{'t': f['t'], 'steps': f['steps']} for f in result.get('fired', [])],
        # The run recorded the hit feedback packets (Animate, SetLastHurtBy,
        # LevelSoundEvent); earlier runs did not.
        'feedback': feedback,
        'start_health': start_health,
        'hits': hits,
        'attempts': attempts,
        'sent': sent,
    }


def results(run_dirs):
    for run in run_dirs:
        for line in (Path(run) / 'results.jsonl').read_text(encoding='utf-8').splitlines():
            if line.strip():
                yield run, json.loads(line)


def summary(trial: dict) -> str:
    lines = [f'== {trial["key"]}#{trial["repeat"]}']
    for hit in trial['hits']:
        kb = hit['knockback']
        lines.append(f'  hit t{hit["t"]} {hit["attacker"]}->{hit["victim"]} {hit["damage"]:g} {hit["cause"]} '
                     f'hp {hit["health_after"]:g}' + (f' kb {kb}' if kb else ' no kb'))
    for s in trial['sent']:
        if s['packet'] in ('SetActorMotion', 'ActorEvent') and s['to'] == 'B' or (
                s['packet'] == 'SetActorMotion' and s['about'] == 'A' and s['to'] == 'A'):
            lines.append(f'  t{s["t"]} ->{s["to"]} {s}')
    return '\n'.join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--summary', action='store_true')
    parser.add_argument('--only', help='keys starting with this')
    parser.add_argument('--dump', type=Path,
                        default=Path(__file__).resolve().parents[2] / 'target' / 'debug' / 'bds-sweep-dump.exe',
                        help='bds-sweep-dump, for the velocities the clients reported')
    parser.add_argument('paths', nargs='+')
    args = parser.parse_args()
    paths = args.paths if args.summary else args.paths[1:]
    reported = {run: reported_inputs(Path(run), args.dump) for run in paths}
    feedback = {run: any(e['event'] == 'out' and e['id'] == 96 for _, r in results([run]) for e in r['events'])
                for run in paths}
    trials = [distil(r, reported[run], feedback[run]) for run, r in results(paths)]
    if args.only:
        trials = [t for t in trials if t['key'].startswith(args.only)]
    if args.summary:
        for trial in trials:
            print(summary(trial))
        return 0
    trials.sort(key=lambda t: (t['key'], t['repeat']))
    Path(args.paths[0]).write_text(json.dumps({
        'source': 'BDS 1.26.51 under Endstone, three lockstep bds-sweep-client players (sweep.py pvp)',
        'roles': 'A = BDSSweep (attacker), B = BDSOther (victim), C = BDSThird',
        'trials': trials,
    }, separators=(',', ':')) + '\n', encoding='utf-8')
    print(f'{len(trials)} trials -> {args.paths[0]}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
