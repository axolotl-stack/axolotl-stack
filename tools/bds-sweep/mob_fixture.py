"""Distil `sweep.py bat|polar-bear|tadpole --client-capture` runs (the
generic per-tick job with `track`) into one committed mob fixture, e.g.
`crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-polar_bear.json`.

    python tools/bds-sweep/mob_fixture.py [--only KIND,...] OUT.json SWEEP_DIR [SWEEP_DIR ...]

Decodes each run's `client/packets.jsonl` with `bds-extractor --decode`
(cargo build -p bds-extractor first; `BDS_EXTRACTOR` overrides the
binary); an item of a later sweep replaces one of the same name, and
`--only` keeps the items whose name (without the trailing `-<n>`) is
listed. Per item
it keeps the item's spec (where the player stood, the setup commands and
the actions) and:

- the player's feet every tick it moved (`t`, x, y, z);

- per tracked actor, in order of first appearance: its type, the first tick
  it was sampled and every tick (`t`, feet, body yaw, pitch, velocity,
  ground contact, health);
- every damage event (tick, actor type, amount, cause);
- per tracked actor, its actor flags at `AddActor` and every flag that
  changed afterwards, every `ActorEvent` and every `LevelSoundEvent` it made,
  each on the tick of the `bds-sweep:mark` before it.

Only observed values are kept: no packet bytes, no login data.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
EXTRACTOR = Path(os.environ.get('BDS_EXTRACTOR') or REPO / 'target' / 'debug' / (
    'bds-extractor.exe' if sys.platform == 'win32' else 'bds-extractor'))
PACKETS = ['TextPacket', 'AddActorPacket', 'SetActorDataPacket', 'ActorEventPacket', 'LevelSoundEventPacket']
# The two actor-flag words of the actor data.
FLAG_WORDS = (0, 92)


def decoded(capture: Path) -> list[dict]:
    out = capture.with_name('decoded-mob.jsonl')
    subprocess.run([str(EXTRACTOR), '--decode', str(capture), '--only', ','.join(PACKETS), '--output', str(out)],
                   check=True, capture_output=True)
    return [json.loads(line) for line in out.read_text(encoding='utf-8').splitlines() if line.strip()]


def marked(records: list[dict]) -> dict[str, list[tuple[int, dict]]]:
    """Every record after a chat mark, grouped by the mark's item, plus the
    records before the first mark of an item under `None` keys."""
    out: dict[str, list[tuple[int, dict]]] = {}
    key, t = None, None
    for record in records:
        if record['name'] == 'TextPacket':
            message = json.dumps(record['packet'])
            if 'bds-sweep:mark ' in message:
                rest = message[message.index('bds-sweep:mark ') + len('bds-sweep:mark '):].split('"')[0]
                key, tick = rest.rsplit(' ', 1)
                t = int(tick)
            continue
        if key is not None:
            out.setdefault(key, []).append((t, record))
    return out


def data_items(entries: list[dict]) -> dict[int, object]:
    out = {}
    for entry in entries:
        (_, payload), = entry['payload'].items()
        out[entry['id']] = payload['value']
    return out


def flags_of(items: dict[int, object], previous: int | None) -> int | None:
    if not any(word in items for word in FLAG_WORDS):
        return previous
    low = items.get(0, (previous or 0) & 0xFFFFFFFFFFFFFFFF)
    high = items.get(92, (previous or 0) >> 64)
    return (int(high) << 64) | (int(low) & 0xFFFFFFFFFFFFFFFF)


def r4(values) -> list[float]:
    return [round(v, 4) for v in values]


def added_flags(records: list[dict]) -> dict[int, int | None]:
    """The actor flags of every AddActor of a capture, by runtime id: an
    item's AddActor can arrive before its first mark, after the previous
    item's last one."""
    return {record['packet']['target_runtime_id']['actor_runtime_id']:
            flags_of(data_items(record['packet']['actor_data']['data']), None)
            for record in records if record['name'] == 'AddActorPacket'}


