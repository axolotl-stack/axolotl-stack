"""Distil `sweep.py slime --client-capture` runs into the committed slime
fixture (`crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-slime.json`).

    python tools/bds-sweep/slime_fixture.py OUT.json SWEEP_DIR [SWEEP_DIR ...]

Decodes each run's `client/packets.jsonl` with `bds-extractor --decode`
(cargo build -p bds-extractor first; `BDS_EXTRACTOR` overrides the
binary); an item of a later sweep replaces one of the same name. Per item
it keeps the mob, its size, the setup, and:

- `hop` and `float` items: the summoned mob's first ticks (`t`, feet,
  ground contact, body yaw) from the plugin's samples, every tick its
  ground contact or yaw changed, and its hops (the tick it landed, the
  tick it left the ground, its yaw then);
- `attack` items: the same, the player's feet, and every damage the
  player took (tick, amount, cause);
- `split` items: the tick the mob was hurt, where its body was last seen
  (its last absolute position in the capture), and each child's AddActor
  (tick, position, `VARIANT`, box) and `born` sound.

A `slime-spawn` run adds `spawns`: where (and on which tick) each slime
BDS spawned stood. Only observed values are kept: no packet bytes, no login
data.
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
PACKETS = ['TextPacket', 'AddActorPacket', 'MoveActorDeltaPacket', 'MoveActorAbsolutePacket',
           'LevelSoundEventPacket', 'RemoveActorPacket']
VARIANT, WIDTH, HEIGHT = 2, 53, 54
PLAYER_DAMAGE = 'minecraft:player'
# Ticks of each hopping item kept tick by tick (the physics of a few hops);
# the rest is summarised by `hops` and `changes`.
TRACE_TICKS = 120


def decoded(capture: Path) -> list[dict]:
    out = capture.with_name('decoded-slime.jsonl')
    subprocess.run([str(EXTRACTOR), '--decode', str(capture), '--only', ','.join(PACKETS), '--output', str(out)],
                   check=True, capture_output=True)
    return [json.loads(line) for line in out.read_text(encoding='utf-8').splitlines() if line.strip()]


def marked(records: list[dict]) -> dict[str, list[tuple[int, dict]]]:
    """Every record after the first chat mark, grouped by the mark's item."""
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


def r4(values) -> list[float]:
    return [round(v, 4) for v in values]


def trace(result: dict) -> tuple[int | None, list[list]]:
    """The summoned mob (the tracked actor present on tick 0) every tick:
    `[t, x, y, z, ground, yaw]`."""
    first = result['samples'][0].get('tracked') or []
    if not first:
        return None, []
    mob = first[0]['id']
    rows = []
    for sample in result['samples']:
        for actor in sample.get('tracked', []):
            if actor['id'] == mob:
                rows.append([sample['t'], *r4(actor['pos']), int(actor['ground']), round(actor['rot'][1], 2)])
    return mob, rows


def hops(rows: list[list]) -> list[dict]:
    """Each hop: the tick the mob landed (None for the first, from the
    spawn), the tick it left the ground and its yaw then."""
    out, landed, ground = [], None, None
    for t, _x, y, _z, on, yaw in rows:
        if ground is not None:
            if on and not ground:
                landed = t
            elif ground and not on:
                out.append({'landed': landed, 'jumped': t, 'yaw': yaw})
        ground = bool(on)
    return out


def changes(rows: list[list]) -> list[list]:
    """Every tick the mob's ground contact or yaw changed: `[t, ground,
    yaw]` (the first row is tick 0)."""
    out, last = [], None
    for t, _x, _y, _z, on, yaw in rows:
        if last is None or (on, yaw) != last:
            out.append([t, on, yaw])
            last = (on, yaw)
    return out


