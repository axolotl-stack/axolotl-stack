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

    python tools/bds-sweep/capture.py [--out DIR] [--extractor PATH] [--commands FILE]

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
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
LEVEL = 'bds-extractor-1.26.51'
PLAYER = 'BDSExtractor'
WORLD = dict(level_seed=12345, gamemode='survival', difficulty='easy')


def session(out: Path, extractor: Path, permission: str, summon: bool, timeout: int,
            commands: Path | None = None) -> int:
    out.mkdir(parents=True)
    properties = bds_runtime.base_properties(LEVEL, default_player_permission_level=permission, **WORLD)
    command = [str(extractor), '--addr', '127.0.0.1:19132', '--name', PLAYER, '--timeout', '120',
               '--capture-dir', str(out), '--log', 'info']
    if summon:
        command.append('--summon')
    if commands is not None:
        command += ['--commands', str(commands)]
    (out / 'session.json').write_text(json.dumps({
        'bds_version': bds_runtime.EXPECTED_VERSION,
        'server': 'bedrock_server.exe (no Endstone)',
        'server_properties': properties,
        'extractor_command': ['bds-extractor', *command[1:]],
    }, indent=2) + '\n', encoding='utf-8')
    with bds_runtime.Bds(properties, out / 'bds.log', endstone=False):
        with (out / 'extractor.log').open('w', encoding='utf-8') as log:
            try:
                return subprocess.run(command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT,
                                      timeout=timeout).returncode
            except subprocess.TimeoutExpired:
                return 124


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    stamp = dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    parser.add_argument('--out', type=Path, default=bds_runtime.CAPTURES / stamp)
    parser.add_argument('--extractor', type=Path, default=REPO / 'target' / 'debug' / 'bds-extractor.exe')
    parser.add_argument('--timeout', type=int, default=1800, help='seconds per extractor session')
    parser.add_argument('--commands', type=Path, help='run only an operator session with this command script')
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
            code = session(out / name, args.extractor, permission, summon, args.timeout, commands)
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
