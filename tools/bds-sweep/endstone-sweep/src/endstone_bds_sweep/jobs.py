"""Sweep jobs. Each job is a generator over the job's `items`; it yields
`None` to let the plugin check its tick budget and an int `n` to sleep `n`
server ticks. Every finished item becomes one line of `results.jsonl`.

Coordinates: the runner puts the sweep player at `arena.player` in a
superflat world; work positions are relative to it and inside its ticking
range.
"""

from __future__ import annotations

import json
from collections import Counter
from pathlib import Path


class Context:
    def __init__(self, plugin, player) -> None:
        self.plugin = plugin
        self.server = plugin.server
        self.player = player
        self.dimension = player.dimension
        self.out = Path(plugin.job['output_dir'])
        self.results_path = self.out / 'results.jsonl'
        self.done = set()
        # Block states (key before `|`) that crashed the server; every later
        # item on such a state is recorded as crashed without touching it.
        self.crashed_states: set[str] = set()
        if self.results_path.exists():
            for line in self.results_path.read_text(encoding='utf-8').splitlines():
                if line.strip():
                    row = json.loads(line)
                    self.done.add(row['key'])
                    if row.get('crashed'):
                        self.crashed_states.add(row['key'].split('|', 1)[0])
        self.results = self.results_path.open('a', encoding='utf-8', buffering=1)
        # An item that was in flight when the previous run died took the
        # server down with it (Endstone crashes on a few block states): record
        # it as crashed so the resumed run skips it instead of crashing again.
        self.inflight = self.out / 'inflight.json'
        if self.inflight.exists():
            item = json.loads(self.inflight.read_text(encoding='utf-8'))
            if item['key'] not in self.done:
                self.record({**item, 'crashed': True})
                self.crashed_states.add(item['key'].split('|', 1)[0])
            self.inflight.unlink()

    def begin(self, item: dict) -> None:
        """Marks `item` as in flight until it is recorded."""
        self.inflight.write_text(json.dumps(item), encoding='utf-8')

    def cmd(self, line: str) -> bool:
        return self.server.dispatch_command(self.server.command_sender, line)

    def must(self, line: str) -> None:
        if not self.cmd(line):
            raise RuntimeError(f'command failed: {line}')

    def record(self, row: dict) -> None:
        self.results.write(json.dumps(row, separators=(',', ':'), sort_keys=True) + '\n')
        self.done.add(row['key'])

    def block(self, pos) -> object:
        return self.dimension.get_block_at(*pos)

    def set_state(self, pos, name: str, states: dict) -> None:
        data = self.server.create_block_data(name, states)
        self.block(pos).set_data(data, False)

    def stack(self, spec):
        """`spec` is null (empty hand) or {item, count?, enchants?: {id: level},
        unbreakable?: bool}."""
        if not spec:
            return None
        from endstone.inventory import ItemStack
        stack = ItemStack(spec['item'], int(spec.get('count', 1)))
        enchants = spec.get('enchants') or {}
        if enchants or spec.get('unbreakable'):
            meta = stack.item_meta
            for enchant, level in enchants.items():
                if not meta.add_enchant(enchant, int(level), True):
                    raise RuntimeError(f'cannot enchant {spec["item"]} with {enchant} {level}')
            if spec.get('unbreakable'):
                meta.is_unbreakable = True
            stack.set_item_meta(meta)
        return stack

    def take_items(self) -> list[tuple[str, int, int]]:
        """Removes every item actor in the dimension and returns their stacks."""
        found = []
        for actor in self.dimension.actors:
            if actor.type == 'minecraft:item':
                stack = actor.item_stack
                found.append((stack.type.id, stack.data, stack.amount))
                actor.remove()
        return found


def run(ctx: Context, job: dict):
    arena = job['arena']
    ctx.must(f'tp "{job["player"]}" {arena["player"][0]} {arena["player"][1]} {arena["player"][2]}')
    for line in job.get('setup_commands', []):
        ctx.must(line)
    yield 5
    yield from JOBS[job['kind']](ctx, job)


def drops(ctx: Context, job: dict):
    """`/loot spawn … mine … mainhand` as the sweep player, `samples` times
    per (block state, tool) item; records the histogram of whole outcomes."""
    pos = job['arena']['block']
    drop = (pos[0], pos[1] + 1, pos[2])
    name = job['player']
    loot = (f'execute as @a[name="{name}"] run loot spawn {drop[0]} {drop[1]} {drop[2]} '
            f'mine {pos[0]} {pos[1]} {pos[2]} mainhand')
    ctx.take_items()
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        if item['key'].split('|', 1)[0] in ctx.crashed_states:
            ctx.record({**item, 'crashed': True})
            continue
        ctx.begin(item)
        ctx.set_state(pos, item['block'], item['states'])
        ctx.player.inventory.item_in_main_hand = ctx.stack(item['tool'])
        placed = ctx.block(pos).data
        outcomes: Counter[str] = Counter()
        failures = 0
        for _ in range(int(item['samples'])):
            if not ctx.cmd(loot):
                failures += 1
            stacks = sorted(ctx.take_items())
            outcomes[';'.join(f'{i}@{d}x{n}' for i, d, n in stacks)] += 1
            yield None
        after = ctx.block(pos).data
        ctx.record({
            'key': item['key'],
            'block': item['block'],
            'states': item['states'],
            'placed': {'type': placed.type, 'states': placed.block_states},
            'unchanged': after.runtime_id == placed.runtime_id,
            'tool': item['tool'],
            'samples': int(item['samples']),
            'command_failures': failures,
            'outcomes': dict(sorted(outcomes.items())),
        })
    ctx.set_state(pos, 'minecraft:air', {})
    ctx.player.inventory.item_in_main_hand = None


SLOTS = {
    'head': ('slot.armor.head', 'helmet'),
    'chest': ('slot.armor.chest', 'chestplate'),
    'legs': ('slot.armor.legs', 'leggings'),
    'feet': ('slot.armor.feet', 'boots'),
    'offhand': ('slot.weapon.offhand', 'item_in_off_hand'),
}


def armor_slots(ctx: Context, job: dict):
    """`/replaceitem entity <player> <slot> 0 <item>` per item and slot: the
    command result and what the slot holds afterwards."""
    name = job['player']
    inventory = ctx.player.inventory
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        row = {'key': item['key'], 'item': item['item'], 'slots': {}}
        for slot, (command_slot, attribute) in SLOTS.items():
            setattr(inventory, attribute, None)
            accepted = ctx.cmd(f'replaceitem entity @a[name="{name}"] {command_slot} 0 {item["item"]}')
            held = getattr(inventory, attribute)
            row['slots'][slot] = {
                'command_succeeded': accepted,
                'holds': held.type.id if held is not None else None,
            }
            setattr(inventory, attribute, None)
            yield None
        ctx.record(row)


