"""Where BDS 1.26.51 saves level data, measured by changing it with commands.

Level data is everything a world keeps outside its chunks and players: the
`level.dat` file (8-byte header, then a little-endian NBT compound: clock,
difficulty, game rules, world spawn, weather, ...) and the world LevelDB's
non-chunk records (`scoreboard`, ticking areas, saved structures, mob event
switches, ...). Steps, each on a BDS slot (BDS_RUNTIME, BDS_PORT) and a
harness-owned world, writing into OUT:

    measure OUT [--extractor EXE]
        fresh: a new flat world (seed 12345), started and stopped; snapshot
        set:   console commands give every value a non-default setting (and
               with --extractor, an operator player sets its own state from
               PLAYER_SCRIPT); stop; snapshot
        query: reload with difficulty normal in server.properties; console
               queries of every value (BDS's answers kept); with --extractor
               a second player lists the scores while the first is offline,
               then the first rejoins and asks for its own; stop; snapshot
    clocks OUT
        the set snapshot with level.dat Time 1111 (WorldClocks still 4321):
        which clock BDS loads
    daylock OUT
        a new world: /daylock, stop, snapshot; reload, query, /daylock
        false, stop, snapshot
    import OUT WORLD
        copies a world directory (level.dat + db, written by unastar) into
        the slot, runs the queries, stops (BDS saves it again); snapshot
    dump WORLD
        level.dat and every non-chunk record of a world directory as
        `path: type = value` lines
    fixture OUT
        writes crates/unastar/tests/fixtures/bds_1_26_51_level_data.json
        (level.dat and level-data records BDS saved, the commands that set
        them, BDS's query answers after a reload, and BDS's answers for the
        world unastar wrote)

Snapshots and console logs stay in OUT (outside the repository); only the
fixture is committed.
"""

from __future__ import annotations

import argparse
import json
import queue
import re
import shutil
import struct
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import le_nbt  # noqa: E402
import leveldb_read  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
FIXTURE = REPO / 'crates' / 'unastar' / 'tests' / 'fixtures' / 'bds_1_26_51_level_data.json'
LEVEL = 'level-data'
DAYLOCK_LEVEL = 'level-data-daylock'
IMPORT_LEVEL = 'level-data-import'
WORLD = dict(level_type='FLAT', level_seed=12345, gamemode='creative', difficulty='easy')
RECORD = re.compile(r'^\[\d{4}-\d\d-\d\d \d\d:\d\d:\d\d:\d{3} (INFO|WARN|ERROR)\] (.*)$')

# Every game rule set away from its default (`gamerule` lists them all).
GAME_RULES = [
    'commandblockoutput false', 'dodaylightcycle false', 'doentitydrops false', 'dofiretick false',
    'recipesunlock false', 'dolimitedcrafting true', 'domobloot false', 'domobspawning false',
    'dotiledrops false', 'doweathercycle false', 'drowningdamage false', 'falldamage false',
    'firedamage false', 'keepinventory true', 'mobgriefing false', 'pvp false', 'showcoordinates true',
    'playerwaypoints off', 'showdaysplayed true', 'naturalregeneration false',
    'tntexplodes false', 'sendcommandfeedback false', 'maxcommandchainlength 54321', 'doinsomnia false',
    'commandblocksenabled false', 'randomtickspeed 7', 'doimmediaterespawn true',
    'showdeathmessages false', 'functioncommandlimit 1234', 'spawnradius 3', 'showtags false',
    'freezedamage false', 'respawnblocksexplode false', 'showbordereffect false',
    'showrecipemessages false', 'playerssleepingpercentage 42', 'projectilescanbreakblocks false',
    'tntexplosiondropdecay true',
]

