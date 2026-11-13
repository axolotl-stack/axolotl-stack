"""Distil `sweep.py effects --client-capture` runs into the committed effects
fixture (`crates/unastar/src/entity/effects/testdata/bds-1.26.51-effects.json`).

    python tools/bds-sweep/effects_fixture.py OUT.json SWEEP_DIR [SWEEP_DIR ...]

Decodes each run's `client-capture/packets.jsonl` with `bds-extractor --decode`
(cargo build -p bds-extractor first) and, per item, keeps the commands and
their ticks, every damage event (tick, whether it hit the mob or the
player, damage, cause), the mob's position and Endstone health per tick,
and every packet about the mob or the player after the spawn: `MobEffect`,
the effect-related actor data (flags 0, effect colour 8, ambience 9,
visible effects 131), `UpdateAttributes` (with modifiers) and
`ActorEvent`, each on the sample (`t`) whose `bds-sweep:mark` precedes it
in the capture. Only observed values are kept: no packet bytes, no login
data.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
EXTRACTOR = REPO / 'target' / 'debug' / ('bds-extractor.exe' if sys.platform == 'win32' else 'bds-extractor')
PACKETS = ['TextPacket', 'MobEffectPacket', 'SetActorDataPacket', 'UpdateAttributesPacket', 'ActorEventPacket',
           'AddActorPacket']
# Actor data rows an effect changes: flags, effect colour, effect ambience
# and the visible effects.
EFFECT_DATA = {0, 8, 9, 131}


def decoded(capture: Path) -> list[dict]:
    out = capture.with_name('decoded-effects.jsonl')
    subprocess.run([str(EXTRACTOR), '--decode', str(capture), '--only', ','.join(PACKETS), '--output', str(out)],
                   check=True, capture_output=True)
    return [json.loads(line) for line in out.read_text(encoding='utf-8').splitlines() if line.strip()]


def payload_value(payload: dict):
    inner = next(iter(payload.values()))
    return inner.get('value')


def attribute(row: dict) -> dict:
    """An `UpdateAttributes` entry; the minimums are kept only when not 0."""
    out = {'name': row['name'], 'current': row['current_value'], 'max': row['max_value'],
           'default': row['default_value'], 'default_max': row['default_max_value']}
    for key, field in (('min', 'min_value'), ('default_min', 'default_min_value')):
        if row[field] != 0.0:
            out[key] = row[field]
    if row['modifiers']:
        out['modifiers'] = [{'id': m['id'], 'name': m['name'], 'amount': m['amount'], 'operation': m['operation'],
                             'operand': m['operand'], 'serializable': m['is_serializable']}
                            for m in row['modifiers']]
    return out


def runtime_id(packet: dict) -> int | None:
    for key in ('target_runtime_id', 'runtime_entity_id'):
        if key in packet:
            value = packet[key]
            return value['actor_runtime_id'] if isinstance(value, dict) else value
    return None


def distil(result: dict, records: list[dict]) -> dict:
    """The packets of one item: those between its first and last mark."""
    key = result['key']
    about = {result['player_runtime_id']: 'player'}
    if result['runtime_id'] is not None:
        about[result['runtime_id']] = 'mob'
    t, packets = None, []
    for record in records:
        packet = record['packet']
        if record['name'] == 'TextPacket':
            message = packet['body'].get('MessageOnly', {}).get('message', '')
            if message.startswith('bds-sweep:mark '):
                _, mark_key, mark_t = message.split(' ')
                t = int(mark_t) if mark_key == key else None
            continue
        if t is None:
            continue
        who = about.get(runtime_id(packet))
        if who is None:
            continue
        name = record['name']
        if name == 'MobEffectPacket':
            packets.append({'t': t, 'about': who, 'kind': 'effect', 'event': packet['event_id'],
                            'effect': packet['effect_id'], 'amplifier': packet['effect_amplifier'],
                            'duration': packet['effect_duration_ticks'], 'particles': packet['show_particles'],
                            'ambient': packet['ambient'], 'tick': packet['tick']['inputtick']})
        elif name == 'SetActorDataPacket':
            data = {str(row['id']): payload_value(row['payload']) for row in packet['actor_data']['data']
                    if row['id'] in EFFECT_DATA}
            # The spawn tick re-sends every row; only changes after it count.
            if data and not (t == 0 and len(packet['actor_data']['data']) > 4):
                packets.append({'t': t, 'about': who, 'kind': 'data', 'data': data})
        elif name == 'UpdateAttributesPacket':
            rows = packet['attribute_list']
            if t == 0 and len(rows) > 3:
                continue
            packets.append({'t': t, 'about': who, 'kind': 'attributes',
                            'attributes': [attribute(row) for row in rows]})
        elif name == 'ActorEventPacket':
            packets.append({'t': t, 'about': who, 'kind': 'event', 'event': packet['event_id'],
                            'data': packet['data']})
    first = result['first_tick']
    damage = []
    for tick, actor_type, amount, cause in result['damage']:
        who = 'player' if actor_type == 'minecraft:player' else 'mob'
        damage.append({'t': tick - first, 'about': who, 'damage': round(amount, 6), 'cause': cause})
    samples = result['samples']
    out = {'mob': result['mob'], 'spec': result['spec'], 'failed_commands': result['failed_commands'],
           'damage': damage, 'packets': packets}
    # Positions only for a mob that moved (levitation, slow falling).
    positions = [[round(v, 6) for v in s['mob']['pos']] + [int(s['mob']['ground'])]
                 if s.get('mob') else None for s in samples]
    if result['mob'] and len({tuple(row) for row in positions if row}) > 1:
        out['mob_positions'] = positions
    return out


def main() -> int:
    out, dirs = Path(sys.argv[1]), [Path(arg) for arg in sys.argv[2:]]
    items, sources = {}, []
    for directory in dirs:
        loaded = json.loads((directory / 'loaded.json').read_text(encoding='utf-8'))
        sources.append({'sweep': directory.name, **loaded})
        records = decoded(directory / 'client-capture' / 'packets.jsonl')
        for line in (directory / 'results.jsonl').read_text(encoding='utf-8').splitlines():
            if line.strip():
                result = json.loads(line)
                items[result['key']] = distil(result, records)
    fixture = {
        'bds_version': '1.26.51',
        'protocol': sources[0]['protocol_version'],
        'sources': sources,
        'method': 'tools/bds-sweep/sweep.py effects --client-capture; Endstone per-tick samples and '
                  'ActorDamageEvent, the sweep player\'s packet capture tied to per-tick chat marks',
        'items': items,
    }
    out.write_text(json.dumps(fixture, separators=(',', ':')) + '\n', encoding='utf-8', newline='\n')
    print(f'{len(items)} items -> {out} ({out.stat().st_size} bytes)')
    return 0


if __name__ == '__main__':
    sys.exit(main())
