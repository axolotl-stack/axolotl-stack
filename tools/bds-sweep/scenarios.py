"""Per-scenario captures of `blocks` sweeps.

    python tools/bds-sweep/scenarios.py split <sweep dir>
    python tools/bds-sweep/scenarios.py commit <sweep dir> [<later sweep dir> ...]

`split` cuts the client's whole-session capture (`<dir>/capture/packets.jsonl`)
into `<dir>/scenarios/<name>/packets.jsonl` at the plugin's
`bds-sweep:scenario begin|end <name>` chat lines (both included; the records
stay byte-identical, so `seq`/`t_ms` count from the session start).
`sweep.py` runs it after every `blocks` run. A scenario whose end marker is
missing (the run died) gets no capture.

`commit` copies every scenario that has both `steps.json` and a capture into
`crates/unastar/tests/scenarios/blocks/<name>/` as `packets.jsonl.gz`
(deterministic gzip) and `steps.json`, a later sweep dir replacing an
earlier one's scenario, and rewrites the folder's `README.md` from the
`steps.json` files there.
"""

from __future__ import annotations

import gzip
import json
import re
import shutil
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
FIXTURES = REPO / 'crates' / 'unastar' / 'tests' / 'scenarios' / 'blocks'
MARKER = re.compile(rb'bds-sweep:scenario (begin|end) ([A-Za-z0-9_]+)')


def marker(record: dict) -> tuple[str, str] | None:
    """(`begin`|`end`, scenario) when the record is a server TextPacket
    carrying a scenario marker."""
    if record.get('dir') != 's2c' or record.get('name') != 'TextPacket':
        return None
    found = MARKER.search(bytes.fromhex(record['hex']))
    return (found.group(1).decode(), found.group(2).decode()) if found else None


def split(capture: Path, scenarios: Path) -> dict[str, int]:
    """Writes the records from each begin marker to its end marker to
    `scenarios/<name>/packets.jsonl`; returns the record count per scenario
    written."""
    written: dict[str, int] = {}
    current: str | None = None
    lines: list[str] = []
    with capture.open(encoding='utf-8') as source:
        for line in source:
            if not line.strip():
                continue
            found = marker(json.loads(line))
            if found and found[0] == 'begin':
                current, lines = found[1], [line]
                continue
            if current is None:
                continue
            lines.append(line)
            if found == ('end', current):
                folder = scenarios / current
                folder.mkdir(parents=True, exist_ok=True)
                (folder / 'packets.jsonl').write_text(''.join(lines), encoding='utf-8')
                written[current] = len(lines)
                current, lines = None, []
    return written


def gzip_bytes(data: bytes) -> bytes:
    """gzip without a file name or timestamp, so equal input gives equal
    bytes."""
    import io
    buffer = io.BytesIO()
    with gzip.GzipFile(filename='', mode='wb', fileobj=buffer, compresslevel=9, mtime=0) as out:
        out.write(data)
    return buffer.getvalue()


def commit(sweeps: list[Path], fixtures: Path = FIXTURES) -> list[str]:
    """Copies the scenarios of `sweeps` (later ones win) into `fixtures`;
    returns the names copied."""
    chosen: dict[str, Path] = {}
    for sweep in sweeps:
        for folder in sorted((sweep / 'scenarios').glob('*')):
            if (folder / 'steps.json').exists() and (folder / 'packets.jsonl').exists():
                chosen[folder.name] = folder
    for name, folder in chosen.items():
        target = fixtures / name
        target.mkdir(parents=True, exist_ok=True)
        (target / 'packets.jsonl.gz').write_bytes(gzip_bytes((folder / 'packets.jsonl').read_bytes()))
        shutil.copyfile(folder / 'steps.json', target / 'steps.json')
    write_readme(fixtures)
    return sorted(chosen)


