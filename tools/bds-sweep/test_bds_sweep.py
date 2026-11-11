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
