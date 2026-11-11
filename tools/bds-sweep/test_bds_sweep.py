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
import player_fixture  # noqa: E402
import player_relay_fixture  # noqa: E402
import player_script  # noqa: E402
import pvp_fixture  # noqa: E402
import pvp_items  # noqa: E402
import range_fixture  # noqa: E402
import replication_fixture  # noqa: E402
import spawn_bonus_fixture  # noqa: E402
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


class PvpItems(unittest.TestCase):
    def test_sets_repeat_every_item_under_its_own_key(self):
        items = pvp_items.pvp_items('hurt', 2, False)
        keys = [item['key'] for item in items]
        self.assertEqual(len(keys), len(set(keys)))
        self.assertIn('burst|up|batch#1', keys)
        burst = next(item for item in items if item['key'] == 'burst|up|batch#0')
        self.assertEqual(burst['script']['5'], '[o0=slot:1,a0>1,o0=slot:2,a0>1,o0=slot:3,a0>1,o0=slot:4,a0>1,'
                                               'o0=slot:5,a0>1,i0] i1 i2')

    def test_strict_reach_items_are_named_apart_and_unknown_keys_refused(self):
        keys = {item['key'] for item in pvp_items.pvp_items('reach', 1, True)}
        self.assertIn('strict|reach|survival|6.7#0', keys)
        with self.assertRaises(SystemExit):
            pvp_items.pvp_items('nonsense', 1, False)
        self.assertEqual([i['key'] for i in pvp_items.pvp_items('weapon|hand', 1, False)], ['weapon|hand#0'])


class PvpFixture(unittest.TestCase):
    def test_server_packets_decode_to_values(self):
        motion = varint(2) + struct.pack('<3f', 0.4, 0.4, 0.0) + varint(273)
        self.assertEqual(pvp_fixture.decode_out(40, motion),
                         {'packet': 'SetActorMotion', 'about': 2, 'motion': [0.400000006, 0.400000006, 0.0],
                          'tick': 273})
        self.assertEqual(pvp_fixture.decode_out(27, bytes([2, 2, 4, 0])),
                         {'packet': 'ActorEvent', 'about': 2, 'event': 2, 'data': 2})
        animate = varint(4) + varint(2) + struct.pack('<f', 55.0)
        self.assertEqual(pvp_fixture.decode_out(44, animate),
                         {'packet': 'Animate', 'about': 2, 'action': 4, 'data': 55.0})
        sound = varint(13) + b'attack.strong' + struct.pack('<3f', 0.5, -59.1, 0.5)
        self.assertEqual(pvp_fixture.decode_out(123, sound)['sound'], 'attack.strong')

    def test_an_attack_joins_its_lines_input_or_the_next(self):
        result = {'spec': {'script': {'5': 'a0>1 i0 i1 i2', '6': 'i0 a0>1 i1 i2', '8': 'o0=slot:3 a0>1 i0'}},
                  'names': ['A', 'B', 'C'], 'first_tick': 0, 'events': [], 'fired': []}
        consumption = {'A': {5: 6, 7: 8, 8: 10}, 'B': {}, 'C': {}}
        attempts = pvp_fixture.attempts_of(result, consumption, [])
        self.assertEqual([(a['joins'], a['consumed'], a['slot']) for a in attempts],
                         [(5, 6, 0), (7, 8, 0), (8, 10, 3)])
        self.assertEqual([a['t'] for a in attempts], [5, 7, 9])

    def test_landed_hits_keep_bds_order_within_a_tick(self):
        result = {'spec': {'script': {'5': 'a0>1 i0 a2>1 i2 i1'}}, 'names': ['A', 'B', 'C'], 'first_tick': 0,
                  'events': [], 'fired': []}
        consumption = {'A': {5: 6}, 'B': {}, 'C': {5: 6}}
        hits = [{'t': 6, 'attacker': 'A', 'victim': 'B'}, {'t': 6, 'attacker': 'C', 'victim': 'B'}]
        attempts = pvp_fixture.attempts_of(result, consumption, hits)
        self.assertEqual([(a['attacker'], a['landed']) for a in attempts], [('A', 0), ('C', 1)])
        attempts = pvp_fixture.attempts_of(result, consumption, hits[1:])
        self.assertEqual([(a['attacker'], a['landed'], a['t']) for a in attempts], [('C', 0, 6), ('A', None, 6)])

    def test_trials_take_their_own_run_of_marked_inputs(self):
        rows = [(-89.99, [], 1, [0.0, 0.0, 0.0]), (-89.98, ['Sprinting'], 2, [0.0, 0.0, 0.0]),
                (-90.0, [], 3, [0.0, 0.0, 0.0]), (-89.99, [], 4, [0.0, 0.0, 0.0]),
                (-89.98, [], 5, [0.0, -0.0784, 0.0])]
        inputs = pvp_fixture.Inputs(rows)
        self.assertEqual({line: row[2] for line, row in inputs.take(-90.0, 40).items()}, {0: 1, 1: 2})
        self.assertEqual({line: row[2] for line, row in inputs.take(-90.0, 40).items()}, {0: 4, 1: 5})
        self.assertEqual(inputs.by_tick[2][1], ['Sprinting'])


if __name__ == '__main__':
    unittest.main()
