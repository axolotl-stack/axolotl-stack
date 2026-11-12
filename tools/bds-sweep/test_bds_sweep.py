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
import effects_fixture  # noqa: E402
import knockback_fixture  # noqa: E402
import replication_fixture  # noqa: E402
import native  # noqa: E402
import player_relay_fixture  # noqa: E402
import range_fixture  # noqa: E402
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


class EffectTrials(unittest.TestCase):
    def args(self, items: str = 'slice', difficulty: str = 'easy'):
        return type('Args', (), {'items': items, 'difficulty': difficulty})()

    def test_mob_and_player_items_time_their_effects_after_the_spawn(self):
        mobs = sweep.effect_items(self.args('mobs'))
        players = sweep.effect_items(self.args('players'))
        everything = sweep.effect_items(self.args())
        self.assertEqual(len(everything), len(mobs) + len(players))
        keys = [item['key'] for item in everything]
        self.assertEqual(len(keys), len(set(keys)))
        for item in mobs:
            self.assertTrue(item['mob'].startswith('minecraft:'), item['key'])
            self.assertTrue(all(action['t'] < item['ticks'] for action in item['actions']), item['key'])
            for action in item['actions']:
                for command in action['commands']:
                    self.assertNotIn('@T', command)
                    if command.startswith('effect') or command.startswith('damage'):
                        self.assertIn(item['mob'], command)
        for item in players:
            self.assertIsNone(item['mob'])
            self.assertIn('gamemode survival', item['setup'][0])
            self.assertIn('gamemode creative', item['cleanup'][-1])

    def test_unknown_effect_set_is_refused(self):
        with self.assertRaises(SystemExit):
            sweep.effect_items(self.args('every'))

    def test_ranged_items_protect_the_survival_player(self):
        items = sweep.ranged_items(self.args())
        self.assertEqual([item['key'] for item in items],
                         ['skeleton|shoot-8', 'skeleton|shoot-15', 'stray|shoot-8', 'bogged|shoot-8',
                          'parched|shoot-8'])
        for item in items:
            self.assertTrue(item['frozen'])
            self.assertTrue(any('resistance 100000 4' in line for line in item['setup']))

    def test_ranged_item_sets_pin_geometry_spread_damage_and_avoidance(self):
        geometry = sweep.ranged_items(self.args('geometry'))
        self.assertEqual(len({tuple(item['at'][:3]) for item in geometry}), len(geometry))
        for item in geometry:
            self.assertEqual(item['mob'], 'minecraft:' + item['key'].split('|')[0])
            self.assertTrue(any('resistance 100000 4' in line for line in item['setup']))
        spread = sweep.ranged_items(self.args('spread'))
        for item in spread:
            # Every barrier the setup places, the cleanup clears.
            placed = [line for line in item['setup'] if line.endswith(' barrier')]
            self.assertEqual(len(placed), 3)
            for line in placed:
                self.assertIn(line.replace(' barrier', ' air'), item['cleanup'])
        damage = sweep.ranged_items(self.args('damage'))
        for item in damage:
            self.assertFalse(any('resistance' in line for line in item['setup']))
            self.assertTrue(any('health_boost' in line for line in item['setup']))
        avoid = sweep.ranged_items(self.args('avoid'))
        for item in avoid:
            self.assertFalse(item['frozen'])
            self.assertTrue(any('summon minecraft:wolf' in line for line in item['setup']))
        with self.assertRaises(SystemExit):
            sweep.ranged_items(self.args('every'))

    def test_ranged_fixture_structures_setup_and_scripted_commands(self):
        import ranged_fixture
        self.assertEqual(ranged_fixture.block_change('fill 0 -60 -1 1 -58 -1 barrier'),
                         {'from': [0, -60, -1], 'to': [1, -58, -1], 'block': 'minecraft:barrier'})
        self.assertEqual(ranged_fixture.block_change('setblock 1 -56 0 dispenser ["facing_direction"=0]'),
                         {'from': [1, -56, 0], 'to': [1, -56, 0], 'block': 'minecraft:dispenser',
                          'states': {'facing_direction': 0}})
        self.assertIsNone(ranged_fixture.block_change('gamemode survival "BDSSweep"'))
        actions = [{'t': 5, 'commands': ['replaceitem entity @e[type=minecraft:skeleton] slot.weapon.mainhand 0 '
                                         'minecraft:air', 'event entity @e[type=minecraft:skeleton] '
                                                          'minecraft:spring_trap']},
                   {'t': 65, 'commands': ['tp "BDSSweep" 0.5 -60 0.5 -90 0']}]
        self.assertEqual(ranged_fixture.scripted(actions),
                         {'hand': [{'t': 5, 'item': None}], 'teleports': [{'t': 65, 'to': [0.5, -60.0, 0.5]}],
                          'fired_events': [{'t': 5, 'event': 'minecraft:spring_trap'}]})

    def test_silverfish_items_restore_every_block_they_place(self):
        merge = sweep.silverfish_items(self.args('merge'))
        self.assertEqual(len(merge), 10)
        wake = sweep.silverfish_items(self.args('wake'))
        for item in wake:
            placed = [line for line in item['setup'] if line.endswith(' infested_stone')]
            self.assertTrue(placed)
            for line in placed:
                self.assertIn(line.replace(' infested_stone', ' air'), item['cleanup'])
            self.assertEqual(item['actions'][0]['t'], 40)
        with self.assertRaises(SystemExit):
            sweep.silverfish_items(self.args('every'))

    def test_mooshroom_items_interact_twice_with_the_held_item(self):
        items = sweep.mooshroom_items(self.args())
        self.assertEqual([item['hold']['item'] for item in items],
                         ['minecraft:bowl', 'minecraft:allium', 'minecraft:shears'])
        for item in items:
            orders = [action['t'] for action in item['actions'] if action.get('order') == 'interact']
            self.assertEqual(orders, [10, 30])

    def test_melee_effect_items_keep_the_player_alive_without_resistance(self):
        items = sweep.melee_effect_items(self.args())
        self.assertEqual([item['key'] for item in items], ['husk|hits'])
        husk = items[0]
        self.assertFalse(husk['frozen'])
        self.assertTrue(any('health_boost' in line for line in husk['setup']))
        self.assertFalse(any('resistance' in line for line in husk['setup']))
        self.assertIn('effect "BDSSweep" clear', husk['cleanup'])

    def test_mooshroom_geometry_items_move_the_player_and_restore_the_block(self):
        items = sweep.mooshroom_items(self.args('geometry'))
        self.assertEqual([item['key'] for item in items],
                         ['mooshroom|allium-raised', 'mooshroom|allium-baby', 'mooshroom|suspicious-stew'])
        raised = items[0]
        self.assertIn('setblock 0 -60 3 stone', raised['setup'])
        self.assertIn('setblock 0 -60 3 air', raised['cleanup'])
        self.assertEqual(raised['player'][:3], [0.5, -59, 3.5])
        stew = items[2]
        orders = sorted(action['t'] for action in stew['actions'] if action.get('order') == 'interact')
        self.assertEqual(orders, [10, 30, 50, 60])
        with self.assertRaises(SystemExit):
            sweep.mooshroom_items(self.args('every'))

    def test_resist_set_hits_the_resistant_mobs_like_the_slice(self):
        items = sweep.knockback_items(type('Args', (), {'items': 'resist'})())
        self.assertEqual([item['mob'] for item in items],
                         ['minecraft:zoglin', 'minecraft:hoglin', 'minecraft:iron_golem'])
        self.assertTrue(all(item['frozen'] and len(item['hits']) == 3 for item in items))


