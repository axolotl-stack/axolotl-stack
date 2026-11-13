"""Distil `sweep.py bonus --keep-world` runs into the committed spawn-bonus
fixture (`crates/unastar/src/entity/mob/persistence/testdata/
bds-1.26.51-spawn-bonus.json`).

    cargo run -p unastar --example bds_actor_records -- RUN_DIR/world/db RUN_DIR/actor_records.jsonl
    python tools/bds-sweep/spawn_bonus_fixture.py OUT.json RUN_DIR [RUN_DIR ...]

Per run (one difficulty) and per actor identifier it keeps how many were
saved, the base of their `minecraft:follow_range`, and the `Amount` of every
`RandomSpawnBonus` modifier on `minecraft:follow_range` and on
`minecraft:knockback_resistance`, with each modifier's `Operation` and
`Operand`. Only these values are kept: no positions, ids or other record
fields.
"""

from __future__ import annotations

import json
import re
import struct
import sys
from pathlib import Path


def read_nbt(data: bytes):
    """A little-endian Bedrock NBT compound as plain Python values (tags
    typed as `{"Float": x}` and so on, strings that are not UTF-8 as
    `hex:...`)."""
    at = 0

    def take(n):
        nonlocal at
        chunk = data[at:at + n]
        at += n
        return chunk

    def name():
        (n,) = struct.unpack('<H', take(2))
        raw = take(n)
        try:
            return raw.decode('utf-8')
        except UnicodeDecodeError:
            return 'hex:' + raw.hex()

    def payload(kind):
        if kind == 1:
            return {'Byte': take(1)[0]}
        if kind == 2:
            return {'Short': struct.unpack('<h', take(2))[0]}
        if kind == 3:
            return {'Int': struct.unpack('<i', take(4))[0]}
        if kind == 4:
            return {'Long': struct.unpack('<q', take(8))[0]}
        if kind == 5:
            return {'Float': struct.unpack('<f', take(4))[0]}
        if kind == 6:
            return {'Double': struct.unpack('<d', take(8))[0]}
        if kind == 7:
            (n,) = struct.unpack('<i', take(4))
            return {'ByteArray': take(n).hex()}
        if kind == 8:
            return name()
        if kind == 9:
            inner = take(1)[0]
            (n,) = struct.unpack('<i', take(4))
            return [payload(inner) for _ in range(n)]
        if kind == 10:
            out = {}
            while True:
                tag = take(1)[0]
                if tag == 0:
                    return out
                key = name()
                out[key] = payload(tag)
        if kind == 11:
            (n,) = struct.unpack('<i', take(4))
            return {'IntArray': list(struct.unpack(f'<{n}i', take(4 * n)))}
        if kind == 12:
            (n,) = struct.unpack('<i', take(4))
            return {'LongArray': list(struct.unpack(f'<{n}q', take(8 * n)))}
        raise ValueError(f'NBT tag {kind} at byte {at}')

    kind = take(1)[0]
    name()
    return payload(kind)


def difficulty(job: dict) -> str:
    for line in job.get('setup_commands', []):
        found = re.fullmatch(r'difficulty (\w+)', line)
        if found:
            return found.group(1)
    raise SystemExit('the job sets no difficulty')


def distil(run: Path) -> dict:
    job = json.loads((run / 'job.json').read_text(encoding='utf-8'))
    classes: dict[str, dict] = {}
    for line in (run / 'actor_records.jsonl').read_text(encoding='utf-8').splitlines():
        record = read_nbt(bytes.fromhex(json.loads(line)['hex']))
        identifier = record.get('identifier')
        if not isinstance(identifier, str) or identifier in ('minecraft:item', 'minecraft:arrow'):
            continue
        row = classes.setdefault(identifier, {'count': 0, 'follow_range_base': None, 'follow_range': [],
                                              'knockback_resistance': [], 'operations': {}})
        row['count'] += 1
        for attribute in record.get('Attributes', []):
            name = attribute['Name']
            if name == 'minecraft:follow_range':
                row['follow_range_base'] = attribute['Base']['Float']
            for modifier in attribute.get('Modifiers', []):
                if modifier['Name'] != 'RandomSpawnBonus':
                    continue
                key = name.removeprefix('minecraft:')
                if key not in ('follow_range', 'knockback_resistance'):
                    raise SystemExit(f'unexpected spawn bonus on {name}')
                row[key].append(modifier['Amount']['Float'])
                row['operations'][key] = [modifier['Operation']['Int'], modifier['Operand']['Int']]
    return {'difficulty': difficulty(job), 'classes': dict(sorted(classes.items()))}


def main() -> int:
    out, runs = Path(sys.argv[1]), [Path(arg) for arg in sys.argv[2:]]
    loaded = json.loads((runs[0] / 'loaded.json').read_text(encoding='utf-8'))
    fixture = {
        'bds_version': '1.26.51',
        'protocol': loaded['protocol_version'],
        'method': 'tools/bds-sweep/sweep.py bonus --difficulty easy|hard --keep-world: 870 mobs spawned frozen '
                  'around the sweep player and left in the world; the attributes BDS saved for each are read '
                  'from the kept world (examples/bds_actor_records.rs, spawn_bonus_fixture.py)',
        'runs': [distil(run) for run in runs],
    }
    out.write_text(json.dumps(fixture, separators=(',', ':')) + '\n', encoding='utf-8', newline='\n')
    total = sum(row['count'] for run in fixture['runs'] for row in run['classes'].values())
    print(f'{total} mobs in {len(runs)} runs -> {out} ({out.stat().st_size} bytes)')
    return 0


if __name__ == '__main__':
    sys.exit(main())
