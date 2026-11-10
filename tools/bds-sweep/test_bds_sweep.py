"""Tests for the BDS-free parts of the sweep harness.

    python -m unittest discover -s tools/bds-sweep -p "test_*.py"
"""

import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import knockback_fixture  # noqa: E402
import native  # noqa: E402
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

if __name__ == '__main__':
    unittest.main()