class EffectFixture(unittest.TestCase):
    def test_packets_are_put_on_their_mark_and_attributed_to_mob_or_player(self):
        def text(message):
            return {'name': 'TextPacket', 'packet': {'body': {'MessageOnly': {'message': message}}}}

        def data(rid, rows):
            return {'name': 'SetActorDataPacket', 'packet': {
                'target_runtime_id': {'actor_runtime_id': rid},
                'actor_data': {'data': [{'id': key, 'payload': {'DataItemInt64Payload': {'value': value}}}
                                        for key, value in rows]}}}

        records = [
            text('bds-sweep:mark cow|poison 9'),
            data(2, [(131, 99)]),
            text('bds-sweep:mark cow|poison 10'),
            data(2, [(131, 38), (7, 300)]),
            data(1, [(131, 2)]),
            data(5, [(131, 4)]),
            {'name': 'ActorEventPacket', 'packet': {'target_runtime_id': {'actor_runtime_id': 2},
                                                    'event_id': 'Hurt', 'data': 14}},
            text('bds-sweep:mark other 0'),
            data(2, [(131, 0)]),
        ]
        result = {'key': 'cow|poison', 'mob': 'minecraft:cow', 'runtime_id': 2, 'player_runtime_id': 1,
                  'first_tick': 100, 'spec': {}, 'failed_commands': [],
                  'damage': [[130, 'minecraft:cow', 1.0, 'magic']],
                  'samples': [{'player': {'health': 20}, 'mob': {'health': 10, 'pos': [1, 2, 3], 'ground': True}}]}
        item = effects_fixture.distil(result, records)
        self.assertEqual(item['packets'], [
            {'t': 9, 'about': 'mob', 'kind': 'data', 'data': {'131': 99}},
            {'t': 10, 'about': 'mob', 'kind': 'data', 'data': {'131': 38}},
            {'t': 10, 'about': 'player', 'kind': 'data', 'data': {'131': 2}},
            {'t': 10, 'about': 'mob', 'kind': 'event', 'event': 'Hurt', 'data': 14},
        ])
        self.assertEqual(item['damage'], [{'t': 30, 'about': 'mob', 'damage': 1.0, 'cause': 'magic'}])
        self.assertNotIn('mob_positions', item)


if __name__ == '__main__':
    unittest.main()
