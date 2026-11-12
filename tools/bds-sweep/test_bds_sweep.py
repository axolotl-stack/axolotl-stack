"""Tests for the BDS-free parts of the sweep harness.

    python -m unittest discover -s tools/bds-sweep -p "test_*.py"
"""

import json
import struct
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import knockback_fixture  # noqa: E402
import replication_fixture  # noqa: E402
import leveldb_read  # noqa: E402
import native  # noqa: E402
import subchunk_fixture  # noqa: E402
import summarize  # noqa: E402
import sweep  # noqa: E402


class LootTableFit(unittest.TestCase):
    def test_identical_samples_fit_exactly_without_weights(self):
        table, fit = summarize.loot_table({'minecraft:cobblestone@0x1': 200})
        self.assertEqual(fit, 'exact')
        self.assertEqual(table, {'pools': [{'rolls': 1, 'entries': [
            {'type': 'item', 'name': 'minecraft:cobblestone'}]}]})

    def test_varied_samples_become_weighted_entries_per_item(self):
        # Fortune-like: separate one-item stacks, sometimes nothing.
        outcomes = {'': 1, 'minecraft:diamond@0x1': 2, 'minecraft:diamond@0x1;minecraft:diamond@0x1': 1,
                    'minecraft:dye@4x3': 1}
        table, fit = summarize.loot_table(outcomes)
        self.assertEqual(fit, 'empirical')
        diamond, dye = table['pools']
        self.assertEqual(diamond['entries'], [
            {'type': 'empty', 'weight': 2},
            {'type': 'item', 'name': 'minecraft:diamond', 'weight': 2},
            {'type': 'item', 'name': 'minecraft:diamond', 'weight': 1,
             'functions': [{'function': 'set_count', 'count': 2}]},
        ])
        self.assertEqual(dye['entries'][1], {
            'type': 'item', 'name': 'minecraft:dye', 'weight': 1,
            'functions': [{'function': 'set_count', 'count': 3}, {'function': 'set_data', 'data': 4}]})
        # Every pool's weights cover all samples.
        for pool in table['pools']:
            self.assertEqual(sum(entry['weight'] for entry in pool['entries']), 5)


class Repeatability(unittest.TestCase):
    def sweep_dir(self, root: Path, name: str, rows: list[dict]) -> Path:
        path = root / name
        path.mkdir()
        (path / 'job.json').write_text(json.dumps({'items': [{'key': r['key']} for r in rows]}))
        (path / 'results.jsonl').write_text(''.join(json.dumps(r) + '\n' for r in rows))
        return path

    def test_deterministic_cells_must_match_and_noise_is_tolerated(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            a = self.sweep_dir(root, 'a', [
                {'key': 'stone|hand', 'outcomes': {'': 200}},
                {'key': 'gravel|hand', 'outcomes': {'minecraft:flint@0x1': 21, 'minecraft:gravel@0x1': 179}},
            ])
            b = self.sweep_dir(root, 'b', [
                {'key': 'stone|hand', 'outcomes': {'minecraft:cobblestone@0x1': 200}},
                {'key': 'gravel|hand', 'outcomes': {'minecraft:flint@0x1': 26, 'minecraft:gravel@0x1': 174}},
            ])
            report = summarize.compare(a, b)
        self.assertEqual(report['deterministic_cells'], 1)
        self.assertEqual(report['random_cells'], 1)
        self.assertEqual(len(report['mismatches']), 1)
        self.assertTrue(report['mismatches'][0].startswith('deterministic stone|hand'))

    def test_unfinished_sweep_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp)
            (path / 'job.json').write_text(json.dumps({'items': [{'key': 'a'}, {'key': 'b'}]}))
            (path / 'results.jsonl').write_text(json.dumps({'key': 'a'}) + '\n')
            with self.assertRaises(SystemExit):
                summarize.results(path)


