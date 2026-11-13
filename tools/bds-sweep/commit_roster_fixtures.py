"""Copy the player-roster packets of `capture.py --pair` runs into the repository.

    python tools/bds-sweep/commit_roster_fixtures.py NAME=CAPTURE_DIR[:SESSION,...] ...

For the listed session directories of each capture (default: every directory
holding a `packets.jsonl`: `host`, `guest`, `guest2`, ...), the server-to-client
packets of `PACKETS` are written as `<NAME>/<session>/<seq>-<Packet>.bin`
under `crates/bds-extractor/captures/1.26.51/roster/` (the same header and
payload format as the join fixtures), with the run's `commands.txt`, each
session's `session.json` and an `index.json` listing every file with its
sequence number and time. Join sequences, chunks, actors and the rest of the
recording stay in the capture directory.
"""

from __future__ import annotations

import json
import shutil
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DEST = REPO / 'crates' / 'bds-extractor' / 'captures' / '1.26.51' / 'roster'
# What a roster change sends: player list, sleeping-player counts, adventure
# settings, plus the join's spawn/health/weather packets.
PACKETS = {
    'PlayerListPacket', 'LevelEventGenericPacket', 'UpdateAdventureSettingsPacket',
    'SetSpawnPositionPacket', 'SetHealthPacket', 'LevelEventPacket',
}
# Level events other than weather (block cracking, particles) depend on the
# terrain and nearby mobs, not on the roster.
WEATHER_EVENTS = {3001, 3002, 3003, 3004}


def level_event_id(payload: bytes) -> int:
    """The zigzag varint event id at the start of a LevelEvent payload."""
    value, shift = 0, 0
    for byte in payload:
        value |= (byte & 0x7F) << shift
        shift += 7
        if not byte & 0x80:
            break
    return (value >> 1) ^ -(value & 1)


def header_length(data: bytes) -> int:
    """Length of the varint packet header in front of the payload."""
    for index, byte in enumerate(data):
        if not byte & 0x80:
            return index + 1
    raise ValueError('truncated header')


def wanted(record: dict) -> bool:
    if record['dir'] != 's2c' or record['name'] not in PACKETS:
        return False
    if record['name'] == 'LevelEventPacket':
        data = bytes.fromhex(record['hex'])
        return level_event_id(data[header_length(data):]) in WEATHER_EVENTS
    return True


def copy_session(session: Path, target: Path) -> list[dict]:
    target.mkdir(parents=True, exist_ok=True)
    if (session / 'session.json').exists():
        shutil.copyfile(session / 'session.json', target / 'session.json')
    files = []
    with (session / 'packets.jsonl').open(encoding='utf-8') as lines:
        for line in lines:
            record = json.loads(line)
            if not wanted(record):
                continue
            name = f"{record['seq']:05}-{record['name']}.bin"
            (target / name).write_bytes(bytes.fromhex(record['hex']))
            files.append({'file': name, 'seq': record['seq'], 't_ms': record['t_ms'], 'packet': record['name']})
    return files


def main() -> int:
    if len(sys.argv) < 2 or any('=' not in arg for arg in sys.argv[1:]):
        print(__doc__)
        return 2
    for arg in sys.argv[1:]:
        name, capture = arg.split('=', 1)
        sessions = None
        if ':' in capture[2:]:
            split = capture.rindex(':')
            capture, sessions = capture[:split], set(capture[split + 1:].split(','))
        capture_dir = Path(capture).resolve()
        target = DEST / name
        if target.exists():
            shutil.rmtree(target)
        target.mkdir(parents=True)
        if (capture_dir / 'commands.txt').exists():
            shutil.copyfile(capture_dir / 'commands.txt', target / 'commands.txt')
        index = {}
        for session in sorted(path for path in capture_dir.iterdir() if (path / 'packets.jsonl').exists()
                              and (sessions is None or path.name in sessions)):
            index[session.name] = copy_session(session, target / session.name)
        (target / 'index.json').write_text(json.dumps(index, indent=2) + '\n', encoding='utf-8')
        print(f'{name}: {sum(len(files) for files in index.values())} packets from {capture_dir}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
