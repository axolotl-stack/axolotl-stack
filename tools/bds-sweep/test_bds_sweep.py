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
import scenarios  # noqa: E402
import sweep  # noqa: E402

sys.path.insert(0, str(Path(__file__).resolve().parent / 'endstone-sweep' / 'src'))
from endstone_bds_sweep import jobs  # noqa: E402


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


class RedstoneSteps(unittest.TestCase):
    @staticmethod
    def state(name, **states):
        return {'name': name, 'states': states, 'runtime_id': hash((name, tuple(states.items()))) & 0xFFFF}

    def test_changes_count_from_the_trigger_and_group_per_tick(self):
        lever_off, lever_on = self.state('minecraft:lever', open_bit=False), self.state('minecraft:lever', open_bit=True)
        dust = self.state('minecraft:redstone_wire', redstone_signal=15)
        step = {'step': 'redstone_dust/on', 'action': 'use_watch', 'block': [0, -59, 0], 'face': 'up',
                'look': {'position': [0.5, -60, -2.5], 'server': [0.0, 10.0]}, 'interactions': [[1, 'r', [0, 0, 0]]],
                'cells': {'lever': [0, -59, 0], 'dust': [1, -60, 0]}, 'trigger': 'lever', 'origin_tick': 100,
                'ticks': 40, 'before': {'lever': lever_off, 'dust': self.state('minecraft:redstone_wire',
                                                                            redstone_signal=0)},
                'changes': [[2, 'lever', lever_on], [4, 'dust', dust]], 'after': {'lever': lever_on, 'dust': dust}}
        out = summarize.redstone_step(step, {'redstone_dust/on': {'sounds': [{'sound': 'click', 'data': 1}]}})
        self.assertEqual(out['trigger_seen_ticks_after_order'], 2)
        self.assertEqual(out['tick_0_server_tick'], 102)
        self.assertEqual(out['changes'], [{'tick': 0, 'cells': {'lever': 'minecraft:lever[open_bit=true]'}},
                                          {'tick': 2, 'cells': {'dust': 'minecraft:redstone_wire[redstone_signal=15]'}}])
        self.assertEqual(out['sounds'], [{'sound': 'click', 'data': 1}])

    def test_fire_samples_list_cells_per_block_name(self):
        fire, air = self.state('minecraft:fire', age=0), self.state('minecraft:air')
        step = {'step': 'fire/set', 'action': 'command_watch', 'commands': ['setblock 0 0 0 fire'],
                'succeeded': [True], 'cells': {'a': [0, 0, 0], 'b': [1, 0, 0]}, 'trigger': None, 'origin_tick': 5,
                'ticks': 40, 'sample_every': 20, 'before': {'a': air, 'b': air},
                'changes': [[0, 'a', fire], [25, 'a', air], [30, 'b', fire]], 'after': {'a': air, 'b': fire}}
        out = summarize.redstone_step(step, {})
        self.assertEqual(out['samples'], [{'tick': 0, 'fire': ['a'], 'air': 1}, {'tick': 20, 'fire': ['a'], 'air': 1},
                                          {'tick': 40, 'air': 1, 'fire': ['b']}])

    def test_redstone_alias_lists_every_scenario_with_palette_states(self):
        import argparse
        from unittest import mock
        args = argparse.Namespace(kind='blocks', scenarios='doors,redstone', growth_runs='1:1')
        palette = [{'name': 'minecraft:lever', 'states': {'open_bit': 1}}, {'name': 'minecraft:wheat', 'states': {}}]
        with mock.patch.object(sweep.native, 'block_states', return_value=palette):
            job = sweep.build_job(args, Path('out'))
        self.assertEqual([item['key'] for item in job['items']], ['doors', *sweep.REDSTONE_SCENARIOS])
        self.assertEqual(job['palette_states'], palette[:1])