class NativeNbt(unittest.TestCase):
    def test_compound_names_are_read_before_payloads(self):
        # "" { name: "minecraft:stone", states: { growth: int 7 } }, network
        # little endian (varint lengths, zigzag int).
        data = bytes([0x0a, 0x00, 0x08, 0x04]) + b'name' + bytes([0x0f]) + b'minecraft:stone'
        data += bytes([0x0a, 0x06]) + b'states' + bytes([0x03, 0x06]) + b'growth' + bytes([0x0e, 0x00, 0x00])
        reader = native._Reader(data)
        self.assertEqual(reader.byte(), 10)
        reader.string()
        self.assertEqual(reader.payload(10), {'name': 'minecraft:stone', 'states': {'growth': 7}})


class ToolSets(unittest.TestCase):
    def test_correct_tool_tiers_wrong_tool_shears_and_enchants(self):
        mining = {'minecraft:diamond_ore': {'effective': ['pickaxe'], 'harvest': ['pickaxe'], 'level': 2}}
        keys = [sweep.tool_key(t) for t in sweep.tools_for('minecraft:diamond_ore', mining)]
        self.assertEqual(keys, [
            'hand', 'wooden_pickaxe', 'stone_pickaxe', 'copper_pickaxe', 'iron_pickaxe', 'golden_pickaxe',
            'diamond_pickaxe', 'netherite_pickaxe', 'diamond_axe', 'shears', 'diamond_pickaxe+silk_touch1',
            'diamond_pickaxe+fortune1', 'diamond_pickaxe+fortune2', 'diamond_pickaxe+fortune3'])

    def test_blocks_without_a_tool_use_the_pickaxe_for_enchants(self):
        keys = [sweep.tool_key(t) for t in sweep.tools_for('minecraft:glass', {})]
        self.assertEqual(keys, ['hand', 'diamond_pickaxe', 'shears', 'diamond_pickaxe+silk_touch1',
                                'diamond_pickaxe+fortune1', 'diamond_pickaxe+fortune2',
                                'diamond_pickaxe+fortune3'])

    def test_state_keys_are_sorted_and_typed(self):
        self.assertEqual(sweep.state_key('minecraft:oak_log', {'pillar_axis': 'y', 'a': 1, 'b': False}),
                         'minecraft:oak_log[a=1,b=false,pillar_axis="y"]')


class HeldAndEffectVariants(unittest.TestCase):
    def args(self, **overrides):
        import argparse
        values = dict(kind='mining', variants='tools', items='slice', blocks='slice', trials=1, mine_ticks=600,
                      gap=0, gamemode=None)
        values.update(overrides)
        return argparse.Namespace(**values)

    def test_non_tool_diggers_are_the_mining_class_items_without_a_digger_tag(self):
        names = sweep.non_tool_diggers()
        for expected in ('minecraft:apple', 'minecraft:bow', 'minecraft:iron_chestplate', 'minecraft:wooden_spear',
                         'minecraft:music_disc_13', 'minecraft:bundle'):
            self.assertIn(expected, names)
        for tool in ('minecraft:iron_pickaxe', 'minecraft:shears', 'minecraft:iron_sword', 'minecraft:dirt',
                     'minecraft:glow_stick'):
            self.assertNotIn(tool, names)

    def test_held_items_follow_a_three_trial_hand_reference_per_block(self):
        items = sweep.mining_items(self.args(variants='held', items='minecraft:apple,minecraft:bow',
                                             blocks='minecraft:dirt'))
        self.assertEqual([(i['key'], i['repeats']) for i in items], [
            ('minecraft:dirt[]|hand', 3), ('minecraft:dirt[]|apple', 1), ('minecraft:dirt[]|bow', 1)])
        self.assertEqual(items[2]['tool'], {'item': 'minecraft:bow'})

    def test_the_default_held_set_adds_every_tool_off_the_slice_blocks(self):
        items = sweep.mining_items(self.args(variants='held'))
        keys = {i['key'] for i in items}
        self.assertIn('minecraft:dirt[]|wooden_pickaxe', keys)
        self.assertIn('minecraft:netherrack[]|mace', keys)
        self.assertNotIn('minecraft:oak_leaves[persistent_bit=false,update_bit=false]|wooden_hoe', keys)
        self.assertIn('minecraft:oak_leaves[persistent_bit=false,update_bit=false]|apple', keys)

    def test_effect_levels_are_zero_based_amplifiers_on_the_hand(self):
        items = sweep.mining_items(self.args(variants='effects', trials=3))
        keys = {i['key']: i['commands'] for i in items}
        self.assertEqual(keys['minecraft:stone[]|hand+haste3'], ['effect {player} haste 1000 2 true'])
        self.assertEqual(keys['minecraft:dirt[]|hand+mining_fatigue2'],
                         ['effect {player} mining_fatigue 1000 1 true'])
        self.assertTrue(all(i['tool'] is None and i['repeats'] == 3 for i in items))

    def test_capture_cells_cover_every_breaking_situation_once(self):
        items = {i['key']: i for i in sweep.mining_items(self.args(variants='capture', mine_ticks=600))}
        self.assertEqual(len(items), 9)
        self.assertTrue(all(i['repeats'] == 1 for i in items.values()))
        self.assertEqual(items['minecraft:dirt[]|hand+predict10']['predict_after'], 10)
        self.assertEqual(items['minecraft:dirt[]|hand+abort5']['max_ticks'], 5)
        self.assertEqual(items['minecraft:dirt[]|hand+stone_above']['cleanup'], ['setblock 1 -58 0 air'])
        self.assertEqual(items['minecraft:stone[]|diamond_pickaxe']['tool'], {'item': 'minecraft:diamond_pickaxe'})

    def test_spears_join_the_attack_list_with_their_gap(self):
        items = sweep.attack_items(self.args(kind='attack', items='spears', gap=20))
        self.assertEqual(items[0], {'key': 'hand', 'tool': None})
        spears = [i for i in items if 'gap' in i]
        self.assertEqual([i['key'] for i in spears], [f'{tier}_spear' for tier in sweep.TIERS])
        self.assertTrue(all(i['gap'] == 20 for i in spears))


