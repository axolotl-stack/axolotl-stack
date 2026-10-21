import json
import tempfile
import unittest
from pathlib import Path

from endstone_client_diff.capture import PacketLog, packet_bytes


class CaptureTests(unittest.TestCase):
    def test_header_is_varuint_and_payload_is_unmodified(self):
        self.assertEqual(packet_bytes(193, b'\x00\xff'), b'\xc1\x01\x00\xff')

    def test_sequence_spans_both_directions(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'capture.jsonl'
            log = PacketLog(path)
            log.record('c2s', 1, b'\xab')
            log.record('s2c', 2, b'\xcd')
            log.close()
            rows = [json.loads(line) for line in path.read_text().splitlines()]
            self.assertEqual([r['seq'] for r in rows], [0, 1])
            self.assertEqual([r['dir'] for r in rows], ['c2s', 's2c'])
            self.assertEqual([r['hex'] for r in rows], ['01ab', '02cd'])
            self.assertLessEqual(rows[0]['t_ms'], rows[1]['t_ms'])


if __name__ == '__main__':
    unittest.main()