# Console commands of the set step, in order. The ticking area `lvl` keeps
# the chunks the other commands touch loaded.
SET_COMMANDS = [
    'tickingarea add 0 0 0 47 0 47 lvl true',
    '!wait-loaded',
    *[f'gamerule {rule}' for rule in GAME_RULES],
    'difficulty hard',
    'time set 4321',
    'setworldspawn 100 70 -200',
    'weather thunder 12345',
    # Objectives named against creation order, holders scored in an order
    # their scoreboard ids do not follow (FakeB gets id 2 before FakeA's obj1
    # score), and a holder whose only score is reset (FakeD).
    'scoreboard objectives add obj2 dummy',
    'scoreboard objectives add obj1 dummy "Objective One"',
    'scoreboard objectives add obj3 dummy third',
    'scoreboard players set FakeA obj2 99',
    'scoreboard players set FakeB obj1 -7',
    'scoreboard players set FakeA obj1 5',
    'scoreboard players set "Fake C" obj3 2147483647',
    'scoreboard players set FakeD obj3 1',
    'scoreboard players reset FakeD',
    'summon cow 20 -60 20',
    'scoreboard players set @e[type=cow,x=20,y=-60,z=20,r=3] obj1 13',
    'scoreboard objectives setdisplay belowname obj3',
    'scoreboard objectives setdisplay list obj2 descending',
    'scoreboard objectives setdisplay sidebar obj1 ascending',
    'tickingarea add circle 100 70 100 2 circlearea',
    'tickingarea add 200 0 200 200 0 200',
    'tickingarea add -20 0 -20 -1 0 -1 negative false',
    'execute in nether run tickingarea add 0 0 0 0 0 0 netherarea',
    'setblock 2 -60 2 stone',
    'setblock 3 -60 2 oak_log ["pillar_axis"="x"]',
    'setblock 2 -59 3 glass',
    'structure save s1 2 -60 2 4 -58 4 false disk true',
    'structure save ns:s2 2 -60 2 3 -60 3 false memory true',
    'structure save s3 2 -60 2 2 -60 2',
    'mobevent minecraft:pillager_patrols_event false',
    'mobevent minecraft:ender_dragon_event false',
    'mobevent events_enabled false',
]

# Console queries of every value, after a reload.
QUERY_COMMANDS = [
    '!wait-loaded',
    'gamerule',
    'time query daytime',
    'time query gametime',
    'time query day',
    'weather query',
    'scoreboard objectives list',
    'scoreboard players list',
    'scoreboard players list *',
    'scoreboard players list FakeA',
    'tickingarea list all-dimensions',
    'mobevent events_enabled',
    'mobevent minecraft:pillager_patrols_event',
    'mobevent minecraft:wandering_trader_event',
    'mobevent minecraft:ender_dragon_event',
    'structure load s1 10 -60 10',
    'testforblock 10 -60 10 stone',
    'testforblock 11 -60 10 oak_log ["pillar_axis"="x"]',
    'testforblock 10 -59 11 glass',
    'structure load ns:s2 10 -60 20',
    'structure load s3 10 -60 30',
    'testforblock 10 -60 30 stone',
    'scoreboard players list FakeD',
]

# The player's self-signed id: BDS keys an offline player's record by it, so
# the same id finds the same record in a later session.
PLAYER_UUID = '6c0ffee0-5eed-4a11-9d00-0000000000a1'

# A second player, who lists the scores while BDSExtractor is offline.
VIEWER_UUID = '6c0ffee0-5eed-4a11-9d00-0000000000b2'
VIEWER_SCRIPT = [
    '/scoreboard players list',
    '/scoreboard players list *',
]

# What the operator player asks after a reload (bds-extractor --commands).
PLAYER_QUERY_SCRIPT = [
    '/scoreboard players list @s',
    '/tag @s list',
    '/scoreboard players list',
]

# What an operator player sets on itself (bds-extractor --commands), with
# sendcommandfeedback off; the first line fails (no such objective).
PLAYER_SCRIPT = [
    '/scoreboard players set @s nosuch 1',
    '/scoreboard players set @s obj1 21',
    '/scoreboard players set @s obj2 -3',
    '/tag @s add red',
    '/tag @s add blue',
    '/spawnpoint @s 10 -60 12',
    '/xp 30L @s',
    '/xp 7 @s',
    '/give @s diamond 17',
    '/give @s iron_sword 1',
    '/replaceitem entity @s slot.armor.head 0 iron_helmet',
    '/replaceitem entity @s slot.weapon.offhand 0 torch 5',
    '/effect @s speed 300 1',
    '/effect @s regeneration 200 0 true',
    '/recipe give @s minecraft:stick',
    '/gamemode survival',
    '/tp @s 5.5 -60 6.5 90 20',
]


