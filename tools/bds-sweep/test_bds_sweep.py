"""Tests for the BDS-free parts of the sweep harness.

    python -m unittest discover -s tools/bds-sweep -p "test_*.py"
"""

import json
import re
import struct
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import capture  # noqa: E402
import commit_fixtures  # noqa: E402
import knockback_fixture  # noqa: E402
import replication_fixture  # noqa: E402
import leveldb_read  # noqa: E402
import native  # noqa: E402
import player_fixture  # noqa: E402
import player_relay_fixture  # noqa: E402
import player_script  # noqa: E402
import range_fixture  # noqa: E402
import spawn_bonus_fixture  # noqa: E402
import subchunk_fixture  # noqa: E402
import subchunk_roundtrip  # noqa: E402
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


class CommandFeedback(unittest.TestCase):
    def test_commands_take_the_last_segment_above_them(self):
        with tempfile.TemporaryDirectory() as tmp:
            script = Path(tmp) / 'commands.txt'
            lines = ['# intro', '/a', '# segment: one', '', '/b', '#note', '/c', '# segment: two', '/d']
            script.write_text('\n'.join(lines) + '\n', encoding='utf-8')
            self.assertEqual(commit_fixtures.script_segments(script),
                             [('', '/a'), ('one', '/b'), ('one', '/c'), ('two', '/d')])

    def test_world_packets_count_only_on_the_platform_or_at_the_origin(self):
        relevance = commit_fixtures.Relevance.__new__(commit_fixtures.Relevance)
        relevance.player_runtime, relevance.player_unique = 1, -5
        relevance.runtime, relevance.unique = {1}, {-5}
        event = {'position': {'x': 0.0, 'y': 0.0, 'z': 0.0}, 'event_id': 3001, 'data': 65535}
        self.assertTrue(relevance.keep('/weather rain', 'LevelEventPacket', event, 30))
        far = dict(event, position={'x': 100.5, 'y': 64.0, 'z': -28.5})
        self.assertFalse(relevance.keep('/weather rain', 'LevelEventPacket', far, 30))
        other = {'target_runtime_id': {'actor_runtime_id': 9}}
        self.assertFalse(relevance.keep('/time query day', 'SetActorDataPacket', other, 30))
        added = {'target_runtime_id': {'actor_runtime_id': 9}, 'target_actor_id': {'actor_unique_id': -9}}
        self.assertTrue(relevance.keep('/execute align xyz run summon pig ~ ~ ~', 'AddActorPacket', added, 50))
        self.assertTrue(relevance.keep('/damage @e 1', 'SetActorDataPacket', other, 30))
        self.assertFalse(relevance.keep('/damage @e 1', 'SetActorDataPacket', other, 400))
        self.assertFalse(relevance.keep('/say hi', 'NetworkChunkPublisherUpdatePacket', {}, 30))
        # A second client's own actor packets are kept from its point of view.
        alice = {'target_runtime_id': {'actor_runtime_id': 2}}
        self.assertFalse(relevance.keep('/effect @a speed', 'MobEffectPacket', alice, 400))
        self.assertTrue(relevance.keep('/effect @a speed', 'MobEffectPacket', alice, 400, viewer=2))
        self.assertTrue(relevance.keep('/effect @a speed', 'MobEffectPacket',
                                       {'runtime_entity_id': {'actor_runtime_id': 1}}, 30, viewer=2))

    def test_console_lines_become_records_without_timestamps(self):
        lines = ['[2026-11-10 16:24:48:388 INFO] §aList of all ticking areas in current dimension',
                 '- area1: 0 0 0 to 15 0 15', '1/10 ticking areas in use.',
                 ' [2026-11-10 16:24:32:789 ERROR] Syntax error: Unexpected "x": at "y"',
                 '[2026-11-10 16:24:34:382 INFO] There are 0/2 players online:', '']
        self.assertEqual(commit_fixtures.console_records(['stray line'] + lines), [
            {'level': 'INFO', 'text': '§aList of all ticking areas in current dimension\n'
                                      '- area1: 0 0 0 to 15 0 15\n1/10 ticking areas in use.'},
            {'level': 'ERROR', 'text': 'Syntax error: Unexpected "x": at "y"'},
            {'level': 'INFO', 'text': 'There are 0/2 players online:\n'},
        ])

    def test_directives_are_steps_of_their_segment(self):
        with tempfile.TemporaryDirectory() as tmp:
            script = Path(tmp) / 'commands.txt'
            lines = ['# segment: two', '!join Alice', '/tp Alice 8 200 8', '!console kick Alice', '!leave Alice']
            script.write_text('\n'.join(lines) + '\n', encoding='utf-8')
            self.assertEqual([c for _, c in commit_fixtures.script_segments(script)], lines[1:])


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

    def test_cadence3_rests_vehicles_twice_and_shoots_from_dispensers(self):
        items = self.items('cadence3')
        vehicles = [item for item in items if item['mob'] in ('minecraft:minecart', 'minecraft:boat')]
        self.assertEqual(len(vehicles), 2)
        for item in vehicles:
            self.assertEqual([action['t'] for action in item['actions']], [40, 80])
        shots = [item for item in items if 'spawn_commands' in item]
        self.assertEqual(len(shots), 4)
        for item in shots:
            self.assertTrue(any(line.startswith('setblock 20 ') and 'dispenser' in line for line in item['setup']))
            self.assertTrue(item['spawn_commands'][0].endswith('redstone_block'))

    def test_range_walks_one_block_per_step_out_and_back(self):
        items = sweep.range_items(type('Args', (), {'items': 'all'})())
        self.assertEqual([item['key'] for item in items],
                         ['east-out', 'east-in', 'diagonal-out', 'diagonal-in', 'west-out', 'west-in', 'jump'])
        east = items[0]['path']
        self.assertEqual(len(east), 73)
        self.assertEqual(east[0], [48.5, 8.5])
        self.assertEqual(east[1][0] - east[0][0], 1)
        self.assertEqual(items[1]['path'], east[::-1])
        self.assertEqual(items[-1]['ticks'], 60)
        self.assertEqual(len(sweep.range_items(type('Args', (), {'items': 'pilot'})())), 1)

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


