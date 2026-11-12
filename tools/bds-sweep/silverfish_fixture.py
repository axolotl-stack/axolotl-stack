"""Distil `sweep.py silverfish --client-capture` runs into the committed
silverfish fixture
(`crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-silverfish.json`).

    python tools/bds-sweep/silverfish_fixture.py OUT.json SWEEP_DIR [SWEEP_DIR ...]

Decodes each run's `client/packets.jsonl` with `bds-extractor --decode`
(cargo build -p bds-extractor first). Merge items (`merge`, `pit`, `blocks`,
`griefing`) keep the block the pit was cut into, whether mobGriefing was on,
the silverfish's spawn feet, the tick of its `SilverfishMergeAnim` event,
the block that changed (position and network id) and the tick of its
`RemoveActor`. Wake items (`wake`, `wake-nogrief`) keep the hurt tick, the
infested cells and, per tick, the blocks whose destroy particles (level
event 2001) showed, the blocks set without particles (`UpdateBlock` alone,
with their network id) and where each released silverfish's `AddActor`
stood. Ticks are the sample (`t`) whose `bds-sweep:mark` precedes the
packet. Only observed values are kept: no packet bytes, no login data.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
EXTRACTOR = REPO / 'target' / 'debug' / ('bds-extractor.exe' if sys.platform == 'win32' else 'bds-extractor')
PACKETS = ['TextPacket', 'ActorEventPacket', 'UpdateBlockPacket', 'UpdateSubChunkBlocksPacket', 'RemoveActorPacket',
           'AddActorPacket', 'LevelEventPacket']
DESTROY_PARTICLES = 2001
SILVERFISH = 'minecraft:silverfish'


def decoded(capture: Path) -> list[dict]:
    out = capture.with_name('decoded-silverfish.jsonl')
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


def block_changes(packet: dict, name: str) -> list[tuple[list[int], int]]:
    """`(position, network id)` of every block an update packet sets on layer 0."""
    if name == 'UpdateBlockPacket':
        if packet['layer'] != 0:
            return []
        pos = packet['block_position']
        return [([pos['x'], pos['y'], pos['z']], packet['block_runtime_id'])]
    changes = packet['blocks_changed']['blocks_changed_standards']
    return [([c['pos']['x'], c['pos']['y'], c['pos']['z']], c['runtime_id']) for c in changes]


def setup_block(result: dict) -> tuple[str, dict]:
    """The block of a pit's `fill 4 -61 0 6 -58 2 <block>` and its states."""
    for line in result['spec']['setup']:
        if line.startswith('fill 4 -61 0 6 -58 2 '):
            spec = line[len('fill 4 -61 0 6 -58 2 '):]
            name, _, states = spec.partition(' ')
            return f'minecraft:{name}', json.loads('{' + states.strip('[]').replace('=', ':') + '}') if states else {}
    raise SystemExit(f'{result["key"]}: no pit fill in its setup')


def merge_item(result: dict, rows: list[tuple[int, dict]]) -> dict:
    runtime_id = result['runtime_id']
    at = result['spec']['at']
    block, states = setup_block(result) if 'merge' not in result['key'] else ('minecraft:stone', {})
    item = {'key': result['key'].split('|')[1], 'block': block, 'states': states,
            'griefing': not any('mobgriefing false' in line for line in result['spec']['setup']),
            'feet': [int(at[0] // 1), int(at[1] // 1), int(at[2] // 1)], 'ticks': result['spec']['ticks'],
            'merge_anim': None, 'changed': None, 'removed': None}
    for t, record in rows:
        packet, name = record['packet'], record['name']
        if name == 'ActorEventPacket' and packet['event_id'] == 'SilverfishMergeAnim' \
                and packet['target_runtime_id']['actor_runtime_id'] == runtime_id:
            item['merge_anim'] = t
        elif name in ('UpdateBlockPacket', 'UpdateSubChunkBlocksPacket') and item['merge_anim'] is not None \
                and item['changed'] is None:
            changes = block_changes(packet, name)
            if changes:
                pos, network_id = changes[0]
                item['changed'] = {'t': t, 'pos': pos, 'network_id': network_id}
        elif name == 'RemoveActorPacket' and item['merge_anim'] is not None and item['removed'] is None:
            item['removed'] = t
    return item


def wake_item(result: dict, rows: list[tuple[int, dict]]) -> dict:
    hurt = next(a['t'] for a in result['spec']['actions'] if any('damage' in c for c in a['commands']))
    cells = [[int(w) for w in line.split()[1:4]] for line in result['spec']['setup']
             if line.startswith('setblock ') and line.endswith(' infested_stone')]
    at = result['spec']['at']
    ticks: dict[int, dict] = {}
    for t, record in rows:
        packet, name = record['packet'], record['name']
        if name == 'LevelEventPacket' and packet['event_id'] == DESTROY_PARTICLES:
            pos = packet['position']
            row = ticks.setdefault(t, {'t': t, 'destroyed': [], 'set': [], 'released': []})
            row['destroyed'].append({'pos': [int(pos['x'] // 1), int(pos['y'] // 1), int(pos['z'] // 1)],
                                     'network_id': packet['data']})
        elif name in ('UpdateBlockPacket', 'UpdateSubChunkBlocksPacket'):
            for pos, network_id in block_changes(packet, name):
                if pos in cells:
                    row = ticks.setdefault(t, {'t': t, 'destroyed': [], 'set': [], 'released': []})
                    row['set'].append({'pos': pos, 'network_id': network_id})
        elif name == 'AddActorPacket' and packet['actor_type'] == SILVERFISH \
                and packet['target_runtime_id']['actor_runtime_id'] != result['runtime_id']:
            pos = packet['position']
            row = ticks.setdefault(t, {'t': t, 'destroyed': [], 'set': [], 'released': []})
            row['released'].append([pos['x'], pos['y'], pos['z']])
    return {'key': result['key'].split('|')[1],
            'griefing': not any('mobgriefing false' in line for line in result['spec']['setup']),
            'feet': [int(at[0] // 1), int(at[1] // 1), int(at[2] // 1)], 'hurt': hurt, 'ticks': result['spec']['ticks'],
            'cells': cells, 'events': [ticks[t] for t in sorted(ticks)]}


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    out = Path(sys.argv[1])
    fixture = {'source': 'BDS 1.26.51, tools/bds-sweep/sweep.py silverfish --client-capture, distilled by '
                         'tools/bds-sweep/silverfish_fixture.py',
               'merge': [], 'wake': []}
    for sweep in map(Path, sys.argv[2:]):
        rows = marked(decoded(sweep / 'client' / 'packets.jsonl'))
        for line in (sweep / 'results.jsonl').read_text(encoding='utf-8').splitlines():
            result = json.loads(line)
            item_rows = rows.get(result['key'], [])
            if '|wake' in result['key']:
                fixture['wake'].append(wake_item(result, item_rows))
            else:
                fixture['merge'].append(merge_item(result, item_rows))
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(fixture, indent=1) + '\n', encoding='utf-8', newline='\n')
    return 0


if __name__ == '__main__':
    sys.exit(main())