class UtilitySteps(unittest.TestCase):
    state = staticmethod(RedstoneSteps.state)

    def test_utility_alias_lists_every_scenario_and_traces_actor_packets(self):
        import argparse
        from unittest import mock
        args = argparse.Namespace(kind='blocks', scenarios='utility', growth_runs='1:1', port=19140,
                                  client=Path('client.exe'))
        palette = [{'name': 'minecraft:composter', 'states': {}}, {'name': 'minecraft:wheat', 'states': {}}]
        with mock.patch.object(sweep.native, 'block_states', return_value=palette):
            job = sweep.build_job(args, Path('out'))
        self.assertEqual([item['key'] for item in job['items']], list(sweep.UTILITY_SCENARIOS))
        self.assertEqual(job['palette_states'], palette[:1])
        trace = sweep.client_command(args, Path('out'))[-1].split(',')
        self.assertIn('AddActorPacket', trace)
        self.assertIn('BlockActorDataPacket', trace)
        args.scenarios = 'redstone'
        self.assertNotIn('AddActorPacket', sweep.client_command(args, Path('out'))[-1])

    def test_region_cells_are_listed_only_when_changed(self):
        stone, air = self.state('minecraft:stone'), self.state('minecraft:air')
        tnt = self.state('minecraft:tnt', explode_bit=False)
        step = {'step': 'tnt/lit', 'action': 'act', 'cells': {'tnt': [0, 0, 0], '1,0,0': [1, 0, 0], '2,0,0': [2, 0, 0]},
                'origin_tick': 7, 'ticks': 90, 'before': {'tnt': tnt, '1,0,0': stone, '2,0,0': stone},
                'changes': [[2, 'tnt', air], [82, '1,0,0', air]], 'after': {'tnt': air, '1,0,0': air, '2,0,0': stone},
                'actors': [{'tick': 2, 'event': 'appeared', 'id': '9', 'type': 'minecraft:tnt',
                            'position': [0.5, 0, 0.5], 'velocity': [0, 0.2, 0]}],
                'actors_after': {}, 'held_after': ['minecraft:flint_and_steel', 1, 1], 'interactions': [],
                'commands': ['summon tnt'], 'succeeded': [True]}
        out = summarize.utility_step(step, {'tnt/lit': {'add_actor': [{'actor': 'minecraft:tnt'}]}})
        self.assertEqual(out['positions'], {'tnt': [0, 0, 0]})
        self.assertEqual(out['before'], {'tnt': 'minecraft:tnt[explode_bit=false]', '1,0,0': 'minecraft:stone'})
        self.assertEqual(out['changes'], [{'tick': 2, 'cells': {'tnt': 'minecraft:air'}},
                                          {'tick': 82, 'cells': {'1,0,0': 'minecraft:air'}}])
        self.assertEqual(out['region_cells_watched'], 2)
        self.assertEqual(out['actors'], [{'tick': 2, 'event': 'appeared', 'type': 'minecraft:tnt',
                                          'position': [0.5, 0, 0.5], 'velocity': [0, 0.2, 0]}])
        self.assertEqual(out['add_actor'], [{'actor': 'minecraft:tnt'}])


