"""Protocol probes: players with their own skins and scripts on one fresh
world, to measure what BDS 1.26.51 relays between players.

    python tools/bds-sweep/protocol_probe.py PROBE OUT_DIR [CASE_PREFIX]

Every player is an operator (seed 12345, survival, as `capture.py --pair`);
each player is a `bds-extractor --capture-dir` session under OUT_DIR. Use the
`BDS_RUNTIME`/`BDS_PORT` of a locked slot. The scripts, ClientData files,
console commands (`console.json`, with wall-clock times) and the plan are
written next to the sessions, so a run is self-describing; `started_unix` in
each `session.json` lines the sessions up.

Skins (`client_data`, `variant`; images are deterministic byte patterns):

- `classic`: a classic custom skin with a cape, a custom geometry and an
  animated face (the host);
- `all-pieces`: a persona skin wearing one piece and one tint of every piece
  type, premium and trusted flags (`BDSExtractor2`);
- `floats`: `classic` with a geometry full of float values, base64 animation
  data and an ARGB skin colour (`BDSExtractor3`);
- `odd`: a persona skin with an unknown piece type and tint type, an
  unparsable skin colour and an empty arm size (`BDSExtractor4`).

Probes:

- `relay`: the host quiets the world and waits. `BDSExtractor2` joins; the
  server console then changes permissions and game modes of both players
  (`CONSOLE`) while it waits, and it sends the client packets of
  `GUEST_RELAY` (interactions, actor picks, emotes, animations, book edits,
  serverbound packets BDS may ignore). After it leaves, `BDSExtractor3` and
  `BDSExtractor4` join one after the other and settle.
- `cache`: one session that claims a chunk blob cache (`--blob-cache`) and
  answers nothing, to see what BDS does with it.
- `abilities`: what every player receives when the console changes a
  player's permission in each game mode (`ABILITY_CONSOLE`), and the join
  abilities of players whose default game mode is creative, adventure or
  spectator (`BDSExtractor3`..`5` join after `/defaultgamemode`).
- `ddui`: which `ServerboundDataDrivenScreenClosed` reasons BDS accepts:
  one short session per `DDUI_REASONS` entry that sends the packet once;
  `ddui.json` records whether BDS answered with a packet violation.
- `skins`: which logins BDS accepts: one short session per `SKIN_CASES`
  entry (a persona with a single piece or tint of one type, and the `odd`
  values one at a time), one after the other on one server; `skins.json`
  records whether each player spawned and any packet violation BDS sent.
  `CASE_PREFIX` runs only the cases whose name starts with it.
- `book`: one session with a `PlayerAuthInput` stream (`--input`) that
  selects and opens (uses) a writable book, edits and signs it (`BOOK`); the
  same in creative; the same without opening it (`unopened`); how long an
  opened book stays open (`BOOK_GATE`); then one that tries the page, text
  and title limits (`BOOK_LIMITS`); `/replaceitem` on another
  slot after each step shows what BDS sends back.
- `pick`: one creative session with input on a stone platform: actor picks
  of many actor kinds with and without data, picks with a full hotbar and
  with the egg already held, then block picks with and without data, then
  the same in survival (`PICK`).
"""

from __future__ import annotations

import base64
import contextlib
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import capture  # noqa: E402

GEOMETRY = {
    'format_version': '1.12.0',
    'minecraft:geometry': [{
        'description': {'identifier': 'geometry.probe.custom', 'texture_width': 64, 'texture_height': 64},
        'bones': [{'name': 'body', 'pivot': [0, 24, 0],
                   'cubes': [{'origin': [-4, 12, -2], 'size': [8, 12, 4], 'uv': [16, 16]}]}],
    }],
}

FLOAT_GEOMETRY = {
    'format_version': '1.12.0',
    'minecraft:geometry': [{
        'description': {'identifier': 'geometry.probe.floats', 'texture_width': 64, 'texture_height': 64,
                        'visible_bounds_width': 1.5, 'visible_bounds_height': 2.25,
                        'visible_bounds_offset': [0, 0.75, 0]},
        'bones': [{'name': 'head', 'parent': 'body', 'pivot': [0.5, 24.125, -0.0],
                   'rotation': [1e-7, 100.0, 3.14159], 'mirror': True, 'inflate': 0.1,
                   'cubes': [{'origin': [-4.0, 24, -4], 'size': [8, 8, 8], 'uv': [0, 0]}]},
                  {'name': 'body', 'pivot': [0, 24, 0], 'locators': {'lead': [0, 12345678, 0]}}],
    }],
}

