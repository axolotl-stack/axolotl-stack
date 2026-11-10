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

    def test_spears_join_the_attack_list_with_their_gap(self):
        items = sweep.attack_items(self.args(kind='attack', items='spears', gap=20))
        self.assertEqual(items[0], {'key': 'hand', 'tool': None})
        spears = [i for i in items if 'gap' in i]
        self.assertEqual([i['key'] for i in spears], [f'{tier}_spear' for tier in sweep.TIERS])
        self.assertTrue(all(i['gap'] == 20 for i in spears))


class Wear(unittest.TestCase):
    def test_wear_is_the_damage_each_trial_added_and_none_once_broken(self):
        trials = [{'held_after': {'item': 'minecraft:golden_sword', 'damage': 1}},
                  {'held_after': {'item': 'minecraft:golden_sword', 'damage': 3}},
                  {'held_after': None}]
        self.assertEqual(summarize.wear_per_trial({'item': 'minecraft:golden_sword', 'damage': 0}, trials),
                         [1, 2, None])


if __name__ == '__main__':
    unittest.main()