class SubchunkRoundTrip(unittest.TestCase):
    def test_writes_follow_setblock_layers(self):
        water = subchunk_roundtrip.WATER
        source = {}  # the section has no record: air
        plan = [
            {'dim': 0, 'pos': [1, 1, 1], 'state': water, 'measure': False},
            {'dim': 0, 'pos': [1, 1, 1], 'state': subchunk_roundtrip.SLAB, 'measure': False},
            {'dim': 0, 'pos': [2, 1, 1], 'state': water, 'measure': False},
            {'dim': 0, 'pos': [2, 1, 1], 'state': subchunk_roundtrip.GLASS, 'measure': False},
            {'dim': 0, 'pos': [3, 1, 1], 'state': subchunk_roundtrip.STONE, 'measure': True},
        ]
        sections, writes = subchunk_roundtrip.apply_plan(source, plan, include_measure=False)
        first, liquid = sections[(0, 0, 0, 0)]
        slab, glass = subchunk_roundtrip.index(1, 1, 1), subchunk_roundtrip.index(2, 1, 1)
        self.assertEqual((first[slab], liquid[slab]), (subchunk_roundtrip.SLAB, water))
        self.assertEqual((first[glass], liquid[glass]), (subchunk_roundtrip.GLASS, subchunk_fixture.AIR))
        self.assertEqual([w[0] for w in writes], [0, 1, 2, 3])  # the measure-only edit is BDS's alone
        plan.append({'dim': 0, 'pos': [1, 1, 1], 'state': subchunk_roundtrip.STONE, 'measure': False})
        sections, _ = subchunk_roundtrip.apply_plan(source, plan, include_measure=False)
        self.assertEqual(sections[(0, 0, 0, 0)][1][slab], subchunk_fixture.AIR)

    def test_sparse_records_rebuild_from_placements_in_palette_order(self):
        # Cells 0 and 300 hold entry 2, cell 5 entry 1, the rest air.
        indices = [0] * 4096
        indices[0], indices[300], indices[5] = 2, 2, 1
        words = b''.join(struct.pack('<I', sum(indices[w * 16 + i] << (2 * i) for i in range(16)))
                         for w in range(256))
        value = (bytes([9, 1, 0, 2 << 1]) + words + struct.pack('<i', 3) + palette_entry('minecraft:air')
                 + palette_entry('minecraft:stone') + palette_entry('minecraft:dirt'))
        self.assertEqual(subchunk_roundtrip.sparse_build(value), [(5, 1), (0, 2), (300, 2)])
        unused = (bytes([9, 1, 0, 2 << 1]) + words + struct.pack('<i', 4) + palette_entry('minecraft:air')
                  + palette_entry('minecraft:stone') + palette_entry('minecraft:dirt')
                  + palette_entry('minecraft:gold_block'))
        self.assertIsNone(subchunk_roundtrip.sparse_build(unused))

    def test_fill_edits_cover_their_box(self):
        edit = {'dim': 0, 'fill': [[0, 0, 0], [1, 2, 0]], 'state': subchunk_fixture.AIR, 'measure': False}
        self.assertEqual(len(subchunk_roundtrip.plan_cells(edit)), 6)
        self.assertEqual(subchunk_roundtrip.plan_column({'dim': 2, 'pos': [-1, 5, 17], 'state': None}), (2, -1, 1))