# Every persona piece type BDS accepts in a login (the 2026-11-11 `skins`
# run: `persona_hands`, `persona_bogus` and `bogus_type` refuse the login).
PIECE_TYPES = [
    'persona_skeleton', 'persona_body', 'persona_skin', 'persona_bottom', 'persona_feet', 'persona_dress',
    'persona_top', 'persona_high_pants', 'persona_outerwear', 'persona_facial_hair',
    'persona_mouth', 'persona_eyes', 'persona_hair', 'persona_hood', 'persona_back', 'persona_face_accessory',
    'persona_head', 'persona_legs', 'persona_left_leg', 'persona_right_leg', 'persona_arms', 'persona_left_arm',
    'persona_right_arm', 'persona_capes', 'persona_classic_skin', 'persona_emote',
]

# The emote the guest plays: the "Waving" emote piece.
EMOTE = '4c8ae710-df2e-47cd-814d-cc7bf21a3d67'


def pattern(width: int, height: int, seed: int) -> str:
    """Base64 RGBA pixels: a deterministic pattern, fully opaque."""
    data = bytearray()
    for index in range(width * height):
        data += bytes([(index * 7 + seed) % 256, (index * 13 + seed) % 256, (index + seed * 5) % 256, 255])
    return base64.b64encode(bytes(data)).decode()


def b64(text: str) -> str:
    return base64.b64encode(text.encode()).decode()


def client_data(kind: str) -> dict[str, object]:
    """ClientData fields replacing jolyne's defaults for a `classic` or `persona` skin."""
    common = {
        'CapeData': pattern(64, 32, 3),
        'CapeImageWidth': 64,
        'CapeImageHeight': 32,
        'SkinAnimationData': '{"probe":"animation"}',
        'PlayFabId': '1a2b3c4d5e6f7a8b',
        'ProfileHash': 'probe-profile-hash',
    }
    if kind == 'classic':
        return {
            **common,
            'SkinId': 'c18e65aa-7b21-4637-9b63-8ad63622ef01.Custom',
            'SkinData': pattern(64, 64, 1),
            'SkinGeometryData': b64(json.dumps(GEOMETRY, separators=(',', ':'))),
            'SkinGeometryDataEngineVersion': b64('1.14.0'),
            'SkinResourcePatch': b64('{"geometry":{"default":"geometry.probe.custom"}}'),
            'CapeId': 'probe-classic-cape',
            'CapeOnClassicSkin': True,
            'ArmSize': 'wide',
            'SkinColor': '#b37b62',
            'AnimatedImageData': [{
                'Frames': 2.0, 'Image': pattern(32, 64, 9), 'ImageWidth': 32, 'ImageHeight': 64,
                'Type': 1, 'AnimationExpression': 1,
            }],
        }
    assert kind == 'persona'
    pieces = [
        ('persona_skeleton', 'skeleton'), ('persona_body', 'body'), ('persona_skin', 'skin'),
        ('persona_bottom', 'bottom'), ('persona_top', 'top'), ('persona_eyes', 'eyes'),
        ('persona_mouth', 'mouth'), ('persona_hair', 'hair'),
    ]
    return {
        **common,
        'SkinId': 'persona-probe-skin-id',
        'SkinData': pattern(128, 128, 2),
        'SkinImageWidth': 128,
        'SkinImageHeight': 128,
        'SkinGeometryData': b64(json.dumps(GEOMETRY, separators=(',', ':'))),
        'SkinGeometryDataEngineVersion': b64('1.16.0'),
        'SkinResourcePatch': b64('{"geometry":{"default":"geometry.persona_probe"}}'),
        'CapeId': 'probe-persona-cape',
        'ArmSize': 'slim',
        'SkinColor': '#7f4f3f',
        'PersonaSkin': True,
        'PremiumSkin': True,
        'TrustedSkin': True,
        'PersonaPieces': [{
            'IsDefault': index % 2 == 0,
            'PackId': f'00000000-0000-4000-8000-{index:012d}',
            'PieceId': f'11111111-0000-4000-8000-{index:012d}',
            'PieceType': piece_type,
            'ProductId': '' if index % 2 == 0 else f'product-{name}',
        } for index, (piece_type, name) in enumerate(pieces)],
        'PieceTintColors': [
            {'Colors': ['#ff0b3f7f', '#ff2a4a6b', '#0', '#0'], 'PieceType': 'persona_eyes'},
            {'Colors': ['#ff1f1f1f', '#0', '#0', '#0'], 'PieceType': 'persona_hair'},
        ],
    }


