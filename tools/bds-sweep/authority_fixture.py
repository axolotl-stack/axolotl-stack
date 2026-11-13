"""Distil movement-authority probes (`player_script.py` runs of a subject
whose sweep client misreports its position on purpose, and a viewer) into a
compact value-only fixture: per phase the subject's inputs, BDS's own copy
of the subject as the viewer saw it (`MovePlayer` relays), the corrections
and teleports BDS sent the subject.

    python tools/bds-sweep/authority_fixture.py SPEC.json OUT.json --captures DIR

SPEC is JSON:

    {"sources": [{"capture": "<dir>", "setup": "<what the run did>"}],
     "phases": {"<name>": {"capture": "<dir name>", "ticks": [LO, HI],
                           "note": "<what the phase shows>"}}}

Each phase keeps the inputs with client ticks LO..HI as
`[tick, x, y, z, pitch, yaw, strafe, forward, flags]` (the flags the
movement simulation reads), the relays of those inputs as
`[tick, x, y, z, on_ground]`, the corrections as
`[tick, x, y, z, vx, vy, vz, on_ground]` and the teleports stamped LO..HI as
`[tick, x, y, z]`. Every correction in the sources must be a player
correction with rotation (0, 0) and no vehicle angular velocity (checked,
and recorded under `correction_fields`). The subject is `Subject` and the
viewer `Viewer`, the subject's runtime id 1. Needs `bds-sweep-dump`
(cargo build -p bds-sweep).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import player_fixture  # noqa: E402

RID = 1
V3 = r'Vec3 \{ x: ([-\d.e]+), y: ([-\d.e]+), z: ([-\d.e]+) \}'
V2 = r'Vec2 \{ x: ([-\d.e]+), y: ([-\d.e]+) \}'
INPUT = re.compile(r'player_rotation: ' + V2 + r', position: ' + V3 + r', move_vector: ' + V2)
FLAGS = re.compile(r'input_data: \[([^\]]*)\]')
RELAY = re.compile(r'position: ' + V3 + r'.*position_mode: (\w+), on_ground: (\w+)')
# The flags the movement simulation and the authority read.
KEPT = ('Jumping', 'Sneaking', 'Sprintdown', 'Startgliding', 'Handledteleport', 'Verticalcollision',
        'Startjumping')


def number(text: str) -> float:
    value = float(text)
    return int(value) if value == int(value) and abs(value) < 1e9 else value


def inputs(subject: Path, lo: int, hi: int) -> list[list]:
    out = []
    for _seq, direction, _name, body in player_fixture.decoded(subject, ['PlayerAuthInput']):
        tick = int(player_fixture.TICK.search(body).group(1))
        if direction != 'c2s' or not lo <= tick <= hi:
            continue
        m = INPUT.search(body)
        pitch, yaw, x, y, z, strafe, forward = (number(v) for v in m.groups())
        flags = [f for f in FLAGS.search(body).group(1).split(', ') if f in KEPT]
        out.append([tick, x, y, z, pitch, yaw, strafe, forward, flags])
    return out


def correction_fields(subject: Path) -> int:
    """Check that every correction the subject got has the fields BDS sends
    whatever the input; returns how many there were."""
    count = 0
    for seq, direction, _name, body in player_fixture.decoded(subject, ['CorrectPlayerMovePrediction']):
        if direction != 's2c':
            continue
        fields = ('prediction_type: Player', 'rotation: Vec2 { x: 0.0, y: 0.0 }',
                  'vehicle_angular_velocity: None')
        missing = [field for field in fields if field not in body]
        if missing:
            raise SystemExit(f'{subject}: correction {seq} lacks {missing}')
        count += 1
    return count


def relays(viewer: Path, lo: int, hi: int) -> list[list]:
    out = []
    for _seq, direction, _name, body in player_fixture.decoded(viewer, ['MovePlayer']):
        actor = player_fixture.ACTOR.search(body)
        if direction != 's2c' or not actor or int(actor.group(1)) != RID:
            continue
        tick = int(player_fixture.TICK.search(body).group(1))
        m = RELAY.search(body)
        if lo <= tick <= hi and m.group(4) == 'Normal':
            out.append([tick, number(m.group(1)), number(m.group(2)), number(m.group(3)),
                        m.group(5) == 'true'])
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('spec', type=Path)
    parser.add_argument('out', type=Path)
    parser.add_argument('--captures', type=Path, required=True,
                        help='directory holding the capture directories the spec names')
    args = parser.parse_args()
    if not player_fixture.DUMP.exists():
        parser.error(f'{player_fixture.DUMP} missing: cargo build -p bds-sweep')
    spec = json.loads(args.spec.read_text(encoding='utf-8'))
    sources = []
    corrections_seen = 0
    for source in spec['sources']:
        clients = {}
        for capture in sorted((args.captures / source['capture']).glob('*/packets.jsonl')):
            clients[capture.parent.name] = hashlib.sha256(capture.read_bytes()).hexdigest()
        sources.append({'capture': source['capture'], 'clients': clients, 'setup': source['setup']})
        corrections_seen += correction_fields(args.captures / source['capture'] / 'Subject' / 'packets.jsonl')
    phases = {}
    for name, body in spec['phases'].items():
        lo, hi = body['ticks']
        root = args.captures / body['capture']
        subject = root / 'Subject' / 'packets.jsonl'
        phases[name] = {
            'capture': body['capture'], 'ticks': [lo, hi], 'note': body['note'],
            'inputs': inputs(subject, lo, hi),
            'relays': relays(root / 'Viewer' / 'packets.jsonl', lo, hi),
            'corrections': player_fixture.corrections(subject, lo, hi),
            'teleports': player_fixture.teleports(subject, RID, lo, hi),
        }
    fixture = {
        'bds_version': '1.26.51',
        'protocol': 2193,
        'sources': sources,
        'columns': {
            'inputs': 'tick, x, y, z (the reported eyes), pitch, yaw, strafe, forward, flags',
            'relays': "tick, x, y, z, on_ground of BDS's copy after that input",
            'corrections': 'tick, x, y, z, vx, vy, vz, on_ground',
            'teleports': 'tick of the input it followed, x, y, z',
        },
        'correction_fields': {
            'prediction_type': 'Player', 'rotation': [0, 0], 'vehicle_angular_velocity': None,
            'corrections': corrections_seen,
        },
        'phases': phases,
    }
    text = json.dumps(fixture, indent=None, separators=(',', ':'))
    # One phase list entry a line keeps the file diffable.
    text = text.replace('],[', '],\n[')
    args.out.write_text(text + '\n', encoding='utf-8', newline='\n')
    print(f'{args.out}: {len(phases)} phases, {len(text)} bytes')
    return 0


if __name__ == '__main__':
    sys.exit(main())