class Console:
    """BDS console records, one per answer (continuation lines joined)."""

    def __init__(self, bds: bds_runtime.Bds):
        self.bds, self.n = bds, 0
        self.pending: list[str] = []

    def _line(self, timeout: float):
        try:
            line = self.bds.lines.get(timeout=timeout)
        except queue.Empty:
            if self.bds.process.poll() is not None:
                raise RuntimeError('BDS exited')
            return None
        self.bds.history.append(line)
        return line

    def records(self, command: str, timeout: float = 30.0) -> list[dict]:
        """Sends `command`, then an unknown marker command; every record BDS
        printed before the marker's answer."""
        self.n += 1
        marker = f'level_data_marker_{self.n}'
        self.bds.command(command)
        self.bds.command(marker)
        lines, deadline = [], time.monotonic() + timeout
        while time.monotonic() < deadline:
            line = self._line(0.2)
            if line is None:
                continue
            if marker in line:
                return group(lines)
            lines.append(line)
        raise TimeoutError(f'no answer to {command!r}')

    def wait_loaded(self, blocks, timeout: float = 120.0) -> None:
        """Polls /testforblock until BDS can read every (x, y, z)."""
        pending, deadline = list(blocks), time.monotonic() + timeout
        while pending:
            if time.monotonic() > deadline:
                raise TimeoutError(f'never loaded: {pending}')
            x, y, z = pending[0]
            text = ' '.join(r['text'] for r in self.records(f'testforblock {x} {y} {z} bedrock'))
            if 'Successfully' in text or 'The block at' in text:
                pending.pop(0)
            else:
                time.sleep(1)


def group(lines: list[str]) -> list[dict]:
    out = []
    for line in lines:
        match = RECORD.match(line)
        if match:
            out.append({'level': match[1], 'text': match[2]})
        elif out:
            out[-1]['text'] += '\n' + line
    return strip_banners(out)


def strip_banners(records: list[dict]) -> list[dict]:
    """Drops the notices BDS prints once after starting (`==== TRANSPORT
    TYPE ERROR ====` ... `=====`), which land in the first command's answer."""
    out, inside = [], False
    for record in records:
        text = record['text']
        if not inside and text.startswith('================ ') and text.rstrip('=').strip():
            inside = True
            continue
        if inside:
            if text and set(text) == {'='}:
                inside = False
            continue
        out.append(record)
    return out


LOADED = [(x, -64, z) for x in (0, 40) for z in (0, 40)]


def run_commands(con: Console, commands: list[str]) -> list[dict]:
    out = []
    for command in commands:
        if command == '!wait-loaded':
            con.wait_loaded(LOADED)
            continue
        records = con.records(command)
        out.append({'command': command, 'records': records})
        print(f'{command} -> {[r["text"] for r in records]}')
    return out


def start(level: str, log: Path, **properties) -> bds_runtime.Bds:
    """BDS on `level`. Every session makes players operators, so the world
    BDS creates in its first session keeps that permission level."""
    values = {**WORLD, 'default_player_permission_level': 'operator', **properties}
    return bds_runtime.Bds(bds_runtime.base_properties(level, **values), log, endstone=False)


def snapshot(world: Path, to: Path) -> None:
    if to.exists():
        shutil.rmtree(to)
    shutil.copytree(world, to)
    print('snapshot', to)


