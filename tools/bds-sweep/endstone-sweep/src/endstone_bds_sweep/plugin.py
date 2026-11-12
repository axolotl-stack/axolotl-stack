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

from endstone.event import (ActorDamageEvent, ActorKnockbackEvent, BlockBreakEvent, EventPriority,
                            PlayerInteractEvent, PlayerJoinEvent, ServerLoadEvent, event_handler)
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
        # Every knockback an actor received: the tick, the pushed actor's and
        # the source's state, and the velocity change BDS applied.
        self.knockbacks: list[dict] = []
        self.register_events(self)
        self.logger.info(f'jobs loaded from {jobs.__file__}')

    @event_handler
    def on_server_load(self, event: ServerLoadEvent) -> None:
        (self.out / 'loaded.json').write_text(json.dumps({
            'minecraft_version': self.server.minecraft_version,
            'protocol_version': self.server.protocol_version,
            'endstone_version': self.server.version,
        }), encoding='utf-8')

    @event_handler
    def on_player_join(self, event: PlayerJoinEvent) -> None:
        # The job starts once its player (and its watcher, if it has one)
        # is online.
        wanted = [self.job['player']] + ([self.job['watcher']] if self.job.get('watcher') else [])
        if event.player.name not in wanted or self.task is not None:
            return
        if any(self.server.get_player(name) is None for name in wanted):
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

    @event_handler(priority=EventPriority.MONITOR, ignore_cancelled=True)
    def on_knockback(self, event: ActorKnockbackEvent) -> None:
        push = event.knockback
        source = event.source
        self.knockbacks.append({
            'tick': self.ticks,
            'actor': event.actor.runtime_id,
            'type': event.actor.type,
            # The velocity change of the knockback; `state.vel` is the
            # actor's velocity with it applied.
            'knockback': [push.x, push.y, push.z],
            'state': jobs.actor_state(event.actor),
            'source': jobs.actor_state(source) if source is not None else None,
        })

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

    def on_disable(self) -> None:
        # A graceful stop (handover, `stop`) interrupts the in-flight item
        # without it having crashed anything: forget it so the resumed run
        # measures it again. A crash never reaches this point.
        if self.job is not None:
            (self.out / 'inflight.json').unlink(missing_ok=True)
