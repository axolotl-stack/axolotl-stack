"""Distil `sweep.py ranged --client-capture` runs into the committed ranged
fixture (`crates/unastar/src/entity/mob/combat/testdata/bds-1.26.51-ranged.json`).

    python tools/bds-sweep/ranged_fixture.py OUT.json SWEEP_DIR [SWEEP_DIR ...]

Decodes each run's `client/packets.jsonl` with `bds-extractor --decode`
(cargo build -p bds-extractor first) and keeps, per item: the run's
difficulty, the mob, where it and the player stood, the blocks the setup
placed, its scripted hand changes, player teleports and fired events, the sample
(`t`) of each change of the mob's target, of its
`FACING_TARGET_TO_RANGE_ATTACK` flag and of its `SHAKING` flag, and every
arrow it shot (or a dispenser shot): the sample, the AddActor position,
velocity and rotation, its owner and `CUSTOM_DISPLAY` actor data, the level
sounds of that batch (name, actor identifier, position) in packet order,
the player's feet on that sample, and when it stuck (its `Shake` event and
shake time) and was taken (`TakeItemActor`). Each damage event (sample,
player or mob, amount, cause), the `MobEffect` adds and updates the player
got, the sample where the mob was gone, and the mob's position per sample
when it moved are kept too. Only observed values are kept: no packet
bytes, no login data.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
EXTRACTOR = REPO / 'target' / 'debug' / ('bds-extractor.exe' if sys.platform == 'win32' else 'bds-extractor')
PACKETS = ['TextPacket', 'AddActorPacket', 'SetActorDataPacket', 'LevelSoundEventPacket', 'MobEffectPacket',
           'ActorEventPacket', 'TakeItemActorPacket']
# Actor flags: SHAKING in the first word, FACING_TARGET_TO_RANGE_ATTACK (88)
# in the second.
SHAKING_BIT = 40
FACING_BIT = 88 - 64


def decoded(capture: Path) -> list[dict]:
    out = capture.with_name('decoded-ranged.jsonl')
    subprocess.run([str(EXTRACTOR), '--decode', str(capture), '--only', ','.join(PACKETS), '--output', str(out)],
                   check=True, capture_output=True)
    return [json.loads(line) for line in out.read_text(encoding='utf-8').splitlines() if line.strip()]


def vec(value: dict) -> list[float]:
    return [round(value['x'], 6), round(value['y'], 6), round(value['z'], 6)]


def data_values(packet: dict) -> dict[int, object]:
    return {row['id']: next(iter(row['payload'].values()))['value'] for row in packet['actor_data']['data']}


def namespaced(name: str) -> str:
    return name if ':' in name else f'minecraft:{name}'


def block_change(line: str) -> dict | None:
    """A `fill` or `setblock` setup line as `{from, to, block, states}`."""
    words = line.split(' ', 7 if line.startswith('fill ') else 4)
    if words[0] == 'setblock':
        at = [int(v) for v in words[1:4]]
        rest = words[4]
        span = (at, at)
    elif words[0] == 'fill':
        span = ([int(v) for v in words[1:4]], [int(v) for v in words[4:7]])
        rest = words[7]
    else:
        return None
    name, _, states = rest.partition(' ')
    out = {'from': span[0], 'to': span[1], 'block': namespaced(name)}
    if states:
        out['states'] = {key.strip('"'): int(value) if value.lstrip('-').isdigit() else value.strip('"')
                         for key, value in (pair.split('=') for pair in states.strip('[]').split(','))}
    return out


def scripted(actions: list[dict]) -> dict:
    """The structured effect of an item's timed commands: hand changes,
    teleports of the player and fired actor events."""
    hand, teleports, fired = [], [], []
    for action in actions:
        for line in action['commands']:
            words = line.split(' ')
            if words[0] == 'replaceitem' and 'slot.weapon.mainhand' in words:
                item = namespaced(words[-1])
                hand.append({'t': action['t'], 'item': None if item == 'minecraft:air' else item})
            elif words[0] == 'tp':
                teleports.append({'t': action['t'], 'to': [float(v) for v in words[2:5]]})
            elif words[:2] == ['event', 'entity']:
                fired.append({'t': action['t'], 'event': words[3]})
    return {'hand': hand, 'teleports': teleports, 'fired_events': fired}


def difficulty(directory: Path) -> str:
    job = json.loads((directory / 'job.json').read_text(encoding='utf-8'))
    return next(line.split(' ')[1] for line in job['setup_commands'] if line.startswith('difficulty '))


def distil(result: dict, records: list[dict], level: str) -> dict:
    key, mob = result['key'], result['runtime_id']
    samples = result['samples']
    shots, events, effects = [], [], []
    target = facing = shaking = None
    unique = None
    arrows: dict[int, dict] = {}
    t = None
    last_shot = None
    for record in records:
        packet, name = record['packet'], record['name']
        if name == 'TextPacket':
            message = packet['body'].get('MessageOnly', {}).get('message', '')
            if message.startswith('bds-sweep:mark '):
                _, mark_key, mark_t = message.split(' ')
                t = int(mark_t) if mark_key == key else None
                last_shot = None
            continue
        if t is None:
            continue
        if name == 'AddActorPacket':
            runtime = packet['target_runtime_id']['actor_runtime_id']
            if runtime == mob:
                unique = packet['target_actor_id']['actor_unique_id']
                continue
            if packet['actor_type'] == 'minecraft:arrow':
                values = data_values(packet)
                feet = samples[t]['player']['pos'] if t < len(samples) else None
                shot = {'t': t, 'position': vec(packet['position']), 'velocity': vec(packet['velocity']),
                        'rotation': [packet['rotation']['x'], packet['rotation']['y']],
                        'owner_is_shooter': values.get(5) == unique, 'custom_display': values.get(18),
                        'player_feet': [round(v, 6) for v in feet] if feet else None, 'sounds': []}
                arrows[runtime] = shot
                shots.append(shot)
                last_shot = shot
            continue
        if name == 'LevelSoundEventPacket':
            if last_shot is not None and last_shot['t'] == t:
                last_shot['sounds'].append({'name': packet['sound_event'],
                                            'actor': packet['actor_identifier'],
                                            'position': vec(packet['position'])})
            continue
        if name == 'ActorEventPacket':
            shot = arrows.get(packet['target_runtime_id']['actor_runtime_id'])
            if shot is not None and packet['event_id'] == 'Shake' and 'stuck' not in shot:
                shot['stuck'] = t
                shot['shake'] = packet['data']
            continue
        if name == 'TakeItemActorPacket':
            shot = arrows.get(packet['item_runtime_id']['actor_runtime_id'])
            if shot is not None:
                shot['taken'] = t
            continue
        if name == 'MobEffectPacket':
            if packet['target_runtime_id']['actor_runtime_id'] == result['player_runtime_id']:
                effects.append({'t': t, 'event': packet['event_id'], 'effect': packet['effect_id'],
                                'amplifier': packet['effect_amplifier'],
                                'duration': packet['effect_duration_ticks']})
            continue
        if name == 'SetActorDataPacket' and packet['target_runtime_id']['actor_runtime_id'] == mob:
            values = data_values(packet)
            if 6 in values and values[6] != target:
                target = values[6]
                events.append({'t': t, 'target': target != 0})
            if 92 in values:
                on = bool(values[92] >> FACING_BIT & 1)
                if on != facing:
                    facing = on
                    events.append({'t': t, 'facing': on})
            if 0 in values:
                on = bool(values[0] >> SHAKING_BIT & 1)
                if on != shaking:
                    shaking = on
                    events.append({'t': t, 'shaking': on})
    first = result['first_tick']
    damage = [{'t': tick - first, 'who': 'player' if actor == 'minecraft:player' else 'mob',
               'amount': round(amount, 6), 'cause': cause}
              for tick, actor, amount, cause in result['damage']]
    gone = next((s['t'] for s in samples if s.get('mob') is None), None)
    # Positions only for a mob that moved (the `avoid` items).
    positions = [[round(v, 4) for v in s['mob']['pos']] if s.get('mob') else None for s in samples]
    moved = len({tuple(p) for p in positions if p}) > 1
    spec = result['spec']
    blocks = [change for change in map(block_change, spec.get('setup', [])) if change]
    return {'difficulty': level, 'mob': result['mob'], 'at': spec['at'], 'player': spec['player'],
            'blocks': blocks, **scripted(spec.get('actions', [])), 'ticks': spec['ticks'], 'events': events,
            'shots': shots,
            'damage': damage, 'effects': effects, 'gone': gone,
            **({'positions': positions} if moved else {})}


def main() -> int:
    out, dirs = Path(sys.argv[1]), [Path(arg) for arg in sys.argv[2:]]
    runs, sources = {}, []
    for directory in dirs:
        loaded = json.loads((directory / 'loaded.json').read_text(encoding='utf-8'))
        sources.append({'sweep': directory.name, **loaded})
        level = difficulty(directory)
        records = decoded(directory / 'client' / 'packets.jsonl')
        items = {}
        for line in (directory / 'results.jsonl').read_text(encoding='utf-8').splitlines():
            if line.strip():
                result = json.loads(line)
                items[result['key']] = distil(result, records, level)
        runs[directory.name] = items
    fixture = {
        'bds_version': '1.26.51',
        'protocol': sources[0]['protocol_version'],
        'sources': sources,
        'method': 'tools/bds-sweep/sweep.py ranged --client-capture (--items slice, geometry, aim, spread, '
                  'damage, damage-long, weapon, filters); Endstone per-tick samples and ActorDamageEvent, the '
                  'sweep player\'s packet capture tied to per-tick chat marks',
        'runs': runs,
    }
    out.write_text(json.dumps(fixture, separators=(',', ':')) + '\n', encoding='utf-8', newline='\n')
    print(f'{sum(len(items) for items in runs.values())} items -> {out} ({out.stat().st_size} bytes)')
    return 0


if __name__ == '__main__':
    sys.exit(main())