README_HEAD = """# Block behaviour scenarios

Each folder is one block-behaviour scenario measured on BDS 1.26.51 with the
bds-sweep blocks job (`tools/bds-sweep`, `sweep.py blocks --scenarios ...`):
a survival player driven by the scripted `bds-sweep-client` on loopback, the
world set up and the steps sequenced by the Endstone sweep plugin. A block
behaviour counts as done when replaying `steps.json` on unastar produces the
same packets as `packets.jsonl.gz`.

* `packets.jsonl.gz`: both directions of the client session from the
  scenario's begin marker to its end marker (the chat lines
  `bds-sweep:scenario begin|end NAME`, both included), in the client-diff
  capture format (`seq`, `t_ms` since the session start, `dir`, `id`, `name`,
  `hex` of the decompressed, decrypted packet). The scenario's last step is
  followed by `settle_ticks` idle ticks before the end marker.
* `steps.json`: the world (`world`), the commands run before the begin
  marker to reach the starting state (`prelude`: game rules, survival,
  peaceful, a reset stone arena, the player at the arena start), and every
  step after it with `tick` = server ticks since the begin marker:
  `command` (with its success), `use` (block, face, click position, held
  item, the player's position and look), `mine_start`/`mine_stop`, `fall`,
  `select_slot`, `remove_item_actors` (item actors the plugin removed through
  the server API, listed with their stacks), `mark` (the `bds-sweep:mark
  SCENARIO/STEP` chat line sent to the client, also in the capture) and
  `wait`. `error` is set when the scenario failed part-way; its capture
  stops being meaningful at that point.

Replay notes:

* Align by the chat lines: the `marker` and `mark` steps appear in the
  capture as server `TextPacket`s in the same order.
* Actor runtime/unique ids depend on everything spawned earlier in the same
  BDS run (scenarios ran back to back); map them by first appearance.
* Item pickup by the player is cancelled by the plugin; drops stay on the
  ground until the plugin removes them (`remove_item_actors`) or a reset
  kills them.
* Random outcomes: item drop spread and velocity, tool/crop drop counts,
  bone meal growth, random-tick driven steps (fire spread, crop growth,
  farmland drying, sugar cane/cactus checks that wait for a random tick)
  and explosion block selection differ between runs; compare those by
  outcome class, not byte for byte.
* The client's `PlayerAuthInput` is sent every 50 ms on wall-clock time, not
  in step with server ticks.

| Scenario | Covers | Recorded (UTC) | Steps | Packets | Size | Status |
|---|---|---|---|---|---|---|
"""


def write_readme(fixtures: Path) -> None:
    rows = []
    for folder in sorted(fixtures.glob('*')):
        steps_path = folder / 'steps.json'
        capture = folder / 'packets.jsonl.gz'
        if not steps_path.exists() or not capture.exists():
            continue
        steps = json.loads(steps_path.read_text(encoding='utf-8'))
        records = gzip.decompress(capture.read_bytes()).count(b'\n')
        size = capture.stat().st_size + steps_path.stat().st_size
        status = f'failed: {steps["error"]}' if steps.get('error') else 'ok'
        actions = sum(1 for step in steps['steps'] if step['op'] in ('use', 'mine_start', 'command', 'fall'))
        rows.append(f'| `{folder.name}` | {steps["covers"]} | {steps["recorded_utc"]} | {actions} | {records} '
                    f'| {size / 1024:.0f} KiB | {status} |')
    (fixtures / 'README.md').write_text(README_HEAD + '\n'.join(rows) + '\n', encoding='utf-8')


def main() -> int:
    if len(sys.argv) < 3 or sys.argv[1] not in ('split', 'commit'):
        print(__doc__)
        return 2
    if sys.argv[1] == 'split':
        sweep = Path(sys.argv[2]).resolve()
        written = split(sweep / 'capture' / 'packets.jsonl', sweep / 'scenarios')
        print(json.dumps(written, indent=1))
    else:
        print('\n'.join(commit([Path(arg).resolve() for arg in sys.argv[2:]])))
    return 0


if __name__ == '__main__':
    sys.exit(main())
