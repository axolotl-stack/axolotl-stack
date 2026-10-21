import json
import threading
import time


def packet_bytes(packet_id: int, payload: bytes) -> bytes:
    header = bytearray()
    value = packet_id
    while value >= 128:
        header.append((value & 127) | 128)
        value >>= 7
    header.append(value)
    return bytes(header) + payload


class PacketLog:
    def __init__(self, path):
        self.file = path.open('x', encoding='utf-8', buffering=1)
        self.started = time.monotonic_ns()
        self.seq = 0
        self.lock = threading.Lock()

    def record(self, direction, packet_id, payload):
        with self.lock:
            row = {'seq': self.seq, 't_ms': (time.monotonic_ns() - self.started) // 1_000_000,
                   'dir': direction, 'id': packet_id, 'name': f'packet_{packet_id}',
                   'hex': packet_bytes(packet_id, payload).hex()}
            self.file.write(json.dumps(row, separators=(',', ':')) + '\n')
            self.seq += 1

    def close(self):
        with self.lock:
            self.file.close()
