"""Copy the committed subset of a capture into the repository.

    python tools/bds-sweep/commit_fixtures.py [CAPTURE_DIR]
    python tools/bds-sweep/commit_fixtures.py --commands CAPTURE_DIR

CAPTURE_DIR defaults to the one named by `<BDS runtime>/captures/1.26.51/
latest.txt`. Captures are Mojang-derived (decision D13): only manifests and
small packet fixtures that tests use are committed, into
`crates/bds-extractor/captures/1.26.51/`.

From a T1 capture (`capture.py`), replacing `member/` and `operator/`:

* both sessions' `manifest.json` (SHA-256 of every file, committed or not),
  `session.json` and the member `join-sequence.json`;
* member join packets up to `PlayStatus(PlayerSpawn)`, except world payload
  and per-tick actor/world updates, at most `MAX_BYTES` each;
* every operator `summons/*.bin` (one AddActor per summonable identifier).

From a command session (`capture.py --commands`), replacing
`operator-commands/`:

* `session.json` and the command script (`commands.txt`);
* `feedback.json`: for every scripted command, its segment (the last
  `# segment: NAME` line above it) and the bytes (packet header and payload,
  hex) of the server packets that answer it: `CommandOutput` and the
  feedback/effect packets in `FEEDBACK`, without the running world's traffic
  (actor packets of actors the script did not summon, block and level events
  away from the platform). The SHA-256 of the session's `command-outputs.json`
  and `packets.jsonl` tie it to the full capture. A `!console` step also
  carries `console`, the BDS console records it printed (`{level, text}`,
  timestamps dropped); a step during which a second client (`!join NAME`)
  was online carries `clients.NAME`, the packets that client kept by the
  same rules from its own point of view.

Everything else (CraftingData, AvailableCommands, JigsawStructureData, the
full `packets.jsonl`, normalised JSON) stays in the capture directory; tests
that need it read it from there and check the manifest hash.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
DEST = REPO / 'crates' / 'bds-extractor' / 'captures' / '1.26.51'
MAX_BYTES = 128 * 1024
# World payload and per-tick world/actor traffic: depends on the terrain and
# nearby mobs, not on the join contract.
SKIP = {
    'LevelChunkPacket', 'SubChunkPacket', 'NetworkChunkPublisherUpdatePacket',
    'UpdateSubChunkBlocksPacket', 'UpdateBlockPacket', 'LevelSoundEventPacket',
    'CurrentStructureFeaturePacket', 'MoveActorDeltaPacket', 'MoveActorAbsolutePacket',
    'SetActorMotionPacket', 'AddActorPacket', 'RemoveActorPacket', 'MobEquipmentPacket',
    'MobArmorEquipmentPacket', 'ActorEventPacket', 'AddItemActorPacket',
}

# Command sessions: the packets a command answers with or causes.
FEEDBACK = {
    'CommandOutputPacket', 'TextPacket', 'SetTitlePacket', 'PlaySoundPacket', 'StopSoundPacket',
    'SpawnParticleEffectPacket', 'CameraShakePacket', 'SetHudPacket', 'PlayerFogPacket',
    'AnimateEntityPacket', 'LevelEventGenericPacket', 'CameraAimAssistPacket',
    'ClientboundControlSchemeSetPacket', 'CameraInstructionPacket', 'UpdateClientInputLocksPacket',
    'SetScorePacket', 'SetDisplayObjectivePacket', 'RemoveObjectivePacket', 'UpdateSoftEnumPacket',
    'DisconnectPacket', 'GameRulesChangedPacket', 'SetDifficultyPacket', 'SetSpawnPositionPacket',
    'MovePlayerPacket', 'UpdatePlayerGameTypePacket', 'SetPlayerGameTypePacket',
    'UpdateAbilitiesPacket', 'UpdateAdventureSettingsPacket', 'InventoryContentPacket',
    'InventorySlotPacket', 'PlayerHotbarPacket', 'UpdateBlockPacket', 'UpdateSubChunkBlocksPacket',
    'BlockActorDataPacket', 'LevelEventPacket', 'LevelSoundEventPacket', 'AddActorPacket',
    'RemoveActorPacket', 'SetActorDataPacket', 'ActorEventPacket', 'UpdateAttributesPacket',
    'MobEffectPacket', 'MobEquipmentPacket', 'MobArmorEquipmentPacket', 'SetActorMotionPacket',
    'MoveActorAbsolutePacket', 'SyncWorldClocksPacket', 'UnlockedRecipesPacket', 'TransferPacket',
    'ShowStoreOfferPacket', 'SetEntityLinkPacket', 'NpcDialoguePacket', 'AddItemActorPacket',
    'ChangeDimensionPacket',
}
# Actor packets are kept for the player and for actors the script summoned.
ACTOR_PACKETS = {
    'SetActorDataPacket', 'ActorEventPacket', 'UpdateAttributesPacket', 'MobEffectPacket',
    'MobEquipmentPacket', 'MobArmorEquipmentPacket', 'SetActorMotionPacket',
    'MoveActorAbsolutePacket',
}
# World events are kept on the platform the script plays on (and at the
# origin, where weather events are placed).
POSITIONAL = {'UpdateBlockPacket', 'LevelEventPacket', 'LevelSoundEventPacket', 'AddItemActorPacket'}
PLATFORM = (8.0, 8.0)
NEAR = 24.0
# A summoned actor's own updates (AI flags, attributes) and the periodic
# clock sync are kept only this soon after the command.
PROMPT_MS = 250
# Committed text uses LF line endings whatever platform captured it.
LF = chr(10)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open('rb') as file:
        for chunk in iter(lambda: file.read(1 << 20), b''):
            digest.update(chunk)
    return digest.hexdigest()


def commit_t1(capture: Path) -> int:
    committed = []

    def copy(relative: str) -> None:
        target = DEST / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(capture / relative, target)
        committed.append(relative)

    for session in ('member', 'operator'):
        if (DEST / session).exists():
            shutil.rmtree(DEST / session)
    for session in ('member', 'operator'):
        copy(f'{session}/manifest.json')
        copy(f'{session}/session.json')
    copy('member/join-sequence.json')

    sequence = json.loads((capture / 'member' / 'join-sequence.json').read_text(encoding='utf-8'))
    start_game = next(row['seq'] for row in sequence if row['packet'] == 'StartGamePacket')
    spawn = next(row['seq'] for row in sequence if row['packet'] == 'PlayStatusPacket' and row['seq'] > start_game)
    for row in sequence:
        if row['seq'] > spawn or row['packet'] in SKIP or row['bytes'] > MAX_BYTES:
            continue
        copy(f'member/join/{row["seq"]:05d}-{row["packet"]}.bin')
    for path in sorted((capture / 'operator' / 'summons').glob('*.bin')):
        copy(f'operator/summons/{path.name}')

    total = sum((DEST / relative).stat().st_size for relative in committed)
    print(f'committed {len(committed)} files, {total} bytes, from {capture}')
    return 0


def script_segments(script: Path) -> list[tuple[str, str]]:
    """`(segment, command)` for every step line of a command script (player
    commands and `!console`/`!join`/`!leave` directives)."""
    segment = ''
    out = []
    for raw in script.read_text(encoding='utf-8').splitlines():
        line = raw.strip()
        if not line:
            continue
        if line.startswith('#'):
            body = line[1:].strip()
            if body.startswith('segment:'):
                segment = body.removeprefix('segment:').strip()
            continue
        out.append((segment, line))
    return out


def bare(command: str) -> str:
    return command.strip().removeprefix('/')


# One BDS console log record starts `[2026-11-10 16:23:20:009 INFO] `;
# the lines after it up to the next record continue its message.
CONSOLE_RECORD = re.compile(r'^\s*\[\d{4}-\d\d-\d\d \d\d:\d\d:\d\d:\d{3} (\w+)\] ?(.*)$')


def console_records(lines: list[str]) -> list[dict]:
    """BDS console lines as `{level, text}` records (multi-line text joined
    with LF, timestamps dropped)."""
    records: list[dict] = []
    for line in lines:
        match = CONSOLE_RECORD.match(line)
        if match:
            records.append({'level': match.group(1), 'text': match.group(2)})
        elif records:
            records[-1]['text'] += LF + line
    return records


class Relevance:
    """Which packets of a command window belong to the command."""

    def __init__(self, session: Path) -> None:
        start = next((session / 'join').glob('*-StartGamePacket.json'))
        data = json.loads(start.read_text(encoding='utf-8'))
        self.player_runtime = data['runtime_id']['actor_runtime_id']
        self.player_unique = data['entity_id']['actor_unique_id']
        self.runtime = {self.player_runtime}
        self.unique = {self.player_unique}

    @staticmethod
    def near(pos: dict) -> bool:
        return max(abs(pos['x'] - PLATFORM[0]), abs(pos['z'] - PLATFORM[1])) <= NEAR

    def keep(self, command: str, name: str, d: dict, dt_ms: float, viewer: int | None = None) -> bool:
        """Whether a packet `viewer` (default: the capturing player; a second
        client's runtime id for its own recording) received belongs to the
        command."""
        viewer = self.player_runtime if viewer is None else viewer
        if name not in FEEDBACK:
            return False
        if name == 'SyncWorldClocksPacket':
            return dt_ms <= PROMPT_MS
        if name == 'AddActorPacket':
            words = bare(command).split()
            if not {'summon', 'summon_rider', 'summon_ride'} & set(words):
                return False
            self.runtime.add(d['target_runtime_id']['actor_runtime_id'])
            self.unique.add(d['target_actor_id']['actor_unique_id'])
            return True
        if name == 'RemoveActorPacket':
            return d['target_actor_id']['actor_unique_id'] in self.unique
        if name in ACTOR_PACKETS:
            target = d.get('target_runtime_id') or d.get('runtime_entity_id')
            if target is None and name == 'MoveActorAbsolutePacket':
                target = d.get('move_data', {}).get('actor_runtime_id')
            if target is None:
                return False
            actor = target['actor_runtime_id']
            if actor != viewer and actor not in self.runtime:
                return False
            return actor == viewer or dt_ms <= PROMPT_MS
        if name == 'LevelSoundEventPacket' and d.get('actor_unique_id', -1) != -1:
            return d['actor_unique_id'] in self.unique
        if name == 'LevelEventPacket' and d['position'] == {'x': 0.0, 'y': 0.0, 'z': 0.0}:
            return True
        if name in POSITIONAL:
            return self.near(d.get('block_position') or d['position'])
        if name == 'UpdateSubChunkBlocksPacket':
            blocks = d['blocks_changed']
            return any(self.near(b['pos']) for key in ('blocks_changed_standards', 'blocks_changed_extras')
                       for b in blocks[key])
        return True


def read_hexes(path: Path, wanted: set[int]) -> dict[int, tuple[str, str]]:
    """`seq -> (name, hex)` of the wanted server records of a recording."""
    hexes: dict[int, tuple[str, str]] = {}
    with path.open(encoding='utf-8') as file:
        for line in file:
            record = json.loads(line)
            if record['dir'] == 's2c' and record['seq'] in wanted:
                hexes[record['seq']] = (record['name'], record['hex'])
    missing = wanted - hexes.keys()
    if missing:
        raise SystemExit(f'{len(missing)} kept packets missing from {path}')
    return hexes


def packet_entry(packet: dict, t_ms: float, hexes: dict[int, tuple[str, str]]) -> dict:
    """A kept packet as committed: seq, time after its step, name and bytes."""
    name, payload = hexes[packet['seq']]
    assert name == packet['packet'], (name, packet)
    return {'seq': packet['seq'], 'dt_ms': round(packet['t_ms'] - t_ms), 'packet': name, 'hex': payload}


def output_t(outputs: list[dict], index: int) -> float:
    return outputs[index]['t_ms']


def commit_commands(capture: Path) -> int:
    session = capture / 'operator-commands'
    outputs_path = session / 'command-outputs.json'
    packets_path = session / 'packets.jsonl'
    script = capture / 'commands.txt'
    outputs = json.loads(outputs_path.read_text(encoding='utf-8'))
    lines = script_segments(script)
    if [bare(c) for _, c in lines] != [bare(o['command']) for o in outputs]:
        raise SystemExit(f'{script} and {outputs_path} list different commands')

    relevance = Relevance(session)
    kept: list[list[dict]] = []
    wanted: set[int] = set()
    for output in outputs:
        packets = [p for p in output['packets']
                   if relevance.keep(output['command'], p['packet'], p['data'], p['t_ms'] - output['t_ms'])]
        kept.append(packets)
        wanted.update(p['seq'] for p in packets)

    hexes = read_hexes(packets_path, wanted)

    # Second clients (`!join NAME`): their packets per step, kept like the
    # player's but from their own point of view.
    clients: dict[str, dict] = {}
    client_packets: dict[str, dict[int, list[dict]]] = {}
    sources = {}
    for path in sorted((session / 'clients').glob('*-outputs.json')):
        data = json.loads(path.read_text(encoding='utf-8'))
        name = data['name']
        recording = session / 'clients' / f'{name}.jsonl'
        clients[name] = {'runtime_id': data['runtime_id'], 'unique_id': data['unique_id']}
        steps = {}
        for step in data['steps']:
            output = outputs[step['step']]
            steps[step['step']] = [
                p for p in step['packets']
                if relevance.keep(output['command'], p['packet'], p['data'], p['t_ms'] - output['t_ms'],
                                  viewer=data['runtime_id'])]
        wanted_client = {p['seq'] for packets in steps.values() for p in packets}
        client_hexes = read_hexes(recording, wanted_client)
        client_packets[name] = {index: [packet_entry(p, output_t(outputs, index), client_hexes) for p in packets]
                                for index, packets in steps.items() if packets}
        sources[f'clients/{name}.jsonl'] = sha256(recording)

    console_path = session / 'console-outputs.json'
    console = {}
    if console_path.exists():
        console = {entry['step']: console_records(entry['lines'])
                   for entry in json.loads(console_path.read_text(encoding='utf-8'))}
        sources['console-outputs.json'] = sha256(console_path)

    commands = []
    for index, ((segment, _), output, packets) in enumerate(zip(lines, outputs, kept)):
        entry = {'segment': segment, 'command': output['command'], 'request_seq': output['request_seq'],
                 'packets': [packet_entry(packet, output['t_ms'], hexes) for packet in packets]}
        if output.get('kind') == 'console':
            entry['console'] = console.get(index, [])
        others = {name: steps[index] for name, steps in client_packets.items() if index in steps}
        if others:
            entry['clients'] = others
        commands.append(entry)

    manifest = json.loads((session / 'manifest.json').read_text(encoding='utf-8'))
    feedback = {
        'format': 1,
        'server_version': manifest['server_version'],
        'protocol_version': manifest['protocol_version'],
        'capture': capture.name,
        'sources': {
            'command-outputs.json': sha256(outputs_path),
            'packets.jsonl': sha256(packets_path),
            **sources,
        },
        'player': {
            'name': 'BDSExtractor',
            'runtime_id': relevance.player_runtime,
            'unique_id': relevance.player_unique,
        },
        'commands': commands,
    }
    if clients:
        feedback['clients'] = clients

    dest = DEST / 'operator-commands'
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    # Text with LF endings whatever platform captured it.
    for source, name in ((session / 'session.json', 'session.json'), (script, 'commands.txt')):
        text = source.read_text(encoding='utf-8')
        (dest / name).write_text(text, encoding='utf-8', newline=LF)
    # One command per line: reviewable diffs without the size of indented JSON.
    head = {key: value for key, value in feedback.items() if key != 'commands'}
    body = ',\n'.join(json.dumps(entry, separators=(',', ':')) for entry in commands)
    text = json.dumps(head, separators=(',', ':'))[:-1] + ',"commands":[\n' + body + '\n]}\n'
    json.loads(text)
    (dest / 'feedback.json').write_text(text, encoding='utf-8', newline=LF)
    total = sum(path.stat().st_size for path in dest.iterdir())
    print(f'committed {len(commands)} commands, {len(wanted)} packets, {total} bytes, from {capture}')
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('capture', nargs='?', type=Path)
    parser.add_argument('--commands', action='store_true',
                        help='commit the operator-commands session of a capture.py --commands capture')
    args = parser.parse_args()
    if args.commands:
        if args.capture is None:
            parser.error('--commands needs CAPTURE_DIR')
        return commit_commands(args.capture.resolve())
    if args.capture is not None:
        capture = args.capture.resolve()
    else:
        capture = Path((bds_runtime.CAPTURES / 'latest.txt').read_text(encoding='utf-8').strip())
    return commit_t1(capture)


if __name__ == '__main__':
    sys.exit(main())
