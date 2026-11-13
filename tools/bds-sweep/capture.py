"""T1: record BDS 1.26.51 sessions with bds-extractor's capture mode.

Runs the plain `bedrock_server.exe` of the shared runtime (no Endstone: its
own commands would leak into AvailableCommands) with BDS default settings
(survival, easy, default game rules), twice, each time on a fresh
harness-owned world (seed 12345):

1. `member/`: first join of a member (join sequence, StartGame with default
   game rules, AvailableCommands as a member);
2. `operator/`: first join with `default-player-permission-level=operator`
   (AvailableCommands as an operator), then /summon of every summonable
   identifier.

Each session needs its own world: the default permission level only applies
to a player's first join, and BDS 1.26.51 does not resend AvailableCommands
when a player is opped while online.

With `--commands FILE` it runs a single session instead,
`operator-commands/`: first join of an operator on the same fresh world, then
the steps of FILE one at a time (`bds-extractor --commands`), giving
`command-outputs.json` (every server packet attributed to the step before
it). `!console COMMAND` steps are typed on BDS's console by this script when
the extractor reaches them; every console line BDS prints is attributed to
the step running at the time (`console-outputs.json`). `!join NAME` and
`!leave NAME` bring a second player in and out (`clients/NAME.jsonl`). The
server files console commands change (`allowlist.json`, `permissions.json`,
`keys/`) are restored afterwards. Such a capture does not update
`latest.txt`.

    python tools/bds-sweep/capture.py [--out DIR] [--extractor PATH] [--commands FILE [--command-wait MS]]

Output (outside the repository, shared by every worktree):
`<BDS runtime>/captures/1.26.51/<UTC timestamp>/` with one directory per
session holding the extractor's files plus `bds.log`, `extractor.log` and
`session.json`; `<BDS runtime>/captures/1.26.51/latest.txt` names the newest
complete capture. `commit_fixtures.py` copies the committed subset into the
repository. See `crates/bds-extractor/README.md`.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import queue
import shutil
import subprocess
import sys
import threading
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
LEVEL = 'bds-extractor-1.26.51'
PLAYER = 'BDSExtractor'
WORLD = dict(level_seed=12345, gamemode='survival', difficulty='easy')
# Printed by bds-extractor before every script step / for a console step.
STEP_MARKER = '@@step@@'
CONSOLE_MARKER = '@@console@@'
# Server files that console commands write (allowlist, permission and
# server identity commands); restored after a command session.
SERVER_FILES = ('allowlist.json', 'permissions.json', 'keys/server_identity_key.pem')


class ConsoleSteps:
    """BDS console lines grouped by the script step running when they arrived."""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.current: int | None = None
        self.lines: dict[int, list[str]] = {}

    def begin(self, index: int) -> None:
        with self.lock:
            self.current = index
            self.lines.setdefault(index, [])

    def add(self, line: str) -> None:
        with self.lock:
            if self.current is not None:
                self.lines[self.current].append(line)

    def outputs(self) -> list[dict]:
        with self.lock:
            return [{'step': index, 'lines': lines} for index, lines in sorted(self.lines.items()) if lines]


def follow_extractor(process: subprocess.Popen, log, bds: bds_runtime.Bds, steps: ConsoleSteps) -> None:
    """Copies the extractor's output to its log and acts on its step markers."""
    assert process.stdout
    for line in process.stdout:
        log.write(line)
        log.flush()
        text = line.rstrip('\r\n')
        if text.startswith(STEP_MARKER + ' '):
            steps.begin(int(text.split()[1]))
        elif text.startswith(CONSOLE_MARKER + ' '):
            bds.command(text[len(CONSOLE_MARKER) + 1:])


def follow_console(bds: bds_runtime.Bds, steps: ConsoleSteps, done: threading.Event) -> None:
    """Attributes every BDS console line to the current step."""
    while not done.is_set():
        try:
            line = bds.lines.get(timeout=0.1)
        except queue.Empty:
            continue
        bds.history.append(line)
        steps.add(line)


