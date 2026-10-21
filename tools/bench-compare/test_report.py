import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class ReportIsolation(unittest.TestCase):
    def test_previous_extra_round_does_not_change_current_median(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            metadata = dict(utc='test', rounds=3, count=1, os='test', architecture='x64',
                            cpu='test', processors=1, rust='rustc', go='go', go_cpu=1,
                            commit='test', corpus_commit='test', conditions='test')
            (root / 'metadata.json').write_text(json.dumps(metadata))
            for number, ns in enumerate((40, 50, 60, 9000), 1):
                for side, value in [('go', 100), ('rust', ns)]:
                    row = dict(area='codec', name='Text', operation='decode',
                               implementation='go' if side == 'go' else 'rust-owned', ns_per_op=value)
                    (root / f'{number}-{side}-codec.jsonl').write_text(json.dumps(row))
            (root / 'interop.json').write_text('[]')
            result = subprocess.run([sys.executable, str(Path(__file__).with_name('report.py')),
                                     str(root), str(root / 'report.md')], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            summary = json.loads((root / 'summary.json').read_text())
            ours = next(row for row in summary['rows'] if row['implementation'] == 'rust-owned')
            self.assertEqual(ours['ns_per_op'], 50)
            self.assertEqual(ours['rounds'], 3)
            self.assertIn('| 0.50 | ours |', (root / 'report.md').read_text())


if __name__ == '__main__':
    unittest.main()
