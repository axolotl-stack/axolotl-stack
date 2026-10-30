"""Readable digest of a `capture.py --commands` session.

    python tools/bds-sweep/command_digest.py <capture>/operator-commands [--out FILE]

Reads the session's `command-outputs.json`, its join packets (StartGame for
the player's ids and position, ItemRegistry for item names) and protocolgen's
block palette (network block hashes to names), and writes one section per
command: the CommandOutput fields and the key fields of every other packet.

Packets that belong to the running world rather than the command are only
counted, as "ambient": actor packets for actors other than the player and the
actors a `/summon` of the script created, block updates, level events and
positioned sounds more than 8 blocks (horizontally) from the player, and
CurrentStructureFeature. `command-outputs.json` keeps all of them. Default
output: `digest.md` in the capture directory (the session's parent).
"""

from __future__ import annotations

import argparse
import collections
import json
import struct
from pathlib import Path

import native

# Runs of at least this many same-name packets of these kinds are folded to
# first/last.
FOLD = 4
FOLDED = {
    'SetActorDataPacket', 'LevelEventPacket', 'UpdateBlockPacket', 'UpdateSubChunkBlocksPacket',
    'InventoryTransactionPacket', 'LevelSoundEventPacket',
}
NEAR = 8.0
# Longer summaries are cut; the full packet is in command-outputs.json.
LINE = 500
# World events kept only within NEAR blocks (horizontally) of the player.
POSITIONAL = {'UpdateBlockPacket', 'LevelEventPacket', 'LevelSoundEventPacket', 'AddItemActorPacket'}
ACTOR_PACKETS = {
    'SetActorDataPacket', 'MobEquipmentPacket', 'MobArmorEquipmentPacket', 'ActorEventPacket',
    'UpdateAttributesPacket', 'MobEffectPacket', 'SetActorMotionPacket', 'MoveActorAbsolutePacket',
    'MoveActorDeltaPacket', 'AnimatePacket', 'SetActorLinkPacket', 'UpdateEquipPacket',
}
AMBIENT_ALWAYS = {'CurrentStructureFeaturePacket'}


def block_names() -> dict[int, str]:
    """Network block hash (FNV-1a 32 of the little-endian NBT `{name,
    states}`, keys sorted) to `name[state=value,...]`."""
    data = (native.DATA / 'registry' / 'block_states.nbt').read_bytes()
    reader = _TypedReader(data)
    out = {}
    while reader.pos < len(data):
        assert reader.byte() == 10
        reader.string()
        entry = reader.compound()
        name = entry['name'][1]
        states = entry['states'][1]
        encoded = _le_compound({'name': entry['name'], 'states': entry['states']})
        hash_ = 0x811C9DC5
        for byte in encoded:
            hash_ = ((hash_ ^ byte) * 0x01000193) & 0xFFFFFFFF
        label = ','.join(f'{k}={v[1]}' for k, v in sorted(states.items()))
        out[hash_] = f'{name}[{label}]' if label else name
    return out


class _TypedReader(native._Reader):
    """Network NBT keeping tag types: `{key: (tag, value)}`."""

    def compound(self) -> dict:
        out = {}
        while (tag := self.byte()) != 0:
            key = self.string()
            out[key] = (tag, self.compound() if tag == 10 else self.payload(tag))
        return out


def _le_string(value: str) -> bytes:
    raw = value.encode('utf-8')
    return struct.pack('<H', len(raw)) + raw


def _le_value(tag: int, value) -> bytes:
    if tag == 1:
        return struct.pack('<b', value)
    if tag == 3:
        return struct.pack('<i', value)
    if tag == 8:
        return _le_string(value)
    if tag == 10:
        return _le_compound(value)[3:]
    raise ValueError(f'block state tag {tag}')


