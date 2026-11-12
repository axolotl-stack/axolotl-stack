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
the commands of FILE one at a time (`bds-extractor --commands`), giving
`command-outputs.json` (every server packet attributed to the command before
it). Such a capture does not update `latest.txt`.

With `--pair FILE` it runs two overlapping operator sessions on one fresh
world: `host/` joins and runs the commands of FILE (as `--commands`);
`--guest-delay` seconds after the host started, `guest/` joins as
`BDSExtractor2`, settles three seconds and leaves while the host's script is
still running. The host's `command-outputs.json` then holds what BDS sends an
online player when another one joins and leaves, and the guest's join
sequence is the join of a second player (for example into weather the host's
script set). Such a capture does not update `latest.txt` either.
`--guest-commands FILE` makes every guest run FILE after its join (scripts
may hold `!` client actions, see `crates/bds-extractor/src/actions.rs`), and
`--guest-client-data FILE` logs guests in with those ClientData fields (a
custom skin). `--client-data` and `--blob-cache` apply to the host (or the
`--commands` session).

    python tools/bds-sweep/capture.py [--out DIR] [--extractor PATH] [--commands FILE | --pair FILE]

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
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
LEVEL = 'bds-extractor-1.26.51'
PLAYER = 'BDSExtractor'
WORLD = dict(level_seed=12345, gamemode='survival', difficulty='easy')


def extractor_command(extractor: Path, out: Path, name: str = PLAYER, summon: bool = False,
                      commands: Path | None = None, settle: int | None = None,
                      client_data: Path | None = None, blob_cache: bool = False,
                      player_input: bool = False) -> list[str]:
    """The `bds-extractor` capture-mode command line of one session;
    `player_input` sends a PlayerAuthInput stream (`--input`)."""
    command = [str(extractor), '--addr', bds_runtime.ADDR, '--name', name, '--timeout', '120',
               '--capture-dir', str(out), '--log', 'info']
    if summon:
        command.append('--summon')
    if commands is not None:
        command += ['--commands', str(commands)]
    if settle is not None:
        command += ['--settle', str(settle)]
    if client_data is not None:
        command += ['--client-data', str(client_data)]
    if blob_cache:
        command.append('--blob-cache')
    if player_input:
        command.append('--input')
    return command


def describe(out: Path, properties: dict[str, object], command: list[str], **extra: object) -> None:
    (out / 'session.json').write_text(json.dumps({
        'bds_version': bds_runtime.EXPECTED_VERSION,
        'server': 'bedrock_server.exe (no Endstone)',
        'server_properties': properties,
        'extractor_command': ['bds-extractor', *command[1:]],
        # Wall clock just before the session starts: lines sessions of one run up.
        'started_unix': round(time.time(), 3),
        **extra,
    }, indent=2) + '\n', encoding='utf-8', newline='\n')


def session(out: Path, extractor: Path, permission: str, summon: bool, timeout: int,
            commands: Path | None = None, client_data: Path | None = None, blob_cache: bool = False,
            player_input: bool = False) -> int:
    out.mkdir(parents=True)
    properties = bds_runtime.base_properties(LEVEL, default_player_permission_level=permission, **WORLD)
    command = extractor_command(extractor, out, summon=summon, commands=commands,
                                client_data=client_data, blob_cache=blob_cache,
                                player_input=player_input)
    describe(out, properties, command)
    with bds_runtime.Bds(properties, out / 'bds.log', endstone=False):
        with (out / 'extractor.log').open('w', encoding='utf-8') as log:
            try:
                return subprocess.run(command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT,
                                      timeout=timeout).returncode
            except subprocess.TimeoutExpired:
                return 124


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
         guest_gap: float | None, timeout: int, guest_script: Path | None = None,
         guest_client_data: Path | None = None, client_data: Path | None = None,
         blob_cache: bool = False) -> int:
    """A host session running `script`, and `guests` guests that join and leave
    during it: one after the other, or with `guest_gap` seconds between their
    starts so that they are online together. Guests run `guest_script` and log
    in with `guest_client_data` when given."""
    host_out = out / 'host'
    host_out.mkdir(parents=True)
    properties = bds_runtime.base_properties(LEVEL, default_player_permission_level='operator',
                                             max_players=guests + 1, **WORLD)
    host = extractor_command(extractor, host_out, commands=script, client_data=client_data,
                             blob_cache=blob_cache)
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
                                          settle=guest_settle(index, guests, guest_gap),
                                          commands=guest_script, client_data=guest_client_data)
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


def copy_client_data(source: Path | None, target: Path) -> Path | None:
    """Copies a ClientData file next to the capture so the session is self-describing."""
    if source is None:
        return None
    shutil.copyfile(source, target)
    return target


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
    parser.add_argument('--guest-commands', type=Path, help='--pair: script every guest runs after its join')
    parser.add_argument('--guest-client-data', type=Path,
                        help='--pair: JSON object of login ClientData fields for the guests')
    parser.add_argument('--client-data', type=Path,
                        help='JSON object of login ClientData fields for the host or --commands session')
    parser.add_argument('--blob-cache', action='store_true',
                        help='the host or --commands session claims a chunk blob cache')
    args = parser.parse_args()
    if not args.extractor.exists():
        parser.error(f'{args.extractor} missing: cargo build -p bds-extractor')
    for script in (args.commands, args.pair, args.guest_commands, args.guest_client_data, args.client_data):
        if script is not None and not script.is_file():
            parser.error(f'{script} is not a file')
    out: Path = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)

    world = bds_runtime.SERVER / 'worlds' / LEVEL
    if args.pair is not None:
        # The script is copied next to the capture so the session is self-describing.
        script = out / 'commands.txt'
        shutil.copyfile(args.pair, script)
        guest_script = None
        if args.guest_commands is not None:
            guest_script = out / 'guest-commands.txt'
            shutil.copyfile(args.guest_commands, guest_script)
        guest_data = copy_client_data(args.guest_client_data, out / 'guest-client-data.json')
        host_data = copy_client_data(args.client_data, out / 'client-data.json')
        if world.exists():
            shutil.rmtree(world)
        try:
            code = pair(out, args.extractor, script, args.guest_delay, args.guests, args.guest_gap,
                        args.timeout, guest_script, guest_data, host_data, args.blob_cache)
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
    client_data = copy_client_data(args.client_data, out / 'client-data.json')
    try:
        for name, permission, summon, commands in sessions:
            if world.exists():
                shutil.rmtree(world)
            code = session(out / name, args.extractor, permission, summon, args.timeout, commands,
                           client_data, args.blob_cache)
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