def variant(kind: str) -> dict[str, object]:
    """The `all-pieces`, `floats` and `odd` skins (module docstring)."""
    if kind == 'all-pieces':
        data = client_data('persona')
        data['PersonaPieces'] = [{
            'IsDefault': True, 'PackId': f'00000000-0000-4000-8000-{index:012d}',
            'PieceId': f'22222222-0000-4000-8000-{index:012d}', 'PieceType': piece_type, 'ProductId': '',
        } for index, piece_type in enumerate(PIECE_TYPES)]
        data['PieceTintColors'] = [{'Colors': [f'#ff0000{index:02x}', '#0', '#0', '#0'], 'PieceType': piece_type}
                                   for index, piece_type in enumerate(PIECE_TYPES)]
        return data
    if kind == 'floats':
        data = client_data('classic')
        data['SkinGeometryData'] = b64(json.dumps(FLOAT_GEOMETRY, separators=(',', ':')))
        data['SkinResourcePatch'] = b64('{"geometry":{"default":"geometry.probe.floats"}}')
        data['SkinAnimationData'] = b64('{"probe":"animation"}')
        data['SkinColor'] = '#ffb37b62'
        return data
    assert kind == 'odd'
    data = client_data('persona')
    data['PersonaPieces'] = data['PersonaPieces'][:2] + [{
        'IsDefault': False, 'PackId': '00000000-0000-4000-8000-0000000000ff',
        'PieceId': '33333333-0000-4000-8000-0000000000ff', 'PieceType': 'persona_bogus', 'ProductId': '',
    }]
    data['PieceTintColors'] = [{'Colors': ['#ff112233', '#0', '#0', '#0'], 'PieceType': 'bogus_type'},
                               {'Colors': ['#ff445566', '#0', '#0', '#0'], 'PieceType': 'persona_eyes'}]
    data['SkinColor'] = 'not-a-colour'
    data['ArmSize'] = ''
    return data


HOST_RELAY = """\
# Quiet the world: no spawns, no random ticks, fixed time and weather.
/gamerule domobspawning false
/gamerule randomtickspeed 0
/gamerule dodaylightcycle false
/gamerule doweathercycle false
/kill @e[type=!player]
!wait 240000
"""

# Console commands after BDSExtractor2 has spawned: (seconds after its spawn, command).
CONSOLE = [
    (8, 'deop BDSExtractor2'), (11, 'op BDSExtractor2'),
    (14, 'gamemode creative BDSExtractor2'), (17, 'gamemode adventure BDSExtractor2'),
    (20, 'gamemode spectator BDSExtractor2'), (23, 'gamemode survival BDSExtractor2'),
    (26, 'gamemode creative BDSExtractor'), (29, 'gamemode survival BDSExtractor'),
    (32, 'deop BDSExtractor'), (35, 'op BDSExtractor'),
    (38, 'summon minecraft:cow 103 ~ -25'),
]