class ArmorWear(unittest.TestCase):
    def test_every_armour_piece_in_its_measured_slot(self):
        import argparse
        items = sweep.armor_wear_items(argparse.Namespace(items='slice'))
        slots = {item['item']: item['slot'] for item in items}
        self.assertEqual(slots['minecraft:iron_chestplate'], 'chest')
        self.assertEqual(slots['minecraft:turtle_helmet'], 'head')
        self.assertNotIn(None, slots)

    def test_summary_keys_wear_by_damage_amount(self):
        with tempfile.TemporaryDirectory() as tmp:
            sweep_dir = Path(tmp) / 'wear'
            sweep_dir.mkdir()
            (sweep_dir / 'job.json').write_text(json.dumps({'items': [{'key': 'minecraft:iron_boots'}]}))
            row = {'key': 'minecraft:iron_boots', 'item': 'minecraft:iron_boots', 'slot': 'feet',
                   'hits': [{'damage': 4, 'before': 0, 'after': 1, 'wear': 1},
                            {'damage': 8, 'before': 1, 'after': 3, 'wear': 2}]}
            (sweep_dir / 'results.jsonl').write_text(json.dumps(row) + '\n')
            (sweep_dir / 'runs.jsonl').write_text(json.dumps({'status': 'done'}) + '\n')
            written = {}
            with (mock.patch.object(summarize, 'write', lambda relative, value: written.setdefault(relative, value)),
                  mock.patch.object(summarize, 'update_manifest', lambda family, entry: None),
                  mock.patch.object(summarize, 'harness_revision', lambda: 'test')):
                summarize.summarize_armor_wear(sweep_dir)
        self.assertEqual(written['armor_wear.json'],
                         {'minecraft:iron_boots': {'slot': 'feet', 'wear': {'4': 1, '8': 2}}})


class Wear(unittest.TestCase):
    def test_wear_is_the_damage_each_trial_added_and_none_once_broken(self):
        trials = [{'held_after': {'item': 'minecraft:golden_sword', 'damage': 1}},
                  {'held_after': {'item': 'minecraft:golden_sword', 'damage': 3}},
                  {'held_after': None}]
        self.assertEqual(summarize.wear_per_trial({'item': 'minecraft:golden_sword', 'damage': 0}, trials),
                         [1, 2, None])


