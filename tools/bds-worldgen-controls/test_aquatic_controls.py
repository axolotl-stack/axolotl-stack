"""BDS-free safety regressions for the aquatic capture tool.

    python -m unittest discover -s tools/bds-worldgen-controls -p "test_*.py"
"""
import subprocess
import sys
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import aquatic_controls  # noqa: E402


class NativeCaptureSafety(unittest.TestCase):
    def test_active_go_build_blocks_capture_even_without_a_bds_process(self):
        # tasklist quotes the memory column because it can contain commas.
        processes = (
            '"gopls.exe","40","Console","1","15,000 K"\r\n'
            '"Go.EXE","41","Console","1","26,000 K"\r\n'
            '"python.exe","42","Console","1","12,000 K"\r\n'
        )
        snapshot = subprocess.CompletedProcess([], 0, stdout=processes)
        with mock.patch.object(aquatic_controls.subprocess, "run", return_value=snapshot):
            with self.assertRaisesRegex(RuntimeError, r"Go[.]EXE"):
                aquatic_controls.resource_guard()


if __name__ == "__main__":
    unittest.main()
