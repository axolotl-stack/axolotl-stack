"""Distil `sweep.py drowned --client-capture` runs into the committed drowned
fixture (`crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-drowned.json`).

    python tools/bds-sweep/drowned_fixture.py OUT.json SWEEP_DIR [SWEEP_DIR ...]

Decodes each run's `client/packets.jsonl` with `bds-extractor --decode`
(cargo build -p bds-extractor first; `BDS_EXTRACTOR` overrides the
binary); an item of a later sweep replaces one of the same name. Per item
it keeps the player's feet, and:

- the first summoned mob's ticks (`t`, feet, body yaw, pitch, ground
  contact) from the plugin's samples, as many as `TRACE_TICKS` says;
- every damage the player took (tick, amount, cause);
- the tick each thrown trident was first sampled, with its position and
  velocity then;
- for `convert` items, the tick the summoned mob was last sampled and the
  type that replaced it, its first tick, and the item it held;
- for `equipment` items, each drowned's main hand and off hand item from
  its `MobEquipment` packets (`equipment` sums them).

Only observed values are kept: no packet bytes, no login data.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
EXTRACTOR = Path(os.environ.get('BDS_EXTRACTOR') or REPO / 'target' / 'debug' / (
    'bds-extractor.exe' if sys.platform == 'win32' else 'bds-extractor'))
PACKETS = ['MobEquipmentPacket', 'ItemRegistryPacket']
# Ticks of the summoned mob kept tick by tick, per item kind; other kinds
# keep none.
TRACE_TICKS = {'sink': 120, 'hunt-floor': 240, 'hunt-surface': 240, 'hunt-land': 120}
TRIDENT = 'minecraft:thrown_trident'
OFFHAND = 119


def decoded(capture: Path) -> list[dict]:
    out = capture.with_name('decoded-drowned.jsonl')
    subprocess.run([str(EXTRACTOR), '--decode', str(capture), '--only', ','.join(PACKETS), '--output', str(out)],
                   check=True, capture_output=True)
    return [json.loads(line) for line in out.read_text(encoding='utf-8').splitlines() if line.strip()]


def item_names(records: list[dict]) -> dict[int, str]:
    for record in records:
        if record['name'] == 'ItemRegistryPacket':
            return {entry['item_id']: entry['item_name'] for entry in record['packet']['item_data']}
    raise SystemExit('no ItemRegistryPacket in the capture')


def r4(values) -> list[float]:
    return [round(v, 4) for v in values]


def equipment(records: list[dict], names: dict[int, str]) -> dict[int, dict[str, str]]:
    """Per actor runtime id: the last item seen in the main hand and off hand."""
    held: dict[int, dict[str, str]] = {}
    for record in records:
        if record['name'] != 'MobEquipmentPacket':
            continue
        packet = record['packet']
        slot = 'offhand' if packet['container_id'] == OFFHAND else 'mainhand'
        item = names.get(packet['item']['id'], 'minecraft:air') if packet['item']['id'] else 'minecraft:air'
        held.setdefault(packet['target_runtime_id']['actor_runtime_id'], {})[slot] = item
    return held


def distil(result: dict, held: dict[int, dict[str, str]]) -> dict:
    key = result['key']
    kind = key.split('|', 1)[1].rsplit('-', 1)[0]
    item = {'key': key, 'kind': kind.replace('-', '_'), 'player': r4(result['samples'][0]['player']['pos'])}
    first: dict[int, tuple[int, str]] = {}
    last: dict[int, int] = {}
    trace, throws = [], []
    for sample in result['samples']:
        for actor in sample.get('tracked', []):
            rid = actor['id']
            if rid not in first:
                first[rid] = (sample['t'], actor['type'])
                if actor['type'] == TRIDENT:
                    throws.append({'t': sample['t'], 'pos': r4(actor['pos']), 'vel': r4(actor['vel'])})
            last[rid] = sample['t']
    mobs = [rid for rid, (_, kind_) in first.items() if kind_ != TRIDENT]
    if kind == 'equipment':
        item['held'] = [held.get(rid, {}) for rid in sorted(mobs)]
        return item
    mob = min(mobs, key=lambda rid: first[rid][0]) if mobs else None
    for sample in result['samples']:
        if sample['t'] >= TRACE_TICKS.get(kind, 0):
            break
        for actor in sample.get('tracked', []):
            if actor['id'] == mob:
                trace.append([sample['t'], *r4(actor['pos']), round(actor['rot'][1], 2), round(actor['rot'][0], 2),
                              int(bool(actor['ground']))])
    item['trace'] = trace
    item['throws'] = throws
    item['damage'] = [{'t': d[0] - result['first_tick'], 'amount': d[2], 'cause': d[3]}
                      for d in result['damage'] if d[1] == 'minecraft:player']
    if kind.startswith('convert'):
        successor = [rid for rid in mobs if rid != mob]
        if successor:
            rid = successor[0]
            item['converted'] = {'last': last[mob], 'first': first[rid][0], 'into': first[rid][1],
                                 'held': held.get(rid, {})}
    return item


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    out = Path(sys.argv[1])
    items: dict[str, dict] = {}
    for directory in map(Path, sys.argv[2:]):
        records = decoded(directory / 'client' / 'packets.jsonl')
        # A mob's equipment arrives with its AddActor, which may precede the
        # first mark of its item, so it is looked up by runtime id run-wide.
        held = equipment(records, item_names(records))
        for line in (directory / 'results.jsonl').read_text(encoding='utf-8').splitlines():
            result = json.loads(line)
            items[result['key']] = distil(result, held)
    hands = [held for item in items.values() for held in item.get('held', [])]
    fixture = {
        'source': 'BDS 1.26.51, sweep.py drowned --client-capture (tools/bds-sweep/drowned_fixture.py)',
        'items': sorted(items.values(), key=lambda item: item['key']),
        'equipment': {
            'total': len(hands),
            'mainhand': dict(sorted(Counter(h.get('mainhand', 'minecraft:air') for h in hands).items())),
            'offhand': dict(sorted(Counter(h.get('offhand', 'minecraft:air') for h in hands).items())),
        },
    }
    out.write_text(json.dumps(fixture, indent=1) + '\n', encoding='utf-8', newline='\n')
    print(f'{len(items)} items, {len(hands)} equipped drowned -> {out}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