def run_player(extractor: Path, out: Path, name: str, script: list[str], *, player: str = 'BDSExtractor',
               uuid: str = '') -> int:
    """An operator player (bds-extractor; BDSExtractor unless `player`)
    runs `script` and leaves; its capture goes to OUT/NAME."""
    path = out / f'{name}-script.txt'
    path.write_text('\n'.join(script) + '\n', encoding='utf-8')
    command = [str(extractor), '--addr', bds_runtime.ADDR, '--name', player, '--timeout', '120',
               '--capture-dir', str(out / name), '--log', 'info', '--commands', str(path),
               '--command-wait', '600', '--uuid', uuid or PLAYER_UUID]
    with (out / f'{name}-extractor.log').open('w', encoding='utf-8') as log:
        code = subprocess.run(command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT, timeout=600).returncode
    print(f'{name}: extractor exit {code}')
    time.sleep(3)
    return code


def measure(out: Path, extractor: Path | None) -> None:
    """The query session starts with difficulty `normal` in
    server.properties (the set session leaves `hard` in level.dat)."""
    world = bds_runtime.SERVER / 'worlds' / LEVEL
    if world.exists():
        shutil.rmtree(world)
    try:
        with start(LEVEL, out / 'fresh.log'):
            pass
        snapshot(world, out / 'fresh')
        with start(LEVEL, out / 'set.log') as bds:
            answers = run_commands(Console(bds), SET_COMMANDS)
            (out / 'set.json').write_text(json.dumps(answers, indent=1) + '\n', encoding='utf-8')
            if extractor is not None:
                run_player(extractor, out, 'player', PLAYER_SCRIPT)
        snapshot(world, out / 'set')
        with start(LEVEL, out / 'query.log', difficulty='normal') as bds:
            con = Console(bds)
            answers = run_commands(con, QUERY_COMMANDS)
            (out / 'query.json').write_text(json.dumps(answers, indent=1) + '\n', encoding='utf-8')
            if extractor is not None:
                # Players see command answers only with this rule on.
                run_commands(con, ['gamerule sendcommandfeedback true'])
                # A second player lists the scores while the first is offline.
                run_player(extractor, out, 'viewer', VIEWER_SCRIPT, player='Viewer', uuid=VIEWER_UUID)
                run_player(extractor, out, 'player-query', PLAYER_QUERY_SCRIPT)
        snapshot(world, out / 'query')
    finally:
        if world.exists():
            shutil.rmtree(world)


def daylock(out: Path) -> None:
    world = bds_runtime.SERVER / 'worlds' / DAYLOCK_LEVEL
    if world.exists():
        shutil.rmtree(world)
    try:
        with start(DAYLOCK_LEVEL, out / 'daylock-fresh.log'):
            pass
        snapshot(world, out / 'daylock-fresh')
        with start(DAYLOCK_LEVEL, out / 'daylock.log') as bds:
            answers = run_commands(Console(bds), ['time set 100', 'daylock'])
        snapshot(world, out / 'daylock-locked')
        with start(DAYLOCK_LEVEL, out / 'daylock2.log') as bds:
            answers += run_commands(Console(bds), ['time query daytime', 'gamerule', 'daylock false'])
        snapshot(world, out / 'daylock-unlocked')
        (out / 'daylock.json').write_text(json.dumps(answers, indent=1) + '\n', encoding='utf-8')
    finally:
        if world.exists():
            shutil.rmtree(world)


def clock_source(out: Path) -> None:
    """Which clock BDS loads: the measured world (OUT/set) with level.dat
    `Time` changed to 1111 while the `WorldClocks` record still holds 4321."""
    world = bds_runtime.SERVER / 'worlds' / IMPORT_LEVEL
    if world.exists():
        shutil.rmtree(world)
    try:
        shutil.copytree(out / 'set', world)
        version, root = read_level_dat(world / 'level.dat')
        root[1][:] = [(name, (le_nbt.LONG, 1111) if name == 'Time' else tag) for name, tag in root[1]]
        body = le_nbt.write('', root)
        (world / 'level.dat').write_bytes(struct.pack('<ii', version, len(body)) + body)
        with start(IMPORT_LEVEL, out / 'clocks.log') as bds:
            answers = run_commands(Console(bds), ['time query daytime', 'time query gametime'])
        (out / 'clocks.json').write_text(json.dumps(answers, indent=1) + '\n', encoding='utf-8')
        snapshot(world, out / 'clocks-after')
    finally:
        if world.exists():
            shutil.rmtree(world)