def actor_packets(rows: list[tuple[int, dict]], runtime_ids: set[int],
                  adds: dict[int, int | None]) -> dict[int, dict]:
    """Per tracked runtime id: flags at AddActor, flag changes, actor events
    and sounds."""
    out: dict[int, dict] = {rid: {'flags': adds.get(rid), 'flag_changes': [], 'events': [], 'sounds': []}
                            for rid in runtime_ids}
    unique: dict[int, int] = {}
    for t, record in rows:
        packet, name = record['packet'], record['name']
        if name == 'AddActorPacket':
            rid = packet['target_runtime_id']['actor_runtime_id']
            if rid in out:
                unique[packet['target_actor_id']['actor_unique_id']] = rid
                out[rid]['flags'] = flags_of(data_items(packet['actor_data']['data']), None)
                out[rid]['added'] = t
        elif name == 'SetActorDataPacket':
            rid = packet['target_runtime_id']['actor_runtime_id']
            if rid not in out:
                continue
            entry = out[rid]
            previous = entry.get('current', entry['flags'])
            now = flags_of(data_items(packet['actor_data']['data']), previous)
            if previous is not None and now is not None and now != previous:
                changed = previous ^ now
                for bit in range(128):
                    if changed >> bit & 1:
                        entry['flag_changes'].append({'t': t, 'flag': bit, 'set': bool(now >> bit & 1)})
            entry['current'] = now
        elif name == 'ActorEventPacket':
            rid = packet['target_runtime_id']['actor_runtime_id']
            if rid in out:
                out[rid]['events'].append({'t': t, 'event': packet['event_id'], 'data': packet['data']})
        elif name == 'LevelSoundEventPacket':
            rid = unique.get(packet['actor_unique_id'])
            if rid is not None:
                out[rid]['sounds'].append({'t': t, 'sound': packet['sound_event']})
    for entry in out.values():
        entry.pop('current', None)
        if entry['flags'] is not None:
            entry['flags'] = str(entry['flags'])
    return out


def distil(result: dict, rows: list[tuple[int, dict]], adds: dict[int, int | None]) -> dict:
    spec = result['spec']
    actors: dict[int, dict] = {}
    for sample in result['samples']:
        for actor in sample.get('tracked', []):
            entry = actors.setdefault(actor['id'], {'type': actor['type'], 'first': sample['t'], 'ticks': []})
            entry['ticks'].append([sample['t'], *r4(actor['pos']), round(actor['rot'][1], 3),
                                   round(actor['rot'][0], 3), *r4(actor['vel']), int(bool(actor['ground'])),
                                   actor.get('health')])
    packets = actor_packets(rows, set(actors), adds)
    player, last = [], None
    for sample in result['samples']:
        feet = r4(sample['player']['pos'])
        if feet != last:
            player.append([sample['t'], *feet])
            last = feet
    return {
        'key': result['key'].split('|', 1)[1],
        'player': r4(spec['player']),
        'setup': spec['setup'],
        'actions': spec['actions'],
        'ticks': spec['ticks'],
        'player_ticks': player,
        'damage': [{'t': d[0] - result['first_tick'], 'type': d[1], 'amount': d[2], 'cause': d[3]}
                   for d in result['damage']],
        'actors': [{**actors[rid], **packets[rid]} for rid in sorted(actors, key=lambda r: actors[r]['first'])],
    }


def main() -> int:
    args = sys.argv[1:]
    only = None
    if args[:1] == ['--only'] and len(args) > 1:
        only = set(args[1].split(','))
        args = args[2:]
    if len(args) < 2:
        print(__doc__)
        return 2
    out = Path(args[0])
    items: dict[str, dict] = {}
    sources = []
    for directory in map(Path, args[1:]):
        records = decoded(directory / 'client' / 'packets.jsonl')
        rows, adds = marked(records), added_flags(records)
        job = json.loads((directory / 'job.json').read_text(encoding='utf-8'))
        sources.append(job['items'][0]['key'].split('|', 1)[0])
        for line in (directory / 'results.jsonl').read_text(encoding='utf-8').splitlines():
            result = json.loads(line)
            if only is not None and result['key'].split('|', 1)[1].rsplit('-', 1)[0] not in only:
                continue
            items[result['key']] = distil(result, rows.get(result['key'], []), adds)
    kinds = ', '.join(sorted(set(sources)))
    fixture = {
        'source': f'BDS 1.26.51, sweep.py {kinds} --client-capture (tools/bds-sweep/mob_fixture.py)',
        'columns': ['t', 'x', 'y', 'z', 'yaw', 'pitch', 'vx', 'vy', 'vz', 'ground', 'health'],
        'items': sorted(items.values(), key=lambda item: item['key']),
    }
    out.write_text(json.dumps(fixture, separators=(',', ':')).replace('{"key"', '\n{"key"') + '\n',
                   encoding='utf-8', newline='\n')
    print(f'{len(items)} items -> {out}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
