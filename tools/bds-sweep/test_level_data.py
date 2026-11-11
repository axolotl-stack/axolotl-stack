"""Tests for the BDS-free parts of level_data.py and le_nbt.py."""

from __future__ import annotations

import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import le_nbt  # noqa: E402
import level_data  # noqa: E402


def compound(*pairs):
    return (le_nbt.COMPOUND, list(pairs))


class NbtTests(unittest.TestCase):
    def sample(self):
        return compound(
            ('Criteria', (le_nbt.LIST, (le_nbt.END, []))),
            ('LastUniqueID', (le_nbt.LONG, 6)),
            ('Objectives', (le_nbt.LIST, (le_nbt.COMPOUND, [[('Name', (le_nbt.STRING, 'obj1'))]]))),
            ('block_indices', (le_nbt.INT_ARRAY, [0, 1, -1])),
            ('raw', (le_nbt.BYTE_ARRAY, [1, -2])),
            ('rainLevel', (le_nbt.FLOAT, 0.5)),
        )

    def test_values_round_trip_with_order_and_types(self):
        data = le_nbt.write('', self.sample())
        name, tag, end = le_nbt.read(data)
        self.assertEqual((name, tag, end), ('', self.sample(), len(data)))
        self.assertEqual(le_nbt.write(name, tag), data)

    def test_truncated_values_are_errors(self):
        data = le_nbt.write('', self.sample())
        for cut in (1, 5, len(data) - 1):
            with self.assertRaises(ValueError):
                le_nbt.read(data[:cut])

    def test_flat_lists_every_leaf(self):
        lines = le_nbt.flat(self.sample())
        self.assertIn('Criteria: list<end> = []', lines)
        self.assertIn('Objectives[0].Name: string = ' + repr('obj1'), lines)
        self.assertIn('rainLevel: float = 0.5', lines)

    def test_get_and_plain(self):
        root = self.sample()
        self.assertEqual(le_nbt.get(root, 'LastUniqueID'), (le_nbt.LONG, 6))
        self.assertIsNone(le_nbt.get(root, 'missing'))
        self.assertEqual(le_nbt.plain(root)['Objectives'], [{'Name': 'obj1'}])


class LevelDataTests(unittest.TestCase):
    def test_level_dat_header_must_cover_the_compound(self):
        body = le_nbt.write('', compound(('Time', (le_nbt.LONG, 4321))))
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'level.dat'
            path.write_bytes(struct.pack('<ii', 10, len(body)) + body)
            version, root = level_data.read_level_dat(path)
            self.assertEqual((version, le_nbt.get(root, 'Time')), (10, (le_nbt.LONG, 4321)))
            path.write_bytes(struct.pack('<ii', 10, len(body) + 1) + body)
            with self.assertRaises(ValueError):
                level_data.read_level_dat(path)

    def test_chunk_records_are_told_from_level_records(self):
        chunk = struct.pack('<ii', -1, 2)
        self.assertTrue(level_data.is_chunk_key(chunk + b','))
        self.assertTrue(level_data.is_chunk_key(chunk + b'/\x00'))
        self.assertTrue(level_data.is_chunk_key(chunk + struct.pack('<i', 1) + b'/\xfc'))
        for key in (b'scoreboard', b'mobevents', b'BiomeData', b'Overworld', b'WorldClocks'):
            self.assertFalse(level_data.is_chunk_key(key), key)
        db = {chunk + b',': b'x', b'digp' + chunk: b'', b'scoreboard': b'y',
              b'tickingarea_a': b'z', b'player_server_b': b'p', b'schedulerWT': b'w'}
        self.assertEqual(sorted(level_data.level_records(db)),
                         [b'player_server_b', b'schedulerWT', b'scoreboard', b'tickingarea_a'])
        self.assertEqual(sorted(level_data.kept_records(db)),
                         ['player_server_b', 'scoreboard', 'tickingarea_a'])

    def test_console_records_join_continuation_lines_and_drop_the_start_notices(self):
        lines = [
            '[2026-11-10 20:30:00:001 ERROR] ================ TRANSPORT TYPE ERROR  ===================',
            '[2026-11-10 20:30:00:002 ERROR] Your current connection type is not set to NetherNet.',
            '[2026-11-10 20:30:00:003 ERROR] =======================================================',
            '[2026-11-10 20:30:00:004 INFO] §aList of all ticking areas in all dimensions',
            'Overworld: ',
            '- lvl: 0 0 0 to 47 0 47 Preload',
        ]
        self.assertEqual(level_data.group(lines), [{
            'level': 'INFO',
            'text': '§aList of all ticking areas in all dimensions\nOverworld: \n- lvl: 0 0 0 to 47 0 47 Preload',
        }])


if __name__ == '__main__':
    unittest.main()
