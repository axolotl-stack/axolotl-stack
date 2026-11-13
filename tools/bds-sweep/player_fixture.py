"""Distil scripted player captures (`player_script.py` runs) into a value-only
test fixture: per phase, the subject's `PlayerAuthInput` packets and the
packets one client received about the subject, as hex.

    python tools/bds-sweep/player_fixture.py SPEC.json OUT.json

SPEC is JSON:

    {"sources": [{"capture": "<dir>", "setup": "<what the run did>"}],
     "phases": {"<name>": {"capture": "<dir name>", "client": "<recording client>",
                           "subject": "<client whose inputs are kept>",
                           "subject_runtime_id": 1, "ticks": [LO, HI],
                           "packets": ["SetActorData", "UpdateAttributes"],
                           "note": "<what the phase shows>"}}}

A phase keeps the subject's inputs with client ticks LO..HI and, from the
recording client, the named server packets about `subject_runtime_id` (or
without an actor) between the first and last server packet about the
subject stamped with a tick in LO..HI, and the subject's attribute values
sent before that window (`attributes_before`). Login packets, names and
keys never enter the fixture; the capture directories are named in
`sources` by their base name only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DUMP = REPO / 'target' / 'debug' / 'bds-sweep-dump.exe'
TICK = re.compile(r'inputtick: (\d+)')
ACTOR = re.compile(r'(?:target_runtime_id|player_runtime_id|runtime_id): ActorRuntimeId \{ actor_runtime_id: (\d+) \}')
ATTRIBUTE = re.compile(r'current_value: ([-\d.e]+), default_min_value: [^,]+, default_max_value: [^,]+, '
                       r'default_value: [^,]+, name: "([^"]+)"')


def decoded(capture: Path, names: list[str]) -> list[tuple[int, str, str, str]]:
    """(seq, direction, name, decoded text) of the named packets of a capture."""
    args = [str(DUMP), str(capture)] + [a for n in names for a in ('--name', n)]
    out = subprocess.run(args, capture_output=True, text=True, check=True).stdout
    rows = []
    for line in out.splitlines():
        seq, _t, direction, name, body = line.split(' ', 4)
        rows.append((int(seq), direction, name, body))
    return rows


def hex_by_seq(capture: Path) -> dict[int, str]:
    rows = {}
    with capture.open(encoding='utf-8') as file:
        for line in file:
            record = json.loads(line)
            rows[record['seq']] = record['hex']
    return rows


def about_subject(body: str, rid: int, spec: dict) -> bool:
    """Whether a stamped packet carries the subject's input tick."""
    actor = ACTOR.search(body)
    if actor:
        return int(actor.group(1)) == rid
    return spec['client'] == spec['subject']


def phase(root: Path, spec: dict) -> dict:
    lo, hi = spec['ticks']
    rid = spec['subject_runtime_id']
    recording = root / spec['capture'] / spec['client'] / 'packets.jsonl'
    subject = root / spec['capture'] / spec['subject'] / 'packets.jsonl'
    # Only packets about the subject carry its ticks: another player's
    # MovePlayer carries that player's own, and a packet without an actor
    # (a correction) is the recording client's.
    stamped = [(seq, int(m.group(1))) for seq, d, _n, body in decoded(recording, [
        'SetActorData', 'UpdateAttributes', 'MovePlayer', 'SetActorMotion', 'MobEffect',
        'CorrectPlayerMovePrediction'])
        if d == 's2c' and (m := TICK.search(body)) and about_subject(body, rid, spec)]
    inside = [seq for seq, tick in stamped if lo <= tick <= hi]
    if not inside:
        raise ValueError(f'no server packet stamped {lo}..{hi} in {recording}')
    first, last = min(inside), max(inside)
    hexes = hex_by_seq(recording)
    packets = []
    for seq, direction, _name, body in decoded(recording, spec['packets']):
        if direction != 's2c' or not first <= seq <= last:
            continue
        actor = ACTOR.search(body)
        if actor and int(actor.group(1)) != rid:
            continue
        packets.append(hexes[seq])
    subject_hex = hex_by_seq(subject)
    inputs = [subject_hex[seq] for seq, direction, _name, body in decoded(subject, ['PlayerAuthInput'])
              if direction == 'c2s' and lo <= int(TICK.search(body).group(1)) <= hi]
    return {
        'capture': spec['capture'], 'client': spec['client'], 'subject': spec['subject'],
        'ticks': [lo, hi], 'seq_range': [first, last], 'subject_runtime_id': rid,
        'note': spec['note'], 'attributes_before': attributes_before(recording, rid, first),
        'inputs': inputs, 'packets': packets,
    }


def attributes_before(recording: Path, rid: int, first: int) -> dict[str, float]:
    """The last value of each of the subject's attributes the recording
    client was sent before `first`: where the phase starts from."""
    values = {}
    for seq, direction, _name, body in decoded(recording, ['UpdateAttributes']):
        if direction != 's2c' or seq >= first:
            continue
        actor = ACTOR.search(body)
        if actor and int(actor.group(1)) != rid:
            continue
        for value, name in ATTRIBUTE.findall(body):
            values[name] = float(value)
    return dict(sorted(values.items()))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('spec', type=Path)
    parser.add_argument('out', type=Path)
    parser.add_argument('--captures', type=Path, required=True,
                        help='directory holding the capture directories the spec names')
    args = parser.parse_args()
    if not DUMP.exists():
        parser.error(f'{DUMP} missing: cargo build -p bds-sweep')
    spec = json.loads(args.spec.read_text(encoding='utf-8'))
    sources = []
    for source in spec['sources']:
        clients = {}
        for capture in sorted((args.captures / source['capture']).glob('*/packets.jsonl')):
            clients[capture.parent.name] = hashlib.sha256(capture.read_bytes()).hexdigest()
        sources.append({'capture': source['capture'], 'clients': clients, 'setup': source['setup']})
    fixture = {
        'bds_version': '1.26.51',
        'protocol': 2193,
        'sources': sources,
        'packets': 'hex of the packet header and payload (no length prefix), as recorded by client_diff; '
                   'inputs are the subject\'s PlayerAuthInput packets of the phase',
        'phases': {name: phase(args.captures, body) for name, body in spec['phases'].items()},
    }
    args.out.write_text(json.dumps(fixture, indent=1) + '\n', encoding='utf-8', newline='\n')
    print(f'{args.out}: {len(fixture["phases"])} phases')
    return 0


if __name__ == '__main__':
    sys.exit(main())