GUEST_RELAY = f"""\
# The console changes permissions and game modes (CONSOLE) during this wait.
!wait 42000
!interact mouseover peer
!interact mouseover peer 1.5 2.5 3.5
!interact mouseover 0
!interact stop-riding self
!interact stop-riding peer
!interact npc-open peer
!pick-actor peer
!pick-actor peer 9 data
!pick-actor actor
!pick-actor actor 9 data
/gamemode creative @s
!pick-actor peer
!pick-actor actor
!pick-actor actor 9 data
/gamemode survival @s
!emote {EMOTE}
!emote {EMOTE} 2 40
!emote-list {EMOTE}
!animate swing
!animate wake-up
!animate critical-hit
!animate magic-critical-hit
/replaceitem entity @s slot.hotbar 0 writable_book
!book 0 replace 0 first page
!book 0 add 1 second page
!book 0 replace 3 fourth page
/replaceitem entity @s slot.hotbar 8 stone
!book 0 swap 0 1
!book 0 delete 1
/replaceitem entity @s slot.hotbar 8 dirt
!book 0 replace 0 last words
!book 0 sign ProbeTitle ProbeAuthor
/replaceitem entity @s slot.hotbar 8 stone
!party-changed
!party-changed probe-party true
!packs-ready
!ddui-closed 1 clientcanceled
!data-store probe_store probe_property probe_path 1
!cookie probe-cookie true
!blob-status missing=1 found=2
!wait 3000
"""

# Quiet world for the scripted single-session probes.
QUIET = """/gamerule domobspawning false
/gamerule dodaylightcycle false
/gamerule doweathercycle false
/gamerule randomtickspeed 0
/kill @e[type=!player]
"""

BOOK = QUIET + """/replaceitem entity @s slot.hotbar 0 writable_book
!wait 1000
!equip 0
!use-item 0
!book 0 replace 0 first page
!book 0 add 1 second page
/replaceitem entity @s slot.hotbar 8 stone
!book 0 replace 3 fourth page
/replaceitem entity @s slot.hotbar 8 dirt
!book 0 add 6 far page
/replaceitem entity @s slot.hotbar 8 stone
!book 0 swap 0 1
/replaceitem entity @s slot.hotbar 8 dirt
!book 0 delete 1
/replaceitem entity @s slot.hotbar 8 stone
!book 3 replace 0 no book here
/replaceitem entity @s slot.hotbar 8 dirt
!book 0 sign ProbeTitle ProbeAuthor
/replaceitem entity @s slot.hotbar 8 stone
!book 0 replace 0 after signing
/replaceitem entity @s slot.hotbar 8 dirt
!wait 1000
"""

BOOK_LIMITS = QUIET + """/replaceitem entity @s slot.hotbar 0 writable_book
!wait 1000
!equip 0
!use-item 0
!book 0 replace 0 {page_256}
/replaceitem entity @s slot.hotbar 8 stone
!book 0 replace 1 {page_257}
/replaceitem entity @s slot.hotbar 8 dirt
!book 0 replace 49 page fifty
/replaceitem entity @s slot.hotbar 8 stone
!book 0 replace 50 page fifty-one
/replaceitem entity @s slot.hotbar 8 dirt
!book 0 add 0 inserted first
/replaceitem entity @s slot.hotbar 8 stone
!book 0 delete 60
/replaceitem entity @s slot.hotbar 8 dirt
!book 0 swap 0 55
/replaceitem entity @s slot.hotbar 8 stone
!book 0 sign {title_17} ProbeAuthor
/replaceitem entity @s slot.hotbar 8 dirt
!book 0 sign {title_16} ProbeAuthor
/replaceitem entity @s slot.hotbar 8 stone
!wait 1000
""".format(page_256='a' * 256, page_257='b' * 257, title_17='T' * 17, title_16='U' * 16)

# How long an opened book stays open: after selecting another slot, after
# selecting it again, after a new book replaced the signed one, and for a
# book opened in another slot.
BOOK_GATE = QUIET + """/replaceitem entity @s slot.hotbar 0 writable_book
/replaceitem entity @s slot.hotbar 1 writable_book
!wait 1000
!equip 0
!use-item 0
!book 0 replace 0 opened
!equip 1
!book 0 replace 0 other slot selected
!book 1 replace 0 never opened
!equip 0
!book 0 replace 0 selected again
!book 0 sign GateTitle GateAuthor
/replaceitem entity @s slot.hotbar 0 writable_book
!book 0 replace 0 new book
!equip 1
!use-item 1
!book 1 replace 0 opened second
!book 0 replace 0 first after second opened
!wait 1000
"""

