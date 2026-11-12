"""Play the movement corpus on BDS 1.26.51 and extract BDS's own simulation
of every input.

    python tools/movement-diff/bds_run.py [--corpus PATH] [--only NAME ...] [--out DIR]

Run it on one BDS slot (`BDS_RUNTIME`, `BDS_PORT`; tools/bds-sweep/README.md,
"Parallel slots"). Two `bds-sweep-client`s join a fresh superflat world:

* `MoveSubject` plays the scenarios (`--scenarios`). For each one it builds
  the world with commands from the lobby, teleports to the start, and plays
  the inputs from the second input after the teleport while reporting a
  position 3 blocks above its own simulation. BDS never moves its copy of
  the player towards a position that far off; it corrects the client at most
  every sixth input instead, so the copy moves by BDS's simulation alone.
* `MoveViewer` stands 6 blocks from the lobby and records BDS's copy after
  every input (the `MovePlayer` relay: position and ground contact).

The run directory holds both captures, `scenarios.json` (where each
scenario was played) and `bds.json`, the traces `movement-diff extract`
reads from them.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(REPO / 'tools' / 'bds-sweep'))
import bds_runtime  # noqa: E402

LEVEL = 'movement-diff'
SUBJECT = 'MoveSubject'
VIEWER = 'MoveViewer'
# Fixed identities: a returning name keeps the same player data.
UUIDS = {SUBJECT: '6d6f7665-0000-4000-8000-000000000001', VIEWER: '6d6f7665-0000-4000-8000-000000000002'}
PROPERTIES = {
    'level-type': 'FLAT',
    'gamemode': 'survival',
    # No hunger (sprinting needs food) and no hostile mobs.
    'difficulty': 'peaceful',
    'default-player-permission-level': 'operator',
    'allow-cheats': 'true',
    'max-players': 4,
}
# The lobby platform (the subject waits at -14.5 100 10.5) and the viewer's
# place, outside the scenario area and within relay range of all of it.
LOBBY = ['fill -16 99 8 -13 99 18 stone', f'tp {VIEWER} -14.5 100 16.5', f'tp {SUBJECT} -14.5 100 10.5',
         'gamerule domobspawning false', 'gamerule dodaylightcycle false', 'gamerule doweathercycle false']


def tellraw(name: str, order: str) -> str:
    text = json.dumps({'rawtext': [{'text': f'bds-sweep:{order}'}]})
    return f'tellraw {name} {text}'


def launch(client: Path, out: Path, name: str, extra: list[str]) -> subprocess.Popen:
    command = [str(client), '--addr', bds_runtime.ADDR, '--name', name, '--uuid', UUIDS[name],
               '--capture-dir', str(out / name.removeprefix('Move').lower()), *extra]
    log_path = out / f'{name}.log'
    log = log_path.open('a', encoding='utf-8')
    process = subprocess.Popen(command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT)
    deadline = time.monotonic() + 60
    while f'spawned as {name}' not in log_path.read_text(encoding='utf-8', errors='replace'):
        if process.poll() is not None:
            raise RuntimeError(f'{name} exited with {process.returncode}')
        if time.monotonic() > deadline:
            raise TimeoutError(f'{name} did not spawn within 60 s')
        time.sleep(0.1)
    return process


def stop(process: subprocess.Popen) -> None:
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--corpus', type=Path, default=HERE / 'corpus.json')
    parser.add_argument('--only', nargs='*', help='play only these scenarios')
    parser.add_argument('--out', type=Path)
    target = REPO / 'target' / 'debug'
    parser.add_argument('--client', type=Path, default=target / 'bds-sweep-client.exe')
    parser.add_argument('--movement-diff', type=Path, default=target / 'movement-diff.exe')
    args = parser.parse_args()
    for binary in (args.client, args.movement_diff):
        if not binary.exists():
            parser.error(f'{binary} missing: cargo build -p bds-sweep')
    corpus = json.loads(args.corpus.read_text(encoding='utf-8'))
    if args.only:
        missing = set(args.only) - {s['name'] for s in corpus['scenarios']}
        if missing:
            parser.error(f'not in the corpus: {sorted(missing)}')
        corpus['scenarios'] = [s for s in corpus['scenarios'] if s['name'] in args.only]
    stamp = dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    out: Path = (args.out or bds_runtime.RUNTIME / 'captures' / f'movement-diff-{stamp}').resolve()
    out.mkdir(parents=True, exist_ok=False)
    played = out / 'corpus.json'
    played.write_text(json.dumps(corpus, indent=2) + '\n', encoding='utf-8')
    ticks = sum(seg['ticks'] for s in corpus['scenarios'] for seg in s['inputs'])
    # Setup (about 30 inputs) and play, at 20 inputs a second, with slack.
    budget = 60 + (ticks + 40 * len(corpus['scenarios'])) * 0.05 * 2
    world = bds_runtime.SERVER / 'worlds' / LEVEL
    if world.exists():
        shutil.rmtree(world)
    values = bds_runtime.base_properties(LEVEL, **{k.replace('-', '_'): v for k, v in PROPERTIES.items()})
    clients: list[subprocess.Popen] = []
    try:
        with bds_runtime.Bds(values, out / 'bds.log', endstone=False) as bds:
            try:
                viewer = launch(args.client, out, VIEWER, [])
                clients.append(viewer)
                subject = launch(args.client, out, SUBJECT,
                                 ['--scenarios', str(played), '--scenario-log', str(out / 'scenarios.json')])
                clients.append(subject)
                for line in LOBBY:
                    bds.command(line)
                time.sleep(3)
                bds.command(tellraw(SUBJECT, 'scenarios'))
                code = subject.wait(timeout=budget)
                if code != 0:
                    raise RuntimeError(f'{SUBJECT} exited with {code} (see {out / (SUBJECT + ".log")})')
                bds.command(tellraw(VIEWER, 'quit'))
                viewer.wait(timeout=30)
            finally:
                for process in clients:
                    stop(process)
    finally:
        if world.exists():
            shutil.rmtree(world)
    subprocess.run([str(args.movement_diff), 'extract', '--corpus', str(played), '--run', str(out),
                    '--out', str(out / 'bds.json')], check=True)
    print(f'output {out}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
