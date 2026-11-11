"""Tests for the BDS-free parts of the sweep harness.

    python -m unittest discover -s tools/bds-sweep -p "test_*.py"
"""

import json
import re
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import native  # noqa: E402
import scenarios  # noqa: E402
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


if __name__ == '__main__':
    unittest.main()


class PlacementCases(unittest.TestCase):
    def test_keys_are_unique_marker_safe_and_grouped(self):
        import placements
        items = placements.items()
        keys = [item['key'] for item in items]
        self.assertEqual(len(keys), len(set(keys)))
        self.assertTrue(all(re.fullmatch(r'[A-Za-z0-9_-]+', key) for key in keys))
        self.assertEqual({item['group'] for item in items}, set(placements.GROUPS))
        signs = placements.items('signs')
        self.assertTrue(all(item['group'] == 'signs' for item in signs))
        steps = {step[0] for item in items for step in item.get('after', [])}
        self.assertLessEqual(steps, {'wait', 'hand', 'use', 'sign'})


class CaptureMarkers(unittest.TestCase):
    def test_packets_are_cut_per_marker_with_hex_only_where_decoded(self):
        import summarize

        def text(message: str) -> str:
            return (bytes([9, 0, 0, 0]) + message.encode()).hex()

        records = [
            {'dir': 's2c', 'name': 'TextPacket', 'hex': text('bds-sweep:mark case_1')},
            {'dir': 'c2s', 'name': 'InventoryTransactionPacket', 'hex': 'aa'},
            {'dir': 's2c', 'name': 'MoveActorDeltaPacket', 'hex': 'bb'},
            {'dir': 's2c', 'name': 'UpdateAttributesPacket', 'hex': 'cc'},
            {'dir': 's2c', 'name': 'TextPacket', 'hex': text('bds-sweep:mark end')},
            {'dir': 's2c', 'name': 'UpdateBlockPacket', 'hex': 'dd'},
        ]
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'packets.jsonl'
            path.write_text('\n'.join(json.dumps(r) for r in records), encoding='utf-8')
            cut = summarize.capture_by_marker(path)
        self.assertEqual(cut, {'case_1': [
            {'dir': 'c2s', 'name': 'InventoryTransactionPacket', 'hex': 'aa'},
            {'dir': 's2c', 'name': 'UpdateAttributesPacket'},
        ]})


class RoundTwoScenarios(unittest.TestCase):
    def test_round_two_families_are_registered_and_small_enough(self):
        import scenarios
        for name in ('stairs_blocks', 'connections_blocks', 'wall_heights_blocks', 'multiface_support', 'rails',
                     'soils_more', 'coral_death'):
            self.assertIn(name, scenarios.FAMILIES)
        rails = scenarios.FAMILIES['rails']()
        self.assertEqual(len(rails), 4 * 81 * 2)
        self.assertEqual(len({item['key'] for item in rails}), len(rails))


