import json
import os
from pathlib import Path
import threading

from endstone.event import EventPriority, PacketReceiveEvent, PacketSendEvent, ServerLoadEvent, event_handler
from endstone.plugin import Plugin

from .capture import PacketLog


class ClientDiff(Plugin):
    api_version = '0.11'
    load = 'STARTUP'

    def on_enable(self):
        self.output = Path(os.environ.get('CLIENT_DIFF_CAPTURE_DIR', 'captures')).resolve()
        self.output.mkdir(parents=True, exist_ok=True)
        self.logs = {}
        self.lock = threading.Lock()
        self.register_events(self)

    def fail(self, error):
        (self.output / 'error.txt').write_text(str(error), encoding='utf-8')
        self.logger.error(str(error))
        self.server.shutdown()

    @event_handler
    def on_load_complete(self, event: ServerLoadEvent):
        try:
            if self.server.protocol_version != 2193:
                raise RuntimeError(f'Expected protocol 2193, got {self.server.protocol_version}')
            for command in ('gamerule dodaylightcycle false', 'gamerule randomtickspeed 0',
                            'gamerule domobspawning false', 'gamerule doweathercycle false',
                            'time set 6000', 'weather clear'):
                if not self.server.dispatch_command(self.server.command_sender, command):
                    raise RuntimeError(f'Deterministic world command failed: {command}')
            (self.output / 'ready.json').write_text(json.dumps({
                'minecraft_version': self.server.minecraft_version,
                'protocol_version': self.server.protocol_version,
            }), encoding='utf-8')
        except Exception as error:
            self.fail(error)

    def capture(self, event, direction):
        try:
            # Endstone exposes only one sub-client ID, not the full original header.
            # The differential clients use primary sessions; reject split screen rather
            # than fabricate sender/recipient header bits that the API cannot expose.
            if event.sub_client_id != 0:
                raise RuntimeError('Only primary-client packet headers can be captured faithfully')
            key = (event.address.hostname, event.address.port)
            with self.lock:
                if key not in self.logs:
                    index = len(self.logs)
                    self.logs[key] = PacketLog(self.output / f'player-{index:03d}.jsonl')
                    (self.output / f'player-{index:03d}.json').write_text(
                        json.dumps({'host': key[0], 'port': key[1]}), encoding='utf-8')
                self.logs[key].record(direction, event.packet_id, event.payload)
        except Exception as error:
            self.fail(error)

    @event_handler(priority=EventPriority.MONITOR, ignore_cancelled=True)
    def receive(self, event: PacketReceiveEvent):
        self.capture(event, 'c2s')

    @event_handler(priority=EventPriority.MONITOR, ignore_cancelled=True)
    def send(self, event: PacketSendEvent):
        self.capture(event, 's2c')

    def on_disable(self):
        for log in self.logs.values():
            log.close()
