"""Deterministic native-control preparation regressions; no server or SDK load."""

import importlib.util
from pathlib import Path
import unittest

_PATH = (Path(__file__).resolve().parents[1] / 'src' /
         'endstone_entity_motion_probe' / 'terrain.py')
_SPEC = importlib.util.spec_from_file_location('motion_terrain', _PATH)
_TERRAIN = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_TERRAIN)


class TerrainPostconditionTests(unittest.TestCase):
    def test_no_change_command_still_verifies_the_complete_air_volume(self):
        minimum, maximum = (-1, -60, 2), (0, -59, 3)
        blocks = {(x, y, z): 'minecraft:air'
                  for x in range(-1, 1) for y in range(-60, -58) for z in range(2, 4)}
        verified = _TERRAIN.fill_verified(
            minimum, maximum, 'minecraft:air', lambda _: False,
            lambda x, y, z: blocks[x, y, z])
        self.assertEqual(verified, 8)

    def test_no_change_result_does_not_hide_a_wrong_maximum_corner(self):
        minimum, maximum = (-1, -60, 2), (0, -59, 3)
        blocks = {(x, y, z): 'minecraft:air'
                  for x in range(-1, 1) for y in range(-60, -58) for z in range(2, 4)}
        blocks[maximum] = 'minecraft:stone'
        with self.assertRaises(RuntimeError):
            _TERRAIN.fill_verified(
                minimum, maximum, 'minecraft:air', lambda _: False,
                lambda x, y, z: blocks[x, y, z])

    def test_successful_dispatch_does_not_prove_the_requested_native_state(self):
        with self.assertRaises(RuntimeError):
            _TERRAIN.fill_verified(
                (0, -60, 0), (0, -60, 0), 'minecraft:air', lambda _: True,
                lambda *_: 'minecraft:water')


if __name__ == '__main__':
    unittest.main()