class RuntimeSlotTest(unittest.TestCase):
    def test_only_processes_from_this_server_folder_belong_to_the_runtime(self):
        server = Path(tempfile.gettempdir()) / 'slot1' / 'server'
        self.assertTrue(bds_runtime.process_in_runtime(str(server / 'bedrock_server.exe'), server))
        other = Path(tempfile.gettempdir()) / 'slot2' / 'server' / 'bedrock_server.exe'
        self.assertFalse(bds_runtime.process_in_runtime(str(other), server))
        self.assertTrue(bds_runtime.process_in_runtime('?', server))

    def test_session_ports_follow_the_runtime_port(self):
        properties = bds_runtime.base_properties('level')
        self.assertEqual(properties['server-port'], bds_runtime.PORT)
        self.assertEqual(properties['server-portv6'], bds_runtime.PORT + 1)
        self.assertEqual(properties['enable-lan-visibility'], 'false')
        self.assertEqual(bds_runtime.ADDR, f'127.0.0.1:{bds_runtime.PORT}')


class KnockbackTrials(unittest.TestCase):
    def items(self, which: str) -> list[dict]:
        return sweep.knockback_items(type('Args', (), {'items': which})())

    def test_sets_have_unique_keys_and_hits_after_the_settle(self):
        sets = {which: self.items(which) for which in ('slice', 'air', 'far', 'all')}
        self.assertEqual([len(sets[which]) for which in ('slice', 'air', 'far')], [8, 7, 8])
        self.assertEqual(len(sets['all']), 23)
        keys = [item['key'] for item in sets['all']]
        self.assertEqual(len(keys), len(set(keys)))
        for item in sets['all']:
            self.assertTrue(item['hits'] or 'player' in item, item['key'])
            for hit in item['hits']:
                self.assertEqual(set(hit), {'from', 'yaw', 'tick'})

    def test_unknown_set_is_refused(self):
        with self.assertRaises(SystemExit):
            self.items('every')

    def test_cadence_sets_place_both_viewers_and_time_their_commands(self):
        items = self.items('cadence') + self.items('cadence2')
        self.assertEqual(len(items), 20)
        keys = [item['key'] for item in items]
        self.assertEqual(len(keys), len(set(keys)))
        for item in items:
            self.assertEqual(len(item['observer']), 4, item['key'])
            self.assertTrue(item['hits'] or 'player' in item, item['key'])
            for action in item.get('actions', []):
                self.assertEqual(set(action), {'t', 'commands'})
                self.assertTrue(action['commands'] and all(isinstance(c, str) for c in action['commands']))

    def test_reload_phases_name_the_same_zombies(self):
        def items(which):
            return sweep.reload_items(type('Args', (), {'items': which})())
        spawn, reload = items('spawn'), items('reload')
        self.assertEqual([i['key'] for i in spawn], [i['key'] for i in reload])
        self.assertEqual({i['phase'] for i in spawn} | {i['phase'] for i in reload}, {'spawn', 'reload'})
        # Away from the world spawn, so they load only once the player is there.
        self.assertTrue(all(i['at'][0] >= 200 for i in spawn))
        with self.assertRaises(SystemExit):
            items('slice')


def varint(value: int) -> bytes:
    out = bytearray()
    while value >= 0x80:
        out.append(value & 0x7F | 0x80)
        value >>= 7
    out.append(value)
    return bytes(out)


