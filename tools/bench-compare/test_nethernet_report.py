import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('nethernet_report', Path(__file__).with_name('nethernet_report.py'))
report = importlib.util.module_from_spec(spec)
if spec.loader:
    spec.loader.exec_module(report)


class NetherNetReport(unittest.TestCase):
    def test_round_isolation_missing_apis_and_setup_statistics(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)
            metadata = dict(rounds=5, count=100, split_count=10, utc='test',
                            conditions='CPU 0; CPU 16-23', versions={'go': 'pinned'})
            (source / 'metadata.json').write_text(json.dumps(metadata))
            for number in range(1, 7):
                rows = [dict(area='nethernet_loopback', name='32', operation='setup',
                             implementation='rust-owned', ns_per_op=number * 1000,
                             p50_us=number, p99_us=number, max_us=number, payload_bytes=0),
                        dict(area='nethernet_signaling', name='connect_request', operation='parse',
                             implementation='go', ns_per_op=10 * number, payload_bytes=32)]
                (source / f'{number}-rust-nethernet.jsonl').write_text('\n'.join(map(json.dumps, rows)))
            (source / 'interop.json').write_text(json.dumps([
                dict(direction='rust-to-go', status='FAIL', error='identity mismatch')]))
            (source / '1-go-nethernet.stderr.log').write_text(json.dumps(dict(
                area='coverage', status='unavailable', name='framing', implementation='go', reason='private API')))
            text, summary = report.render(source)
            self.assertEqual(len(summary['rows']), 2)
            self.assertTrue(all(row['rounds'] == 5 for row in summary['rows']))
            setup = next(row for row in summary['rows'] if row['operation'] == 'setup')
            self.assertEqual(setup['ns_per_op'], 3000)
            self.assertIn('identity mismatch', text)
            self.assertIn('private API', text)
            self.assertIn('NetherNet framing micro', text)
            self.assertIn('NetherNet discovery micro', text)
            self.assertIn('3.00', text)

    def test_missing_round_fails_instead_of_silently_comparing_unequal_samples(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)
            (source / 'metadata.json').write_text(json.dumps(dict(rounds=5)))
            (source / '1-go-nethernet.jsonl').write_text(json.dumps(dict(
                area='nethernet_loopback', name='32', operation='echo', implementation='go', ns_per_op=100)))
            with self.assertRaisesRegex(ValueError, 'round'):
                report.render(source)

    def test_append_preserves_existing_report_without_duplicate_sections(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / 'report.md'
            destination.write_text('# Existing RakNet report\n\nMeasured data.\n')
            report.update(destination, 'First measurement\n')
            report.update(destination, 'Second measurement\n')
            text = destination.read_text()
            self.assertIn('Measured data.', text)
            self.assertNotIn('First measurement', text)
            self.assertEqual(text.count(report.MARKER), 1)
            self.assertIn('Second measurement', text)

    def test_small_positive_throughput_is_not_reported_as_zero(self):
        actual = float(report.fmt(0.0000526))
        self.assertGreater(actual, 0)
        self.assertAlmostEqual(actual, 0.0000526, delta=0.0000001)


if __name__ == '__main__':
    unittest.main()
