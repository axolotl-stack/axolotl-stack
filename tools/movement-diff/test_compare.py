"""Tests of the movement-diff comparator: python -m unittest discover -s tools/movement-diff -p "test_*.py"."""

import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import compare  # noqa: E402


def exact(*values: float) -> dict:
    return {'value': list(values), 'bits': [f'{struct.unpack("<I", struct.pack("<f", v))[0]:08x}' for v in values]}


def state(tick: int, z: float, velocity: float | None = None, on_ground: bool = True) -> dict:
    out = {'tick': tick, 'position': exact(0.5, 101.62001, z), 'on_ground': on_ground}
    if velocity is not None:
        out['velocity'] = exact(0.0, -0.0784, velocity)
    return out


class CompareTest(unittest.TestCase):
    def test_ulps_count_across_zero_and_between_neighbours(self):
        self.assertEqual(compare.ulps('3f800000', '3f800001'), 1)
        self.assertEqual(compare.ulps('00000000', '80000000'), 0)
        self.assertEqual(compare.ulps('00000001', '80000001'), 2)

    def test_the_first_difference_is_found_and_velocity_only_where_both_have_it(self):
        left = [state(1, 0.5, 0.0), state(2, 0.598), state(3, 0.749508)]
        right = [state(1, 0.5), state(2, 0.598, 0.05350801), state(3, 0.74950796)]
        diffs, compared = compare.differences(left, right)
        self.assertEqual(compared, 3)
        self.assertEqual([(d.tick, d.field) for d in diffs], [(3, 'position.z')])
        self.assertEqual(diffs[0].ulps, 1)

    def test_ground_contact_differences_count(self):
        diffs, _ = compare.differences([state(1, 0.5, on_ground=True)], [state(1, 0.5, on_ground=False)])
        self.assertEqual([d.field for d in diffs], ['on_ground'])

    def test_f32_text_is_the_shortest_round_trip(self):
        self.assertEqual(compare.f32_text(struct.unpack('<f', struct.pack('<f', 0.1))[0]), '0.1')


if __name__ == '__main__':
    unittest.main()