# Actors picked in `pick`: one per kind of spawn egg and actors without one.
PICK_ACTORS = ['cow', 'zombie', 'villager_v2', 'sheep', 'armor_stand', 'minecart', 'chest_minecart',
               'boat', 'chest_boat', 'ender_crystal', 'tnt', 'snow_golem', 'iron_golem', 'wither',
               'ender_dragon', 'xp_orb', 'arrow', 'leash_knot', 'trader_llama', 'zombie_horse']

# Blocks placed around the platform for block picks: (x, z, setblock state).
PICK_BLOCKS = [(2, 0, 'chest'), (-2, 0, 'oak_log ["pillar_axis"="x"]'), (0, 2, 'red_wool'),
               (0, -2, 'furnace'), (2, 2, 'oak_stairs ["weirdo_direction"=2]'),
               (-2, -2, 'bed'), (2, -2, 'flower_pot'), (-2, 2, 'oak_sign')]


def pick_script() -> str:
    """The `pick` script: actor picks, then block picks, in creative then
    survival, on a platform at y 100 (blocks at y 101)."""
    # The platform's chunks load once the player is there.
    lines = [QUIET.rstrip('\n'), '/tp @s 0 101 0', '!wait 3000', '/fill -4 100 -4 4 100 4 stone',
             '/tp @s 0 101 0', '/gamemode creative @s', '/clear @s', '!wait 1000']
    for actor in PICK_ACTORS:
        lines += [f'/summon {actor} 0 101 3', '!wait 500', '!pick-actor actor', '!pick-actor actor 9 data',
                  '/kill @e[type=!player]', '/clear @s', '!wait 300']
    # A full hotbar, then the egg already held in slot 4 (not selected).
    lines += [f'/replaceitem entity @s slot.hotbar {slot} stone' for slot in range(9)]
    lines += ['/summon cow 0 101 3', '!wait 500', '!pick-actor actor', '/clear @s',
              '/replaceitem entity @s slot.hotbar 4 cow_spawn_egg', '!pick-actor actor',
              '!pick-actor actor 3', '/kill @e[type=!player]', '/clear @s']
    lines += [f'/setblock {x} 101 {z} {block}' for x, z, block in PICK_BLOCKS]
    lines += ['/replaceitem block 2 101 0 slot.container 0 diamond 5', '!wait 500']
    for x, z, _ in PICK_BLOCKS:
        lines += [f'!pick-block {x} 101 {z}', f'!pick-block {x} 101 {z} 9 data', '/clear @s']
    lines += ['!pick-block 30 101 30', '!pick-block 0 101 0', '/gamemode survival @s',
              '/replaceitem entity @s slot.inventory 0 chest', '/replaceitem entity @s slot.hotbar 6 red_wool',
              '/summon cow 0 101 3', '!wait 500', '!pick-actor actor', '!pick-block 2 101 0',
              '!pick-block 2 101 0 9 data', '!pick-block 0 101 2', '!wait 1000']
    return '\n'.join(lines) + '\n'


def single(out: Path, extractor: Path, scripts: dict[str, str]) -> int:
    """One operator session per script, one after the other, each with a
    PlayerAuthInput stream."""
    code = 0
    for name, script in scripts.items():
        code = code or capture.session(out / name, extractor, 'operator', False, 600,
                                       commands=write(out / f'{name}-commands.txt', script),
                                       player_input=True)
    return code


CACHE = """\
!wait 5000
!blob-status missing=1 found=2
!wait 5000
"""


def write(path: Path, text: str) -> Path:
    path.write_text(text, encoding='utf-8', newline='\n')
    return path


def write_json(path: Path, value: object) -> Path:
    path.write_text(json.dumps(value, indent=1) + '\n', encoding='utf-8', newline='\n')
    return path


# (seconds after BDSExtractor2 spawned, console command) for `abilities`.
ABILITY_CONSOLE = [(6 + 3 * index, command) for index, command in enumerate([
    command
    for mode in ('creative', 'adventure', 'spectator', 'survival')
    for command in (f'gamemode {mode} BDSExtractor2', 'deop BDSExtractor2', 'op BDSExtractor2')
])]


