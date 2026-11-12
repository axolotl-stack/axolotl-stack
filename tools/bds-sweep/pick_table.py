"""Build the measured actor pick table from a `protocol_probe.py pick-actors` run.

    python tools/bds-sweep/pick_table.py RUN_DIR [RUN_DIR ...]

For every `/summon` of each run's `pick-actors` session it reads the actor BDS
announced (`AddActor`), then BDS's answers to the creative pick without data
and with data: the `GuiDataPickItem` name and the item that landed in the
inventory (`InventoryContent`). Writes
`crates/unastar-data/data/measured/actor_pick_items.json`:

- `actors`: `{actor: {"item", "aux", "name"}}` for actors whose pick gives an
  item, plus `"data_user_data"` (hex) when the pick with data gave the item
  user data;
- `no_item`: actors BDS announced whose picks got no answer in every run
  (an answer in any run counts: a pick of an actor that moved or died is
  missed, so a recheck run with a longer settle fills those in);
- `source_sha256`: each run's `packets.jsonl` SHA-256.

Packets are decoded with `target/debug/bds-extractor --dump`.
"""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
OUT = REPO / 'crates' / 'unastar-data' / 'data' / 'measured' / 'actor_pick_items.json'
PACKETS = ['CommandRequestPacket', 'AddActorPacket', 'ActorPickRequestPacket', 'GuiDataPickItemPacket',
           'InventoryContentPacket']


def item_names() -> dict[int, str]:
    data = json.loads((REPO / 'crates' / 'unastar-data' / 'src' / 'items.json').read_text(encoding='utf-8'))
    rows = data if isinstance(data, list) else data['items']
    return {row['network_id']: row['identifier'] for row in rows}


def records(packets: Path) -> list[dict]:
    extractor = REPO / 'target' / 'debug' / 'bds-extractor.exe'
    out = subprocess.run([str(extractor), '--dump', str(packets), '--packets', ','.join(PACKETS)],
                         capture_output=True, text=True, check=True).stdout
    return [json.loads(line) for line in out.splitlines()]


def summons(rows: list[dict]) -> list[dict]:
    """One entry per `/summon`: the announced actor and the two picks' answers."""
    found: list[dict] = []
    current: dict | None = None
    for row in rows:
        packet, name = row['packet'], row['name']
        if name == 'CommandRequestPacket':
            current = None
            if packet['command'].startswith('/summon '):
                current = {'command': packet['command'], 'actor': None, 'picks': []}
                found.append(current)
        elif current is None:
            continue
        elif name == 'AddActorPacket' and current['actor'] is None:
            current['actor'] = packet['actor_type']
        elif name == 'ActorPickRequestPacket':
            current['picks'].append({'data': packet['with_data'], 'name': None, 'index': None, 'slot': None})
        elif name == 'GuiDataPickItemPacket' and current['picks']:
            current['picks'][-1]['name'] = packet['item_name']
            current['picks'][-1]['index'] = packet['slot']
        elif name == 'InventoryContentPacket' and packet['container_id'] == 0 and current['picks']:
            # The slot the pick selected (`GuiDataPickItem`), the picked stack.
            pick = current['picks'][-1]
            if pick['index'] is not None:
                pick['slot'] = packet['slots'][pick['index']]
    return found


def table(found: list[dict], names: dict[int, str]) -> tuple[dict, list[str]]:
    actors: dict[str, dict] = {}
    no_item: set[str] = set()
    for summon in found:
        actor = summon['actor']
        if actor is None or actor in actors:
            continue
        plain = next((pick for pick in summon['picks'] if not pick['data']), None)
        if plain is None or plain['slot'] is None:
            no_item.add(actor)
            continue
        slot = plain['slot']
        entry = {'item': names[slot['id']], 'aux': slot['auxvalue'], 'name': plain['name']}
        with_data = next((pick for pick in summon['picks'] if pick['data']), None)
        # The with-data pick adds a second stack when its user data differs.
        if with_data is not None and with_data['slot'] is not None:
            buffer = with_data['slot']['user_data_buffer']
            if buffer != slot['user_data_buffer']:
                entry['data_user_data'] = buffer
        actors[actor] = entry
    return dict(sorted(actors.items())), sorted(no_item - actors.keys())


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    runs = [Path(run).resolve() / 'pick-actors' / 'packets.jsonl' for run in sys.argv[1:]]
    found = [summon for packets in runs for summon in summons(records(packets))]
    actors, no_item = table(found, item_names())
    document = {
        'bds_version': '1.26.51',
        'method': 'protocol_probe.py pick-actors: creative player with PlayerAuthInput, /summon <actor> ~ ~ ~3, '
                  'ActorPickRequest without then with data on an empty inventory; actors without an answer picked '
                  'again 1.5 s after a second summon',
        'source_sha256': [hashlib.sha256(packets.read_bytes()).hexdigest() for packets in runs],
        'actors': actors,
        'no_item': no_item,
    }
    OUT.write_text(json.dumps(document, indent=1) + '\n', encoding='utf-8', newline='\n')
    print(f'{len(actors)} actors with an item, {len(no_item)} without; wrote {OUT}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