def import_world(out: Path, source: Path) -> None:
    world = bds_runtime.SERVER / 'worlds' / IMPORT_LEVEL
    if world.exists():
        shutil.rmtree(world)
    try:
        shutil.copytree(source, world)
        snapshot(world, out / 'import-before')
        with start(IMPORT_LEVEL, out / 'import.log') as bds:
            answers = run_commands(Console(bds), QUERY_COMMANDS)
            (out / 'import.json').write_text(json.dumps(answers, indent=1) + '\n', encoding='utf-8')
        snapshot(world, out / 'import-after')
    finally:
        if world.exists():
            shutil.rmtree(world)


# --- decoding --------------------------------------------------------------

def read_level_dat(path: Path) -> tuple[int, tuple]:
    """(storage version, root compound) of a level.dat; the header's length
    must cover exactly the NBT that follows."""
    data = path.read_bytes()
    version, length = struct.unpack_from('<ii', data, 0)
    if length != len(data) - 8:
        raise ValueError(f'{path}: header length {length}, {len(data) - 8} bytes follow')
    name, root, end = le_nbt.read(data, 8)
    if end != len(data) or name != '':
        raise ValueError(f'{path}: root {name!r} ends at {end} of {len(data)}')
    return version, root


def is_chunk_key(key: bytes) -> bool:
    """Chunk records: x, z (and a dimension) then a tag byte (and a y)."""
    if len(key) in (9, 10, 13, 14):
        tag = key[8] if len(key) in (9, 10) else key[12]
        return 0x2B <= tag <= 0x41 or tag == 0x76
    return False


def level_records(db: dict[bytes, bytes]) -> dict[bytes, bytes]:
    return {key: value for key, value in db.items()
            if not is_chunk_key(key) and not key.startswith((b'digp', b'actorprefix'))}


def decode_record(value: bytes):
    """Rooted NBT values of a record (one or several), else None."""
    try:
        return le_nbt.read_all(value)
    except (ValueError, struct.error, UnicodeDecodeError):
        return None


def dump(world: Path) -> list[str]:
    lines = []
    if (world / 'level.dat').exists():
        version, root = read_level_dat(world / 'level.dat')
        lines.append(f'level.dat storage version {version}')
        lines += [f'  {line}' for line in le_nbt.flat(root)]
    db = leveldb_read.read_db(world / 'db')
    chunks = sum(1 for key in db if is_chunk_key(key))
    actors = sum(1 for key in db if key.startswith((b'digp', b'actorprefix')))
    lines.append(f'db: {len(db)} keys, {chunks} chunk records, {actors} actor records')
    for key, value in sorted(level_records(db).items()):
        decoded = decode_record(value)
        lines.append(f'key {key!r} ({len(value)} bytes)')
        if decoded is None:
            lines.append(f'  raw {value[:64].hex()}{"..." if len(value) > 64 else ""}')
            continue
        for name, tag in decoded:
            lines += [f'  {line}' for line in le_nbt.flat(tag)]
    return lines


# --- fixture -----------------------------------------------------------------

def kept_records(db: dict[bytes, bytes]) -> dict[str, str]:
    """The level records unastar reads, and the player records, by key."""
    keep = (b'scoreboard', b'mobevents', b'WorldClocks')
    prefixes = (b'tickingarea_', b'structuretemplate_', b'player_')
    return {key.decode(): value.hex() for key, value in sorted(db.items())
            if key in keep or key.startswith(prefixes)}


def world_files(world: Path) -> dict:
    return {
        'level.dat': (world / 'level.dat').read_bytes().hex(),
        'records': kept_records(leveldb_read.read_db(world / 'db')),
    }


