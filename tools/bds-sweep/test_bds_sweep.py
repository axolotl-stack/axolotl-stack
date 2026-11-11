"""Tests for the BDS-free parts of the sweep harness.

    python -m unittest discover -s tools/bds-sweep -p "test_*.py"
"""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import native  # noqa: E402
import player_fixture  # noqa: E402
import player_script  # noqa: E402
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


class PlayerScript(unittest.TestCase):
    def test_steps_and_properties_parse(self):
        properties, steps = player_script.parse('\n'.join([
            'property difficulty=peaceful',
            '# comment',
            'launch Mover 00000000-0000-0000-0000-000000000001',
            'wait 1.5',
            'cmd tp Mover 0.5 -60 0.5',
            'order Mover move 1 0',
            'mark walk  # trailing comment',
            "expect 'Teleported Mover' 10",
            'quit Mover',
        ]))
        self.assertEqual(properties, {'difficulty': 'peaceful'})
        self.assertEqual(steps, [
            ['launch', 'Mover', '00000000-0000-0000-0000-000000000001'],
            ['wait', '1.5'],
            ['cmd', 'tp Mover 0.5 -60 0.5'],
            ['order', 'Mover', 'move 1 0'],
            ['mark', 'walk'],
            ['expect', 'Teleported Mover', '10'],
            ['quit', 'Mover'],
        ])

    def test_unknown_steps_and_bad_values_are_refused(self):
        for text in ('walk 3', 'property difficulty', 'wait soon'):
            with self.assertRaises(ValueError):
                player_script.parse(text)

    def test_orders_are_tellraw_to_one_player(self):
        self.assertEqual(player_script.tellraw('Mover', 'jump 3'),
                         'tellraw Mover {"rawtext": [{"text": "bds-sweep:jump 3"}]}')



class PlayerFixture(unittest.TestCase):
    SPEC = {'client': 'Subject', 'subject': 'Subject'}

    def test_only_packets_about_the_subject_carry_its_ticks(self):
        about = player_fixture.about_subject
        mine = 'MovePlayerPacket { player_runtime_id: ActorRuntimeId { actor_runtime_id: 1 }, tick: 5 }'
        other = 'MovePlayerPacket { player_runtime_id: ActorRuntimeId { actor_runtime_id: 2 }, tick: 5 }'
        correction = 'CorrectPlayerMovePredictionPacket { tick: PlayerInputTick { inputtick: 5 } }'
        self.assertTrue(about(mine, 1, self.SPEC))
        self.assertFalse(about(other, 1, self.SPEC))
        self.assertTrue(about(correction, 1, self.SPEC))
        self.assertFalse(about(correction, 1, {'client': 'Viewer', 'subject': 'Subject'}))

    def test_attribute_values_are_read_by_name(self):
        body = ('attribute_list: [AttributeData { min_value: 0.0, max_value: 20.0, current_value: 19.0, '
                'default_min_value: 0.0, default_max_value: 20.0, default_value: 20.0, '
                'name: "minecraft:health", modifiers: [] }, AttributeData { min_value: 0.0, '
                'max_value: 20.0, current_value: 1.3973968, default_min_value: 0.0, '
                'default_max_value: 20.0, default_value: 0.0, name: "minecraft:player.exhaustion", '
                'modifiers: [] }]')
        self.assertEqual(player_fixture.ATTRIBUTE.findall(body), [
            ('19.0', 'minecraft:health'), ('1.3973968', 'minecraft:player.exhaustion')])


if __name__ == '__main__':
    unittest.main()