def abilities(out: Path, extractor: Path) -> int:
    """The `abilities` probe (module docstring)."""
    properties = bds_runtime.base_properties(capture.LEVEL, default_player_permission_level='operator',
                                             max_players=6, **capture.WORLD)
    host_script = write(out / 'commands.txt', HOST_RELAY)
    wait = write(out / 'guest-commands.txt', '!wait 45000\n')
    sessions = {'host': capture.extractor_command(extractor, out / 'host', commands=host_script),
                'guest': capture.extractor_command(extractor, out / 'guest', name='BDSExtractor2', settle=3,
                                                   commands=wait)}
    joins = [('creative', 'BDSExtractor3'), ('adventure', 'BDSExtractor4'), ('spectator', 'BDSExtractor5')]
    for mode, name in joins:
        sessions[mode] = capture.extractor_command(extractor, out / mode, name=name, settle=3)
    console_log: list[dict[str, object]] = []
    processes: dict[str, subprocess.Popen] = {}
    codes: dict[str, int] = {}
    with bds_runtime.Bds(properties, out / 'bds.log', endstone=False) as bds, contextlib.ExitStack() as logs:
        def start(name: str) -> subprocess.Popen:
            (out / name).mkdir()
            capture.describe(out / name, properties, sessions[name], console=ABILITY_CONSOLE)
            log = logs.enter_context((out / name / 'extractor.log').open('w', encoding='utf-8'))
            processes[name] = subprocess.Popen(sessions[name], cwd=capture.REPO, stdout=log,
                                               stderr=subprocess.STDOUT)
            return processes[name]

        def console(command: str) -> None:
            console_log.append({'unix': round(time.time(), 3), 'command': command})
            bds.command(command)

        try:
            start('host')
            bds.wait_for(r'Player Spawned: BDSExtractor ', 120)
            time.sleep(10)
            start('guest')
            bds.wait_for(r'Player Spawned: BDSExtractor2 ', 120)
            spawned = time.time()
            for offset, command in ABILITY_CONSOLE:
                time.sleep(max(0.0, spawned + offset - time.time()))
                console(command)
            processes['guest'].wait(timeout=600)
            for mode, _ in joins:
                console(f'defaultgamemode {mode}')
                time.sleep(2)
                start(mode).wait(timeout=300)
            console('defaultgamemode survival')
            codes = {name: process.wait(timeout=600) for name, process in processes.items()}
        finally:
            for process in processes.values():
                if process.poll() is None:
                    process.kill()
            write_json(out / 'console.json', console_log)
    print(f'extractor exits {codes}')
    return next((code for code in codes.values() if code), 0)


def relay(out: Path, extractor: Path) -> int:
    """The `relay` probe (module docstring)."""
    properties = bds_runtime.base_properties(capture.LEVEL, default_player_permission_level='operator',
                                             max_players=4, **capture.WORLD)
    sessions = {
        'host': capture.extractor_command(
            extractor, out / 'host', commands=write(out / 'commands.txt', HOST_RELAY),
            client_data=write_json(out / 'client-data.json', client_data('classic'))),
        'guest': capture.extractor_command(
            extractor, out / 'guest', name='BDSExtractor2', settle=3,
            commands=write(out / 'guest-commands.txt', GUEST_RELAY),
            client_data=write_json(out / 'guest-client-data.json', variant('all-pieces'))),
        'floats': capture.extractor_command(
            extractor, out / 'floats', name='BDSExtractor3', settle=3,
            client_data=write_json(out / 'floats-client-data.json', variant('floats'))),
        'odd': capture.extractor_command(
            extractor, out / 'odd', name='BDSExtractor4', settle=3,
            client_data=write_json(out / 'odd-client-data.json', variant('odd'))),
    }
    console_log: list[dict[str, object]] = []
    processes: dict[str, subprocess.Popen] = {}
    codes: dict[str, int] = {}
    with bds_runtime.Bds(properties, out / 'bds.log', endstone=False) as bds, contextlib.ExitStack() as logs:
        def start(name: str) -> subprocess.Popen:
            (out / name).mkdir()
            capture.describe(out / name, properties, sessions[name], console=CONSOLE)
            log = logs.enter_context((out / name / 'extractor.log').open('w', encoding='utf-8'))
            processes[name] = subprocess.Popen(sessions[name], cwd=capture.REPO, stdout=log,
                                               stderr=subprocess.STDOUT)
            return processes[name]

        try:
            start('host')
            bds.wait_for(r'Player Spawned: BDSExtractor ', 120)
            time.sleep(12)
            start('guest')
            bds.wait_for(r'Player Spawned: BDSExtractor2 ', 120)
            spawned = time.time()
            for offset, command in CONSOLE:
                time.sleep(max(0.0, spawned + offset - time.time()))
                console_log.append({'unix': round(time.time(), 3), 'command': command})
                bds.command(command)
            processes['guest'].wait(timeout=600)
            for name in ('floats', 'odd'):
                start(name).wait(timeout=300)
            codes = {name: process.wait(timeout=600) for name, process in processes.items()}
        finally:
            for process in processes.values():
                if process.poll() is None:
                    process.kill()
            write_json(out / 'console.json', console_log)
    print(f'extractor exits {codes}')
    return next((code for code in codes.values() if code), 0)


