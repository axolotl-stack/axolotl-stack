"""Tests for the BDS-free parts of the sweep harness.

    python -m unittest discover -s tools/bds-sweep -p "test_*.py"
"""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import base64  # noqa: E402

import bds_runtime  # noqa: E402
import capture  # noqa: E402
import commit_protocol_fixtures  # noqa: E402
import native  # noqa: E402
import protocol_probe  # noqa: E402
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


class PairCaptureTest(unittest.TestCase):
    def test_guests_are_numbered_after_the_host(self):
        self.assertEqual(capture.guest_names(3), ['BDSExtractor2', 'BDSExtractor3', 'BDSExtractor4'])

    def test_overlapping_guests_stay_until_the_last_one_joined(self):
        # Taking turns: a short stay each.
        self.assertEqual([capture.guest_settle(i, 3, None) for i in range(3)], [3, 3, 3])
        # Started 7 s apart: the first stays 14 s longer than the last.
        self.assertEqual([capture.guest_settle(i, 3, 7.0) for i in range(3)], [17, 10, 3])

    def test_settle_is_passed_to_the_extractor(self):
        command = capture.extractor_command(Path('x.exe'), Path('out'), name='G', settle=4)
        self.assertEqual(command[command.index('--name') + 1], 'G')
        self.assertEqual(command[command.index('--settle') + 1], '4')
        self.assertNotIn('--commands', command)


class ProtocolProbeTest(unittest.TestCase):
    def test_client_data_images_match_their_dimensions(self):
        for data in [protocol_probe.client_data('classic'), protocol_probe.client_data('persona'),
                     *(protocol_probe.variant(kind) for kind in ('all-pieces', 'floats', 'odd'))]:
            width, height = data.get('SkinImageWidth', 64), data.get('SkinImageHeight', 64)
            self.assertEqual(len(base64.b64decode(data['SkinData'])), width * height * 4)
            self.assertEqual(len(base64.b64decode(data['CapeData'])), 64 * 32 * 4)

    def test_skin_cases_cover_every_type_once_for_pieces_and_tints(self):
        cases = protocol_probe.skin_cases()
        types = len(protocol_probe.PIECE_TYPES) + 1  # and persona_bogus
        self.assertEqual(sum(name.startswith('piece-') for name in cases), types)
        self.assertEqual(sum(name.startswith('tint-') for name in cases), types + 1)  # and bogus_type
        self.assertNotIn('persona_hands', protocol_probe.PIECE_TYPES)

    def test_extractor_command_passes_skins_and_the_blob_cache(self):
        command = capture.extractor_command(Path('x.exe'), Path('out'), client_data=Path('skin.json'),
                                            blob_cache=True)
        self.assertEqual(command[command.index('--client-data') + 1], 'skin.json')
        self.assertIn('--blob-cache', command)
        self.assertNotIn('--input', command)

    def test_extractor_command_sends_input_on_request(self):
        command = capture.extractor_command(Path('x.exe'), Path('out'), player_input=True)
        self.assertIn('--input', command)

    def test_fixture_timelines_start_after_the_join(self):
        with tempfile.TemporaryDirectory() as tmp:
            session, target = Path(tmp) / 'guest', Path(tmp) / 'out'
            session.mkdir()
            (session / 'manifest.json').write_text(json.dumps({'phases': {'join_end': 2}}), encoding='utf-8')
            records = [
                {'seq': 0, 't_ms': 0, 'dir': 's2c', 'name': 'EmotePacket', 'hex': '8a01'},
                {'seq': 2, 't_ms': 5, 'dir': 'c2s', 'name': 'EmotePacket', 'hex': '8a01'},
                {'seq': 3, 't_ms': 6, 'dir': 's2c', 'name': 'MoveActorDeltaPacket', 'hex': '6f'},
            ]
            (session / 'packets.jsonl').write_text(''.join(json.dumps(r) + '\n' for r in records),
                                                   encoding='utf-8')
            copied = commit_protocol_fixtures.copy_session(session, target, None, False)
            self.assertEqual(copied, 2)
            self.assertTrue((target / '00000-EmotePacket.bin').exists())
            self.assertTrue((target / 'sent' / '00002-EmotePacket.bin').exists())
            timeline = json.loads((target / 'timeline.json').read_text(encoding='utf-8'))
            self.assertEqual([entry[0] for entry in timeline], [2, 3])


if __name__ == '__main__':
    unittest.main()
