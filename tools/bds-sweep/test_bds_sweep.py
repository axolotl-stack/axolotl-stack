"""Tests for the BDS-free parts of the sweep harness.

    python -m unittest discover -s tools/bds-sweep -p "test_*.py"
"""

import json
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import breaking_fixture  # noqa: E402
import combat_fixture  # noqa: E402
import experience_table  # noqa: E402
import food_table  # noqa: E402
import native  # noqa: E402
import summarize  # noqa: E402
import sweep  # noqa: E402
import unbreaking_table  # noqa: E402


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


def record(direction, packet_id, name, payload, seq=0):
    return {'seq': seq, 't_ms': seq, 'dir': direction, 'id': packet_id, 'name': name, 'hex': payload.hex()}


def auth_input(tick):
    """A PlayerAuthInput laid out as `combat_fixture.input_tick` reads it."""
    return bytes([144, 1]) + bytes(32) + bytes([0, 0, 0, 0]) + bytes(8) + bytes([tick])


class CombatFixture(unittest.TestCase):
    def test_windows_keep_inputs_actions_and_answers_but_not_ambient_sounds(self):
        sound = lambda name: bytes([123, len(name)]) + name  # noqa: E731
        rows = [
            record('c2s', 144, 'PlayerAuthInputPacket', auth_input(5)),
            record('c2s', 30, 'InventoryTransactionPacket', bytes([30])),
            record('c2s', 144, 'PlayerAuthInputPacket', auth_input(10)),
            record('c2s', 30, 'InventoryTransactionPacket', bytes([30])),
            record('s2c', 123, 'LevelSoundEventPacket', sound(b'item.iron_spear.attack_miss')),
            record('s2c', 123, 'LevelSoundEventPacket', sound(b'ambient')),
            record('s2c', 29, 'UpdateAttributesPacket', bytes([29, 1])),
            record('s2c', 29, 'UpdateAttributesPacket', bytes([29, 4])),
            record('s2c', 39, 'SetActorDataPacket', bytes([39, 1])),
            record('s2c', 13, 'AddActorPacket', bytes([13, 8])),
        ]
        kept = combat_fixture.cut(rows, windows=[(8, 20)])
        self.assertEqual([(r['name'], r['input']) for r in kept], [
            ('PlayerAuthInputPacket', 10), ('InventoryTransactionPacket', 10), ('LevelSoundEventPacket', 10),
            ('UpdateAttributesPacket', 10), ('SetActorDataPacket', 10), ('AddActorPacket', 10)])
        self.assertEqual(kept[3]['hex'], '1d04', "only other actors' attributes")

    def test_commands_keep_one_command_and_the_output_answering_each(self):
        request = lambda text: bytes([77, len(text)]) + text  # noqa: E731
        rows = [
            record('c2s', 77, 'CommandRequestPacket', request(b'/replaceitem entity @s slot.weapon.mainhand 0 iron_spear')),
            record('c2s', 77, 'CommandRequestPacket', request(b'/enchant @s sharpness 1')),
            record('s2c', 79, 'CommandOutputPacket', bytes([79, 1])),
            record('s2c', 79, 'CommandOutputPacket', bytes([79, 2])),
        ]
        kept = combat_fixture.cut(rows, commands='enchant')
        self.assertEqual([r['hex'] for r in kept], [request(b'/enchant @s sharpness 1').hex(), '4f02'])


class ExperienceTable(unittest.TestCase):
    def test_breaks_take_the_tool_the_script_gave_last(self):
        script = '\n'.join([
            '# comment',
            '30 /give @s netherite_pickaxe',
            '32 /enchant @s efficiency 5',
            '40 /setblock 1 -59 0 coal_ore',
            '50 /clear @s',
            '60 /setblock 1 -59 0 coal_ore',
            '61 /setblock 9 9 9 stone',
        ])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'script.txt'
            path.write_text(script, encoding='utf-8')
            breaks = experience_table.schedule(path)
        self.assertEqual(breaks, [(40, 'minecraft:coal_ore', 'netherite_pickaxe+efficiency5'),
                                  (60, 'minecraft:coal_ore', 'hand')])