class RoundThree(unittest.TestCase):
    """Circuit and random tick families and the wave signature."""

    def test_measured_families_have_unique_keys_and_one_layout(self):
        import circuits
        import random_ticks
        for family in list(circuits.MEASURED) + list(random_ticks.FAMILIES):
            items = scenarios.FAMILIES[family]()
            keys = [item['key'] for item in items]
            self.assertEqual(len(keys), len(set(keys)), family)
            self.assertTrue(all(key.startswith(family + '|') for key in keys), family)
        self.assertEqual(scenarios.layout(','.join(circuits.MEASURED)), 'circuits')
        self.assertEqual(scenarios.layout(','.join(random_ticks.FAMILIES)), 'fields')
        with self.assertRaises(SystemExit):
            scenarios.layout('redstone_dust,random_crops')

    def test_coral_families_share_the_spaced_layout(self):
        import coral
        for family in list(coral.FAMILIES) + ['coral_death']:
            keys = [item['key'] for item in scenarios.FAMILIES[family]()]
            self.assertEqual(len(keys), len(set(keys)), family)
        self.assertEqual(scenarios.layout('coral_death,coral_water,coral_timing'), 'coral')
        spacing = coral.LAYOUT['spacing']
        (low_x, _, low_z), (high_x, _, high_z) = coral.LAYOUT['clear_box']
        # Water flows 7 blocks: two cells' boxes never overlap.
        self.assertGreater(min(spacing), max(high_x - low_x, high_z - low_z))

    def test_leftover_clicks_end_with_the_normal_difficulty_cases(self):
        import placements
        items = placements.items('leftovers')
        keys = [item['key'] for item in items]
        self.assertEqual(len(keys), len(set(keys)))
        changes = [i for i, item in enumerate(items) if item.get('before')]
        self.assertEqual(changes, [len(items) - 2, len(items) - 1])

    def test_signature_splits_clicks_and_commands_into_their_own_waves(self):
        sys.path.insert(0, str(Path(__file__).resolve().parent / 'endstone-sweep' / 'src'))
        from endstone_bds_sweep import jobs
        click = {'key': 'f|a', 'steps': [['build', []], ['timeline', 10, 't', [[1, ['use', [0, 0, 0], 1,
                                                                                     [0, 0, 0]]]], []]]}
        plain = {'key': 'f|b', 'steps': [['build', []], ['timeline', 10, 't', [[1, ['set', [0, 0, 0],
                                                                                     'minecraft:air', {}, True]]],
                                                         []]]}
        fast = {'key': 'f|c', 'steps': [['cmd', 'gamerule randomtickspeed 8', [0, 0, 0]]]}
        slow = {'key': 'f|d', 'steps': [['cmd', 'gamerule randomtickspeed 128', [0, 0, 0]]]}
        for item in (click, plain, fast, slow):
            self.assertEqual(list(jobs.step_signature(item)), scenarios.signature(item))
        self.assertIn('f|a', jobs.step_signature(click)[1])
        self.assertEqual(jobs.step_signature(plain)[1], 'timeline10')
        self.assertNotEqual(jobs.step_signature(fast), jobs.step_signature(slow))

    def test_circuit_rows_derive_conductors_cuts_and_climbing(self):
        trace_lit = [[0, 1, 'unpowered_repeater', {}], [0, 2, 'stone', {}], [0, 3, 'redstone_lamp', {}],
                     [4, 1, 'powered_repeater', {}], [4, 3, 'lit_redstone_lamp', {}]]
        trace_late = [[0, 1, 'unpowered_repeater', {}], [0, 2, 'bell', {}], [0, 3, 'redstone_lamp', {}],
                      [4, 1, 'powered_repeater', {}], [8, 3, 'lit_redstone_lamp', {}]]
        cut = [[0, 2, 'redstone_wire', {'redstone_signal': 0}], [0, 3, 'stone', {}],
               [5, 1, 'redstone_wire', {'redstone_signal': 15}]]
        climb = [[0, 3, 'redstone_wire', {'redstone_signal': 0}], [4, 3, 'redstone_wire', {'redstone_signal': 14}],
                 [6, 3, 'air', {}]]
        derived = summarize.circuit_blocks({
            'redstone_conductors': [[['block', 'stone', {}], 2, [], trace_lit],
                                    [['block', 'bell', {}], 2, [], trace_late]],
            'redstone_dust_cut': [[['cut', 'stone', {}], 2, [], cut]],
            'redstone_dust_blocks': [[['climb', 'acacia_trapdoor', {'open_bit': False}], 2, [], climb]],
        })
        self.assertEqual(derived['conductors'], ['minecraft:stone'])
        self.assertEqual(derived['cuts'], ['minecraft:stone'])
        # Ever lit: the dust on top popped later.
        self.assertEqual(derived['rows'], [['minecraft:acacia_trapdoor', {'open_bit': False}, True, False]])