def skin_cases() -> dict[str, dict[str, object]]:
    """Login variants for the `skins` probe, by name."""
    cases: dict[str, dict[str, object]] = {}
    base = client_data('persona')
    for piece_type in [*PIECE_TYPES, 'persona_bogus']:
        piece = {'IsDefault': True, 'PackId': '00000000-0000-4000-8000-000000000001',
                 'PieceId': '22222222-0000-4000-8000-000000000001', 'PieceType': piece_type, 'ProductId': ''}
        cases[f'piece-{piece_type}'] = {**base, 'PersonaPieces': [piece], 'PieceTintColors': []}
        cases[f'tint-{piece_type}'] = {**base, 'PieceTintColors': [
            {'Colors': ['#ff112233', '#0', '#0', '#0'], 'PieceType': piece_type}]}
    cases['tint-bogus_type'] = {**base, 'PieceTintColors': [
        {'Colors': ['#ff112233', '#0', '#0', '#0'], 'PieceType': 'bogus_type'}]}
    cases['colour-unparsable'] = {**base, 'SkinColor': 'not-a-colour'}
    cases['colour-argb'] = {**base, 'SkinColor': '#ffb37b62'}
    cases['arm-empty'] = {**base, 'ArmSize': ''}
    cases['floats'] = variant('floats')
    numbers = client_data('classic')
    numbers['SkinGeometryData'] = b64(
        '{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.probe.numbers",'
        '"texture_width":64,"texture_height":64},"bones":[{"name":"body","pivot":[0,24,0],"locators":{'
        f'"numbers":[{NUMBERS}]'
        '}}]}]}')
    cases['numbers'] = numbers
    return cases


# Number literals whose re-serialisation the `numbers` case measures.
NUMBERS = ('0.3,0.333333333333333314829616256247,0.0001,0.00001,0.000123,1e-5,2.5e-5,1e-4,0.001,'
           '123456789.5,1e15,1e16,1e17,1e20,1.5e300,-2.5,-0.001,65536.0,4294967296.0,1e21,0.1e1,5E-1,'
           '1.0000000000000002,9007199254740993,-9223372036854775808,18446744073709551615,1e400')


# Close reasons tried by the `ddui` probe: the three BDS names in its
# violation message, then guesses for the two it leaves out.
DDUI_REASONS = ['programmaticclose', 'programmaticcloseall', 'clientcanceled', 'userbusy', 'invalidform',
                'servershutdown', 'clientclosed', 'timeout', 'replaced', 'unknown', 'none', 'busy',
                'clientcancelled', 'ProgrammaticClose', '']