class ScenarioCaptures(unittest.TestCase):
    @staticmethod
    def record(seq: int, name: str, payload: bytes, direction: str = 's2c') -> str:
        return json.dumps({'seq': seq, 't_ms': seq * 10, 'dir': direction, 'id': 9, 'name': name,
                           'hex': payload.hex()}) + '\n'

    def capture(self, root: Path) -> Path:
        text = lambda body: b'\t\x00\x00\x00' + bytes([len(body)]) + body + b'\x00'  # noqa: E731
        lines = [
            self.record(0, 'PlayerAuthInputPacket', b'\x90\x01', 'c2s'),
            self.record(1, 'TextPacket', text(b'bds-sweep:scenario begin doors')),
            self.record(2, 'UpdateBlockPacket', b'\x15\x00'),
            self.record(3, 'TextPacket', text(b'bds-sweep:mark doors/place')),
            self.record(4, 'TextPacket', text(b'bds-sweep:scenario end doors')),
            self.record(5, 'UpdateBlockPacket', b'\x15\x01'),
            self.record(6, 'TextPacket', text(b'bds-sweep:scenario begin torches')),
            self.record(7, 'UpdateBlockPacket', b'\x15\x02'),
        ]
        path = root / 'capture' / 'packets.jsonl'
        path.parent.mkdir(parents=True)
        path.write_text(''.join(lines), encoding='utf-8')
        return path

    def test_split_keeps_marker_to_marker_records_and_drops_unfinished_scenarios(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            written = scenarios.split(self.capture(root), root / 'scenarios')
            self.assertEqual(written, {'doors': 4})
            seqs = [json.loads(line)['seq'] for line in
                    (root / 'scenarios' / 'doors' / 'packets.jsonl').read_text(encoding='utf-8').splitlines()]
            self.assertEqual(seqs, [1, 2, 3, 4])
            self.assertFalse((root / 'scenarios' / 'torches').exists())

    def test_commit_gzips_deterministically_and_lists_scenarios(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            sweep_dir = root / 'sweep'
            scenarios.split(self.capture(sweep_dir), sweep_dir / 'scenarios')
            steps = {'scenario': 'doors', 'covers': 'Oak doors.', 'recorded_utc': '2026-10-30T00:00:00Z',
                     'steps': [{'tick': 0, 'op': 'use'}, {'tick': 1, 'op': 'wait', 'ticks': 5}],
                     'error': 'RuntimeError: boom'}
            (sweep_dir / 'scenarios' / 'doors' / 'steps.json').write_text(json.dumps(steps), encoding='utf-8')
            fixtures = root / 'fixtures'
            self.assertEqual(scenarios.commit([sweep_dir], fixtures), ['doors'])
            first = (fixtures / 'doors' / 'packets.jsonl.gz').read_bytes()
            scenarios.commit([sweep_dir], fixtures)
            self.assertEqual((fixtures / 'doors' / 'packets.jsonl.gz').read_bytes(), first)
            readme = (fixtures / 'README.md').read_text(encoding='utf-8')
            self.assertIn('| `doors` | Oak doors. | 2026-10-30T00:00:00Z | 1 | 4 |', readme)
            self.assertIn('failed: RuntimeError: boom', readme)

    def test_scenario_groups_expand_in_order(self):
        self.assertEqual(sweep.scenario_names('blocks')[:2], ['doors', 'trapdoors'])
        self.assertEqual(sweep.scenario_names('all'), list(sweep.BLOCK_SCENARIOS + sweep.REDSTONE_SCENARIOS
                                                            + sweep.UTILITY_SCENARIOS))
        self.assertEqual(set(sweep.scenario_names('all')), set(jobs.SCENARIOS))

    def test_every_scenario_says_what_it_covers(self):
        for key in jobs.SCENARIOS:
            self.assertTrue(jobs.scenario_summary(key), key)

    def test_step_log_merges_consecutive_waits_and_keeps_the_result(self):
        class Plugin:
            ticks = 100

        plugin = Plugin()
        log = jobs.StepLog(plugin)

        def scenario():
            yield 2
            yield 3
            log.add('command', command='setblock 0 0 0 stone')
            yield None
            yield 4
            return 'done'

        runner = jobs.logged(scenario(), log)
        try:
            while True:
                wait = next(runner)
                plugin.ticks += wait or 0
        except StopIteration as stop:
            self.assertEqual(stop.value, 'done')
        self.assertEqual(log.entries, [{'tick': 0, 'op': 'wait', 'ticks': 5},
                                       {'tick': 5, 'op': 'command', 'command': 'setblock 0 0 0 stone'},
                                       {'tick': 5, 'op': 'wait', 'ticks': 4}])


if __name__ == '__main__':
    unittest.main()