def snapshot_server_files() -> dict[str, bytes | None]:
    return {name: (bds_runtime.SERVER / name).read_bytes() if (bds_runtime.SERVER / name).exists() else None
            for name in SERVER_FILES}


def restore_server_files(saved: dict[str, bytes | None]) -> None:
    for name, data in saved.items():
        path = bds_runtime.SERVER / name
        if data is None:
            path.unlink(missing_ok=True)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)


def session(out: Path, extractor: Path, permission: str, summon: bool, timeout: int,
            commands: Path | None = None, command_wait: int | None = None) -> int:
    out.mkdir(parents=True)
    properties = bds_runtime.base_properties(LEVEL, default_player_permission_level=permission, **WORLD)
    command = [str(extractor), '--addr', bds_runtime.ADDR, '--name', PLAYER, '--timeout', '120',
               '--capture-dir', str(out), '--log', 'info']
    if summon:
        command.append('--summon')
    if commands is not None:
        command += ['--commands', str(commands)]
    if command_wait is not None:
        command += ['--command-wait', str(command_wait)]
    (out / 'session.json').write_text(json.dumps({
        'bds_version': bds_runtime.EXPECTED_VERSION,
        'server': 'bedrock_server.exe (no Endstone)',
        'server_properties': properties,
        'extractor_command': ['bds-extractor', *command[1:]],
    }, indent=2) + '\n', encoding='utf-8')
    saved = snapshot_server_files()
    steps = ConsoleSteps()
    try:
        with bds_runtime.Bds(properties, out / 'bds.log', endstone=False) as bds:
            done = threading.Event()
            console = threading.Thread(target=follow_console, args=(bds, steps, done), daemon=True)
            console.start()
            with (out / 'extractor.log').open('w', encoding='utf-8') as log:
                process = subprocess.Popen(command, cwd=REPO, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                           text=True, encoding='utf-8', errors='replace', bufsize=1)
                follower = threading.Thread(target=follow_extractor, args=(process, log, bds, steps), daemon=True)
                follower.start()
                try:
                    code = process.wait(timeout=timeout)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                    code = 124
                follower.join(timeout=10)
            done.set()
            console.join(timeout=5)
    finally:
        restore_server_files(saved)
    if commands is not None:
        (out / 'console-outputs.json').write_text(json.dumps(steps.outputs(), indent=1) + '\n', encoding='utf-8')
    return code


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    stamp = dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    parser.add_argument('--out', type=Path, default=bds_runtime.CAPTURES / stamp)
    parser.add_argument('--extractor', type=Path, default=REPO / 'target' / 'debug' / 'bds-extractor.exe')
    parser.add_argument('--timeout', type=int, default=1800, help='seconds per extractor session')
    parser.add_argument('--commands', type=Path, help='run only an operator session with this command script')
    parser.add_argument('--command-wait', type=int,
                        help='milliseconds after each scripted command (extractor default 1500)')
    args = parser.parse_args()
    if not args.extractor.exists():
        parser.error(f'{args.extractor} missing: cargo build -p bds-extractor')
    if args.commands is not None and not args.commands.is_file():
        parser.error(f'{args.commands} is not a file')
    out: Path = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)

    if args.commands is None:
        sessions = [('member', 'member', False, None), ('operator', 'operator', True, None)]
    else:
        # The script is copied next to the capture so the session is self-describing.
        script = out / 'commands.txt'
        shutil.copyfile(args.commands, script)
        sessions = [('operator-commands', 'operator', False, script)]
    world = bds_runtime.SERVER / 'worlds' / LEVEL
    try:
        for name, permission, summon, commands in sessions:
            if world.exists():
                shutil.rmtree(world)
            code = session(out / name, args.extractor, permission, summon, args.timeout, commands,
                           args.command_wait)
            print(f'{name}: extractor exit {code}')
            if code != 0:
                return code
    finally:
        if world.exists():
            shutil.rmtree(world)
    if args.commands is None:
        (bds_runtime.CAPTURES / 'latest.txt').write_text(f'{out}\n', encoding='utf-8')
    print(f'output {out}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