def _le_compound(entries: dict) -> bytes:
    """Little-endian NBT compound with an empty name, keys sorted."""
    out = b'\x0a' + _le_string('')
    for key in sorted(entries):
        tag, value = entries[key]
        out += bytes([tag]) + _le_string(key) + _le_value(tag, value)
    return out + b'\x00'


def compact(value, limit: int = 240) -> str:
    text = json.dumps(value, ensure_ascii=False, separators=(',', ':'))
    return text if len(text) <= limit else text[:limit] + '…'


def vec(v: dict) -> str:
    return '(' + ', '.join(f'{v[k]:g}' if isinstance(v[k], float) else str(v[k]) for k in ('x', 'y', 'z')) + ')'


class Digest:
    def __init__(self, session: Path) -> None:
        self.session = session
        join = {p.name.split('-', 1)[1]: p for p in (session / 'join').glob('*.json')}
        start = json.loads(join['StartGamePacket.json'].read_text(encoding='utf-8'))
        items = json.loads(join['ItemRegistryPacket.json'].read_text(encoding='utf-8'))
        self.items = {i['item_id']: i['item_name'].removeprefix('minecraft:') for i in items['item_data']}
        self.blocks = block_names()
        self.player_runtime = start['runtime_id']['actor_runtime_id']
        self.player_unique = start['entity_id']['actor_unique_id']
        self.position = start['position']
        self.actors_runtime = {self.player_runtime}
        self.actors_unique = {self.player_unique}

    # -- field formatting ---------------------------------------------------

    def item(self, item: dict | None) -> str:
        if not item or item.get('id', 0) == 0:
            return 'air'
        name = self.items.get(item['id'], f"#{item['id']}")
        extra = f":{item['auxvalue']}" if item.get('auxvalue') else ''
        nbt = '' if item.get('user_data_buffer') in ('', '00000000000000000000') else ' +userdata'
        net = f" net={item['net_id_variant']}" if item.get('net_id_variant') is not None else ''
        return f"{name}{extra}×{item.get('stacksize', 0)}{net}{nbt}"

    def block(self, runtime_id: int) -> str:
        return self.blocks.get(runtime_id, f'#{runtime_id}')

    def actor(self, runtime: dict) -> str:
        rid = runtime['actor_runtime_id']
        return 'player' if rid == self.player_runtime else f'actor {rid}'

    def summary(self, name: str, d: dict) -> str:
        n = name.removesuffix('Packet')
        if name == 'CommandOutputPacket':
            o = d['output']
            msgs = '; '.join(
                f"{json.dumps(m['message_id'], ensure_ascii=False)} ok={str(m['successful']).lower()} "
                f"params={json.dumps(m['parameters'], ensure_ascii=False)}" for m in o['output_messages'])
            origin = d['origin_data']
            return (f"**CommandOutput** type={o['output_type']} success_count={o['success_count']} "
                    f"data_set={json.dumps(o['data_set'])} origin={origin['type_']}; messages: {msgs}")
        if name == 'TextPacket':
            (variant, body), = d['body'].items()
            rest = {k: v for k, v in body.items() if k not in ('message', 'message_type')}
            extra = {k: v for k, v in d.items() if k != 'body' and v not in (None, '', False)}
            return (f"Text {body.get('message_type')} ({variant}) message={json.dumps(body.get('message'), ensure_ascii=False)}"
                    f"{' ' + compact(rest) if rest else ''} localize={str(d['localize']).lower()}"
                    f"{' ' + compact(extra) if extra else ''}")
        if name == 'MovePlayerPacket':
            return (f"MovePlayer {self.actor(d['player_runtime_id'])} mode={d['position_mode']} pos={vec(d['position'])} "
                    f"rot=({d['rotation']['x']:g}, {d['rotation']['y']:g}) head_yaw={d['y_head_rotation']:g} "
                    f"on_ground={str(d['on_ground']).lower()} teleport={compact(d['teleport_data'])} "
                    f"riding={d['riding_runtime_id']['actor_runtime_id']} tick={d['tick']['inputtick']}")
        if name == 'InventorySlotPacket':
            return (f"InventorySlot container={d['container_id']} slot={d['slot']} item={self.item(d['item'])} "
                    f"full_container_name={compact(d['full_container_name'])} storage_item={self.item(d['storage_item'])}")
        if name == 'InventoryContentPacket':
            slots = ', '.join(f'{i}:{self.item(s)}' for i, s in enumerate(d['slots']) if s.get('id'))
            return (f"InventoryContent container={d['container_id']} ({len(d['slots'])} slots) "
                    f"full_container_name={compact(d['full_container_name'])} storage_item={self.item(d.get('storage_item'))} "
                    f"non-empty: [{slots}]")
        if name == 'InventoryTransactionPacket':
            (kind, tx), = d['transaction'].items()
            actions = tx.get('actions', {}).get('actions', []) if isinstance(tx, dict) else []
            parts = []
            for a in actions:
                src = a['source']
                where = src['source_type'] + (f"/{src['container_id']}" if src.get('container_id') is not None else '')
                parts.append(f"{where}[{a['slot']}] {self.item(a['from_item'])} → {self.item(a['to_item'])}")
            return f"InventoryTransaction {kind} request={d['legacy_request_id']['id']} actions: " + '; '.join(parts)
        if name in ('MobEquipmentPacket',):
            return (f"MobEquipment {self.actor(d['target_runtime_id'])} container={d['container_id']} slot={d['slot']} "
                    f"selected={d['selected_slot']} item={self.item(d['item'])}")
        if name == 'MobArmorEquipmentPacket':
            gear = {k: self.item(v) for k, v in d.items() if isinstance(v, dict) and 'stacksize' in v}
            return f"MobArmorEquipment {self.actor(d['target_runtime_id'])} {compact(gear)}"
        if name == 'UpdateAttributesPacket':
            attrs = ', '.join(f"{a['name'].removeprefix('minecraft:')}={a['current_value']:g} [{a['min_value']:g}..{a['max_value']:g}]"
                              f"{' mods=' + compact(a['modifiers']) if a['modifiers'] else ''}" for a in d['attribute_list'])
            return f"UpdateAttributes {self.actor(d['target_runtime_id'])} tick={d['tick']['inputtick']}: {attrs}"
        if name == 'SetActorDataPacket':
            items = ', '.join(f"{e['id']}={next(iter(e['payload'].values()))['value']}" for e in d['actor_data']['data'])
            props = d['synched_properties']
            extra = '' if not (props['float_entries_list'] or props['int_entries_list']) else ' props=' + compact(props)
            return f"SetActorData {self.actor(d['target_runtime_id'])} {{{items}}}{extra} tick={d['tick']['inputtick']}"
        if name == 'AddActorPacket':
            meta = {e['id']: next(iter(e['payload'].values()))['value'] for e in d['actor_data']['data']}
            return (f"AddActor {d['actor_type']} unique={d['target_actor_id']['actor_unique_id']} "
                    f"runtime={d['target_runtime_id']['actor_runtime_id']} pos={vec(d['position'])} "
                    f"rot=({d['rotation']['x']:g}, {d['rotation']['y']:g}) head_yaw={d['y_head_rotation']:g} "
                    f"body_yaw={d['y_body_rotation']:g} vel={vec(d['velocity'])} nametag={json.dumps(meta.get(4))} "
                    f"links={len(d['actor_links'])} attributes={len(d['attributes_list'])} metadata={len(meta)}")
        if name == 'AddItemActorPacket':
            return (f"AddItemActor unique={d['target_actor_id']['actor_unique_id']} runtime={d['target_runtime_id']['actor_runtime_id']} "
                    f"item={self.item(d['item'])} pos={vec(d['position'])} from_fishing={str(d['is_from_fishing']).lower()}")
        if name == 'UpdateBlockPacket':
            return (f"UpdateBlock {vec(d['block_position'])} {self.block(d['block_runtime_id'])} "
                    f"flags={d['flags']} layer={d['layer']}")
        if name == 'UpdateSubChunkBlocksPacket':
            changes = [f"{vec(b['pos'])} {self.block(b['runtime_id'])} flags={b['update_flags']}"
                       for key in ('blocks_changed_standards', 'blocks_changed_extras')
                       for b in d['blocks_changed'][key]]
            return f"UpdateSubChunkBlocks at {vec(d['sub_chunk_block_position'])}: " + '; '.join(changes)
        if name == 'LevelEventPacket':
            return f"LevelEvent id={d['event_id']} pos={vec(d['position'])} data={d['data']}"
        if name == 'LevelSoundEventPacket':
            return (f"LevelSoundEvent {d['sound_event']} pos={vec(d['position'])} data={d['data']} "
                    f"actor={json.dumps(d['actor_identifier'])} unique={d['actor_unique_id']} "
                    f"baby={str(d['is_baby']).lower()} global={str(d['is_global']).lower()}")
        if name == 'GameRulesChangedPacket':
            rules = ', '.join(f"{r['rule_name']}={next(iter(r['rule_value'].values()))}"
                              f"{'' if r['rule_can_be_modified'] else ' (fixed)'}" for r in d['rule_data']['rules_list'])
            return f"GameRulesChanged [{rules}]"
        if name == 'LegacyTelemetryEventPacket':
            event = d['event_data'].get('SlashCommand')
            if event is None:
                return f"LegacyTelemetryEvent {d['event_type']} {compact(d['event_data'])}"
            return (f"LegacyTelemetryEvent SlashCommand {event['command_name']} success_count={event['success_count']} "
                    f"error_count={event['error_count']} error_list={json.dumps(event['error_list'])} "
                    f"use_player_id={str(d['use_player_id']).lower()} target={d['target_actor_id']['actor_unique_id']}")
        if name == 'RemoveActorPacket':
            return f"RemoveActor unique={d['target_actor_id']['actor_unique_id']}"
        if name in ('MobEffectPacket', 'ActorEventPacket'):
            rest = {k: v for k, v in d.items() if k != 'target_runtime_id'}
            return f"{n} {self.actor(d['target_runtime_id'])} {compact(rest, 400)}"
        return f'{n} {compact(d, 400)}'

    # -- relevance ------------------------------------------------------------

    def near(self, pos: dict) -> bool:
        return max(abs(pos['x'] - self.position['x']), abs(pos['z'] - self.position['z'])) <= NEAR

    def relevant(self, command: str, name: str, d: dict) -> bool:
        if name in AMBIENT_ALWAYS:
            return False
        if name == 'AddActorPacket':
            parts = command.split()
            summoned = parts[0] == '/summon' and len(parts) > 1 and \
                d['actor_type'].removeprefix('minecraft:') == parts[1].removeprefix('minecraft:')
            if summoned:
                self.actors_runtime.add(d['target_runtime_id']['actor_runtime_id'])
                self.actors_unique.add(d['target_actor_id']['actor_unique_id'])
            return summoned
        if name == 'RemoveActorPacket':
            return d['target_actor_id']['actor_unique_id'] in self.actors_unique
        if name in ACTOR_PACKETS:
            target = d.get('target_runtime_id') or d.get('move_data', {}).get('actor_runtime_id')
            return target is not None and target['actor_runtime_id'] in self.actors_runtime
        if name == 'LevelSoundEventPacket' and d['actor_unique_id'] != -1:
            return d['actor_unique_id'] in self.actors_unique
        if name == 'UpdateSubChunkBlocksPacket':
            blocks = d['blocks_changed']
            return any(self.near(b['pos']) for key in ('blocks_changed_standards', 'blocks_changed_extras')
                       for b in blocks[key])
        if name == 'LevelEventPacket' and d['position'] == {'x': 0.0, 'y': 0.0, 'z': 0.0}:
            return True
        if name in POSITIONAL:
            return self.near(d.get('block_position') or d['position'])
        return True

    @staticmethod
    def fold_key(name: str, d: dict) -> tuple | None:
        """Packets with the same key are folded when there are many of them in
        one command window (an actor ticking its air supply, bubble particles)."""
        if name not in FOLDED:
            return None
        target = d.get('target_runtime_id', {}).get('actor_runtime_id')
        if name == 'SetActorDataPacket':
            return name, target, tuple(e['id'] for e in d['actor_data']['data'])
        if name == 'LevelEventPacket':
            return name, d['event_id']
        if name == 'LevelSoundEventPacket':
            return name, d['sound_event'], d['actor_unique_id']
        return name, target

    # -- output ---------------------------------------------------------------

    def render(self, commands: list[dict]) -> str:
        lines = [
            '# BDS 1.26.51 command feedback digest',
            '',
            f'Session: `{self.session}`; player runtime id {self.player_runtime}, unique id {self.player_unique}.',
            'Every server packet between a command and the next one, in order; LevelChunk, SubChunk,',
            'NetworkChunkPublisherUpdate, NetworkStackLatency and movement of other actors are left out of',
            '`command-outputs.json` (counted there under `excluded`). "ambient" counts world packets not caused by the',
            f'command (other actors, block/level events more than {NEAR:g} blocks from the player, CurrentStructureFeature).',
            f'{FOLD}+ packets of one kind (same actor and metadata keys, same level event id) in one window show first, last and count. Item ids are named from ItemRegistry, block',
            'hashes from the protocolgen palette. SyncWorldClocks also arrives periodically, not only after /time.',
            '',
        ]
        for index, command in enumerate(commands, 1):
            lines.append(f"## {index}. `{command['command']}`")
            lines.append('')
            shown, ambient = [], collections.Counter()
            for packet in command['packets']:
                name, data = packet['packet'], packet['data']
                if name == 'MovePlayerPacket' and data['player_runtime_id']['actor_runtime_id'] == self.player_runtime:
                    self.position = data['position']
                if self.relevant(command['command'], name, data):
                    text = f"+{packet['t_ms'] - command['t_ms']:.0f}ms {self.summary(name, data)}"
                    if len(text) > LINE and name not in ('CommandOutputPacket', 'TextPacket'):
                        text = f"{text[:LINE]}… (seq {packet['seq']})"
                    shown.append((self.fold_key(name, data), name, text))
                else:
                    ambient[name.removesuffix('Packet')] += 1
            if not any(name == 'CommandOutputPacket' for _, name, _ in shown):
                lines.append('- **no CommandOutput**')
            groups = collections.defaultdict(list)
            for key, _, text in shown:
                if key is not None:
                    groups[key].append(text)
            for key, name, text in shown:
                group = groups.get(key, [])
                if len(group) < FOLD:
                    lines.append(f'- {text}')
                elif text is group[0]:
                    lines.append(f'- {name.removesuffix("Packet")} ×{len(group)}:')
                    lines.append(f'  - first: {group[0]}')
                    lines.append(f'  - last: {group[-1]}')
            excluded = dict(command['excluded'])
            if ambient:
                lines.append('- ambient: ' + ', '.join(f'{k}×{v}' for k, v in sorted(ambient.items())))
            if excluded:
                lines.append('- excluded: ' + ', '.join(f"{k.removesuffix('Packet')}×{v}" for k, v in sorted(excluded.items())))
            lines.append('')
        return '\n'.join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('session', type=Path, help='the operator-commands directory of a capture')
    parser.add_argument('--out', type=Path, help='default: digest.md in the capture directory')
    args = parser.parse_args()
    session = args.session.resolve()
    commands = json.loads((session / 'command-outputs.json').read_text(encoding='utf-8'))
    out = args.out or session.parent / 'digest.md'
    out.write_text(Digest(session).render(commands), encoding='utf-8')
    print(f'wrote {out}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