def ddui(out: Path, extractor: Path) -> int:
    """The `ddui` probe (module docstring)."""
    properties = bds_runtime.base_properties(capture.LEVEL, default_player_permission_level='operator',
                                             **capture.WORLD)
    results = []
    with bds_runtime.Bds(properties, out / 'bds.log', endstone=False):
        for index, reason in enumerate(DDUI_REASONS):
            session = out / f'reason-{index:02}'
            session.mkdir()
            script = write(session / 'commands.txt', f'!wait 500\n!ddui-closed 7 {reason}\n!wait 1500\n')
            command = capture.extractor_command(extractor, session, name=f'Ddui{index}', settle=1, commands=script)
            with (session / 'extractor.log').open('w', encoding='utf-8') as log:
                code = subprocess.run(command, cwd=capture.REPO, stdout=log, stderr=subprocess.STDOUT,
                                      timeout=120).returncode
            violation = None
            for line in (session / 'packets.jsonl').read_text(encoding='utf-8').splitlines():
                record = json.loads(line)
                if record['name'] == 'PacketViolationWarningPacket':
                    violation = record['hex']
            results.append({'reason': reason, 'exit': code, 'violation_hex': violation})
            print(repr(reason), code, 'violation' if violation else '')
    write_json(out / 'ddui.json', results)
    return 0


def skins(out: Path, extractor: Path, only: str = '') -> int:
    """The `skins` probe (module docstring)."""
    properties = bds_runtime.base_properties(capture.LEVEL, default_player_permission_level='operator',
                                             **capture.WORLD)
    results = []
    with bds_runtime.Bds(properties, out / 'bds.log', endstone=False):
        for index, (name, data) in enumerate(skin_cases().items()):
            if not name.startswith(only):
                continue
            session = out / name
            session.mkdir()
            command = capture.extractor_command(extractor, session, name=f'Skin{index}', settle=1,
                                                client_data=write_json(session / 'client-data.json', data))
            with (session / 'extractor.log').open('w', encoding='utf-8') as log:
                code = subprocess.run(command, cwd=capture.REPO, stdout=log, stderr=subprocess.STDOUT,
                                      timeout=120).returncode
            violation = None
            for line in (session / 'packets.jsonl').read_text(encoding='utf-8').splitlines():
                record = json.loads(line)
                if record['name'] == 'PacketViolationWarningPacket':
                    violation = record['hex']
            results.append({'case': name, 'exit': code, 'violation_hex': violation})
            print(name, code, 'violation' if violation else '')
    write_json(out / 'skins.json', results)
    return 0


def main() -> int:
    if len(sys.argv) not in (3, 4) or sys.argv[1] not in ('relay', 'cache', 'skins', 'abilities', 'ddui',
                                                           'book', 'pick'):
        print(__doc__)
        return 2
    probe, out = sys.argv[1], Path(sys.argv[2]).resolve()
    extractor = capture.REPO / 'target' / 'debug' / 'bds-extractor.exe'
    out.mkdir(parents=True)
    world = bds_runtime.SERVER / 'worlds' / capture.LEVEL
    if world.exists():
        shutil.rmtree(world)
    try:
        if probe == 'relay':
            code = relay(out, extractor)
        elif probe == 'ddui':
            code = ddui(out, extractor)
        elif probe == 'abilities':
            code = abilities(out, extractor)
        elif probe == 'book':
            code = single(out, extractor, {'book': BOOK, 'creative': '/gamemode creative @s\n' + BOOK,
                                           'unopened': BOOK.replace('!use-item 0\n', ''), 'gate': BOOK_GATE,
                                           'limits': BOOK_LIMITS})
        elif probe == 'pick':
            code = single(out, extractor, {'pick': pick_script()})
        elif probe == 'skins':
            code = skins(out, extractor, sys.argv[3] if len(sys.argv) == 4 else '')
        else:
            code = capture.session(out / 'host', extractor, 'operator', False, 600,
                                   commands=write(out / 'commands.txt', CACHE),
                                   client_data=write_json(out / 'client-data.json', client_data('classic')),
                                   blob_cache=True)
    except subprocess.TimeoutExpired:
        code = 124
    finally:
        if world.exists():
            shutil.rmtree(world)
    print(f'{probe}: exit {code}; output {out}')
    return code


if __name__ == '__main__':
    sys.exit(main())
