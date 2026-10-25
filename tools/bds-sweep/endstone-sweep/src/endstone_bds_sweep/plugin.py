"""Runs one sweep job inside BDS.

The plugin is installed into the shared Endstone venv but stays inert unless
`BDS_SWEEP_JOB` names a job file written by `tools/bds-sweep/sweep.py`. When
the job's player (the scripted client) joins, the job generator runs on the
server thread, a slice of at most `TICK_BUDGET` seconds per tick, and appends
one JSON line per finished work item to `<output_dir>/results.jsonl`. Items
already present there are skipped, so an interrupted sweep resumes. The
outcome is `done.json` or `error.txt` in the output directory.
"""

# No `from __future__ import annotations`: Endstone reads the event type from
# each handler's real annotation.

import json
import os
import time
import traceback
from pathlib import Path

from endstone.event import (ActorDamageEvent, BlockBreakEvent, EventPriority, PlayerInteractEvent,
                            PlayerJoinEvent, ServerLoadEvent, event_handler)
from endstone.plugin import Plugin

from . import jobs

# Seconds of sweep work per server tick (a tick is 50 ms).
TICK_BUDGET = 0.035


class BdsSweep(Plugin):
    api_version = '0.11'
    load = 'POSTWORLD'

    def on_enable(self) -> None:
        self.job = None
        path = os.environ.get('BDS_SWEEP_JOB')
        if not path:
            self.logger.info('inactive: BDS_SWEEP_JOB is not set')
            return
        self.job = json.loads(Path(path).read_text(encoding='utf-8'))
        self.out = Path(self.job['output_dir'])
        self.out.mkdir(parents=True, exist_ok=True)
        self.runner = None
        self.task = None
        self.ticks = 0
        self.wake = 0
        self.started = 0.0
        # (tick, kind, (x, y, z)) of the sweep player's block clicks and breaks.
        self.events: list[tuple[int, str, tuple[int, int, int]]] = []
        # (tick, actor type, damage, cause) of every damage event.
        self.damage: list[tuple[int, str, float, str]] = []
        self.register_events(self)

    @event_handler
    def on_server_load(self, event: ServerLoadEvent) -> None:
        (self.out / 'loaded.json').write_text(json.dumps({
            'minecraft_version': self.server.minecraft_version,
            'protocol_version': self.server.protocol_version,
            'endstone_version': self.server.version,
        }), encoding='utf-8')

    @event_handler
    def on_player_join(self, event: PlayerJoinEvent) -> None:
        if event.player.name != self.job['player'] or self.task is not None:
            return
        self.task = self.server.scheduler.run_task(
            self, self.tick, delay=int(self.job.get('start_delay_ticks', 60)), period=1)

    @event_handler(priority=EventPriority.MONITOR, ignore_cancelled=True)
    def on_interact(self, event: PlayerInteractEvent) -> None:
        if event.player.name == self.job['player'] and event.has_block:
            block = event.block
            self.events.append((self.ticks, event.action.name.lower(), (block.x, block.y, block.z)))

    @event_handler(priority=EventPriority.MONITOR, ignore_cancelled=True)
    def on_break(self, event: BlockBreakEvent) -> None:
        if event.player.name == self.job['player']:
            block = event.block
            self.events.append((self.ticks, 'break', (block.x, block.y, block.z)))

    @event_handler(priority=EventPriority.MONITOR, ignore_cancelled=True)
    def on_damage(self, event: ActorDamageEvent) -> None:
        self.damage.append((self.ticks, event.actor.type, event.damage, event.damage_source.type))

    def tick(self) -> None:
        if self.runner is None:
            player = self.server.get_player(self.job['player'])
            if player is None:
                self.finish('error.txt', f'player {self.job["player"]} left before the job started')
                return
            self.started = time.monotonic()
            self.runner = jobs.run(jobs.Context(self, player), self.job)
        self.ticks += 1
        if self.ticks < self.wake:
            return
        deadline = time.perf_counter() + TICK_BUDGET
        try:
            while time.perf_counter() < deadline:
                wait = next(self.runner)
                if wait:
                    self.wake = self.ticks + int(wait)
                    return
        except StopIteration:
            self.finish('done.json', json.dumps({
                'ticks': self.ticks,
                'seconds': round(time.monotonic() - self.started, 1),
            }))
        except Exception:  # noqa: BLE001 - every failure must surface to the runner
            self.finish('error.txt', traceback.format_exc())

    def finish(self, name: str, text: str) -> None:
        if self.task is not None:
            self.task.cancel()
        (self.out / name).write_text(text, encoding='utf-8')
        self.logger.info(f'sweep finished: {name}')
