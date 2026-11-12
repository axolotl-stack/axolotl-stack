"""Copy the packets of `protocol_probe.py` runs into the repository.

    python tools/bds-sweep/commit_protocol_fixtures.py [--packets P,...] [--no-client-data] [--sent-sizes]
        NAME=CAPTURE_DIR[:SESSION,...] ...

For the listed session directories of each run (default: every directory
holding a `packets.jsonl`), writes under
`crates/bds-extractor/captures/1.26.51/protocol/<NAME>/<session>/`:

- `<seq>-<Packet>.bin`: every server-to-client packet of `RECEIVED` (header
  and payload, the format of the join fixtures);
- `sent/<seq>-<Packet>.bin`: every client-to-server packet of `SENT` (the
  scripted actions and the login's cache status);
- `timeline.json`: `[seq, t_ms, dir, packet]` of every packet recorded after
  the join (the commands phase, from `manifest.json`) except the client's
  `PlayerAuthInput` stream, so a test can show
  what BDS did not send after an action;
- `session.json`.

The run's scripts (`commands.txt`, `<session>-commands.txt`), `console.json` and (unless `--no-client-data`) ClientData
files are copied beside the sessions. `--packets` limits both packet sets to
the names given, for the runs after it. `--sent-sizes` (the `size` probe)
writes `sent.json`, `[seq, packet, bytes]` of every client packet of
`SIZED`, instead of `sent/` and the scripts, whose packets run to 128 KiB. Chunks, actors and payloads of other
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
    'ClientCacheMissResponsePacket', 'PacketViolationWarningPacket', 'InventoryTransactionPacket',
    'InventoryContentPacket', 'InventorySlotPacket', 'SetActorDataPacket', 'UpdateAttributesPacket',
}
SENT = {
    'InteractPacket', 'ActorPickRequestPacket', 'EmotePacket', 'EmoteListPacket', 'AnimatePacket',
    'BookEditPacket', 'PartyChangedPacket', 'ResourcePacksReadyForValidationPacket',
    'ServerboundDataDrivenScreenClosedPacket', 'ServerboundDataStorePacket',
    'PartyDestinationCookieResponsePacket', 'ClientCacheBlobStatusPacket', 'ClientCacheStatusPacket',
    'InventoryTransactionPacket', 'MobEquipmentPacket', 'BlockPickRequestPacket',
}
SIZED = {'CommandRequestPacket', 'TextPacket', 'BookEditPacket'}
SCRIPTS = ['commands.txt', 'guest-commands.txt', 'console.json']
CLIENT_DATA = ['client-data.json', 'guest-client-data.json', 'floats-client-data.json', 'odd-client-data.json']


def join_end(session: Path) -> int:
    manifest = session / 'manifest.json'
    if not manifest.exists():
        return 0
    return json.loads(manifest.read_text(encoding='utf-8'))['phases']['join_end']


def copy_session(session: Path, target: Path, packets: set[str] | None, client_data: bool,
                 sent_sizes: bool = False) -> int:
    target.mkdir(parents=True, exist_ok=True)
    if (session / 'session.json').exists():
        shutil.copyfile(session / 'session.json', target / 'session.json')
    if client_data and (session / 'client-data.json').exists():
        shutil.copyfile(session / 'client-data.json', target / 'client-data.json')
    start = join_end(session)
    timeline = []
    sizes = []
    copied = 0
    with (session / 'packets.jsonl').open(encoding='utf-8') as lines:
        for line in lines:
            record = json.loads(line)
            # The client's own input stream (`--input`) is left out.
            if record['seq'] >= start and record['name'] != 'PlayerAuthInputPacket':
                timeline.append([record['seq'], record['t_ms'], record['dir'], record['name']])
            if sent_sizes and record['dir'] == 'c2s' and record['name'] in SIZED:
                sizes.append([record['seq'], record['name'], len(record['hex']) // 2])
            if packets is not None and record['name'] not in packets:
                continue
            received = record['dir'] == 's2c' and record['name'] in RECEIVED
            sent = record['dir'] == 'c2s' and record['name'] in SENT and not sent_sizes
            if not (received or sent):
                continue
            directory = target if received else target / 'sent'
            directory.mkdir(exist_ok=True)
            (directory / f"{record['seq']:05}-{record['name']}.bin").write_bytes(bytes.fromhex(record['hex']))
            copied += 1
    text = '[\n' + ',\n'.join(json.dumps(entry) for entry in timeline) + '\n]\n'
    (target / 'timeline.json').write_text(text, encoding='utf-8', newline='\n')
    if sent_sizes:
        text = '[\n' + ',\n'.join(json.dumps(entry) for entry in sizes) + '\n]\n'
        (target / 'sent.json').write_text(text, encoding='utf-8', newline='\n')
    return copied


def main() -> int:
    args = sys.argv[1:]
    packets: set[str] | None = None
    client_data = True
    sent_sizes = False
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
        if arg == '--sent-sizes':
            sent_sizes = True
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
        for file in ([] if sent_sizes else SCRIPTS) + (CLIENT_DATA if client_data else []):
            if (capture_dir / file).exists():
                shutil.copyfile(capture_dir / file, target / file)
        # `protocol_probe.single` names each session's script `<session>-commands.txt`.
        for script in [] if sent_sizes else sorted(capture_dir.glob('*-commands.txt')):
            shutil.copyfile(script, target / script.name)
        copied = 0
        for session in sorted(path for path in capture_dir.iterdir() if (path / 'packets.jsonl').exists()
                              and (sessions is None or path.name in sessions)):
            copied += copy_session(session, target / session.name, packets, client_data, sent_sizes)
        print(f'{name}: {copied} packets from {capture_dir}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
