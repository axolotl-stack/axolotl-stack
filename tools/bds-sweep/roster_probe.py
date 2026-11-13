"""Player-colour reuse probe: does a joining player take the colour a player
who left freed, or the next one in the palette?

    python tools/bds-sweep/roster_probe.py OUT_DIR SCRIPT

One fresh world, all players operators (as `capture.py --pair`). The host
`BDSExtractor` runs SCRIPT (filler commands that keep it online). Then, by
start time after the host: A (`BDSExtractorA`) at 25 s, staying 12 s after its
join; B (`BDSExtractorB`) 6 s later, staying 45 s, so it joins while A is
online; C (`BDSExtractorC`) 25 s after B, after A has left. Each session is a
`bds-extractor --capture-dir` directory under OUT_DIR (`host`, `A`, `B`,
`C`). In the 2026-11-11 run C took A's colour (the second of the palette)
while B kept the third. Use the `BDS_RUNTIME`/`BDS_PORT` of a locked slot.
"""

from __future__ import annotations

import contextlib
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import capture  # noqa: E402

# (session, player name, seconds it stays after its join, seconds after the previous start)
PLAN = [('A', 'BDSExtractorA', 12, 25), ('B', 'BDSExtractorB', 45, 6), ('C', 'BDSExtractorC', 5, 25)]


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    out, script = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
    extractor = capture.REPO / 'target' / 'debug' / 'bds-extractor.exe'
    out.mkdir(parents=True)
    shutil.copyfile(script, out / 'commands.txt')
    world = bds_runtime.SERVER / 'worlds' / capture.LEVEL
    if world.exists():
        shutil.rmtree(world)
    properties = bds_runtime.base_properties(capture.LEVEL, default_player_permission_level='operator',
                                             max_players=len(PLAN) + 1, **capture.WORLD)
    processes = []
    try:
        with bds_runtime.Bds(properties, out / 'bds.log', endstone=False), contextlib.ExitStack() as logs:
            sessions = [('host', capture.extractor_command(extractor, out / 'host', commands=out / 'commands.txt'), 0)]
            sessions += [(name, capture.extractor_command(extractor, out / name, name=player, settle=settle), wait)
                         for name, player, settle, wait in PLAN]
            for name, command, wait in sessions:
                time.sleep(wait)
                (out / name).mkdir()
                capture.describe(out / name, properties, command, plan=PLAN)
                log = logs.enter_context((out / name / 'extractor.log').open('w', encoding='utf-8'))
                processes.append(subprocess.Popen(command, cwd=capture.REPO, stdout=log, stderr=subprocess.STDOUT))
            codes = [process.wait(timeout=600) for process in processes]
    finally:
        for process in processes:
            if process.poll() is None:
                process.kill()
        if world.exists():
            shutil.rmtree(world)
    print(f'extractor exits {codes}; output {out}')
    return next((code for code in codes if code), 0)


if __name__ == '__main__':
    sys.exit(main())
