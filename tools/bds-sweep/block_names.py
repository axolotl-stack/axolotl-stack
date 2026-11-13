"""Measure the translation keys BDS names blocks by in command feedback.

    python tools/bds-sweep/block_names.py script OUT.txt
    python tools/bds-sweep/capture.py --commands OUT.txt --command-wait 150
    python tools/bds-sweep/block_names.py table CAPTURE_DIR

`script` writes a command script with one `/testforblock 8 199 8 <block>` per
palette block (default state, protocolgen's CC0 palette) against a stone
block. `table` reads the session's `command-outputs.json` and writes
`crates/unastar-data/data/measured/block_name_keys.json`: the failure
`commands.testforblock.failed.tile` names the expected block's key as its
fifth parameter. Every command answers exactly one `CommandOutput`, in
command order, so answers are paired with commands by order (the short wait
can push an answer into the next command's window). Blocks the `Block`
command enum refuses (a syntax error) are left out.
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import native  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
TABLE = REPO / 'crates' / 'unastar-data' / 'data' / 'measured' / 'block_name_keys.json'
PLATFORM_KEY = '%tile.stone.stone.name'

PRELUDE = [
    '# segment: prelude',
    '/gamerule domobspawning false',
    '/tp @s 8 200 8',
    '/time query daytime',
    '/time query daytime',
    '/time query daytime',
    '/time query daytime',
    '/fill 0 199 0 15 199 15 stone',
    '/testforblock 8 199 8 stone',
]


def write_script(out: Path) -> None:
    names = sorted(native.default_states())
    lines = ['# Block translation keys: /testforblock against stone names the expected block.', *PRELUDE,
             '# segment: names', *(f'/testforblock 8 199 8 {name}' for name in names)]
    out.write_text('\n'.join(lines) + '\n', encoding='utf-8', newline='\n')
    print(f'{len(names)} blocks')


def pair(outputs: list[dict]) -> list[tuple[str, dict]]:
    """`(command, answer)` from the prelude's /testforblock on, by order."""
    start = next(i for i, o in enumerate(outputs) if o['command'].startswith('/testforblock'))
    commands = [o['command'] for o in outputs[start:]]
    answers = [p['data']['output'] for o in outputs[start:] for p in o['packets']
               if p['packet'] == 'CommandOutputPacket']
    if len(answers) != len(commands):
        raise SystemExit(f'{len(commands)} commands but {len(answers)} answers')
    return list(zip(commands, answers))


def keys(pairs: list[tuple[str, dict]]) -> tuple[dict[str, str], list[str]]:
    found: dict[str, str] = {}
    refused = []
    for command, answer in pairs:
        block = command.split()[-1]
        message = answer['output_messages'][0]
        params = message['parameters']
        if message['message_id'] == 'commands.testforblock.failed.tile':
            if params[3] != PLATFORM_KEY:
                raise SystemExit(f'{command}: the platform is {params[3]}')
            found[block] = params[4].removeprefix('%')
        elif message['message_id'] == 'commands.testforblock.success':
            found[block] = PLATFORM_KEY.removeprefix('%')
        else:
            refused.append(block)
    found.pop('stone', None)
    return dict(sorted(found.items())), refused


def write_table(capture: Path) -> None:
    outputs_path = capture / 'operator-commands' / 'command-outputs.json'
    outputs = json.loads(outputs_path.read_text(encoding='utf-8'))
    found, refused = keys(pair(outputs))
    table = {
        'source_sha256': hashlib.sha256(outputs_path.read_bytes()).hexdigest(),
        'method': '/testforblock 8 199 8 <block> against a stone block, one per palette block '
                  '(default state); commands.testforblock.failed.tile names the expected block',
        'keys': found,
    }
    TABLE.write_text(json.dumps(table, indent=1) + '\n', encoding='utf-8', newline='\n')
    print(f'{len(found)} keys, {len(refused)} blocks the command refused')


def main() -> int:
    if len(sys.argv) != 3 or sys.argv[1] not in ('script', 'table'):
        print(__doc__)
        return 2
    if sys.argv[1] == 'script':
        write_script(Path(sys.argv[2]))
    else:
        write_table(Path(sys.argv[2]))
    return 0


if __name__ == '__main__':
    sys.exit(main())