class BreakingFixture(unittest.TestCase):
    @staticmethod
    def rows(*specs):
        return [{'seq': i, 't_ms': t, 'dir': d, 'id': packet_id, 'name': name, 'hex': payload}
                for i, (t, d, packet_id, name, payload) in enumerate(specs)]

    def test_the_observer_is_moved_onto_the_players_clock_and_inputs(self):
        player = self.rows(
            (1000, 'c2s', 144, 'PlayerAuthInputPacket', '80'),
            (1040, 's2c', 21, 'UpdateBlockPacket', 'aa'),
            (1050, 'c2s', 144, 'PlayerAuthInputPacket', '81'),
            (1090, 's2c', 21, 'UpdateBlockPacket', 'bb'),
            (1100, 'c2s', 144, 'PlayerAuthInputPacket', '82'),
        )
        observer = self.rows(
            (3040, 's2c', 21, 'UpdateBlockPacket', 'aa'),
            (3060, 's2c', 123, 'LevelSoundEventPacket', 'cc'),
            (3090, 's2c', 21, 'UpdateBlockPacket', 'bb'),
            (3095, 's2c', 30, 'InventoryTransactionPacket', 'dd'),
        )
        # The inputs' ticks, without encoding real PlayerAuthInputs.
        with mock.patch.object(breaking_fixture, 'input_tick', int):
            self.assertEqual(breaking_fixture.clock_offset(player, observer), -2000)
            out = breaking_fixture.cut_observer(player, observer, [(80, 90)])
        self.assertEqual([(r['dir'], r['input'], r['t_ms'], r['hex']) for r in out],
                         [('s2o', 80, 1040, 'aa'), ('s2o', 81, 1060, 'cc'), ('s2o', 81, 1090, 'bb')])

    def test_captures_without_a_common_block_update_are_not_aligned(self):
        with self.assertRaises(SystemExit):
            breaking_fixture.clock_offset(self.rows((0, 's2c', 21, 'UpdateBlockPacket', 'aa')),
                                          self.rows((0, 's2c', 21, 'UpdateBlockPacket', 'bb')))

    def test_windows_parse(self):
        self.assertEqual(breaking_fixture.parse_windows('80-110,120-130'), [(80, 110), (120, 130)])


class FoodTable(unittest.TestCase):
    def test_each_use_runs_to_the_clear_after_it(self):
        script = '\n'.join([
            '# comment',
            '50 /effect @s hunger 6 63 true',
            '175 /effect @s clear',
            '177 /give @s apple 1 0',
            '180 use',
            '220 /effect @s clear',
            '225 /effect @s clear',
            '230 /give @s suspicious_stew 1 5',
            '235 use',
            '275 /effect @s clear',
        ])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'script.txt'
            path.write_text(script, encoding='utf-8')
            foods = food_table.schedule(path)
        self.assertEqual(foods, [(180, 220, 'apple', 0), (235, 275, 'suspicious_stew', 5)])


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


class UnbreakingTable(unittest.TestCase):
    def test_damage_reads_the_first_stack_that_has_one(self):
        body = ('InventoryContentPacket { container_id: 0, slots: [{ id: 0, user_data_buffer: b"" }, '
                r'{ id: 301, user_data_buffer: b"\xff\xff\x01\n\0\0\x03\x06\0Damage\x05\0\0\0\0" }] }')
        self.assertEqual(unbreaking_table.damage(body), 5)
        self.assertEqual(unbreaking_table.damage('no stacks'), 0)

    def test_script_counts_hits_and_uses_per_marker(self):
        with tempfile.TemporaryDirectory() as tmp:
            script = Path(tmp) / 'unbreaking.txt'
            script.write_text('\n'.join([
                '10 /say unbreaking armour 1', '11 /damage @s 8 entity_attack', '22 /damage @s 8 entity_attack',
                '30 /say unbreaking use 2', '31 use-on 2 -60 0 1',
                '40 /say unbreaking use 2', '41 use-on 2 -60 0 1', '43 use-on 2 -60 0 1',
            ]), encoding='utf-8')
            hits, uses = unbreaking_table.script_counts(script)
        self.assertEqual(hits, {'1': 2})
        self.assertEqual(uses, {'2': [1, 2]})


if __name__ == '__main__':
    unittest.main()