def fuel(ctx: Context, job: dict):
    """Furnace burn time per item: a pool of furnaces, each loaded with one
    smeltable input and one candidate fuel; counts server ticks from the
    first tick the furnace is lit to the first tick it is unlit again. An
    item that has not lit the furnace after `ignite_ticks` is not a fuel."""
    origin = job['arena']['furnaces']
    pending = [item for item in job['items'] if item['key'] not in ctx.done]
    size = int(job.get('pool', 32))
    positions = [(origin[0] + 2 * (i % 16), origin[1], origin[2] + 2 * (i // 16)) for i in range(size)]
    limit = int(job.get('max_ticks', 24000))
    ignite = int(job.get('ignite_ticks', 40))
    lit_type = job['lit_furnace']
    slots: list[dict | None] = [None] * size
    tick = 0

    def load(index: int) -> None:
        pos = positions[index]
        ctx.set_state(pos, 'minecraft:air', {})
        while pending:
            item = pending.pop(0)
            ctx.must(f'setblock {pos[0]} {pos[1]} {pos[2]} {job["furnace"]}')
            ctx.must(f'replaceitem block {pos[0]} {pos[1]} {pos[2]} slot.container 0 {job["input"]} 1')
            # Block-only identifiers (double slabs, wall signs, ...) are not
            # items the command accepts: not a fuel.
            if ctx.cmd(f'replaceitem block {pos[0]} {pos[1]} {pos[2]} slot.container 1 {item["item"]} 1'):
                slots[index] = {'item': item, 'loaded': tick, 'lit_at': None}
                return
            ctx.record({'key': item['key'], 'item': item['item'], 'furnace': job['furnace'],
                        'input': job['input'], 'accepted': False, 'lit': False, 'burn_ticks': None,
                        'timed_out': False})
            ctx.set_state(pos, 'minecraft:air', {})
        slots[index] = None

    for index in range(size):
        load(index)
    while any(slots):
        yield 1
        tick += 1
        for index, slot in enumerate(slots):
            if slot is None:
                continue
            lit = ctx.block(positions[index]).type == lit_type
            age = tick - slot['loaded']
            burn = None
            if lit and slot['lit_at'] is None:
                slot['lit_at'] = tick
                continue
            if slot['lit_at'] is None and age < ignite:
                continue
            if slot['lit_at'] is not None and lit and age < limit:
                continue
            if slot['lit_at'] is not None and not lit:
                burn = tick - slot['lit_at']
            ctx.record({
                'key': slot['item']['key'],
                'item': slot['item']['item'],
                'furnace': job['furnace'],
                'input': job['input'],
                'lit': slot['lit_at'] is not None,
                'burn_ticks': burn,
                'timed_out': slot['lit_at'] is not None and burn is None,
            })
            load(index)


def mining(ctx: Context, job: dict):
    """Break time per (block state, tool, effects): the sweep player (survival,
    facing the block from `arena.miner`) gets a `bds-sweep:mine` order and
    keeps sending ContinueDestroyBlock; BDS breaks the block itself when its
    own progress completes. The time is the number of server ticks from the
    LEFT_CLICK_BLOCK interaction to the BlockBreakEvent, measured `repeats`
    times per item; every trial ends with a stop order so no mining state
    carries over. With `predict_after` > 0 the client predicts the break after
    that many ticks instead."""
    name = job['player']
    miner = job['arena']['miner']
    pos = tuple(job['arena']['mine_block'])
    face = int(job['arena']['mine_face'])
    ctx.must(f'gamemode survival "{name}"')

    def trial(item):
        ctx.set_state(pos, item['block'], item['states'])
        yield 3
        ctx.plugin.events.clear()
        environment = {'on_ground': ctx.player.is_on_ground, 'in_water': ctx.player.is_in_water}
        predict = int(item.get('predict_after', 0))
        ctx.player.send_message(f'bds-sweep:mine {pos[0]} {pos[1]} {pos[2]} {face} {predict}')
        start = broke = None
        for _ in range(int(item.get('max_ticks', 1200))):
            yield 1
            for tick, kind, where in ctx.plugin.events:
                if where == pos and kind == 'left_click_block' and start is None:
                    start = tick
                if where == pos and kind == 'break' and broke is None:
                    broke = tick
            if broke is not None:
                break
        ctx.player.send_message('bds-sweep:stop')
        yield 3
        ctx.set_state(pos, 'minecraft:air', {})
        ctx.take_items()
        events = [[t, k] for t, k, w in ctx.plugin.events if w == pos][:8]
        return {'start_tick': start, 'break_tick': broke, 'events': events, **environment,
                'ticks': broke - start if start is not None and broke is not None else None}

    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        ctx.must(f'tp "{name}" {miner[0]} {miner[1]} {miner[2]} {miner[3]} {miner[4]}')
        # Fails when there is nothing to clear.
        ctx.cmd(f'effect "{name}" clear')
        for line in item.get('commands', []):
            ctx.must(line.replace('{player}', f'"{name}"'))
        ctx.player.inventory.item_in_main_hand = ctx.stack(item['tool'])
        trials = []
        for _ in range(int(item.get('repeats', 3))):
            trials.append((yield from trial(item)))
        ticks = [t['ticks'] for t in trials]
        for line in item.get('cleanup', []):
            ctx.must(line.replace('{player}', f'"{name}"'))
        ctx.record({
            'key': item['key'],
            'block': item['block'],
            'states': item['states'],
            'tool': item['tool'],
            'commands': item.get('commands', []),
            'predict_after': int(item.get('predict_after', 0)),
            'ticks': ticks,
            'consistent': len(set(ticks)) == 1 and ticks[0] is not None,
            'trials': trials,
        })
    ctx.must(f'gamemode creative "{name}"')


ARMOR_ATTRIBUTES = {'head': 'helmet', 'chest': 'chestplate', 'legs': 'leggings', 'feet': 'boots'}


def armor_values(ctx: Context, job: dict):
    """Damage reduction per armour item: the sweep player (survival, raised
    max health, no natural regeneration) wears only that item and takes
    `hits` x `/damage <player> D entity_attack` per configured D, spaced past
    the hurt cooldown. Endstone reports health as an integer, so the health
    lost over all hits of one D gives the per-hit value to 1/hits."""
    name = job['player']
    inventory = ctx.player.inventory
    for line in ('gamerule naturalregeneration false', 'difficulty normal', f'gamemode survival "{name}"',
                 f'effect "{name}" saturation 1000000 255 true'):
        ctx.must(line)
    ctx.player.max_health = int(job['max_health'])
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        for attribute in ARMOR_ATTRIBUTES.values():
            setattr(inventory, attribute, None)
        if item['item']:
            # Unbreakable: a 1000-point hit would otherwise destroy the piece.
            setattr(inventory, ARMOR_ATTRIBUTES[item['slot']],
                    ctx.stack({'item': item['item'], 'unbreakable': True}))
        yield 2
        hits = []
        for spec in job['damage']:
            amount, count = spec['amount'], int(spec['hits'])
            ctx.player.health = ctx.player.max_health
            ctx.plugin.damage.clear()
            before = ctx.player.health
            for _ in range(count):
                ctx.must(f'damage "{name}" {amount} entity_attack')
                yield int(job.get('cooldown_ticks', 15))
            after = ctx.player.health
            events = sorted({round(d, 4) for _, kind, d, _ in ctx.plugin.damage if kind == 'minecraft:player'})
            hits.append({'damage': amount, 'hits': count, 'health_lost': before - after,
                         'per_hit': (before - after) / count, 'event_damage': events})
        worn = getattr(inventory, ARMOR_ATTRIBUTES[item['slot']]) if item['item'] else None
        ctx.record({
            'key': item['key'],
            'item': item['item'],
            'slot': item['slot'],
            'worn': worn.type.id if worn is not None else None,
            'max_health': ctx.player.max_health,
            'hits': hits,
        })
    for attribute in ARMOR_ATTRIBUTES.values():
        setattr(inventory, attribute, None)
    ctx.player.health = ctx.player.max_health
    ctx.must(f'gamemode creative "{name}"')


def attack(ctx: Context, job: dict):
    """Melee damage per held item: a fresh target mob is spawned in front of
    the sweep player (creative, on the ground, not sprinting), the client
    gets a `bds-sweep:attack <runtime id>` order, and the damage events of
    that mob are recorded; `repeats` trials per item."""
    from endstone.level import Location
    spot = job['arena']['target']
    miner = job['arena']['miner']
    ctx.must(f'tp "{job["player"]}" {miner[0]} {miner[1]} {miner[2]} {miner[3]} {miner[4]}')
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        ctx.player.inventory.held_item_slot = 0
        ctx.player.inventory.item_in_main_hand = ctx.stack(item['tool'])
        yield 5
        trials = []
        # Trial 0 is a warm-up and not recorded as a measurement: the first
        # attack after the held item changed through Endstone never deals
        # damage (observed with 5 and 20 ticks of wait; BDS then resyncs the
        # slot and later attacks hit).
        for index in range(int(item.get('repeats', 3)) + 1):
            # Keep only the runtime id: an Endstone actor wrapper used after
            # its actor is destroyed (the target can die from one hit)
            # crashes the server.
            target = ctx.dimension.spawn_actor(
                Location(ctx.dimension, spot[0], spot[1], spot[2]), job['target']).runtime_id
            yield 3
            ctx.plugin.damage.clear()
            ctx.player.send_message(f'bds-sweep:attack {target}')
            yield 6
            damage = [round(d, 4) for _, kind, d, _ in ctx.plugin.damage if kind == job['target']]
            causes = sorted({c for _, kind, _, c in ctx.plugin.damage if kind == job['target']})
            trials.append({'damage': damage, 'causes': causes, 'warmup': index == 0})
            for actor in ctx.dimension.actors:
                if actor.runtime_id == target:
                    actor.remove()
            ctx.take_items()
            yield 2
        ctx.record({'key': item['key'], 'tool': item['tool'], 'target': job['target'], 'trials': trials})
    ctx.player.inventory.item_in_main_hand = None


def _json_value(value):
    if isinstance(value, (bool, int, float, str)) or value is None:
        return value
    return str(value)


EYE_HEIGHT = 1.62
# Faces as the protocol numbers them, with the click position on each face
# (relative to the block) and the offset of the cell a placement fills.
FACES = {
    'down': (0, (0.5, 0.0, 0.5), (0, -1, 0)),
    'up': (1, (0.5, 1.0, 0.5), (0, 1, 0)),
    'north': (2, (0.5, 0.5, 0.0), (0, 0, -1)),
    'south': (3, (0.5, 0.5, 1.0), (0, 0, 1)),
    'west': (4, (0.0, 0.5, 0.5), (-1, 0, 0)),
    'east': (5, (1.0, 0.5, 0.5), (1, 0, 0)),
}


def look_at(stand, target) -> tuple[float, float]:
    """Bedrock yaw/pitch (yaw 0 south, 90 west, 180 north, 270 east; pitch
    positive looks down) from the eyes of a player standing at `stand` to
    `target`."""
    import math
    dx, dy, dz = target[0] - stand[0], target[1] - (stand[1] + EYE_HEIGHT), target[2] - stand[2]
    yaw = math.degrees(math.atan2(-dx, dz)) % 360
    pitch = -math.degrees(math.atan2(dy, math.hypot(dx, dz)))
    return round(yaw, 2), round(pitch, 2)


class Blocks:
    """Shared steps of the `blocks` job: arena reset, held items, block
    clicks through the scripted client and per-tick block watches. Every
    step sends the client a `bds-sweep:mark SCENARIO/STEP` chat line first,
    so the packets it logs (`--trace`) can be attributed to the step."""

    def __init__(self, ctx: Context, job: dict) -> None:
        self.ctx = ctx
        self.job = job
        self.name = job['player']
        self.arena = job['arena']['blocks']
        self.scenario = ''

    @property
    def tick(self) -> int:
        return self.ctx.plugin.ticks

    def state(self, pos) -> dict:
        data = self.ctx.block(pos).data
        return {'name': data.type, 'states': {k: _json_value(v) for k, v in sorted(data.block_states.items())},
                'runtime_id': data.runtime_id}

    def cells(self, cells: dict) -> dict:
        return {label: self.state(pos) for label, pos in cells.items()}

    def mark(self, step: str) -> str:
        label = f'{self.scenario}/{step}'
        self.ctx.player.send_message(f'bds-sweep:mark {label}')
        return label

    def reset(self):
        """Flat stone floor at y = floor, air above, no item actors, empty
        inventory. Marked as `SCENARIO/reset` so the block updates it causes
        are not attributed to the previous step."""
        self.mark('reset')
        a = self.arena
        (x0, z0), (x1, z1), floor = a['min'], a['max'], a['floor']
        # `fill` reports failure when nothing changes (a second reset).
        self.ctx.cmd(f'fill {x0} {floor} {z0} {x1} {floor} {z1} stone')
        self.ctx.cmd(f'fill {x0} {floor + 1} {z0} {x1} {floor + 8} {z1} air')
        for x, z in ((x0, z0), (x1, z1)):
            if self.ctx.block((x, floor, z)).type != 'minecraft:stone':
                raise RuntimeError('arena reset did not fill the stone floor')
        self.ctx.take_items()
        # Fails when the inventory is already empty.
        self.ctx.cmd(f'clear "{self.name}"')
        yield 2

    def hold(self, item: str | None, count: int = 64):
        """Hotbar slot 0 (selected) holds `count` x `item`, or nothing. Set
        with vanilla commands: after an Endstone `item_in_main_hand` change
        the client's first click with the new stack was rejected (BDS
        resynced the clicked blocks without using the item)."""
        self.ctx.player.inventory.held_item_slot = 0
        if item:
            self.ctx.must(f'replaceitem entity "{self.name}" slot.hotbar 0 {item} {count}')
        else:
            self.ctx.cmd(f'clear "{self.name}"')
        # Let the slot update reach the client before it clicks with it.
        yield 10

    def stand(self, stand, target, yaw: float | None = None):
        """Teleports the player to `stand` (feet) looking at `target`; an
        explicit `yaw` keeps the pitch towards the target. Returns the look
        requested and the one the server holds after the client's inputs."""
        look_yaw, pitch = look_at(stand, target)
        yaw = look_yaw if yaw is None else yaw
        self.ctx.must(f'tp "{self.name}" {stand[0]} {stand[1]} {stand[2]} {yaw} {pitch}')
        yield 6
        location = self.ctx.player.location
        return {'requested': [yaw, pitch],
                'server': [round(location.yaw % 360, 2), round(location.pitch, 2)],
                'position': [round(location.x, 3), round(location.y, 3), round(location.z, 3)]}

    def click(self, step: str, block, face: str, stand, cells: dict, yaw: float | None = None,
              click=None, wait: int = 10):
        """Right-clicks `face` of `block` from `stand`; returns the step record
        with the watched `cells` before and `wait` ticks after the click."""
        number, face_click, _ = FACES[face]
        click = click or face_click
        target = [block[0] + click[0], block[1] + click[1], block[2] + click[2]]
        look = yield from self.stand(stand, target, yaw)
        before = self.cells(cells)
        label = self.mark(step)
        self.ctx.plugin.events.clear()
        runtime_id = self.ctx.block(block).data.runtime_id
        self.ctx.player.send_message(
            f'bds-sweep:use {block[0]} {block[1]} {block[2]} {number} {click[0]} {click[1]} {click[2]} {runtime_id}')
        yield wait
        after = self.cells(cells)
        held = self.ctx.player.inventory.item_in_main_hand
        return {'step': label, 'action': 'use', 'block': list(block), 'face': face, 'click': list(click),
                'look': look, 'cells': {k: list(v) for k, v in cells.items()}, 'before': before, 'after': after,
                'changed': sorted(k for k in cells if before[k] != after[k]),
                'held_after': [held.type.id, held.amount] if held is not None else None,
                'interactions': [[t, k, list(w)] for t, k, w in self.ctx.plugin.events]}

    def watch(self, cells: dict, ticks: int, origin: int, until=None):
        """Samples `cells` every tick for `ticks` ticks (or until
        `until(states)`); returns the changes as [ticks since `origin`, cell,
        state] and the final states."""
        last = self.cells(cells)
        changes = []
        for _ in range(ticks):
            yield 1
            now = self.cells(cells)
            for label in cells:
                if now[label] != last[label]:
                    changes.append([self.tick - origin, label, now[label]])
            last = now
            if until is not None and until(now):
                break
        return changes, last

    def items(self, remove: bool = True) -> list[dict]:
        """Returns the stacks and positions of every item actor and removes
        them unless `remove` is false."""
        found = []
        for actor in self.ctx.dimension.actors:
            if actor.type == 'minecraft:item':
                stack = actor.item_stack
                location = actor.location
                found.append({'item': stack.type.id, 'data': stack.data, 'count': stack.amount,
                              'position': [round(location.x, 2), round(location.y, 2), round(location.z, 2)]})
                if remove:
                    actor.remove()
        return sorted(found, key=lambda item: (item['item'], item['position']))

    def remove(self, step: str, pos, cells: dict, ticks: int):
        """`/setblock` `pos` to air and watch `cells`: their states in the
        same tick, every change after it, and the item actors spawned."""
        label = self.mark(step)
        command = f'setblock {pos[0]} {pos[1]} {pos[2]} air'
        origin = self.tick
        self.ctx.must(command)
        same_tick = self.cells(cells)
        changes, final = yield from self.watch(cells, ticks, origin)
        yield 10
        return {'step': label, 'action': 'setblock', 'command': command,
                'cells': {k: list(v) for k, v in cells.items()}, 'same_tick': same_tick,
                'changes': changes, 'after': final, 'drops': self.items()}

    def place(self, step: str, item: str, block, face: str, stand, yaw: float | None = None, click=None):
        """Holds `item` and right-clicks `face` of `block`; the step watches
        the clicked block and the cell the placement fills (`target`)."""
        d = FACES[face][2]
        cell = (block[0] + d[0], block[1] + d[1], block[2] + d[2])
        yield from self.hold(item)
        return (yield from self.click(step, block, face, stand, {'target': cell, 'clicked': tuple(block)},
                                      yaw=yaw, click=click))

    def use_watch(self, step: str, block, face: str, stand, cells: dict, ticks: int, trigger: str | None = None,
                  yaw: float | None = None, click=None):
        """Right-clicks `face` of `block` (the held item stays as it is) and
        samples `cells` every tick for `ticks` ticks. `trigger` names the
        watched cell the click itself changes (lever, button, repeater): the
        summary counts ticks from its first change."""
        number, face_click, _ = FACES[face]
        click = click or face_click
        target = [block[0] + click[0], block[1] + click[1], block[2] + click[2]]
        look = yield from self.stand(stand, target, yaw)
        before = self.cells(cells)
        label = self.mark(step)
        self.ctx.plugin.events.clear()
        runtime_id = self.ctx.block(block).data.runtime_id
        origin = self.tick
        self.ctx.player.send_message(
            f'bds-sweep:use {block[0]} {block[1]} {block[2]} {number} {click[0]} {click[1]} {click[2]} {runtime_id}')
        changes, final = yield from self.watch(cells, ticks, origin)
        return {'step': label, 'action': 'use_watch', 'block': list(block), 'face': face, 'click': list(click),
                'look': look, 'cells': {k: list(v) for k, v in cells.items()}, 'trigger': trigger,
                'origin_tick': origin, 'ticks': ticks, 'before': before, 'changes': changes, 'after': final,
                'interactions': [[t - origin, k, list(w)] for t, k, w in self.ctx.plugin.events]}

    def command_watch(self, step: str, commands: list[str], cells: dict, ticks: int, trigger: str | None = None,
                      sample_every: int | None = None):
        """Runs `commands` in one tick and samples `cells` every tick for
        `ticks` ticks: their states in the same tick and every change after
        it; `sample_every` asks the summary for snapshots at that period."""
        label = self.mark(step)
        before = self.cells(cells)
        origin = self.tick
        succeeded = [self.ctx.cmd(command) for command in commands]
        same_tick = self.cells(cells)
        changes = [[0, k, same_tick[k]] for k in cells if same_tick[k] != before[k]]
        more, final = yield from self.watch(cells, ticks, origin)
        row = {'step': label, 'action': 'command_watch', 'commands': commands, 'succeeded': succeeded,
               'cells': {k: list(v) for k, v in cells.items()}, 'trigger': trigger, 'origin_tick': origin,
               'ticks': ticks, 'before': before, 'changes': changes + more, 'after': final}
        if sample_every:
            row['sample_every'] = sample_every
        return row

    def palette(self, states: list[dict]) -> dict:
        """Runtime id -> block state of each `{name, states}` entry: sounds
        carry a block state's runtime id as their data."""
        found = {}
        for entry in states:
            data = self.ctx.server.create_block_data(entry['name'], entry['states'])
            found[str(data.runtime_id)] = {'name': data.type, 'states': {
                k: _json_value(v) for k, v in sorted(data.block_states.items())}}
        return found


def blocks(ctx: Context, job: dict):
    """Block behaviour scenarios (doors, torches, crops; redstone and fire)
    on a flat stone arena, played by the survival sweep player through
    `bds-sweep:use` and `bds-sweep:mine`; one result row per scenario with
    the full block states of the watched cells after every step (per tick
    for the redstone and fire scenarios)."""
    b = Blocks(ctx, job)
    ctx.must(f'gamemode survival "{job["player"]}"')
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        b.scenario = item['key']
        steps = yield from SCENARIOS[item['key']](b, item)
        row = {'key': item['key'], 'steps': steps}
        if item['key'] in REDSTONE_SCENARIOS:
            row['palette'] = b.palette(job.get('palette_states', []))
        ctx.record(row)
    ctx.player.inventory.clear()
    ctx.must(f'gamemode creative "{job["player"]}"')


def _door_cells(pos) -> dict:
    return {'lower': tuple(pos), 'upper': (pos[0], pos[1] + 1, pos[2])}


def _place_door(b: Blocks, step: str, cell, look: str, item: str = 'minecraft:wooden_door', click=None,
                extra_cells: dict | None = None):
    """Places a door into `cell` by clicking the top of the floor below it
    from two blocks away, looking `look` (north: from the south, yaw 180;
    east: from the west, yaw 270)."""
    x, y, z = cell
    stand, yaw = {'north': ((x + 0.5, y, z + 2.5), 180), 'east': ((x - 1.5, y, z + 0.5), 270)}[look]
    yield from b.hold(item)
    cells = {**_door_cells(cell), **(extra_cells or {})}
    return (yield from b.click(step, (x, y - 1, z), 'up', stand, cells, yaw=yaw, click=click))


def doors(b: Blocks, item: dict):
    ctx = b.ctx
    floor = b.arena['floor']
    cell = (0, floor + 1, 0)
    y = cell[1]
    watch_ticks = int(item.get('watch_ticks', 20))
    steps = []

    # Placement: facing, hinge and the neighbours that decide the hinge.
    placements = [
        ('place_looking_north', 'north', [], None),
        ('place_looking_east', 'east', [], None),
        ('place_looking_north_click_west_half', 'north', [], (0.2, 1.0, 0.5)),
        ('place_looking_north_click_east_half', 'north', [], (0.8, 1.0, 0.5)),
        ('place_stone_stacked_west', 'north', [(-1, y, 0), (-1, y + 1, 0)], None),
        ('place_stone_stacked_east', 'north', [(1, y, 0), (1, y + 1, 0)], None),
    ]
    for step, look, stones, click in placements:
        yield from b.reset()
        for pos in stones:
            ctx.must(f'setblock {pos[0]} {pos[1]} {pos[2]} stone')
        steps.append((yield from _place_door(b, step, cell, look, click=click)))

    # A second door directly east of the first (both placed looking north).
    yield from b.reset()
    steps.append((yield from _place_door(b, 'pair_first', cell, 'north')))
    pair = {'first_lower': cell, 'first_upper': (0, y + 1, 0)}
    steps.append((yield from _place_door(b, 'pair_second_east', (1, y, 0), 'north', extra_cells=pair)))

    # Toggling an oak door with dirt in hand (not sneaking).
    stand = (0.5, y, 2.5)
    watched = {**_door_cells(cell), 'in_front': (0, y, 1)}
    yield from b.reset()
    steps.append((yield from _place_door(b, 'toggle_place', cell, 'north')))
    yield from b.hold('minecraft:dirt')
    for index in range(3):
        steps.append((yield from b.click(f'toggle_lower_{index + 1}', cell, 'south', stand, watched)))
    steps.append((yield from b.click('toggle_upper_1', (0, y + 1, 0), 'south', stand, watched)))

    # Iron door: empty hand, then dirt in hand.
    yield from b.reset()
    steps.append((yield from _place_door(b, 'iron_place', cell, 'north', item='minecraft:iron_door')))
    yield from b.hold(None)
    steps.append((yield from b.click('iron_click_empty_hand', cell, 'south', stand, watched)))
    yield from b.hold('minecraft:dirt')
    steps.append((yield from b.click('iron_click_dirt', cell, 'south', stand, watched)))

    # Survival player breaks one half by hand.
    for half in ('upper', 'lower'):
        yield from b.reset()
        steps.append((yield from _place_door(b, f'break_{half}_place', cell, 'north')))
        yield from b.hold(None)
        target = _door_cells(cell)[half]
        look = yield from b.stand(stand, (target[0] + 0.5, target[1] + 0.5, target[2] + 1.0))
        label = b.mark(f'break_{half}_by_hand')
        ctx.plugin.events.clear()
        origin = b.tick
        ctx.player.send_message(f'bds-sweep:mine {target[0]} {target[1]} {target[2]} {FACES["south"][0]} 0')
        changes, _ = yield from b.watch(
            _door_cells(cell), int(item.get('max_break_ticks', 400)), origin,
            until=lambda s: s['lower']['name'] == 'minecraft:air' and s['upper']['name'] == 'minecraft:air')
        ctx.player.send_message('bds-sweep:stop')
        more, final = yield from b.watch(_door_cells(cell), watch_ticks, origin)
        events = [[t - origin, k, list(w)] for t, k, w in ctx.plugin.events]
        steps.append({'step': label, 'action': 'mine', 'block': list(target), 'face': 'south', 'look': look,
                      'cells': {k: list(v) for k, v in _door_cells(cell).items()},
                      'changes': changes + more, 'after': final, 'events': events, 'drops': b.items()})

    # Removing the floor under the lower half.
    yield from b.reset()
    steps.append((yield from _place_door(b, 'floor_removed_place', cell, 'north')))
    steps.append((yield from b.remove('floor_removed', (0, floor, 0), _door_cells(cell), watch_ticks)))
    return steps


def torches(b: Blocks, item: dict):
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    steps = []
    yield from b.reset()
    support = (6, y, 0)
    overhang = (12, y + 2, 0)
    fence = (16, y, 0)
    glass = (20, y, 0)
    for pos, block in ((support, 'stone'), (overhang, 'stone'), (fence, 'oak_fence'), (glass, 'glass')):
        ctx.must(f'setblock {pos[0]} {pos[1]} {pos[2]} {block}')
    yield 2
    yield from b.hold('minecraft:torch')

    # (step, clicked block, face, standing position)
    clicks = [
        ('floor_top', (0, floor, 0), 'up', (0.5, y, 2.5)),
        ('stone_north_face', support, 'north', (6.5, y, -2.5)),
        ('stone_south_face', support, 'south', (6.5, y, 3.5)),
        ('stone_west_face', support, 'west', (3.5, y, 0.5)),
        ('stone_east_face', support, 'east', (9.5, y, 0.5)),
        ('overhang_bottom_face', overhang, 'down', (12.5, y, 2.5)),
        ('oak_fence_top', fence, 'up', (16.5, y, 2.5)),
        ('glass_top', glass, 'up', (20.5, y, 2.5)),
    ]
    side_torches = {}
    for step, block, face, stand in clicks:
        d = FACES[face][2]
        cell = (block[0] + d[0], block[1] + d[1], block[2] + d[2])
        steps.append((yield from b.click(step, block, face, stand, {'target': cell, 'clicked': block})))
        if block == support:
            side_torches[face] = cell
    steps.append((yield from b.remove('support_removed', support, side_torches,
                                      int(item.get('watch_ticks', 20)))))
    return steps


def crops(b: Blocks, item: dict):
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    steps = []
    yield from b.reset()
    # Seeds: farmland (planted) and dirt (refused).
    ctx.must(f'setblock 0 {floor} -6 farmland')
    ctx.must(f'setblock 3 {floor} -6 dirt')
    yield 2
    yield from b.hold('minecraft:wheat_seeds')
    steps.append((yield from b.click('seeds_on_farmland', (0, floor, -6), 'up', (0.5, y, -3.5),
                                     {'target': (0, y, -6), 'clicked': (0, floor, -6)})))
    steps.append((yield from b.click('seeds_on_dirt', (3, floor, -6), 'up', (3.5, y, -3.5),
                                     {'target': (3, y, -6), 'clicked': (3, floor, -6)})))

    # Growth: wheat (growth 0) on 8 farmland cells around one water source,
    # open sky at noon, sampled every `sample_ticks`.
    interval = int(item.get('sample_ticks', 20))
    for speed, seconds in item['growth_runs']:
        yield from b.reset()
        plants = {}
        for dx in (-1, 0, 1):
            for dz in (-1, 0, 1):
                x, z = dx, 8 + dz
                if dx == dz == 0:
                    ctx.must(f'setblock {x} {floor} {z} water')
                    continue
                ctx.must(f'setblock {x} {floor} {z} farmland')
                ctx.must(f'setblock {x} {y} {z} wheat')
                plants[f'{x},{z}'] = (x, z)
        yield from b.stand((4.5, y, 8.5), (0.5, y, 8.5))
        label = b.mark(f'growth_randomtickspeed_{speed}')
        ctx.must(f'gamerule randomtickspeed {speed}')
        origin = b.tick
        samples = []
        for _ in range(int(seconds * 20 / interval) + 1):
            row = {'tick': b.tick - origin, 'growth': {}, 'moisturized_amount': {}, 'other': {}}
            for key, (x, z) in plants.items():
                crop, land = b.state((x, y, z)), b.state((x, floor, z))
                if crop['name'] == 'minecraft:wheat':
                    row['growth'][key] = crop['states']['growth']
                else:
                    row['other'][key] = crop['name']
                row['moisturized_amount'][key] = (land['states'].get('moisturized_amount')
                                                  if land['name'] == 'minecraft:farmland' else land['name'])
            samples.append(row)
            yield interval
        ctx.must('gamerule randomtickspeed 0')
        steps.append({'step': label, 'action': 'random_ticks', 'randomtickspeed': speed, 'seconds': seconds,
                      'samples': samples, 'after': {key: b.state((x, y, z)) for key, (x, z) in plants.items()}})
    return steps


# Redstone and fire scenarios: components are clicked into place by the
# player when they have an orientation (levers, repeaters, comparators,
# buttons, torches, doors), everything else is `/setblock`; toggles are the
# player's empty-hand clicks and the watched cells are sampled every tick.

def _set(ctx: Context, blocks) -> None:
    """`/setblock` each (pos, block); `setblock` fails when the block is
    already there, so the result is checked by reading the cell back."""
    for (x, y, z), block in blocks:
        ctx.cmd(f'setblock {x} {y} {z} {block}')
        if ctx.block((x, y, z)).type != f'minecraft:{block}':
            raise RuntimeError(f'setblock {x} {y} {z} {block} left {ctx.block((x, y, z)).type}')


def _lever_on_top(b: Blocks, support, stand, step: str = 'lever_place'):
    """Lever clicked onto the top of `support`; returns the step and the
    lever cell."""
    placed = yield from b.place(step, 'minecraft:lever', support, 'up', stand)
    return placed, (support[0], support[1] + 1, support[2])


def redstone_dust(b: Blocks, item: dict):
    """Lever on a stone block, a dust line along +x starting next to it and
    a lamp after the line: 15 dust (the lamp gets signal 1), then 16."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    ticks = int(item.get('watch_ticks', 40))
    steps = []
    for length in (15, 16):
        yield from b.reset()
        support = (0, y, 0)
        _set(ctx, [(support, 'stone')] + [((x, y, 0), 'redstone_wire') for x in range(1, length + 1)]
             + [((length + 1, y, 0), 'redstone_lamp')])
        yield 2
        stand = (0.5, y, -2.5)
        placed, lever = yield from _lever_on_top(b, support, stand, f'line_{length}_lever_place')
        steps.append(placed)
        yield from b.hold(None)
        cells = {'lever': lever, **{f'dust_{x:02}': (x, y, 0) for x in range(1, length + 1)},
                 'lamp': (length + 1, y, 0)}
        for cycle in range(1, 3 if length == 15 else 2):
            for state in ('on', 'off'):
                steps.append((yield from b.use_watch(f'line_{length}_lever_{state}_{cycle}', lever, 'up', stand,
                                                     cells, ticks, trigger='lever')))
    return steps


def _repeater_line(b: Blocks, prefix: str):
    """Stone with a lever on top at x=0, a repeater clicked onto the floor
    at x=1 while looking east (input from the stone), a lamp at x=2."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    support, repeater, lamp = (0, y, 0), (1, y, 0), (2, y, 0)
    _set(ctx, [(support, 'stone')])
    yield 2
    stand = (0.5, y, -2.5)
    steps = []
    placed, lever = yield from _lever_on_top(b, support, stand, f'{prefix}_lever_place')
    steps.append(placed)
    steps.append((yield from b.place(f'{prefix}_repeater_place', 'minecraft:repeater', (1, y - 1, 0), 'up',
                                     (1.5, y, -2.5), yaw=270)))
    _set(ctx, [(lamp, 'redstone_lamp')])
    yield 2
    return steps, {'lever': lever, 'repeater': repeater, 'lamp': lamp}, stand


def repeaters(b: Blocks, item: dict):
    ctx = b.ctx
    y = b.arena['floor'] + 1
    ticks = int(item.get('watch_ticks', 40))
    steps = []
    for delay in (1, 2, 3, 4):
        yield from b.reset()
        prefix = f'delay_{delay}'
        placed, cells, stand = yield from _repeater_line(b, prefix)
        steps += placed
        yield from b.hold(None)
        for click in range(1, delay):
            steps.append((yield from b.click(f'{prefix}_click_{click}', cells['repeater'], 'up', (1.5, y, -2.5),
                                             {'repeater': cells['repeater']})))
        for state in ('on', 'off'):
            steps.append((yield from b.use_watch(f'{prefix}_lever_{state}', cells['lever'], 'up', stand, cells,
                                                 ticks, trigger='lever')))

    # Locking: a second repeater (input from a stone with its own lever)
    # points north into the side of the first one.
    yield from b.reset()
    placed, cells, stand = yield from _repeater_line(b, 'lock')
    steps += placed
    side_support = (1, y, 2)
    _set(ctx, [(side_support, 'stone')])
    yield 2
    side_stand = (1.5, y, 4.5)
    steps.append((yield from b.place('lock_side_repeater_place', 'minecraft:repeater', (1, y - 1, 1), 'up',
                                     (3.5, y, 1.5), yaw=180)))
    placed, side_lever = yield from _lever_on_top(b, side_support, side_stand, 'lock_side_lever_place')
    steps.append(placed)
    yield from b.hold(None)
    cells = {**cells, 'side_repeater': (1, y, 1), 'side_lever': side_lever}
    sequence = [('main_on', 'lever', stand), ('side_on_lock', 'side_lever', side_stand),
                ('main_off_while_locked', 'lever', stand), ('side_off_unlock', 'side_lever', side_stand),
                ('side_on_lock_unpowered', 'side_lever', side_stand),
                ('main_on_while_locked', 'lever', stand), ('side_off_unlock_2', 'side_lever', side_stand)]
    for step, lever, where in sequence:
        steps.append((yield from b.use_watch(f'lock_{step}', cells[lever], 'up', where, cells, ticks,
                                             trigger=lever)))
    return steps


def redstone_torch(b: Blocks, item: dict):
    ctx = b.ctx
    y = b.arena['floor'] + 1
    ticks = int(item.get('watch_ticks', 40))
    steps = []
    # Torch on the east face of a stone block with a lever on top; a lamp
    # south of the torch shows its output.
    yield from b.reset()
    support = (0, y, 0)
    _set(ctx, [(support, 'stone')])
    yield 2
    stand = (0.5, y, -2.5)
    steps.append((yield from b.place('torch_place', 'minecraft:redstone_torch', support, 'east', (1.5, y, -2.5))))
    _set(ctx, [((1, y, 1), 'redstone_lamp')])
    placed, lever = yield from _lever_on_top(b, support, stand)
    steps.append(placed)
    yield from b.hold(None)
    cells = {'lever': lever, 'torch': (1, y, 0), 'lamp': (1, y, 1)}
    for cycle in (1, 2):
        for state in ('on', 'off'):
            steps.append((yield from b.use_watch(f'lever_{state}_{cycle}', lever, 'up', stand, cells, ticks,
                                                 trigger='lever')))

    # Burnout: the torch's own output runs back through dust into the block
    # it hangs on (torch -> 6 dust -> the stone's south side).
    yield from b.reset()
    _set(ctx, [(support, 'stone')])
    yield 2
    steps.append((yield from b.place('clock_torch_place', 'minecraft:redstone_torch', support, 'east',
                                     (1.5, y, -2.5))))
    path = [(2, y, 0), (2, y, 1), (2, y, 2), (1, y, 2), (0, y, 2)]
    _set(ctx, [(pos, 'redstone_wire') for pos in path])
    yield 10
    closing = (0, y, 1)
    cells = {'torch': (1, y, 0), **{f'dust_{i + 1}': pos for i, pos in enumerate(path + [closing])}}
    steps.append((yield from b.command_watch(
        'clock_closed', [f'setblock {closing[0]} {closing[1]} {closing[2]} redstone_wire'], cells,
        int(item.get('clock_ticks', 400)))))
    return steps


# (rear, side) input signals of the comparator configurations.
COMPARATOR_INPUTS = [(15, 0), (12, 5), (12, 12), (12, 14), (5, 10), (8, 7)]


def comparators(b: Blocks, item: dict):
    """Comparator at (8, y, 0) clicked into place looking east; rear dust
    along -x fed by a redstone block (signal at the comparator = 16 - dust
    count), side dust along +z the same way, two output dust along +x."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    ticks = int(item.get('watch_ticks', 30))
    cx = 8
    stand = (cx + 0.5, y, -2.5)
    steps = []
    for rear, side in COMPARATOR_INPUTS:
        prefix = f'rear_{rear}_side_{side}'
        yield from b.reset()
        steps.append((yield from b.place(f'{prefix}_place', 'minecraft:comparator', (cx, y - 1, 0), 'up', stand,
                                         yaw=270)))
        yield from b.hold(None)
        rear_dust = 16 - rear
        wires = [((x, y, 0), 'redstone_wire') for x in range(cx - rear_dust, cx)]
        wires += [((cx + 1, y, 0), 'redstone_wire'), ((cx + 2, y, 0), 'redstone_wire')]
        sources = [(cx - rear_dust - 1, y, 0)]
        if side:
            side_dust = 16 - side
            wires += [((cx, y, z), 'redstone_wire') for z in range(1, side_dust + 1)]
            sources.append((cx, y, side_dust + 1))
        _set(ctx, wires)
        yield 4
        cells = {'comparator': (cx, y, 0), 'rear_dust': (cx - 1, y, 0), 'out_1': (cx + 1, y, 0),
                 'out_2': (cx + 2, y, 0)}
        if side:
            cells['side_dust'] = (cx, y, 1)
        steps.append((yield from b.command_watch(
            f'{prefix}_compare', [f'setblock {x} {y_} {z} redstone_block' for x, y_, z in sources], cells, ticks)))
        steps.append((yield from b.use_watch(f'{prefix}_subtract', (cx, y, 0), 'up', stand, cells, ticks,
                                             trigger='comparator')))
        steps.append((yield from b.use_watch(f'{prefix}_compare_again', (cx, y, 0), 'up', stand, cells, ticks,
                                             trigger='comparator')))
    return steps


def buttons(b: Blocks, item: dict):
    """A button clicked onto the east face of a stone block, a lamp on the
    stone's west side; one press each."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    ticks = int(item.get('button_ticks', 60))
    steps = []
    stand = (1.5, y, -2.5)
    for button in ('stone_button', 'wooden_button'):
        yield from b.reset()
        support = (0, y, 0)
        _set(ctx, [(support, 'stone'), ((-1, y, 0), 'redstone_lamp')])
        yield 2
        steps.append((yield from b.place(f'{button}_place', f'minecraft:{button}', support, 'east', stand)))
        yield from b.hold(None)
        cells = {'button': (1, y, 0), 'lamp': (-1, y, 0)}
        steps.append((yield from b.use_watch(f'{button}_press', (1, y, 0), 'east', stand, cells, ticks,
                                             trigger='button', click=(0.125, 0.5, 0.5))))
    return steps


def pressure_plates(b: Blocks, item: dict):
    """Stone and oak plates on the floor, each with a lamp east of it; an
    item (`/loot spawn` of a mined dirt block, left to settle elsewhere)
    teleported onto each plate and removed with `/kill`, then the player
    teleported onto and off each plate (two dwell times)."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    ticks = int(item.get('watch_ticks', 60))
    name = b.name
    steps = []
    yield from b.reset()
    plates = {'stone': (0, y, 0), 'oak': (0, y, 4)}
    _set(ctx, [(plates['stone'], 'stone_pressure_plate'), ((1, y, 0), 'redstone_lamp'),
               (plates['oak'], 'wooden_pressure_plate'), ((1, y, 4), 'redstone_lamp'), ((6, y, 8), 'dirt')])
    yield 2
    cells = {'stone_plate': plates['stone'], 'stone_lamp': (1, y, 0), 'oak_plate': plates['oak'],
             'oak_lamp': (1, y, 4)}
    away = (4.5, y, 2.5)
    yield from b.hold(None)
    yield from b.stand(away, (0.5, y, 2.5))
    for plate in ('oak', 'stone'):
        x, _, z = plates[plate]
        ctx.must(f'execute as @a[name="{name}"] run loot spawn 10.5 {y + 0.1} 10.5 mine 6 {y} 8 mainhand')
        yield 40
        steps.append((yield from b.command_watch(
            f'{plate}_item_on', [f'tp @e[type=item] {x + 0.5} {y + 0.05} {z + 0.5}'], cells, ticks,
            trigger=f'{plate}_plate')))
        steps[-1]['items'] = b.items(remove=False)
        steps.append((yield from b.command_watch(f'{plate}_item_killed', ['kill @e[type=item]'], cells, ticks)))
    for plate in ('stone', 'oak'):
        x, _, z = plates[plate]
        for trial, dwell in enumerate((ticks, ticks - 13), start=1):
            steps.append((yield from b.command_watch(
                f'{plate}_player_on_{trial}', [f'tp "{name}" {x + 0.5} {y} {z + 0.5}'], cells, dwell,
                trigger=f'{plate}_plate')))
            steps.append((yield from b.command_watch(
                f'{plate}_player_off_{trial}', [f'tp "{name}" {away[0]} {away[1]} {away[2]}'], cells, ticks)))
    return steps


def iron_door_lever(b: Blocks, item: dict):
    """Iron door (placed looking north) with a stone block east of its lower
    half and a lever on that stone's east face."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    ticks = int(item.get('watch_ticks', 30))
    steps = []
    yield from b.reset()
    cell = (0, y, 0)
    steps.append((yield from _place_door(b, 'door_place', cell, 'north', item='minecraft:iron_door')))
    support = (1, y, 0)
    _set(ctx, [(support, 'stone')])
    yield 2
    stand = (3.5, y, 2.5)
    steps.append((yield from b.place('lever_place', 'minecraft:lever', support, 'east', stand)))
    yield from b.hold(None)
    cells = {'lever': (2, y, 0), **_door_cells(cell)}
    for cycle in (1, 2):
        for state in ('on', 'off'):
            steps.append((yield from b.use_watch(f'lever_{state}_{cycle}', (2, y, 0), 'east', stand, cells, ticks,
                                                 trigger='lever', click=(0.25, 0.5, 0.5))))
    return steps


def note_blocks(b: Blocks, item: dict):
    """Note block on dirt / on the stone floor, powered through a stone block
    east of it by a lever on that stone's east face."""
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    ticks = int(item.get('watch_ticks', 20))
    steps = []
    stand = (3.5, y, 2.5)
    for under in ('dirt', 'stone'):
        yield from b.reset()
        _set(ctx, [((0, floor, 0), under), ((0, y, 0), 'noteblock'), ((1, y, 0), 'stone')])
        yield 2
        steps.append((yield from b.place(f'on_{under}_lever_place', 'minecraft:lever', (1, y, 0), 'east', stand)))
        yield from b.hold(None)
        cells = {'lever': (2, y, 0), 'note_block': (0, y, 0), 'under': (0, floor, 0)}
        for state in ('on', 'off'):
            steps.append((yield from b.use_watch(f'on_{under}_lever_{state}', (2, y, 0), 'east', stand, cells,
                                                 ticks, trigger='lever', click=(0.25, 0.5, 0.5))))
    return steps


def fire(b: Blocks, item: dict):
    """`/setblock` fires with doFireTick on at randomtickspeed 1: one on
    netherrack, eight on the bare stone floor and one beside a 4-high oak
    planks column (every cell around the column watched); the planks
    column again on normal difficulty; then the player punches (starts
    breaking, then stops) the block under a fire on netherrack and on
    stone."""
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    ticks = int(item.get('fire_ticks', 1200))
    steps = []
    ctx.must('gamerule dofiretick true')
    ctx.must(f'gamerule randomtickspeed {int(item.get("randomtickspeed", 1))}')
    netherrack = (-8, floor, 0)
    stones = [(x, y, -6) for x in (-8, -4, 0, 4, 8, 16, 20, 24)]
    column = [(12, y + i, 0) for i in range(4)]
    around = {}
    for x in (11, 12, 13):
        for z in (-1, 0, 1):
            for dy in range(6):
                around[f'{x},{y + dy},{z}'] = (x, y + dy, z)
    for difficulty in ('peaceful', 'normal'):
        yield from b.reset()
        first = difficulty == 'peaceful'
        _set(ctx, ([(netherrack, 'netherrack')] if first else []) + [(pos, 'oak_planks') for pos in column])
        yield 2
        yield from b.stand((4.5, y, 6.5), (4.5, y, 0.5))
        cells = dict(around)
        fires = [(13, y, 0)]
        if first:
            cells = {'netherrack_fire': (-8, y, 0), **{f'stone_fire_{i + 1}': pos for i, pos in enumerate(stones)},
                     **cells}
            fires += [(-8, y, 0)] + stones
        ctx.cmd(f'difficulty {difficulty}')
        steps.append((yield from b.command_watch(
            'fires_set' if first else 'planks_fire_normal_difficulty',
            [f'setblock {x} {y_} {z} fire' for x, y_, z in fires], cells, ticks, sample_every=20)))
        steps[-1]['difficulty'] = difficulty
    ctx.cmd('difficulty peaceful')

    # Punching: start breaking the block under a fire (face up), stop after
    # a few ticks.
    yield from b.hold(None)
    for under, pos in (('netherrack', netherrack), ('stone', (0, floor, 6))):
        yield from b.reset()
        _set(ctx, [(pos, under), ((pos[0], y, pos[2]), 'fire')])
        yield 2
        watched = {'fire': (pos[0], y, pos[2]), 'under': pos}
        look = yield from b.stand((pos[0] + 0.5, y, pos[2] + 2.5), (pos[0] + 0.5, y, pos[2] + 0.5))
        label = b.mark(f'punch_{under}')
        before = b.cells(watched)
        ctx.plugin.events.clear()
        origin = b.tick
        ctx.player.send_message(f'bds-sweep:mine {pos[0]} {pos[1]} {pos[2]} {FACES["up"][0]} 0')
        changes, _ = yield from b.watch(watched, 5, origin)
        ctx.player.send_message('bds-sweep:stop')
        more, final = yield from b.watch(watched, 20, origin)
        steps.append({'step': label, 'action': 'mine_watch', 'block': list(pos), 'face': 'up', 'look': look,
                      'stopped_after': 5, 'ticks': 25, 'cells': {k: list(v) for k, v in watched.items()},
                      'trigger': None,
                      'origin_tick': origin, 'before': before, 'changes': changes + more, 'after': final,
                      'interactions': [[t - origin, k, list(w)] for t, k, w in ctx.plugin.events]})
    ctx.must('gamerule randomtickspeed 0')
    return steps


REDSTONE_SCENARIOS = {'redstone_dust': redstone_dust, 'repeaters': repeaters, 'redstone_torch': redstone_torch,
                      'comparators': comparators, 'buttons': buttons, 'pressure_plates': pressure_plates,
                      'iron_door_lever': iron_door_lever, 'note_blocks': note_blocks, 'fire': fire}

SCENARIOS = {'doors': doors, 'torches': torches, 'crops': crops, **REDSTONE_SCENARIOS}

JOBS = {'drops': drops, 'armor_slots': armor_slots, 'armor_values': armor_values, 'attack': attack,
        'fuel': fuel, 'mining': mining, 'blocks': blocks}
