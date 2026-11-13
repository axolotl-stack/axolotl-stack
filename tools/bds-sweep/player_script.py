"""Scripted player captures on plain BDS: run a timed script of console
commands and client orders against one BDS runtime (a slot) with
`bds-sweep-client` players recording their packets.

    python tools/bds-sweep/player_script.py SCRIPT [--label NAME] [--out DIR]

The script is one step per line (`#` starts a comment):

    property KEY=VALUE      server.properties override (read before BDS starts)
    launch NAME [UUID]      start a client; waits until it has spawned
    wait SECONDS            sleep
    cmd LINE...             a console command
    order NAME TEXT...      `tellraw NAME {"rawtext":[{"text":"bds-sweep:TEXT"}]}`
    mark LABEL              record a phase marker (phases.json)
    expect REGEX [SECONDS]  wait for a console line (default 30 s)
    quit NAME               order the client to leave and wait for BDS to drop it

The world is plain BDS on a fresh harness-owned level (`bds-sweep-player`,
superflat survival unless overridden), removed afterwards. Output, outside
the repository: `<BDS runtime>/captures/<label>-<UTC>/` with each client's
`NAME/packets.jsonl` and `NAME.log`, `bds.log`, `phases.json` (every step
with its wall time) and a copy of the script. Set `BDS_RUNTIME` and
`BDS_PORT` for a slot (README, "Parallel slots").
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import shlex
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
LEVEL = 'bds-sweep-player'
DEFAULTS = {'level-type': 'FLAT', 'gamemode': 'survival', 'difficulty': 'easy', 'max-players': 8,
            'default-player-permission-level': 'operator'}


def parse(text: str) -> tuple[dict[str, str], list[list[str]]]:
    """The property overrides and the steps of a script."""
    properties: dict[str, str] = {}
    steps: list[list[str]] = []
    for number, raw in enumerate(text.splitlines(), 1):
        line = raw.split('#', 1)[0].strip()
        if not line:
            continue
        verb, _, rest = line.partition(' ')
        if verb == 'property':
            key, sep, value = rest.strip().partition('=')
            if not sep or not key:
                raise ValueError(f'line {number}: property needs KEY=VALUE')
            properties[key.strip()] = value.strip()
            continue
        if verb not in {'launch', 'wait', 'cmd', 'order', 'mark', 'expect', 'quit'}:
            raise ValueError(f'line {number}: unknown step {verb!r}')
        if verb in {'cmd', 'order', 'mark'}:
            # Free text after the verb (and the client name for `order`).
            if verb == 'order':
                name, _, text_rest = rest.strip().partition(' ')
                steps.append([verb, name, text_rest.strip()])
            else:
                steps.append([verb, rest.strip()])
        else:
            steps.append([verb, *shlex.split(rest)])
        if verb == 'wait':
            float(steps[-1][1])
    return properties, steps


def tellraw(name: str, order: str) -> str:
    """The console command that sends `order` to one client."""
    text = json.dumps({'rawtext': [{'text': f'bds-sweep:{order}'}]})
    return f'tellraw {name} {text}'


class Run:
    def __init__(self, out: Path, client: Path, bds: bds_runtime.Bds):
        self.out = out
        self.client = client
        self.bds = bds
        self.clients: dict[str, subprocess.Popen] = {}
        self.phases: list[dict] = []
        self.start = time.time()

    def record(self, phase: str, **extra) -> None:
        now = time.time()
        self.phases.append({'phase': phase, 'wall': now, 'since_start_s': round(now - self.start, 3), **extra})

    def launch(self, name: str, uuid: str | None) -> None:
        command = [str(self.client), '--addr', bds_runtime.ADDR, '--name', name,
                   '--capture-dir', str(self.out / name)]
        if uuid:
            command += ['--uuid', uuid]
        log = (self.out / f'{name}.log').open('a', encoding='utf-8')
        self.clients[name] = subprocess.Popen(command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT)
        self.record('launch', client=name)
        deadline = time.monotonic() + 60
        while f'spawned as {name}' not in (self.out / f'{name}.log').read_text(encoding='utf-8', errors='replace'):
            if self.clients[name].poll() is not None:
                raise RuntimeError(f'client {name} exited with {self.clients[name].returncode}')
            if time.monotonic() > deadline:
                raise TimeoutError(f'client {name} did not spawn within 60 s')
            time.sleep(0.1)
        self.record('spawned', client=name)

    def step(self, step: list[str]) -> None:
        verb = step[0]
        if verb == 'launch':
            self.launch(step[1], step[2] if len(step) > 2 else None)
        elif verb == 'wait':
            time.sleep(float(step[1]))
        elif verb == 'cmd':
            self.record('cmd', line=step[1])
            self.bds.command(step[1])
        elif verb == 'order':
            self.record('order', client=step[1], order=step[2])
            self.bds.command(tellraw(step[1], step[2]))
        elif verb == 'mark':
            self.record('mark', label=step[1])
        elif verb == 'expect':
            timeout = float(step[2]) if len(step) > 2 else 30.0
            match = self.bds.wait_for(step[1], timeout)
            self.record('expect', pattern=step[1], line=match.string)
        elif verb == 'quit':
            name = step[1]
            self.record('order', client=name, order='quit')
            self.bds.command(tellraw(name, 'quit'))
            self.bds.wait_for(rf'Player disconnected: {re.escape(name)}\b', 30)
            self.clients.pop(name).wait(timeout=30)
            self.record('left', client=name)

    def close(self) -> None:
        for process in self.clients.values():
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
        (self.out / 'phases.json').write_text(json.dumps(self.phases, indent=2) + '\n', encoding='utf-8')


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('script', type=Path)
    parser.add_argument('--label', default='player')
    parser.add_argument('--out', type=Path)
    parser.add_argument('--client', type=Path, default=REPO / 'target' / 'debug' / 'bds-sweep-client.exe')
    args = parser.parse_args()
    if not args.client.exists():
        parser.error(f'{args.client} missing: cargo build -p bds-sweep')
    properties, steps = parse(args.script.read_text(encoding='utf-8'))
    stamp = dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    out: Path = (args.out or bds_runtime.RUNTIME / 'captures' / f'{args.label}-{stamp}').resolve()
    out.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(args.script, out / 'script.txt')
    world = bds_runtime.SERVER / 'worlds' / LEVEL
    if world.exists():
        shutil.rmtree(world)
    values = bds_runtime.base_properties(LEVEL, **{k.replace('-', '_'): v for k, v in {**DEFAULTS, **properties}.items()})
    try:
        with bds_runtime.Bds(values, out / 'bds.log', endstone=False) as bds:
            run = Run(out, args.client, bds)
            run.record('server-started')
            try:
                for step in steps:
                    run.step(step)
            finally:
                run.close()
    finally:
        if world.exists():
            shutil.rmtree(world)
    print(f'output {out}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
