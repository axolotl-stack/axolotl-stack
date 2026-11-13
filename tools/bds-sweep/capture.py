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

With `--pair FILE` it runs two overlapping operator sessions on one fresh
world: `host/` joins and runs the commands of FILE (as `--commands`, but
`!console` steps are not typed);
`--guest-delay` seconds after the host started, `guest/` joins as
`BDSExtractor2`, settles three seconds and leaves while the host's script is
still running. The host's `command-outputs.json` then holds what BDS sends an
online player when another one joins and leaves, and the guest's join
sequence is the join of a second player (for example into weather the host's
script set). Such a capture does not update `latest.txt` either.

    python tools/bds-sweep/capture.py [--out DIR] [--extractor PATH] [--commands FILE [--command-wait MS] | --pair FILE]

Output (outside the repository, shared by every worktree):
`<BDS runtime>/captures/1.26.51/<UTC timestamp>/` with one directory per
session holding the extractor's files plus `bds.log`, `extractor.log` and
`session.json`; `<BDS runtime>/captures/1.26.51/latest.txt` names the newest
complete capture. `commit_fixtures.py` copies the committed subset into the
repository. See `crates/bds-extractor/README.md`.
"""

from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import json
import queue
import shutil
import subprocess
import sys
import threading
import time
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


def extractor_command(extractor: Path, out: Path, name: str = PLAYER, summon: bool = False,
                      commands: Path | None = None, settle: int | None = None,
                      command_wait: int | None = None) -> list[str]:
    """The `bds-extractor` capture-mode command line of one session."""
    command = [str(extractor), '--addr', bds_runtime.ADDR, '--name', name, '--timeout', '120',
               '--capture-dir', str(out), '--log', 'info']
    if summon:
        command.append('--summon')
    if commands is not None:
        command += ['--commands', str(commands)]
    if command_wait is not None:
        command += ['--command-wait', str(command_wait)]
    if settle is not None:
        command += ['--settle', str(settle)]
    return command


def describe(out: Path, properties: dict[str, object], command: list[str], **extra: object) -> None:
    (out / 'session.json').write_text(json.dumps({
        'bds_version': bds_runtime.EXPECTED_VERSION,
        'server': 'bedrock_server.exe (no Endstone)',
        'server_properties': properties,
        'extractor_command': ['bds-extractor', *command[1:]],
        **extra,
    }, indent=2) + '\n', encoding='utf-8')


def session(out: Path, extractor: Path, permission: str, summon: bool, timeout: int,
            commands: Path | None = None, command_wait: int | None = None) -> int:
    out.mkdir(parents=True)
    properties = bds_runtime.base_properties(LEVEL, default_player_permission_level=permission, **WORLD)
    command = extractor_command(extractor, out, summon=summon, commands=commands, command_wait=command_wait)
    describe(out, properties, command)
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


def guest_names(count: int) -> list[str]:
    """Guest player names: `BDSExtractor2`, `BDSExtractor3`, ..."""
    return [f'{PLAYER}{index}' for index in range(2, count + 2)]


def guest_settle(index: int, count: int, gap: float | None) -> int:
    """Seconds guest `index` stays after its join: 3 when guests take turns;
    with a `gap` between overlapping guests, long enough that every guest is
    online once the last one has joined."""
    if gap is None:
        return 3
    return 3 + int(gap * (count - 1 - index) + 0.999)


def pair(out: Path, extractor: Path, script: Path, guest_delay: float, guests: int,
         guest_gap: float | None, timeout: int) -> int:
    """A host session running `script`, and `guests` guests that join and leave
    during it: one after the other, or with `guest_gap` seconds between their
    starts so that they are online together."""
    host_out = out / 'host'
    host_out.mkdir(parents=True)
    properties = bds_runtime.base_properties(LEVEL, default_player_permission_level='operator',
                                             max_players=guests + 1, **WORLD)
    host = extractor_command(extractor, host_out, commands=script)
    plan = dict(guest_delay_s=guest_delay, guests=guests, guest_gap_s=guest_gap)
    describe(host_out, properties, host, **plan)
    processes: list[subprocess.Popen] = []
    with bds_runtime.Bds(properties, out / 'bds.log', endstone=False), contextlib.ExitStack() as logs:
        host_log = logs.enter_context((host_out / 'extractor.log').open('w', encoding='utf-8'))
        host_process = subprocess.Popen(host, cwd=REPO, stdout=host_log, stderr=subprocess.STDOUT)
        try:
            time.sleep(guest_delay)
            for index, name in enumerate(guest_names(guests)):
                guest_out = out / ('guest' if index == 0 else f'guest{index + 1}')
                guest_out.mkdir(parents=True)
                guest = extractor_command(extractor, guest_out, name=name,
                                          settle=guest_settle(index, guests, guest_gap))
                describe(guest_out, properties, guest, **plan)
                guest_log = logs.enter_context((guest_out / 'extractor.log').open('w', encoding='utf-8'))
                processes.append(subprocess.Popen(guest, cwd=REPO, stdout=guest_log, stderr=subprocess.STDOUT))
                if guest_gap is None:
                    processes[-1].wait(timeout=timeout)
                else:
                    time.sleep(guest_gap)
            codes = [process.wait(timeout=timeout) for process in processes]
            host_code = host_process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            return 124
        finally:
            for process in [host_process, *processes]:
                if process.poll() is None:
                    process.kill()
    print(f'host: extractor exit {host_code}; guests: extractor exits {codes}')
    return host_code or next((code for code in codes if code), 0)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    stamp = dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    parser.add_argument('--out', type=Path, default=bds_runtime.CAPTURES / stamp)
    parser.add_argument('--extractor', type=Path, default=REPO / 'target' / 'debug' / 'bds-extractor.exe')
    parser.add_argument('--timeout', type=int, default=1800, help='seconds per extractor session')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--commands', type=Path, help='run only an operator session with this command script')
    mode.add_argument('--pair', type=Path,
                      help='run a host session with this command script and a guest that joins during it')
    parser.add_argument('--guest-delay', type=float, default=20.0,
                        help='--pair: seconds between the host and the first guest starting')
    parser.add_argument('--guests', type=int, default=1,
                        help='--pair: guests that join and leave one after the other')
    parser.add_argument('--guest-gap', type=float,
                        help='--pair: start the guests this many seconds apart, online together')
    parser.add_argument('--command-wait', type=int,
                        help='milliseconds after each scripted command (extractor default 1500)')
    args = parser.parse_args()
    if not args.extractor.exists():
        parser.error(f'{args.extractor} missing: cargo build -p bds-extractor')
    for script in (args.commands, args.pair):
        if script is not None and not script.is_file():
            parser.error(f'{script} is not a file')
    out: Path = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)

    world = bds_runtime.SERVER / 'worlds' / LEVEL
    if args.pair is not None:
        # The script is copied next to the capture so the session is self-describing.
        script = out / 'commands.txt'
        shutil.copyfile(args.pair, script)
        if world.exists():
            shutil.rmtree(world)
        try:
            code = pair(out, args.extractor, script, args.guest_delay, args.guests, args.guest_gap,
                        args.timeout)
        finally:
            if world.exists():
                shutil.rmtree(world)
        print(f'output {out}')
        return code

    if args.commands is None:
        sessions = [('member', 'member', False, None), ('operator', 'operator', True, None)]
    else:
        # The script is copied next to the capture so the session is self-describing.
        script = out / 'commands.txt'
        shutil.copyfile(args.commands, script)
        sessions = [('operator-commands', 'operator', False, script)]
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