def outputs(capture: Path) -> list[dict]:
    """A player's CommandOutput per scripted command: its output type,
    success count and messages (`output` is null when none came)."""
    out = []
    for step in json.loads((capture / 'command-outputs.json').read_text(encoding='utf-8')):
        packets = [packet['data']['output'] for packet in step['packets']
                   if packet['packet'] == 'CommandOutputPacket']
        output = None
        if packets:
            output = {'type': packets[0]['output_type'], 'success_count': packets[0]['success_count']}
        messages = [{'key': m['message_id'], 'params': m['parameters'], 'success': m['successful']}
                    for packet in packets for m in packet['output_messages']]
        out.append({'command': step['command'], 'output': output, 'messages': messages})
    return out


def answers(path: Path) -> list[dict]:
    """A step's console answers, startup notices dropped."""
    return [{'command': step['command'], 'records': strip_banners(step['records'])}
            for step in json.loads(path.read_text(encoding='utf-8'))]


def join_packets(capture: Path) -> dict:
    """What the rejoining player was sent about the level: its StartGame's
    level fields and the scoreboard packets (hex, header included)."""
    start = json.loads(next((capture / 'join').glob('*-StartGamePacket.json')).read_text(encoding='utf-8'))
    packets = []
    for line in (capture / 'packets.jsonl').read_text(encoding='utf-8').splitlines():
        record = json.loads(line)
        if record['dir'] == 's2c' and record['name'] in ('SetDisplayObjectivePacket', 'SetScorePacket'):
            packets.append(record['hex'])
        if record['dir'] == 'c2s' and record['name'] == 'CommandRequestPacket':
            break
    return {
        'start_game': {
            'level_current_time': start['level_current_time'],
            'day_cycle_stop_time': start['settings']['day_cycle_stop_time'],
            'default_spawn_block_position': start['settings']['default_spawn_block_position'],
        },
        'scoreboard_packets': packets,
    }


def fixture(out: Path) -> None:
    data = {
        'source': 'BDS 1.26.51: a flat world (seed 12345) given non-default level data with console '
                  'commands (set) and an operator player (player_script), saved by a graceful stop, then '
                  'reloaded and asked from the console (query) and by a second player while the first was '
                  'offline (viewer); import: the world unastar wrote (unastar_world), opened by BDS and '
                  'asked the same queries',
        'generator': 'tools/bds-sweep/level_data.py',
        'set': answers(out / 'set.json'),
        'player_script': PLAYER_SCRIPT,
        'saved': world_files(out / 'set'),
        'query': answers(out / 'query.json'),
        'player_set': outputs(out / 'player'),
        'viewer': outputs(out / 'viewer'),
        'player_query': outputs(out / 'player-query'),
    }
    data['player_join'] = join_packets(out / 'player-query')
    if (out / 'clocks.json').exists():
        data['clocks'] = {
            'note': 'the saved world with level.dat Time 1111 and WorldClocks still 4321',
            'answers': answers(out / 'clocks.json'),
        }
    if (out / 'import.json').exists():
        data['unastar_world'] = world_files(out / 'import-before')
        data['import'] = answers(out / 'import.json')
    FIXTURE.write_text(json.dumps(data, indent=1) + '\n', encoding='utf-8', newline='\n')
    print('fixture', FIXTURE, FIXTURE.stat().st_size, 'bytes')


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('step', choices=['measure', 'daylock', 'clocks', 'import', 'dump', 'fixture'])
    parser.add_argument('path', type=Path)
    parser.add_argument('world', type=Path, nargs='?')
    parser.add_argument('--extractor', type=Path)
    args = parser.parse_args()
    path = args.path.resolve()
    if args.step == 'dump':
        print('\n'.join(dump(path)))
        return 0
    path.mkdir(parents=True, exist_ok=True)
    if args.step == 'measure':
        measure(path, args.extractor)
    elif args.step == 'daylock':
        daylock(path)
    elif args.step == 'clocks':
        clock_source(path)
    elif args.step == 'import':
        if args.world is None:
            parser.error('import needs a WORLD directory')
        import_world(path, args.world.resolve())
    else:
        fixture(path)
    return 0


if __name__ == '__main__':
    sys.exit(main())