def split(result: dict, rows: list[tuple[int, dict]], mob: int | None) -> dict:
    hurt = next(action['t'] for action in result['spec']['actions'])
    last, children, born = None, [], []
    ids = set()
    for t, record in rows:
        packet, name = record['packet'], record['name']
        if name == 'MoveActorDeltaPacket':
            move = packet['move_data']
            if move['actor_runtime_id']['actor_runtime_id'] == mob:
                last = [move['new_position_x'] if move['new_position_x'] is not None else (last or [None] * 3)[0],
                        move['new_position_y'] if move['new_position_y'] is not None else (last or [None] * 3)[1],
                        move['new_position_z'] if move['new_position_z'] is not None else (last or [None] * 3)[2]]
        elif name == 'AddActorPacket' and packet['actor_type'] == result['spec']['setup'][-1].split()[1] \
                and hurt < t < result['spec']['ticks'] \
                and packet['target_runtime_id']['actor_runtime_id'] != mob:
            data = data_items(packet['actor_data']['data'])
            pos = packet['position']
            ids.add(packet['target_actor_id']['actor_unique_id'])
            children.append({'t': t, 'pos': r4([pos['x'], pos['y'], pos['z']]), 'variant': data[VARIANT],
                             'box': [data[WIDTH], data[HEIGHT]], 'yaw': round(packet['y_body_rotation'], 2)})
        elif name == 'LevelSoundEventPacket' and packet['sound_event'] == 'born' \
                and packet['actor_unique_id'] in ids:
            pos = packet['position']
            born.append({'t': t, 'pos': r4([pos['x'], pos['y'], pos['z']])})
    return {'hurt': hurt, 'body': r4(last) if last and None not in last else None, 'children': children,
            'born': born}


def distil(result: dict, rows: list[tuple[int, dict]]) -> dict:
    mob_name, case = result['key'].split('|')
    summon = result['spec']['setup'][-1].split()
    item = {'key': result['key'], 'mob': mob_name, 'size': summon[-1].removeprefix('spawn_'),
            'kind': case.split('-')[0], 'at': [float(v) for v in summon[2:5]], 'yaw': float(summon[5]),
            'ticks': result['spec']['ticks']}
    mob, path = trace(result)
    if item['kind'] in ('split', 'kill'):
        item['split'] = split(result, rows, mob)
        item['trace'] = [row for row in path if row[0] <= item['split']['hurt']]
        return item
    item['trace'] = path[:TRACE_TICKS + 1]
    item['hops'] = hops(path)
    item['changes'] = changes(path)
    if item['kind'] == 'attack':
        first = result['first_tick']
        item['player'] = [[s['t'], *r4(s['player']['pos'])] for s in result['samples']]
        item['damage'] = [{'t': d[0] - first, 'amount': d[2], 'cause': d[3]}
                          for d in result['damage'] if d[1] == PLAYER_DAMAGE and d[3] != 'self_destruct']
    return item


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    out = Path(sys.argv[1])
    items: dict[str, dict] = {}
    spawns: list[dict] = []
    for sweep in map(Path, sys.argv[2:]):
        results = [json.loads(line) for line in
                   (sweep / 'results.jsonl').read_text(encoding='utf-8').splitlines()]
        if all('spawns' in result for result in results):
            # `slime-spawn`: where each naturally spawned slime stood.
            spawns += [{'key': result['key'], 't': spawn['t'], 'pos': r4(spawn['pos'])}
                       for result in results for spawn in result['spawns']]
            continue
        rows = marked(decoded(sweep / 'client' / 'packets.jsonl'))
        for result in results:
            items[result['key']] = distil(result, rows.get(result['key'], []))
    fixture = {'source': 'BDS 1.26.51, tools/bds-sweep/sweep.py slime --client-capture and slime-spawn, '
                         'distilled by tools/bds-sweep/slime_fixture.py',
               'items': list(items.values()), 'spawns': spawns}
    out.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(fixture, separators=(',', ':')).replace('{"key"', '\n{"key"')
    out.write_text(text + '\n', encoding='utf-8', newline='\n')
    return 0


if __name__ == '__main__':
    sys.exit(main())