class KnockbackFixture(unittest.TestCase):
    def capture(self, records: list[tuple[int, int, bytes]]) -> Path:
        path = Path(tempfile.mkdtemp()) / 'packets.jsonl'
        lines = [json.dumps({'t_ms': t, 'dir': 's2c', 'id': pid, 'hex': (varint(pid) + body).hex()})
                 for t, pid, body in records]
        path.write_text('\n'.join(lines) + '\n', encoding='utf-8')
        return path

    def test_position_updates_and_motion_are_tied_to_their_samples(self):
        # MoveActorDelta: runtime id, three optional f32 positions, three
        # optional rotation bytes, on-ground and three force flags, ticks.
        delta = (varint(2) + b'\x01' + struct.pack('<f', 2.75) + b'\x01' + struct.pack('<f', -59.5) + b'\x00'
                 + b'\x00' * 3 + b'\x00\x00\x00\x00' + varint(0))
        motion = varint(2) + struct.pack('<3f', 0.21875, 0.3125, 0.0) + varint(0)
        rotation_only = varint(2) + b'\x00' * 3 + b'\x01\x05' + b'\x00' * 2 + b'\x01\x00\x00\x00' + varint(0)
        capture = list(knockback_fixture.packets(self.capture(
            [(100, 111, rotation_only), (150, 111, delta), (152, 40, motion)])))
        trial = {'key': 'cow', 'runtime_id': 2}
        samples = [{'mob': {'pos': [2.5, -60.0, 0.5]}}, {'mob': {'pos': [2.75, -59.5, 0.5]}}]
        wire = knockback_fixture.wire_for(trial, capture, samples)
        self.assertEqual(wire, [
            {'sample': 1, 'delta': [2.75, -59.5, None], 'ground': False, 'ticks': 0},
            {'sample': 1, 'motion': [0.21875, 0.3125, 0.0], 'tick': 0},
        ])

    def test_motion_before_the_mob_was_added_is_refused(self):
        motion = varint(2) + struct.pack('<3f', 0.1, 0.0, 0.0) + varint(0)
        capture = list(knockback_fixture.packets(self.capture([(10, 40, motion)])))
        with self.assertRaises(SystemExit):
            knockback_fixture.wire_for({'key': 'cow', 'runtime_id': 2}, capture, [{'mob': {'pos': [0, 0, 0]}}])

    def test_a_far_mobs_motion_without_an_update_counts_ticks_from_the_last_one(self):
        add_actor = varint(4) + varint(2) + varint(len(b'minecraft:cow')) + b'minecraft:cow'
        motion = varint(2) + struct.pack('<3f', 0.0, -0.25, 0.0) + varint(0)
        capture = list(knockback_fixture.packets(self.capture([(1000, 13, add_actor), (1148, 40, motion)])))
        samples = [{'mob': {'pos': [0.5, -40.0, 0.5]}} for _ in range(5)]
        wire = knockback_fixture.wire_for({'key': 'cow', 'runtime_id': 2}, capture, samples)
        self.assertEqual(wire, [{'sample': 3, 'motion': [0.0, -0.25, 0.0], 'tick': 0}])


class ReplicationFixture(unittest.TestCase):
    capture = KnockbackFixture.capture

    def test_timed_packets_stay_with_their_batch_or_between_anchors(self):
        add_actor = (varint(4) + varint(2) + varint(len(b'minecraft:cow')) + b'minecraft:cow'
                     + struct.pack('<6f', 0.5, -40.0, 0.5, 0.0, -1e-5, 0.0))
        rotation_only = varint(2) + b'\x00' * 3 + b'\x00\x01\x05\x00' + b'\x00' * 4 + varint(3)
        delta = (varint(2) + b'\x00' + b'\x01' + struct.pack('<f', -40.5) + b'\x00' + b'\x00' * 3
                 + b'\x00' * 4 + varint(0))
        motion = varint(2) + struct.pack('<3f', 0.0, -0.25, 0.0) + varint(0)
        capture = list(replication_fixture.packets(self.capture([
            (1000, 13, add_actor), (1001, 111, rotation_only),
            # Arrives 77 ms after the AddActor batch but 31 ms before the
            # batch of the next matched update: it belongs to the tick before.
            (1077, 40, motion), (1108, 111, delta), (1109, 40, motion)])))
        samples = [{'mob': {'pos': [0.5, y, 0.5]}} for y in (-40.0, -40.0, -40.2, -40.5, -41.0)]
        wire = replication_fixture.wire_for(2, capture, samples)
        self.assertEqual([(event['sample'], event.get('timed', False)) for event in wire],
                         [(0, False), (1, True), (2, True), (3, False), (3, True)])
        self.assertEqual(wire[0]['add'], {'pos': [0.5, -40.0, 0.5], 'vel': [0.0, -1e-05, 0.0]})
        self.assertEqual(wire[1]['rot'], [None, 5, None])
        self.assertEqual(wire[1]['ticks'], 3)
