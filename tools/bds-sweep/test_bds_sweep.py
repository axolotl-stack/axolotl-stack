"""Tests for the BDS-free parts of the sweep harness.

    python -m unittest discover -s tools/bds-sweep -p "test_*.py"
"""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
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


class BlocksTrace(unittest.TestCase):
    LOG = '\n'.join([
        'Z INFO bds_sweep_client: trace LevelSoundEventPacket(LevelSoundEventPacket { sound_event: "place", '
        'position: Vec3 { x: 0.0, y: -60.0, z: 0.0 }, data: -1, actor_identifier: "", is_baby: false, '
        'is_global: false, actor_unique_id: -1, fire_at_position: None })',
        'Z INFO bds_sweep_client: plugin message MessageOnly(TextPacketPayloadMessageOnly { message_type: Raw, '
        'message: "bds-sweep:mark doors/toggle" })',
        'Z INFO bds_sweep_client: trace LevelSoundEventPacket(LevelSoundEventPacket { sound_event: "door.open", '
        'position: Vec3 { x: 0.5, y: -59.5, z: 0.5 }, data: -1476754069, actor_identifier: "", is_baby: false, '
        'is_global: false, actor_unique_id: -1, fire_at_position: None })',
        'Z INFO bds_sweep_client: trace UpdateBlockPacket(UpdateBlockPacket { block_position: BlockPos '
        '{ x: 0, y: -60, z: 0 }, block_runtime_id: 7, flags: 3, layer: 0 })',
        'Z INFO bds_sweep_client: trace UpdateBlockPacket(UpdateBlockPacket { block_position: BlockPos '
        '{ x: 0, y: -60, z: 0 }, block_runtime_id: 7, flags: 3, layer: 0 })',
        'Z INFO bds_sweep_client: trace LevelEventPacket(LevelEventPacket { event_id: 3600, position: Vec3 '
        '{ x: 0.0, y: -59.0, z: 0.0 }, data: 728 })',
    ])

    def test_packets_after_a_mark_are_decoded_grouped_and_named(self):
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / 'client.log'
            log.write_text(self.LOG, encoding='utf-8')
            steps = summarize.trace_packets(log, {2818213227: 'minecraft:wooden_door', 7: 'minecraft:air'})
        # Packets before the first mark belong to no step.
        self.assertEqual(list(steps), ['doors/toggle'])
        step = steps['doors/toggle']
        self.assertEqual(step['sounds'], [{'sound': 'door.open', 'position': [0.5, -59.5, 0.5],
                                           'data': 'minecraft:wooden_door'}])
        self.assertEqual(step['block_updates'], [{'position': [0, -60, 0], 'block': 'minecraft:air', 'times': 2}])
        self.assertEqual(step['level_events'], [{'event': 3600, 'name': 'StartBlockCracking',
                                                 'position': [0.0, -59.0, 0.0], 'data': 728}])

    def test_compact_json_keeps_short_values_on_one_line(self):
        text = summarize.compact_json({'a': [1, 2], 'b': {'c': 'x' * 200}}, width=40)
        self.assertEqual(json.loads(text), {'a': [1, 2], 'b': {'c': 'x' * 200}})
        self.assertIn('"a": [1, 2]', text)



if __name__ == '__main__':
    unittest.main()