class VehicleWireOffset(unittest.TestCase):
    capture = KnockbackFixture.capture

    def test_a_minecart_is_matched_0_35_above_its_sampled_position(self):
        add_actor = (varint(4) + varint(2) + varint(len(b'minecraft:minecart')) + b'minecraft:minecart'
                     + struct.pack('<6f', 32.5, -54.0, 0.5, 0.0, 0.0, 0.0))
        delta = (varint(2) + b'\x00' + b'\x01' + struct.pack('<f', -54.04) + b'\x00' + b'\x00' * 3
                 + b'\x00' * 4 + varint(10))
        capture = list(replication_fixture.packets(self.capture([(1000, 13, add_actor), (1001, 111, delta)])))
        samples = [{'mob': {'pos': [32.5, -54.35, 0.5]}}, {'mob': {'pos': [32.5, -54.39, 0.5]}}]
        offset = replication_fixture.wire_offset(2, capture, samples)
        self.assertAlmostEqual(offset, 0.35, places=4)
        wire = replication_fixture.wire_for(2, capture, samples, offset)
        self.assertEqual([(event['sample'], event.get('timed', False)) for event in wire], [(0, False), (1, False)])
        self.assertEqual(wire[1]['ticks'], 10)


class RangeFixture(unittest.TestCase):
    def test_actor_packets_are_put_on_the_last_mark_before_them(self):
        def text(message: str) -> bytes:
            body = message.encode()
            return varint(9) + b'\x00' + varint(len(body)) + body
        remove = varint(14) + varint(9)  # unique id -5, zigzag
        add_actor = varint(13) + varint(7) + varint(3)
        records = [
            {'t_ms': 1, 'dir': 's2c', 'id': 14, 'hex': remove.hex()},
            {'t_ms': 2, 'dir': 's2c', 'id': 9, 'hex': text('bds-sweep:mark east-out 4 104').hex()},
            {'t_ms': 3, 'dir': 's2c', 'id': 13, 'hex': add_actor.hex()},
            {'t_ms': 50, 'dir': 's2c', 'id': 9, 'hex': text('bds-sweep:mark east-out 4 105').hex()},
            {'t_ms': 51, 'dir': 's2c', 'id': 14, 'hex': remove.hex()},
        ]
        path = Path(tempfile.mkdtemp()) / 'packets.jsonl'
        path.write_text('\n'.join(json.dumps(record) for record in records) + '\n', encoding='utf-8')
        events = list(range_fixture.actor_events(path))
        self.assertEqual(events, [
            (('east-out', 4, 104), 'add', ('unique', -4)),
            (('east-out', 4, 105), 'remove', ('unique', -5)),
        ])


class SpawnBonusFixture(unittest.TestCase):
    def test_nbt_compounds_lists_and_binary_strings_are_read(self):
        def named(kind: int, name: str, payload: bytes) -> bytes:
            return bytes([kind]) + struct.pack('<H', len(name)) + name.encode() + payload
        modifier = (named(5, 'Amount', struct.pack('<f', 1.25)) + named(3, 'Operation', struct.pack('<i', 2))
                    + b'\x00')
        attribute = (named(8, 'Name', struct.pack('<H', 22) + b'minecraft:follow_range')
                     + named(9, 'Modifiers', bytes([10]) + struct.pack('<i', 1) + modifier) + b'\x00')
        root = named(10, '', named(8, 'identifier', struct.pack('<H', 16) + b'minecraft:zombie')
                     + named(8, 'Key', struct.pack('<H', 2) + b'\xff\x00')
                     + named(9, 'Attributes', bytes([10]) + struct.pack('<i', 1) + attribute) + b'\x00')
        record = spawn_bonus_fixture.read_nbt(root)
        self.assertEqual(record['identifier'], 'minecraft:zombie')
        self.assertEqual(record['Key'], 'hex:ff00')
        self.assertEqual(record['Attributes'][0]['Modifiers'][0], {'Amount': {'Float': 1.25}, 'Operation': {'Int': 2}})


class PlayerRelayFixture(unittest.TestCase):
    def test_inputs_are_read_by_their_tick(self):
        body = (varint(144) + struct.pack('<2f', -2.0, 5.0) + struct.pack('<3f', 0.5, -58.38, 0.5)
                + struct.pack('<2f', 0.0, 1.0) + struct.pack('<f', 5.0) + varint(1) + varint(20) + varint(1)
                + varint(0) + varint(2) + struct.pack('<2f', 0.0, 0.0) + varint(800))
        path = Path(tempfile.mkdtemp()) / 'packets.jsonl'
        path.write_text(json.dumps({'t_ms': 0, 'dir': 'c2s', 'id': 144, 'hex': body.hex()}) + '\n', encoding='utf-8')
        self.assertEqual(player_relay_fixture.inputs(path), {800: {'pitch': -2.0, 'yaw': 5.0, 'head': 5.0}})


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