def nbt_string(text: str) -> bytes:
    return struct.pack('<H', len(text)) + text.encode()


def palette_entry(name: str, states: bytes = b'') -> bytes:
    """`{name, states, version}` as BDS 1.26.51 saves a palette entry."""
    return (b'\x0a' + nbt_string('') + b'\x08' + nbt_string('name') + nbt_string(name)
            + b'\x0a' + nbt_string('states') + states + b'\x00'
            + b'\x03' + nbt_string('version') + struct.pack('<i', 18168865) + b'\x00')


class SubchunkRecords(unittest.TestCase):
    def test_two_bit_persistent_storage_decodes_in_xzy_order(self):
        words = b''.join(struct.pack('<I', sum(((w * 16 + i) % 3) << (2 * i) for i in range(16)))
                         for w in range(256))
        axis = b'\x08' + nbt_string('pillar_axis') + nbt_string('x')
        value = (bytes([9, 1, 0xFC, 2 << 1]) + words + struct.pack('<i', 3) + palette_entry('minecraft:air')
                 + palette_entry('minecraft:stone') + palette_entry('minecraft:oak_log', axis))
        (layer,) = subchunk_fixture.subchunk(value)
        self.assertEqual(layer['bits'], 2)
        cells = subchunk_fixture.layer_states(layer)
        self.assertEqual(cells[0], ('minecraft:air', ()))
        self.assertEqual(cells[4095], ('minecraft:air', ()))  # 4095 % 3 == 0
        self.assertEqual(subchunk_fixture.cell(cells[2]), 'minecraft:oak_log|pillar_axis=sx')
        with self.assertRaises(ValueError):
            subchunk_fixture.subchunk(value + b'\x00')  # bytes after the last storage

    def test_zero_bit_storage_has_no_palette_length(self):
        (layer,) = subchunk_fixture.subchunk(bytes([9, 1, 1, 0]) + palette_entry('minecraft:end_stone'))
        self.assertEqual(layer['palette'], [('minecraft:end_stone', ())])
        self.assertEqual(set(layer['indices']), {0})

    def test_runtime_form_and_out_of_palette_indices_are_rejected(self):
        with self.assertRaises(ValueError):
            subchunk_fixture.subchunk(bytes([9, 1, 0, 1, 2]))
        words = struct.pack('<I', 1) + bytes(4 * 127)
        with self.assertRaises(ValueError):
            subchunk_fixture.subchunk(bytes([9, 1, 0, 2]) + words + struct.pack('<i', 1)
                                      + palette_entry('minecraft:air'))

    def test_keys_name_dimension_chunk_and_y(self):
        key = subchunk_fixture.chunk_key(1, -2, 3, -4)
        self.assertEqual(len(key), 14)
        self.assertEqual(subchunk_fixture.parse_key(key), (1, -2, 3, -4))
        self.assertEqual(subchunk_fixture.parse_key(subchunk_fixture.chunk_key(0, 5, 6, 7)), (0, 5, 6, 7))


class LevelDbLog(unittest.TestCase):
    def test_newest_write_and_deletions_win(self):
        def batch(sequence: int, writes: list) -> bytes:
            body = struct.pack('<QI', sequence, len(writes))
            for key, value in writes:
                body += bytes([0 if value is None else 1, len(key)]) + key
                if value is not None:
                    body += bytes([len(value)]) + value
            return struct.pack('<IHB', 0, len(body), 1) + body  # one FULL record
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / '000003.log'
            log.write_bytes(batch(1, [(b'a', b'old'), (b'b', b'gone')]) + batch(3, [(b'a', b'new'), (b'b', None)]))
            self.assertEqual(leveldb_read.read_db(tmp), {b'a': b'new'})


if __name__ == '__main__':
    unittest.main()
