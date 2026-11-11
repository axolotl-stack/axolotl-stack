"""Copy the packets of `protocol_probe.py` runs into the repository.

    python tools/bds-sweep/commit_protocol_fixtures.py [--packets P,...] [--no-client-data]
        NAME=CAPTURE_DIR[:SESSION,...] ...

For the listed session directories of each run (default: every directory
holding a `packets.jsonl`), writes under
`crates/bds-extractor/captures/1.26.51/protocol/<NAME>/<session>/`:

- `<seq>-<Packet>.bin`: every server-to-client packet of `RECEIVED` (header
  and payload, the format of the join fixtures);
- `sent/<seq>-<Packet>.bin`: every client-to-server packet of `SENT` (the
  scripted actions and the login's cache status);
- `timeline.json`: `[seq, t_ms, dir, packet]` of every packet recorded after
  the join (the commands phase, from `manifest.json`), so a test can show
  what BDS did not send after an action;
- `session.json`.

The run's scripts, `console.json` and (unless `--no-client-data`) ClientData
files are copied beside the sessions. `--packets` limits both packet sets to
the names given, for the runs after it. Chunks, actors and payloads of other
packets stay in the capture directory.
"""

from __future__ import annotations

import json
import shutil
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DEST = REPO / 'crates' / 'bds-extractor' / 'captures' / '1.26.51' / 'protocol'
RECEIVED = {
    'PlayerListPacket', 'UpdateAbilitiesPacket', 'UpdatePlayerGameTypePacket', 'UpdateAdventureSettingsPacket',
    'EmotePacket', 'EmoteListPacket', 'AnimatePacket', 'GuiDataPickItemPacket', 'PlayerHotbarPacket',
    'ClientCacheMissResponsePacket', 'PacketViolationWarningPacket',
}
SENT = {
    'InteractPacket', 'ActorPickRequestPacket', 'EmotePacket', 'EmoteListPacket', 'AnimatePacket',
    'BookEditPacket', 'PartyChangedPacket', 'ResourcePacksReadyForValidationPacket',
    'ServerboundDataDrivenScreenClosedPacket', 'ServerboundDataStorePacket',
    'PartyDestinationCookieResponsePacket', 'ClientCacheBlobStatusPacket', 'ClientCacheStatusPacket',
}
SCRIPTS = ['commands.txt', 'guest-commands.txt', 'console.json']
CLIENT_DATA = ['client-data.json', 'guest-client-data.json', 'floats-client-data.json', 'odd-client-data.json']


def join_end(session: Path) -> int:
    manifest = session / 'manifest.json'
    if not manifest.exists():
        return 0
    return json.loads(manifest.read_text(encoding='utf-8'))['phases']['join_end']


def copy_session(session: Path, target: Path, packets: set[str] | None, client_data: bool) -> int:
    target.mkdir(parents=True, exist_ok=True)
    if (session / 'session.json').exists():
        shutil.copyfile(session / 'session.json', target / 'session.json')
    if client_data and (session / 'client-data.json').exists():
        shutil.copyfile(session / 'client-data.json', target / 'client-data.json')
    start = join_end(session)
    timeline = []
    copied = 0
    with (session / 'packets.jsonl').open(encoding='utf-8') as lines:
        for line in lines:
            record = json.loads(line)
            if record['seq'] >= start:
                timeline.append([record['seq'], record['t_ms'], record['dir'], record['name']])
            if packets is not None and record['name'] not in packets:
                continue
            received = record['dir'] == 's2c' and record['name'] in RECEIVED
            sent = record['dir'] == 'c2s' and record['name'] in SENT
            if not (received or sent):
                continue
            directory = target if received else target / 'sent'
            directory.mkdir(exist_ok=True)
            (directory / f"{record['seq']:05}-{record['name']}.bin").write_bytes(bytes.fromhex(record['hex']))
            copied += 1
    text = '[\n' + ',\n'.join(json.dumps(entry) for entry in timeline) + '\n]\n'
    (target / 'timeline.json').write_text(text, encoding='utf-8', newline='\n')
    return copied


def main() -> int:
    args = sys.argv[1:]
    packets: set[str] | None = None
    client_data = True
    if not args:
        print(__doc__)
        return 2
    while args:
        arg = args.pop(0)
        if arg == '--packets':
            packets = set(args.pop(0).split(','))
            continue
        if arg == '--no-client-data':
            client_data = False
            continue
        if '=' not in arg:
            print(__doc__)
            return 2
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
        for file in SCRIPTS + (CLIENT_DATA if client_data else []):
            if (capture_dir / file).exists():
                shutil.copyfile(capture_dir / file, target / file)
        copied = 0
        for session in sorted(path for path in capture_dir.iterdir() if (path / 'packets.jsonl').exists()
                              and (sessions is None or path.name in sessions)):
            copied += copy_session(session, target / session.name, packets, client_data)
        print(f'{name}: {copied} packets from {capture_dir}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
