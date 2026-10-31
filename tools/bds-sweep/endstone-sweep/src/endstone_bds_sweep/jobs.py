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
        # The `blocks` job's log of the scenario in progress (`StepLog`):
        # every command run while it is set is recorded in it.
        self.steps = None

    def begin(self, item: dict) -> None:
        """Marks `item` as in flight until it is recorded."""
        self.inflight.write_text(json.dumps(item), encoding='utf-8')

    def cmd(self, line: str) -> bool:
        ok = self.server.dispatch_command(self.server.command_sender, line)
        if self.steps is not None:
            self.steps.add('command', command=line, ok=bool(ok))
        return ok

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
        if found and self.steps is not None:
            self.steps.add('remove_item_actors', items=[list(stack) for stack in found])
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


class StepLog:
    """What the plugin did during one `blocks` scenario, for its
    `steps.json`: every command, client order, step mark, direct server-side
    change and wait, each with its tick offset from the log's start."""

    def __init__(self, plugin) -> None:
        self.plugin = plugin
        self.origin = plugin.ticks
        self.entries: list[dict] = []

    @property
    def now(self) -> int:
        return self.plugin.ticks - self.origin

    def add(self, op: str, **fields) -> None:
        self.entries.append({'tick': self.now, 'op': op, **fields})

    def wait(self, ticks: int) -> None:
        """A wait directly after another one (nothing happened in between)
        extends it."""
        last = self.entries[-1] if self.entries else None
        if last is not None and last['op'] == 'wait' and last['tick'] + last['ticks'] == self.now:
            last['ticks'] += ticks
        else:
            self.add('wait', ticks=ticks)


def logged(steps, log: StepLog):
    """Runs the job generator `steps`, recording each sleep it asks for in
    `log`; returns its value."""
    while True:
        try:
            wait = next(steps)
        except StopIteration as stop:
            return stop.value
        if wait:
            log.wait(int(wait))
        yield wait


class Blocks:
    """Shared steps of the `blocks` job: arena reset, held items, block
    clicks through the scripted client and per-tick block watches. Every
    step sends the client a `bds-sweep:mark SCENARIO/STEP` chat line first,
    so the packets it logs (`--trace`) can be attributed to the step; every
    command, order and direct change is also recorded in the scenario's
    `StepLog` (`ctx.steps`)."""

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

    def note(self, op: str, **fields) -> None:
        if self.ctx.steps is not None:
            self.ctx.steps.add(op, **fields)

    def mark(self, step: str) -> str:
        label = f'{self.scenario}/{step}'
        self.ctx.player.send_message(f'bds-sweep:mark {label}')
        self.note('mark', label=label)
        return label

    def marker(self, kind: str) -> str:
        """The scenario's `begin`/`end` marker chat line; the capture is cut
        at these."""
        text = f'bds-sweep:scenario {kind} {self.scenario}'
        self.ctx.player.send_message(text)
        self.note('marker', text=text)
        return text

    def use_order(self, block, face: str, click) -> None:
        """`bds-sweep:use`: the client right-clicks `face` of `block` at
        `click` with the selected hotbar slot."""
        runtime_id = self.ctx.block(block).data.runtime_id
        location = self.ctx.player.location
        self.note('use', pos=list(block), face=face, face_id=FACES[face][0], click=list(click),
                  block=self.state(block)['name'], block_runtime_id=runtime_id, held=_held(self.ctx),
                  player={'position': [round(location.x, 3), round(location.y, 3), round(location.z, 3)],
                          'yaw': round(location.yaw % 360, 2), 'pitch': round(location.pitch, 2)})
        self.ctx.player.send_message(
            f'bds-sweep:use {block[0]} {block[1]} {block[2]} {FACES[face][0]} {click[0]} {click[1]} {click[2]} '
            f'{runtime_id}')

    def mine_order(self, pos, face: str) -> None:
        """`bds-sweep:mine`: the client starts breaking `pos` and continues
        every tick until stopped or the server breaks the block."""
        self.note('mine_start', pos=list(pos), face=face, face_id=FACES[face][0], held=_held(self.ctx))
        self.ctx.player.send_message(f'bds-sweep:mine {pos[0]} {pos[1]} {pos[2]} {FACES[face][0]} 0')

    def stop_order(self) -> None:
        self.note('mine_stop')
        self.ctx.player.send_message('bds-sweep:stop')

    def fall_order(self, distance: float) -> None:
        """`bds-sweep:fall`: the client falls `distance` blocks under
        vanilla gravity and lands."""
        self.note('fall', distance=distance)
        self.ctx.player.send_message(f'bds-sweep:fall {distance}')

    def reset(self, mark: bool = True):
        """Flat stone floor at y = floor, air above, no item actors, empty
        inventory. Marked as `SCENARIO/reset` so the block updates it causes
        are not attributed to the previous step."""
        if mark:
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

    def prelude(self):
        """Run before a scenario's begin marker so every scenario starts
        from the same world: no order in progress, no actors or effects, the
        job's game rules, survival, peaceful, the superflat dirt under the
        floor repaired, a reset arena, the player at the arena start."""
        ctx = self.ctx
        self.stop_order()
        ctx.cmd('kill @e[type=!player]')
        ctx.cmd(f'effect "{self.name}" clear')
        for line in self.job.get('setup_commands', []):
            if not line.startswith('gamemode'):
                ctx.cmd(line)
        ctx.cmd(f'gamemode survival "{self.name}"')
        ctx.cmd('difficulty peaceful')
        (x0, z0), (x1, z1), floor = self.arena['min'], self.arena['max'], self.arena['floor']
        ctx.cmd(f'fill {x0} {floor - 2} {z0} {x1} {floor - 1} {z1} dirt')
        yield from self.reset(mark=False)
        start = self.job['arena']['player']
        ctx.cmd(f'tp "{self.name}" {start[0]} {start[1]} {start[2]} 0 0')
        yield 20

    def hold(self, item: str | None, count: int = 64):
        """Hotbar slot 0 (selected) holds `count` x `item`, or nothing. Set
        with vanilla commands: after an Endstone `item_in_main_hand` change
        the client's first click with the new stack was rejected (BDS
        resynced the clicked blocks without using the item)."""
        self.ctx.player.inventory.held_item_slot = 0
        self.note('select_slot', slot=0)
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
        _, face_click, _ = FACES[face]
        click = click or face_click
        target = [block[0] + click[0], block[1] + click[1], block[2] + click[2]]
        look = yield from self.stand(stand, target, yaw)
        before = self.cells(cells)
        label = self.mark(step)
        self.ctx.plugin.events.clear()
        self.use_order(block, face, click)
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
        if remove and found:
            self.note('remove_item_actors', items=[[i['item'], i['data'], i['count']] for i in found])
        return sorted(found, key=lambda item: (item['item'], item['position']))

    def remove(self, step: str, pos, cells: dict, ticks: int, block: str = 'air'):
        """`/setblock` `pos` to air (or `block`) and watch `cells`: their
        states in the same tick, every change after it, and the item actors
        spawned."""
        label = self.mark(step)
        command = f'setblock {pos[0]} {pos[1]} {pos[2]} {block}'
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
        _, face_click, _ = FACES[face]
        click = click or face_click
        target = [block[0] + click[0], block[1] + click[1], block[2] + click[2]]
        look = yield from self.stand(stand, target, yaw)
        before = self.cells(cells)
        label = self.mark(step)
        self.ctx.plugin.events.clear()
        origin = self.tick
        self.use_order(block, face, click)
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

    def mine_watch(self, step: str, target, face: str, stand, cells: dict, until=None, stop_after: int = 400,
                   after: int = 20):
        """Breaks `target` by hand from `stand` (looking at the centre of
        `face`): the client keeps breaking until `until(states)` holds or
        `stop_after` ticks passed, then gets `bds-sweep:stop`; `cells` are
        sampled every tick until `after` ticks later. Returns the changes,
        the interaction/break events and the item actors spawned."""
        click = FACES[face][1]
        look = yield from self.stand(stand, (target[0] + click[0], target[1] + click[1], target[2] + click[2]))
        before = self.cells(cells)
        label = self.mark(step)
        self.ctx.plugin.events.clear()
        origin = self.tick
        self.mine_order(target, face)
        changes, _ = yield from self.watch(cells, stop_after, origin, until=until)
        self.stop_order()
        more, final = yield from self.watch(cells, after, origin)
        return {'step': label, 'action': 'mine_watch', 'block': list(target), 'face': face, 'look': look,
                'cells': {k: list(v) for k, v in cells.items()}, 'origin_tick': origin, 'before': before,
                'changes': changes + more, 'after': final,
                'interactions': [[t - origin, k, list(w)] for t, k, w in self.ctx.plugin.events],
                'drops': self.items()}

    def palette(self, states: list[dict]) -> dict:
        """Runtime id -> block state of each `{name, states}` entry: sounds
        carry a block state's runtime id as their data."""
        found = {}
        for entry in states:
            data = self.ctx.server.create_block_data(entry['name'], entry['states'])
            found[str(data.runtime_id)] = {'name': data.type, 'states': {
                k: _json_value(v) for k, v in sorted(data.block_states.items())}}
        return found


# Ticks a scenario keeps running after its last step before the end marker,
# so late packets (block updates, sounds, item actors) land in its capture.
SETTLE_TICKS = 20


def blocks(ctx: Context, job: dict):
    """Block behaviour scenarios (doors, torches, crops; redstone and fire;
    utility blocks) on a flat stone arena, played by the survival sweep
    player through `bds-sweep:use` and `bds-sweep:mine`; one result row per
    scenario with the full block states of the watched cells after every
    step (per tick for the redstone, fire and utility scenarios). Each
    scenario runs after a `Blocks.prelude` between a `bds-sweep:scenario
    begin NAME` and `… end NAME` chat line (the capture is split at them)
    and writes `scenarios/NAME/steps.json`; a failing scenario is recorded
    with its error and the next one runs."""
    b = Blocks(ctx, job)
    ctx.must(f'gamemode survival "{job["player"]}"')
    for item in job['items']:
        key = item['key']
        if key in ctx.done:
            continue
        b.scenario = key
        prelude = ctx.steps = StepLog(ctx.plugin)
        yield from logged(b.prelude(), prelude)
        log = ctx.steps = StepLog(ctx.plugin)
        b.marker('begin')
        try:
            steps = yield from logged(SCENARIOS[key](b, item), log)
            row = {'key': key, 'steps': steps}
        except Exception as error:  # noqa: BLE001
            row = {'key': key, 'steps': [], 'error': f'{type(error).__name__}: {error}'}
        yield from logged(iter([SETTLE_TICKS]), log)
        b.marker('end')
        ctx.steps = None
        write_steps(ctx, job, item, prelude, log, row)
        if key in REDSTONE_SCENARIOS or key in UTILITY_SCENARIOS:
            row['palette'] = b.palette(job.get('palette_states', []))
        ctx.record(row)
    ctx.player.inventory.clear()
    ctx.must(f'gamemode creative "{job["player"]}"')
    # Let the last end marker reach the client before the runner stops it.
    yield 20


def scenario_summary(key: str) -> str:
    """The first paragraph of the scenario function's docstring."""
    import inspect
    doc = inspect.getdoc(SCENARIOS[key]) or ''
    return ' '.join(doc.split('\n\n', 1)[0].split())


def write_steps(ctx: Context, job: dict, item: dict, prelude: StepLog, log: StepLog, row: dict) -> None:
    """`<output_dir>/scenarios/<key>/steps.json`: how to reproduce the
    scenario's capture (world, prelude, every command, order and wait with
    its tick offset from the begin marker) and its outcome."""
    import datetime
    key = item['key']
    folder = ctx.out / 'scenarios' / key
    folder.mkdir(parents=True, exist_ok=True)
    arena = job['arena']['blocks']
    doc = {
        'scenario': key,
        'covers': scenario_summary(key),
        'minecraft_version': ctx.server.minecraft_version,
        'protocol_version': ctx.server.protocol_version,
        'recorded_utc': datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
        'world': {
            'level': 'superflat (default layers: bedrock y=-64, dirt y=-63..-62, grass y=-61), peaceful, '
                     'operators by default, view distance 6, tick distance 4',
            'arena': {'x': [arena['min'][0], arena['max'][0]], 'z': [arena['min'][1], arena['max'][1]],
                      'floor_y': arena['floor'], 'floor': 'stone', 'cleared_above': 8},
            'player': job['player'],
            'player_mode': 'survival; the plugin cancels every item pickup by the player',
            'job_setup': [f'tp "{job["player"]}" {" ".join(str(v) for v in job["arena"]["player"])}',
                          *job.get('setup_commands', []), f'gamemode survival "{job["player"]}"'],
        },
        'prelude': prelude.entries,
        'markers': {'begin': f'bds-sweep:scenario begin {key}', 'end': f'bds-sweep:scenario end {key}'},
        'tick_origin': 'server tick of the begin marker; `tick` = server ticks after it',
        'settle_ticks': SETTLE_TICKS,
        'parameters': {k: v for k, v in item.items() if k != 'key'},
        'steps': log.entries,
        'duration_ticks': log.now,
        'error': row.get('error'),
    }
    (folder / 'steps.json').write_text(json.dumps(doc, indent=1) + '\n', encoding='utf-8')


def _door_cells(pos) -> dict:
    return {'lower': tuple(pos), 'upper': (pos[0], pos[1] + 1, pos[2])}


def _floor_stand(cell, look: str):
    """Where to stand (feet) and the yaw to look `look` at the floor-top
    click that places into `cell`: two blocks away on the opposite side
    (north: from the south, yaw 180; south: from the north, yaw 0; east:
    from the west, yaw 270; west: from the east, yaw 90)."""
    x, y, z = cell
    return {'north': ((x + 0.5, y, z + 2.5), 180), 'south': ((x + 0.5, y, z - 1.5), 0),
            'east': ((x - 1.5, y, z + 0.5), 270), 'west': ((x + 2.5, y, z + 0.5), 90)}[look]


def _place_looking(b: Blocks, step: str, item: str, cell, look: str, cells: dict, click=None):
    """Holds `item` and clicks the top of the block under `cell` looking
    `look` (see `_floor_stand`)."""
    stand, yaw = _floor_stand(cell, look)
    yield from b.hold(item)
    return (yield from b.click(step, (cell[0], cell[1] - 1, cell[2]), 'up', stand, cells, yaw=yaw, click=click))


def _place_door(b: Blocks, step: str, cell, look: str, item: str = 'minecraft:wooden_door', click=None,
                extra_cells: dict | None = None):
    """Places a door into `cell` by clicking the top of the floor below it
    from two blocks away, looking `look`."""
    cells = {**_door_cells(cell), **(extra_cells or {})}
    return (yield from _place_looking(b, step, item, cell, look, cells, click=click))


def doors(b: Blocks, item: dict):
    """Oak doors placed looking north/east/south/west, with the hinge
    decided by the click half and by stone stacked on either side; a door
    pair (second door east of the first) toggled; one door toggled lower,
    upper, lower, upper with dirt in hand; an iron door clicked with an
    empty hand and with dirt (no-op); the upper and the lower half broken by
    hand; the floor under a door removed."""
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
        ('place_looking_south', 'south', [], None),
        ('place_looking_west', 'west', [], None),
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
    pair_cells = {**pair, 'second_lower': (1, y, 0), 'second_upper': (1, y + 1, 0)}
    yield from b.hold(None)
    for door, x in (('first', 0), ('second', 1)):
        for state in ('open', 'close'):
            steps.append((yield from b.click(f'pair_{door}_{state}', (x, y, 0), 'south', (0.5, y, 2.5),
                                             pair_cells)))

    # Toggling an oak door with dirt in hand (not sneaking).
    stand = (0.5, y, 2.5)
    watched = {**_door_cells(cell), 'in_front': (0, y, 1)}
    yield from b.reset()
    steps.append((yield from _place_door(b, 'toggle_place', cell, 'north')))
    yield from b.hold('minecraft:dirt')
    for index in range(3):
        steps.append((yield from b.click(f'toggle_lower_{index + 1}', cell, 'south', stand, watched)))
    steps.append((yield from b.click('toggle_upper_1', (0, y + 1, 0), 'south', stand, watched)))
    steps.append((yield from b.click('toggle_lower_4', cell, 'south', stand, watched)))
    steps.append((yield from b.click('toggle_upper_2', (0, y + 1, 0), 'south', stand, watched)))

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
        b.mine_order(target, 'south')
        changes, _ = yield from b.watch(
            _door_cells(cell), int(item.get('max_break_ticks', 400)), origin,
            until=lambda s: s['lower']['name'] == 'minecraft:air' and s['upper']['name'] == 'minecraft:air')
        b.stop_order()
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
    """Torches clicked onto the floor top, each side face of a stone block,
    the underside of an overhanging stone (refused), an oak fence top and a
    glass top; then the stone with the four side torches removed."""
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
    """Wheat seeds used on farmland (planted) and on dirt (refused); wheat
    growth on watered farmland at each `growth_runs` randomtickspeed."""
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


# Placement and survival scenarios added for the per-scenario captures:
# every support block is `/setblock` (checked by `_set`), every placement and
# toggle is the survival player's click, every support removal `/setblock`.

def trapdoors(b: Blocks, item: dict):
    """Oak trapdoors clicked onto the north face of a stone block (lower and
    upper click half), its south, west and east faces, the floor top and the
    underside of an overhanging stone; one toggled open and closed three
    times with an empty hand; an iron trapdoor clicked with an empty hand
    (no-op), then opened and closed by a lever on top of its support."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    yield from b.reset()
    # (step, clicked block, face, click, standing position)
    clicks = [
        ('north_face_lower', (0, y, 0), 'north', (0.5, 0.25, 0.0), (0.5, y, -2.5)),
        ('north_face_upper', (4, y, 0), 'north', (0.5, 0.75, 0.0), (4.5, y, -2.5)),
        ('south_face_lower', (8, y, 0), 'south', (0.5, 0.25, 1.0), (8.5, y, 3.5)),
        ('west_face_lower', (14, y, 0), 'west', (0.0, 0.25, 0.5), (11.5, y, 0.5)),
        ('east_face_lower', (18, y, 0), 'east', (1.0, 0.25, 0.5), (21.5, y, 0.5)),
        ('floor_top', (0, y - 1, 6), 'up', None, (0.5, y, 8.5)),
        ('overhang_bottom', (4, y + 2, 6), 'down', None, (4.5, y, 8.5)),
    ]
    _set(ctx, [(block, 'stone') for step, block, *_ in clicks if step != 'floor_top'])
    yield 2
    for step, block, face, click, stand in clicks:
        steps.append((yield from b.place(f'place_{step}', 'minecraft:trapdoor', block, face, stand, click=click)))

    trapdoor = (0, y, -1)
    yield from b.hold(None)
    for index in range(1, 4):
        for state in ('open', 'close'):
            steps.append((yield from b.click(f'toggle_{state}_{index}', trapdoor, 'up', (0.5, y, -3.5),
                                             {'trapdoor': trapdoor}, click=(0.5, 0.1875, 0.5))))

    support = (10, y, 6)
    iron = (10, y, 5)
    stand = (10.5, y, 3.5)
    _set(ctx, [(support, 'stone')])
    yield 2
    steps.append((yield from b.place('iron_place', 'minecraft:iron_trapdoor', support, 'north', stand,
                                     click=(0.5, 0.25, 0.0))))
    yield from b.hold(None)
    steps.append((yield from b.click('iron_click_empty_hand', iron, 'up', stand, {'trapdoor': iron},
                                     click=(0.5, 0.1875, 0.5))))
    placed, lever = yield from _lever_on_top(b, support, stand, 'iron_lever_place')
    steps.append(placed)
    yield from b.hold(None)
    cells = {'lever': lever, 'trapdoor': iron}
    for state in ('on', 'off'):
        steps.append((yield from b.use_watch(f'iron_lever_{state}', lever, 'up', stand, cells, 20,
                                             trigger='lever')))
    return steps


def fence_gates(b: Blocks, item: dict):
    """An oak fence gate placed looking north, opened and closed with an
    empty hand from the south and then from the north (it opens away from
    the player); a gate placed between two cobblestone walls; a cobblestone
    wall set beside a placed gate."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    yield from b.reset()
    gate = (0, y, 0)
    steps.append((yield from _place_looking(b, 'place_looking_north', 'minecraft:fence_gate', gate, 'north',
                                            {'gate': gate})))
    yield from b.hold(None)
    south, north = (0.5, y, 2.5), (0.5, y, -1.5)
    for step, stand, face in (('open_from_south', south, 'south'), ('close_from_south', south, 'south'),
                              ('open_from_north', north, 'north'), ('close_from_north', north, 'north')):
        steps.append((yield from b.click(step, gate, face, stand, {'gate': gate})))

    walled = (6, y, 0)
    _set(ctx, [((5, y, 0), 'cobblestone_wall'), ((7, y, 0), 'cobblestone_wall')])
    yield 2
    steps.append((yield from _place_looking(b, 'place_between_walls', 'minecraft:fence_gate', walled, 'north',
                                            {'gate': walled, 'wall_west': (5, y, 0), 'wall_east': (7, y, 0)})))
    beside = (12, y, 0)
    steps.append((yield from _place_looking(b, 'place_before_wall', 'minecraft:fence_gate', beside, 'north',
                                            {'gate': beside})))
    steps.append((yield from b.command_watch('wall_set_beside', [f'setblock 13 {y} 0 cobblestone_wall'],
                                             {'gate': beside, 'wall': (13, y, 0)}, 10)))
    return steps


def attachables(b: Blocks, item: dict):
    """Ladders, vines, stone buttons and levers clicked onto the north face
    of a stone block and onto the floor top (ladder and vine refused);
    buttons and levers under an overhanging stone; then every support
    removed with `/setblock` (the attached block breaks)."""
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    ticks = int(item.get('watch_ticks', 20))
    steps = []
    yield from b.reset()
    kinds = {'ladder': 0, 'vine': 4, 'stone_button': 8, 'lever': 12}
    _set(ctx, [((x, y, 0), 'stone') for x in kinds.values()]
         + [((x, y + 2, 6), 'stone') for name, x in kinds.items() if name in ('stone_button', 'lever')])
    yield 2
    for name, x in kinds.items():
        steps.append((yield from b.place(f'{name}_north_face', f'minecraft:{name}', (x, y, 0), 'north',
                                         (x + 0.5, y, -2.5))))
        steps.append((yield from b.place(f'{name}_floor_top', f'minecraft:{name}', (x, floor, -6), 'up',
                                         (x + 0.5, y, -3.5))))
        if name in ('stone_button', 'lever'):
            steps.append((yield from b.place(f'{name}_overhang_bottom', f'minecraft:{name}', (x, y + 2, 6), 'down',
                                             (x + 0.5, y, 8.5))))
    yield from b.hold(None)
    for name, x in kinds.items():
        steps.append((yield from b.remove(f'{name}_wall_support_removed', (x, y, 0), {'attached': (x, y, -1)},
                                          ticks)))
        if name in ('stone_button', 'lever'):
            steps.append((yield from b.remove(f'{name}_floor_removed', (x, floor, -6), {'attached': (x, y, -6)},
                                              ticks)))
            steps.append((yield from b.remove(f'{name}_overhang_removed', (x, y + 2, 6),
                                              {'attached': (x, y + 1, 6)}, ticks)))
    return steps


def signs(b: Blocks, item: dict):
    """Oak signs: standing on the floor top looking north and looking east,
    on the north face of a stone block (wall sign), hanging under an
    overhanging stone and on a stone block's north face; then each support
    removed with `/setblock` (the sign breaks and drops)."""
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    ticks = int(item.get('watch_ticks', 20))
    steps = []
    yield from b.reset()
    _set(ctx, [((6, y, 0), 'stone'), ((10, y + 2, 0), 'stone'), ((14, y, 0), 'stone')])
    yield 2
    steps.append((yield from _place_looking(b, 'standing_looking_north', 'minecraft:oak_sign', (0, y, 0), 'north',
                                            {'sign': (0, y, 0)})))
    steps.append((yield from _place_looking(b, 'standing_looking_east', 'minecraft:oak_sign', (3, y, 0), 'east',
                                            {'sign': (3, y, 0)})))
    steps.append((yield from b.place('wall_sign', 'minecraft:oak_sign', (6, y, 0), 'north', (6.5, y, -2.5))))
    steps.append((yield from b.place('hanging_under_overhang', 'minecraft:oak_hanging_sign', (10, y + 2, 0), 'down',
                                     (10.5, y, 2.5))))
    steps.append((yield from b.place('hanging_on_side', 'minecraft:oak_hanging_sign', (14, y, 0), 'north',
                                     (14.5, y, -2.5))))
    yield from b.hold(None)
    for step, support, sign in (('standing_floor_removed', (0, floor, 0), (0, y, 0)),
                                ('wall_support_removed', (6, y, 0), (6, y, -1)),
                                ('overhang_removed', (10, y + 2, 0), (10, y + 1, 0)),
                                ('hanging_side_support_removed', (14, y, 0), (14, y, -1))):
        steps.append((yield from b.remove(step, support, {'sign': sign}, ticks)))
    return steps


def _both_air(states) -> bool:
    return all(state['name'] == 'minecraft:air' for state in states.values())


def beds(b: Blocks, item: dict):
    """A bed placed looking north (foot at the origin, head north of it);
    the placement refused with a stone where the head would go; the foot
    and then the head half of a placed bed broken by hand (the other half
    goes too, one bed drops)."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    cell = (0, y, 0)
    cells = {'foot': cell, 'head': (0, y, -1)}
    yield from b.reset()
    steps.append((yield from _place_looking(b, 'place_looking_north', 'minecraft:bed', cell, 'north', cells)))
    yield from b.reset()
    _set(ctx, [((0, y, -1), 'stone')])
    yield 2
    steps.append((yield from _place_looking(b, 'place_head_blocked', 'minecraft:bed', cell, 'north', cells)))
    for half in ('foot', 'head'):
        yield from b.reset()
        steps.append((yield from _place_looking(b, f'break_{half}_place', 'minecraft:bed', cell, 'north', cells)))
        yield from b.hold(None)
        steps.append((yield from b.mine_watch(f'break_{half}_by_hand', cells[half], 'up', (0.5, y, 2.5), cells,
                                              until=_both_air, stop_after=200)))
    return steps


def stairs(b: Blocks, item: dict):
    """Oak stairs placed on the floor looking north, east, south and west;
    upside down on the upper half of a stone block's north face; corner
    shapes: a second stair placed looking east or west directly in front
    of or behind a north-facing stair."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    yield from b.reset()
    for x, look in ((0, 'north'), (3, 'east'), (6, 'south'), (9, 'west')):
        cell = (x, y, 0)
        steps.append((yield from _place_looking(b, f'place_looking_{look}', 'minecraft:oak_stairs', cell, look,
                                                {'stair': cell})))
    _set(ctx, [((12, y, 0), 'stone')])
    yield 2
    steps.append((yield from b.place('upside_down_north_face_upper', 'minecraft:oak_stairs', (12, y, 0), 'north',
                                     (12.5, y, -2.5), click=(0.5, 0.75, 0.0))))
    for x, dz, look in ((0, -1, 'east'), (4, 1, 'east'), (8, -1, 'west'), (12, 1, 'west')):
        first, second = (x, y, 6), (x, y, 6 + dz)
        where = 'in_front' if dz < 0 else 'behind'
        cells = {'first': first, 'second': second}
        steps.append((yield from _place_looking(b, f'corner_{look}_{where}_first', 'minecraft:oak_stairs', first,
                                                'north', cells)))
        steps.append((yield from _place_looking(b, f'corner_{look}_{where}_second', 'minecraft:oak_stairs', second,
                                                look, cells)))
    return steps


def slabs(b: Blocks, item: dict):
    """Oak slabs: a bottom slab on the floor top and a second one clicked
    onto its top (double slab); a top slab on the upper half of a stone
    block's north face and a second one clicked into it from the floor
    below (double slab); an oak slab clicked onto a smooth stone slab (no
    merge, placed above)."""
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    steps = []
    yield from b.reset()
    _set(ctx, [((4, y, 0), 'stone'), ((8, y, 0), 'smooth_stone_slab')])
    yield 2
    bottom = (0, y, 0)
    cells = {'slab': bottom, 'above': (0, y + 1, 0)}
    steps.append((yield from _place_looking(b, 'bottom_on_floor', 'minecraft:oak_slab', bottom, 'north', cells)))
    steps.append((yield from b.click('merge_onto_bottom', bottom, 'up', (0.5, y, 2.5), cells,
                                     click=(0.5, 0.5, 0.5))))
    top = (4, y, -1)
    steps.append((yield from b.place('top_on_north_face_upper', 'minecraft:oak_slab', (4, y, 0), 'north',
                                     (4.5, y, -2.5), click=(0.5, 0.75, 0.0))))
    steps.append((yield from b.click('merge_into_top_from_below', (4, floor, -1), 'up', (4.5, y, -3.5),
                                     {'slab': top, 'clicked': (4, floor, -1)})))
    steps.append((yield from b.click('onto_smooth_stone_slab', (8, y, 0), 'up', (8.5, y, 2.5),
                                     {'smooth': (8, y, 0), 'above': (8, y + 1, 0)}, click=(0.5, 0.5, 0.5))))
    return steps


def connections(b: Blocks, item: dict):
    """Oak fences, cobblestone walls and glass panes clicked onto the floor
    one after another along +x; a stone set north of the first; a fourth
    piece south of the second (a junction); the second removed: the
    connection states of every piece after each step."""
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    ticks = int(item.get('watch_ticks', 10))
    steps = []
    yield from b.reset()
    for name, x0 in (('oak_fence', 0), ('cobblestone_wall', 6), ('glass_pane', 12)):
        cells = {'first': (x0, y, 0), 'second': (x0 + 1, y, 0), 'third': (x0 + 2, y, 0),
                 'north_of_first': (x0, y, -1), 'junction': (x0 + 1, y, 1)}
        yield from b.hold(f'minecraft:{name}')
        for index, key in enumerate(('first', 'second', 'third')):
            x = x0 + index
            steps.append((yield from b.click(f'{name}_{key}', (x, floor, 0), 'up', (x + 0.5, y, 3.5), cells)))
        steps.append((yield from b.command_watch(f'{name}_stone_north_of_first',
                                                 [f'setblock {x0} {y} -1 stone'], cells, ticks)))
        steps.append((yield from b.click(f'{name}_junction', (x0 + 1, floor, 1), 'up', (x0 + 1.5, y, 3.5), cells)))
        steps.append((yield from b.remove(f'{name}_second_removed', (x0 + 1, y, 0), cells, ticks)))
    return steps


def crop_care(b: Blocks, item: dict):
    """Wheat (growth 0) on farmland given bone meal three times; ripe wheat
    (growth 7) and growth 3 wheat broken by hand (drops); the player falling
    2 blocks onto farmland (trampled to dirt); bare dry farmland (moisture
    7, no water) and farmland under wheat at randomtickspeed `dry_speed`
    until the bare one turns to dirt."""
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    steps = []
    yield from b.reset()
    _set(ctx, [((x, floor, 0), 'farmland') for x in (0, 4, 8, 12)] + [((x, y, 0), 'wheat') for x in (0, 4, 8)])
    _setblock(ctx, (4, y, 0), 'wheat', {'growth': 7})
    _setblock(ctx, (8, y, 0), 'wheat', {'growth': 3})
    yield 2
    cells = {'wheat': (0, y, 0), 'farmland': (0, floor, 0)}
    yield from b.hold('minecraft:bone_meal')
    for use in (1, 2, 3):
        steps.append((yield from b.click(f'bone_meal_{use}', (0, y, 0), 'up', (0.5, y, 2.5), cells,
                                         click=(0.5, 0.25, 0.5))))
    yield from b.hold(None)
    for x, growth in ((4, 7), (8, 3)):
        steps.append((yield from b.mine_watch(f'break_growth_{growth}', (x, y, 0), 'up', (x + 0.5, y, 2.5),
                                              {'wheat': (x, y, 0), 'farmland': (x, floor, 0)},
                                              until=lambda s: s['wheat']['name'] == 'minecraft:air',
                                              stop_after=40)))

    # Trampling: the player is teleported 2 blocks above farmland and falls
    # in the same tick (the fall order follows the teleport to the client).
    land = (12, floor, 0)
    cells = {'farmland': land, 'above': (12, y, 0)}
    yield from b.stand((12.5, y, 3.5), (12.5, y, 0.5))
    before = b.cells(cells)
    label = b.mark('trample_fall_2')
    origin = b.tick
    ctx.must(f'tp "{b.name}" 12.5 {y + 2} 0.5 0 30')
    b.fall_order(2)
    changes, final = yield from b.watch(cells, 40, origin)
    location = ctx.player.location
    steps.append({'step': label, 'action': 'fall', 'distance': 2, 'from': [12.5, y + 2, 0.5],
                  'cells': {k: list(v) for k, v in cells.items()}, 'origin_tick': origin, 'before': before,
                  'changes': changes, 'after': final,
                  'player_after': [round(location.x, 3), round(location.y, 3), round(location.z, 3)]})
    yield from b.stand((0.5, y, 4.5), (0.5, y, 0.5))

    # Drying: no water anywhere near.
    bare, planted = (16, floor, 0), (20, floor, 0)
    _setblock(ctx, bare, 'farmland', {'moisturized_amount': 7})
    _setblock(ctx, planted, 'farmland', {'moisturized_amount': 7})
    _set(ctx, [((20, y, 0), 'wheat')])
    yield 2
    cells = {'bare': bare, 'planted': planted, 'wheat': (20, y, 0)}
    speed = int(item.get('dry_speed', 300))
    steps.append((yield from act(b, f'drying_randomtickspeed_{speed}', cells, int(item.get('dry_ticks', 2400)),
                                 commands=[f'gamerule randomtickspeed {speed}'],
                                 until=lambda s, t, log: s['bare']['name'] == 'minecraft:dirt')))
    ctx.cmd('gamerule randomtickspeed 0')
    return steps


def plant_survival(b: Blocks, item: dict):
    """Sugar cane clicked onto sand beside water (twice: two high) and onto
    stone (refused); the sand under one cane removed, the water beside
    another removed. Cactus clicked onto sand; a stone set beside it; the
    sand under a second cactus removed."""
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    ticks = int(item.get('watch_ticks', 40))
    steps = []
    yield from b.reset()
    _set(ctx, [((0, floor, 0), 'sand'), ((1, floor, 0), 'water'), ((6, floor, 0), 'sand'), ((7, floor, 0), 'water'),
               ((10, floor, 0), 'sand'), ((14, floor, 0), 'sand')])
    yield 2
    yield from b.hold('minecraft:sugar_cane')
    for x in (0, 6):
        cells = {'lower': (x, y, 0), 'upper': (x, y + 1, 0)}
        steps.append((yield from b.click(f'cane_on_sand_{x}', (x, floor, 0), 'up', (x + 0.5, y, 2.5), cells)))
        steps.append((yield from b.click(f'cane_on_cane_{x}', (x, y, 0), 'up', (x + 0.5, y, 2.5), cells)))
    steps.append((yield from b.click('cane_on_stone', (3, floor, 0), 'up', (3.5, y, 2.5),
                                     {'target': (3, y, 0)})))
    yield from b.hold('minecraft:cactus')
    for x in (10, 14):
        steps.append((yield from b.click(f'cactus_on_sand_{x}', (x, floor, 0), 'up', (x + 0.5, y, 2.5),
                                         {'cactus': (x, y, 0)})))
    yield from b.hold(None)
    steps.append((yield from b.remove('cane_sand_removed', (0, floor, 0),
                                      {'lower': (0, y, 0), 'upper': (0, y + 1, 0)}, ticks)))
    steps.append((yield from b.remove('cane_water_removed', (7, floor, 0),
                                      {'lower': (6, y, 0), 'upper': (6, y + 1, 0)}, ticks)))
    steps.append((yield from b.remove('cactus_stone_beside', (11, y, 0), {'cactus': (10, y, 0)}, ticks,
                                      block='stone')))
    steps.append((yield from b.remove('cactus_sand_removed', (14, floor, 0), {'cactus': (14, y, 0)}, ticks)))
    return steps


def falling_blocks(b: Blocks, item: dict):
    """Sand clicked onto the north face of a stone block two blocks above the
    floor (falls onto the floor); sand set in the air with `/setblock`;
    gravel set above a torch (breaks on it, drops as an item); white
    concrete powder set above a water source in the floor (falls into it)
    and set beside a second water source in the floor."""
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    ticks = int(item.get('fall_ticks', 60))
    steps = []
    yield from _ureset(b)
    _set(ctx, [((0, y + 2, 0), 'stone'), ((12, floor, 0), 'water'), ((16, floor, 0), 'water')])
    _setblock(ctx, (8, y, 0), 'torch', {'torch_facing_direction': 'top'})
    yield 2
    column = _region((0,), range(y, y + 3), (-1,))
    yield from b.hold('minecraft:sand')
    steps.append((yield from act(b, 'sand_placed_on_wall', column, ticks,
                                 use=_use((0, y + 2, 0), 'north', (0.5, y, -2.5)))))
    yield from b.hold(None)
    for step, x, block, bottom in (('sand_set_in_air', 4, 'sand', y), ('gravel_onto_torch', 8, 'gravel', y),
                                   ('powder_into_water', 12, 'white_concrete_powder', floor)):
        top = y + 4
        steps.append((yield from act(b, step, _region((x,), range(bottom, top + 1), (0,)), ticks,
                                     commands=[f'setblock {x} {top} 0 {block}'])))
    steps.append((yield from act(b, 'powder_beside_water', {'powder': (17, floor, 0), 'water': (16, floor, 0)}, 10,
                                 commands=[f'setblock 17 {floor} 0 white_concrete_powder'])))
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
    """Repeaters set to delays 1-4 (clicked) between a lever and a lamp,
    the lever switched on and off; a repeater locked from the side by a
    second powered repeater, toggled locked and unlocked."""
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
    """A redstone torch on a stone block with a lever on top inverts the
    lever (lamp beside the torch); a torch feeding its own block through
    dust burns out (clock)."""
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
    return (yield from _plates(b, item, {'stone': 'stone_pressure_plate', 'oak': 'wooden_pressure_plate'},
                               items=('oak', 'stone'), players=('stone', 'oak')))


def weighted_plates(b: Blocks, item: dict):
    """Light and heavy weighted pressure plates on the floor, each with a
    lamp east of it, the same steps as `pressure_plates`: an item
    teleported onto each plate and killed, then the player teleported onto
    and off each plate (two dwell times)."""
    return (yield from _plates(b, item, {'light': 'light_weighted_pressure_plate',
                                         'heavy': 'heavy_weighted_pressure_plate'},
                               items=('light', 'heavy'), players=('light', 'heavy')))


def _plates(b: Blocks, item: dict, kinds: dict, items: tuple, players: tuple):
    """`kinds` = {label: plate block}: the first plate at the origin, the
    second 4 blocks south; `items` and `players` are the plate orders of the
    item and the player steps."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    ticks = int(item.get('watch_ticks', 60))
    name = b.name
    steps = []
    yield from b.reset()
    plates = {label: (0, y, 4 * index) for index, label in enumerate(kinds)}
    _set(ctx, [entry for label, block in kinds.items()
               for entry in ((plates[label], block), ((1, y, plates[label][2]), 'redstone_lamp'))]
         + [((6, y, 8), 'dirt')])
    yield 2
    cells = {}
    for label, pos in plates.items():
        cells.update({f'{label}_plate': pos, f'{label}_lamp': (1, y, pos[2])})
    away = (4.5, y, 2.5)
    yield from b.hold(None)
    yield from b.stand(away, (0.5, y, 2.5))
    for plate in items:
        x, _, z = plates[plate]
        ctx.must(f'execute as @a[name="{name}"] run loot spawn 10.5 {y + 0.1} 10.5 mine 6 {y} 8 mainhand')
        yield 40
        steps.append((yield from b.command_watch(
            f'{plate}_item_on', [f'tp @e[type=item] {x + 0.5} {y + 0.05} {z + 0.5}'], cells, ticks,
            trigger=f'{plate}_plate')))
        steps[-1]['items'] = b.items(remove=False)
        steps.append((yield from b.command_watch(f'{plate}_item_killed', ['kill @e[type=item]'], cells, ticks)))
    for plate in players:
        x, _, z = plates[plate]
        for trial, dwell in enumerate((ticks, ticks - 13), start=1):
            steps.append((yield from b.command_watch(
                f'{plate}_player_on_{trial}', [f'tp "{name}" {x + 0.5} {y} {z + 0.5}'], cells, dwell,
                trigger=f'{plate}_plate')))
            steps.append((yield from b.command_watch(
                f'{plate}_player_off_{trial}', [f'tp "{name}" {away[0]} {away[1]} {away[2]}'], cells, ticks)))
    return steps


def comparator_container(b: Blocks, item: dict):
    """A comparator clicked into place looking east with a chest behind it
    and two dust after it; the chest given 1 item, then 14 full stacks, then
    27 (full), then removed: the comparator output follows the fill level."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    ticks = int(item.get('watch_ticks', 20))
    cx = 8
    chest = (cx - 1, y, 0)
    steps = []
    yield from b.reset()
    steps.append((yield from b.place('place', 'minecraft:comparator', (cx, y - 1, 0), 'up', (cx + 0.5, y, -2.5),
                                     yaw=270)))
    yield from b.hold(None)
    _set(ctx, [(chest, 'chest'), ((cx + 1, y, 0), 'redstone_wire'), ((cx + 2, y, 0), 'redstone_wire')])
    yield 4
    cells = {'chest': chest, 'comparator': (cx, y, 0), 'out_1': (cx + 1, y, 0), 'out_2': (cx + 2, y, 0)}
    slot = f'replaceitem block {chest[0]} {chest[1]} {chest[2]} slot.container'
    for step, commands in (('one_item', [f'{slot} 0 stone 1']),
                           ('fourteen_stacks', [f'{slot} {i} stone 64' for i in range(14)]),
                           ('full', [f'{slot} {i} stone 64' for i in range(14, 27)]),
                           ('chest_removed', [f'setblock {chest[0]} {chest[1]} {chest[2]} air'])):
        steps.append((yield from b.command_watch(step, commands, cells, ticks)))
    return steps


# Horizontal neighbours of a piston by name.
SIDES = {'north': (0, 0, -1), 'south': (0, 0, 1), 'west': (-1, 0, 0), 'east': (1, 0, 0)}


def pistons(b: Blocks, item: dict):
    """A piston and a sticky piston clicked into place from three blocks
    east looking west (they face east), powered and unpowered by a redstone
    block set on top: nothing in front, one stone (pushed; the sticky
    piston pulls it back), twelve stones (pushed), thirteen (refused),
    obsidian (refused) and a torch in front (broken, drops)."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    ticks = int(item.get('watch_ticks', 30))
    steps = []
    yield from _ureset(b)
    cases = {
        'piston': [('push_1', ['stone'], None), ('push_12', ['stone'] * 12, None),
                   ('push_13_refused', ['stone'] * 13, None), ('obsidian_refused', ['obsidian'], None),
                   ('torch_broken', ['torch'], {'torch_facing_direction': 'top'})],
        'sticky_piston': [('pull_1', ['stone'], None), ('push_12', ['stone'] * 12, None)],
    }
    for kind, z in (('piston', 0), ('sticky_piston', 8)):
        pos = (5, y, z)
        placed = yield from _place(b, f'{kind}_place', f'minecraft:{kind}', (pos[0], y - 1, z), 'up',
                                   (pos[0] + 3.5, y, z + 2.5), {'piston': pos}, yaw=90)
        steps.append(placed)
        yield from b.hold(None)
        around = {name: _add(pos, offset) for name, offset in SIDES.items()}
        power = f'setblock {pos[0]} {y + 1} {z} redstone_block'
        unpower = f'setblock {pos[0]} {y + 1} {z} air'
        extended = yield from act(b, f'{kind}_extend_empty', {'piston': pos, **around}, ticks, commands=[power])
        steps.append(extended)
        steps.append((yield from act(b, f'{kind}_retract_empty', {'piston': pos, **around}, ticks,
                                     commands=[unpower])))
        front = next((SIDES[name] for name in SIDES if 'arm_collision' in extended['after'][name]['name']), None)
        if front is None:
            raise RuntimeError(f'{kind} did not extend sideways: {placed["after"]["piston"]}')
        line = [_add(pos, front, i) for i in range(1, 15)]
        cells = {'piston': pos, **{f'line_{i:02}': cell for i, cell in enumerate(line, start=1)}}
        clear = f'fill {line[0][0]} {y} {line[0][2]} {line[-1][0]} {y} {line[-1][2]} air'
        for case, blocks, states in cases[kind]:
            for cell, block in zip(line, blocks):
                _setblock(ctx, cell, block, states)
            yield 4
            steps.append((yield from act(b, f'{kind}_{case}_on', cells, ticks, commands=[power])))
            steps.append((yield from act(b, f'{kind}_{case}_off', cells, ticks, commands=[unpower])))
            ctx.cmd(clear)
            b.items()
            yield 4
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
        b.mine_order(pos, 'up')
        changes, _ = yield from b.watch(watched, 5, origin)
        b.stop_order()
        more, final = yield from b.watch(watched, 20, origin)
        steps.append({'step': label, 'action': 'mine_watch', 'block': list(pos), 'face': 'up', 'look': look,
                      'stopped_after': 5, 'ticks': 25, 'cells': {k: list(v) for k, v in watched.items()},
                      'trigger': None,
                      'origin_tick': origin, 'before': before, 'changes': changes + more, 'after': final,
                      'interactions': [[t - origin, k, list(w)] for t, k, w in ctx.plugin.events]})
    ctx.must('gamerule randomtickspeed 0')
    return steps


# Utility scenarios: interactive blocks (observers, dispensers, TNT, note
# blocks, rails, composters, cauldrons, campfires, beehives, bells,
# jukeboxes, flower pots, cake, end and nether portals, respawn anchors,
# beds). Every step is an `act`: the watched cells every tick from the
# order or command, the non-player actors that appeared or went away and
# the held item after the step. The player has resistance V so explosions
# leave it alive.

# facing_direction (0 down, 1 up, 2 north, 3 south, 4 west, 5 east) -> offset.
FACING_OFFSETS = {0: (0, -1, 0), 1: (0, 1, 0), 2: (0, 0, -1), 3: (0, 0, 1), 4: (-1, 0, 0), 5: (1, 0, 0)}


def _add(pos, offset, scale: int = 1):
    return (pos[0] + offset[0] * scale, pos[1] + offset[1] * scale, pos[2] + offset[2] * scale)


def _setblock(ctx: Context, pos, block: str, states: dict | None = None) -> None:
    """`/setblock` with an optional state map; checks the block name only
    (`setblock` fails when the identical block is already there)."""
    spec = block
    if states:
        spec += ' [' + ','.join(f'"{k}"={json.dumps(v)}' for k, v in states.items()) + ']'
    ctx.cmd(f'setblock {pos[0]} {pos[1]} {pos[2]} {spec}')
    if ctx.block(pos).type != f'minecraft:{block}':
        raise RuntimeError(f'setblock {pos} {spec} left {ctx.block(pos).type}')


def _vec(v) -> list[float]:
    return [round(v.x, 3), round(v.y, 3), round(v.z, 3)]


def _region(x_range, y_range, z_range) -> dict:
    return {f'{x},{y},{z}': (x, y, z) for x in x_range for y in y_range for z in z_range}


def _actors(ctx: Context) -> dict:
    """Every non-player actor: type, position, velocity (and the stack of
    item actors), by unique id."""
    found = {}
    for actor in ctx.dimension.actors:
        if actor.type == 'minecraft:player':
            continue
        info = {'type': actor.type, 'position': _vec(actor.location), 'velocity': _vec(actor.velocity)}
        if actor.type == 'minecraft:item':
            stack = actor.item_stack
            info.update(item=stack.type.id, data=stack.data, count=stack.amount)
        found[str(actor.id)] = info
    return found


def _held(ctx: Context):
    held = ctx.player.inventory.item_in_main_hand
    return [held.type.id, held.amount, held.data] if held is not None else None


def act(b: Blocks, step: str, cells: dict, ticks: int, use: dict | None = None, commands=(), schedule=(),
        until=None):
    """One utility step. `use` = {block, face, stand, yaw?, click?}: the
    player right-clicks with the held item; `commands` run in the same tick;
    `schedule` = [(tick, command)] run that many ticks later. The watched
    `cells` and the actors are read every tick for `ticks` ticks or until
    `until(states, elapsed, actor_log)`."""
    ctx = b.ctx
    look = None
    if use:
        _, face_click, _ = FACES[use['face']]
        click = use.get('click') or face_click
        block = use['block']
        target = [block[0] + click[0], block[1] + click[1], block[2] + click[2]]
        look = yield from b.stand(use['stand'], target, use.get('yaw'))
    before = b.cells(cells)
    seen = _actors(ctx)
    label = b.mark(step)
    ctx.plugin.events.clear()
    origin = b.tick
    if use:
        b.use_order(block, use['face'], click)
    succeeded = [ctx.cmd(command) for command in commands]
    last = b.cells(cells)
    changes = [[0, k, last[k]] for k in cells if last[k] != before[k]]
    log = []
    pending = sorted(schedule)
    ran = []
    elapsed = 0
    for _ in range(ticks):
        yield 1
        elapsed = b.tick - origin
        while pending and pending[0][0] <= elapsed:
            at, command = pending.pop(0)
            ran.append([elapsed, command, ctx.cmd(command)])
        now = b.cells(cells)
        for key in cells:
            if now[key] != last[key]:
                changes.append([elapsed, key, now[key]])
        current = _actors(ctx)
        for actor_id, info in current.items():
            if actor_id not in seen:
                log.append({'tick': elapsed, 'event': 'appeared', 'id': actor_id, **info})
        for actor_id, info in seen.items():
            if actor_id not in current:
                log.append({'tick': elapsed, 'event': 'gone', 'id': actor_id, 'type': info['type'],
                            'last_position': info['position']})
        seen, last = current, now
        if until is not None and until(now, elapsed, log):
            break
    row = {'step': label, 'action': 'act', 'cells': {k: list(v) for k, v in cells.items()}, 'origin_tick': origin,
           'ticks': elapsed, 'before': before, 'changes': changes, 'after': last, 'actors': log,
           'actors_after': seen, 'held_after': _held(ctx),
           'interactions': [[t - origin, k, list(w)] for t, k, w in ctx.plugin.events]}
    if use:
        row.update(block=list(block), face=use['face'], click=list(click), look=look)
    if commands:
        row.update(commands=list(commands), succeeded=succeeded)
    if ran:
        row['scheduled'] = ran
    return row


def _ureset(b: Blocks):
    yield from b.reset()
    b.ctx.cmd('kill @e[type=!player]')
    b.ctx.cmd(f'effect "{b.name}" resistance 99999 4 true')
    yield 2


def _use(block, face: str, stand, yaw: float | None = None, click=None) -> dict:
    return {'block': block, 'face': face, 'stand': stand, 'yaw': yaw, 'click': click}


def _place(b: Blocks, step: str, item: str, support, face: str, stand, cells: dict, yaw: float | None = None,
           ticks: int = 10, count: int = 64):
    """Holds `count` x `item` and clicks `face` of `support`; watches `cells`."""
    yield from b.hold(item, count)
    return (yield from act(b, step, cells, ticks, use=_use(support, face, stand, yaw)))


def _lever(b: Blocks, step: str, support, stand, cells: dict):
    """A lever clicked onto the top of `support`; returns the step and the
    lever cell."""
    lever = _add(support, (0, 1, 0))
    placed = yield from _place(b, step, 'minecraft:lever', support, 'up', stand, {'lever': lever, **cells})
    return placed, lever


def observer(b: Blocks, item: dict):
    """Observers set facing east: one with the changed block east of it, one
    with the changed block west of it; then two changes one tick apart."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    yield from _ureset(b)
    _setblock(ctx, (0, y, 0), 'observer', {'minecraft:facing_direction': 'east'})
    _setblock(ctx, (1, y, 0), 'stone')
    _setblock(ctx, (0, y, 3), 'observer', {'minecraft:facing_direction': 'east'})
    _setblock(ctx, (-1, y, 3), 'stone')
    yield 10
    yield from b.stand((4.5, y, 1.5), (0.5, y, 1.5))
    cells = {'observer_a': (0, y, 0), 'east_of_a': (1, y, 0), 'observer_b': (0, y, 3), 'west_of_b': (-1, y, 3)}
    steps.append((yield from act(b, 'change_east_of_a', cells, 12, commands=[f'setblock 1 {y} 0 dirt'])))
    steps.append((yield from act(b, 'change_west_of_b', cells, 12, commands=[f'setblock -1 {y} 3 dirt'])))
    steps.append((yield from act(b, 'two_changes_east_of_a', cells, 12, commands=[f'setblock 1 {y} 0 stone'],
                                 schedule=[(1, f'setblock 1 {y} 0 dirt')])))
    return steps


def _place_facing(b: Blocks, step: str, item: str, pos):
    """Places `item` into `pos` by clicking the floor top from three blocks
    east, looking west; returns the step and the facing offset placed."""
    stand = (pos[0] + 3.5, pos[1], pos[2] + 2.5)
    placed = yield from _place(b, step, item, _add(pos, (0, -1, 0)), 'up', stand, {'placed': pos}, yaw=90)
    facing = placed['after']['placed']['states'].get('facing_direction')
    return placed, FACING_OFFSETS.get(facing)


def dispensers(b: Blocks, item: dict):
    """A dispenser placed by the player (looking west) holding arrows, a
    water bucket or stone; a stone block behind it with a lever on top. A
    dropper the same way with a chest in front. Containers are destroyed
    with `setblock … air destroy` at the end to show their contents."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    pos = (0, y, 0)
    stand = (3.5, y, 2.5)
    for content, count in (('arrow', 3), ('water_bucket', 1), ('stone', 2), ('dropper', 2)):
        yield from _ureset(b)
        block = 'dropper' if content == 'dropper' else 'dispenser'
        prefix = content
        placed, facing = yield from _place_facing(b, f'{prefix}_place', f'minecraft:{block}', pos)
        steps.append(placed)
        if facing is None or facing[1] != 0:
            continue
        front, back = _add(pos, facing), _add(pos, facing, -1)
        side = _add(pos, (facing[2], 0, facing[0]))
        if content == 'dropper':
            _setblock(ctx, front, 'chest')
        ctx.must(f'replaceitem block {pos[0]} {pos[1]} {pos[2]} slot.container 0 '
                 f'{"stone" if content == "dropper" else content} {count}')
        _setblock(ctx, back, 'stone')
        yield 4
        cells = {block: pos, 'front': front, 'front_2': _add(pos, facing, 2), 'back': back, 'side': side}
        lever_step, lever = yield from _lever(b, f'{prefix}_lever_place', back, stand, cells)
        steps.append(lever_step)
        cells['lever'] = lever
        yield from b.hold(None)
        flip = _use(lever, 'up', stand)
        steps.append((yield from act(b, f'{prefix}_lever_on', cells, 30, use=flip)))
        if content == 'arrow':
            steps.append((yield from act(b, 'arrow_second_power_while_on', cells, 20,
                                         commands=[f'setblock {side[0]} {side[1]} {side[2]} redstone_block'])))
            steps.append((yield from act(b, 'arrow_lever_off_still_powered', cells, 20, use=flip)))
            steps.append((yield from act(b, 'arrow_unpowered', cells, 20,
                                         commands=[f'setblock {side[0]} {side[1]} {side[2]} air'])))
            steps.append((yield from act(b, 'arrow_lever_on_again', cells, 30, use=flip)))
        elif content == 'water_bucket':
            steps.append((yield from act(b, 'water_bucket_lever_off', cells, 20, use=flip)))
            steps.append((yield from act(b, 'water_bucket_lever_on_again', cells, 30, use=flip)))
        else:
            steps.append((yield from act(b, f'{prefix}_lever_off', cells, 20, use=flip)))
        if content == 'dropper':
            steps.append((yield from act(b, 'dropper_chest_destroyed', cells, 10,
                                         commands=[f'setblock {front[0]} {front[1]} {front[2]} air destroy'])))
        steps.append((yield from act(b, f'{prefix}_container_destroyed', cells, 10,
                                     commands=[f'setblock {pos[0]} {pos[1]} {pos[2]} air destroy'])))
    return steps


def _tnt_cells(center, floor: int) -> dict:
    return _region(range(center[0] - 3, center[0] + 4), range(floor - 1, floor + 2),
                   range(center[2] - 3, center[2] + 4))


def _no_tnt_left(states, elapsed, log) -> bool:
    appeared = {e['id'] for e in log if e['event'] == 'appeared' and e['type'] == 'minecraft:tnt'}
    gone = {e['id'] for e in log if e['event'] == 'gone'}
    return bool(appeared) and appeared <= gone and elapsed >= max(e['tick'] for e in log) + 10


def tnt(b: Blocks, item: dict):
    """TNT on the floor at the origin: lit with flint and steel; lit by a
    lever on a stone block next to it; primed by the explosion of a summoned
    primed TNT two blocks away. The player walks off after the click; the
    7x3x7 cells around the TNT are watched (floor-1 .. floor+1)."""
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    ticks = int(item.get('tnt_ticks', 160))
    away = f'tp "{b.name}" 0.5 {y} 18.5'
    steps = []
    tnt_pos = (0, y, 0)
    cells = _tnt_cells(tnt_pos, floor)

    yield from _ureset(b)
    _setblock(ctx, tnt_pos, 'tnt')
    yield 2
    yield from b.hold('minecraft:flint_and_steel', 1)
    steps.append((yield from act(b, 'flint_and_steel', cells, ticks, use=_use(tnt_pos, 'south', (0.5, y, 3.5)),
                                 schedule=[(5, away)], until=_no_tnt_left)))

    yield from _ureset(b)
    _setblock(ctx, tnt_pos, 'tnt')
    _setblock(ctx, (-1, y, 0), 'stone')
    yield 2
    stand = (-0.5, y, 3.5)
    lever_step, lever = yield from _lever(b, 'lever_place', (-1, y, 0), stand, {})
    steps.append(lever_step)
    yield from b.hold(None)
    steps.append((yield from act(b, 'lever_on', cells, ticks, use=_use(lever, 'up', stand),
                                 schedule=[(5, away)], until=_no_tnt_left)))

    yield from _ureset(b)
    _setblock(ctx, (2, y, 0), 'tnt')
    yield 2
    yield from b.stand((0.5, y, 18.5), (0.5, y, 0.5))
    steps.append((yield from act(b, 'primed_by_explosion', {**cells, **_tnt_cells((2, y, 0), floor)}, ticks,
                                 commands=[f'summon tnt 0.5 {y} 0.5'], until=_no_tnt_left)))
    return steps


def note_block_use(b: Blocks, item: dict):
    """Note block on dirt: three empty-hand clicks, then a lever on the
    east face of a stone block east of it, on and off."""
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    steps = []
    yield from _ureset(b)
    _setblock(ctx, (0, floor, 0), 'dirt')
    _setblock(ctx, (0, y, 0), 'noteblock')
    _setblock(ctx, (1, y, 0), 'stone')
    yield 2
    cells = {'note_block': (0, y, 0), 'under': (0, floor, 0), 'lever': (2, y, 0)}
    yield from b.hold(None)
    for click in (1, 2, 3):
        steps.append((yield from act(b, f'use_{click}', cells, 6, use=_use((0, y, 0), 'south', (0.5, y, 2.5)))))
    stand = (3.5, y, 2.5)
    steps.append((yield from _place(b, 'lever_place', 'minecraft:lever', (1, y, 0), 'east', stand, cells)))
    yield from b.hold(None)
    for state in ('on', 'off'):
        steps.append((yield from act(b, f'lever_{state}', cells, 10,
                                     use=_use((2, y, 0), 'east', stand, click=(0.25, 0.5, 0.5)))))
    return steps


def rails(b: Blocks, item: dict):
    """Rails clicked onto the floor: corner (0,0), then east (1,0), then
    south (0,1); a minecart placed on the south rail. Ten golden rails along
    +x at z=6 (/setblock, east-west) next to a stone block with a lever."""
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    steps = []
    yield from _ureset(b)
    cells = {'corner': (0, y, 0), 'east': (1, y, 0), 'south': (0, y, 1)}
    stand = (3.5, y, 3.5)
    for key in ('corner', 'east', 'south'):
        x, _, z = cells[key]
        steps.append((yield from _place(b, f'rail_{key}', 'minecraft:rail', (x, floor, z), 'up', stand, cells)))
    steps.append((yield from _place(b, 'minecart', 'minecraft:minecart', cells['south'], 'up', stand, cells,
                                    ticks=20, count=1)))

    golden = {f'golden_{x:02}': (x, y, 6) for x in range(1, 11)}
    _setblock(ctx, (0, y, 6), 'stone')
    for pos in golden.values():
        _setblock(ctx, pos, 'golden_rail', {'rail_direction': 1})
    yield 4
    stand = (0.5, y, 9.5)
    lever_step, lever = yield from _lever(b, 'golden_lever_place', (0, y, 6), stand, golden)
    steps.append(lever_step)
    golden['lever'] = lever
    yield from b.hold(None)
    for state in ('on', 'off'):
        steps.append((yield from act(b, f'golden_lever_{state}', golden, 20, use=_use(lever, 'up', stand))))
    return steps


def _fill(states) -> int:
    return states['composter']['states']['composter_fill_level']


def composter(b: Blocks, item: dict):
    """Ten wheat seed uses on an empty composter; then a composter set to
    level 6 used with seeds until it reaches 7 (watched until 8); then used
    at 8 with an empty hand."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    yield from _ureset(b)
    _setblock(ctx, (0, y, 0), 'composter')
    yield 2
    cells = {'composter': (0, y, 0)}
    click = _use((0, y, 0), 'up', (0.5, y, 2.5))
    yield from b.hold('minecraft:wheat_seeds')
    for use in range(1, 11):
        steps.append((yield from act(b, f'seeds_{use}', cells, 4, use=click)))
    _setblock(ctx, (0, y, 0), 'composter', {'composter_fill_level': 6})
    yield 4
    for use in range(1, 41):
        steps.append((yield from act(b, f'level_6_seeds_{use}', cells, 60, use=click,
                                     until=lambda s, t, log: _fill(s) == 8 or (_fill(s) == 6 and t >= 3))))
        if _fill(steps[-1]['after']) >= 7:
            break
    yield from b.hold(None)
    steps.append((yield from act(b, 'use_at_8', cells, 20, use=click)))
    return steps


def cauldron(b: Blocks, item: dict):
    """A cauldron filled with a water bucket, emptied by three glass
    bottles, filled with a lava bucket and emptied with a bucket."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    yield from _ureset(b)
    _setblock(ctx, (0, y, 0), 'cauldron')
    yield 2
    cells = {'cauldron': (0, y, 0)}
    click = _use((0, y, 0), 'up', (0.5, y, 2.5))
    yield from b.hold('minecraft:water_bucket', 1)
    steps.append((yield from act(b, 'water_bucket', cells, 6, use=click)))
    yield from b.hold('minecraft:glass_bottle', 3)
    for use in (1, 2, 3):
        steps.append((yield from act(b, f'glass_bottle_{use}', cells, 6, use=click)))
    yield from b.hold('minecraft:lava_bucket', 1)
    steps.append((yield from act(b, 'lava_bucket', cells, 6, use=click)))
    yield from b.hold('minecraft:bucket', 1)
    steps.append((yield from act(b, 'empty_bucket', cells, 6, use=click)))
    return steps


def campfire(b: Blocks, item: dict):
    """A campfire placed looking north; raw beef put on it (watched until
    the cooked item pops out); extinguished with a wooden shovel."""
    y = b.arena['floor'] + 1
    steps = []
    yield from _ureset(b)
    cells = {'campfire': (0, y, 0)}
    stand = (0.5, y, 2.5)
    steps.append((yield from _place(b, 'place', 'minecraft:campfire', (0, y - 1, 0), 'up', stand, cells, yaw=180)))
    yield from b.hold('minecraft:beef', 4)
    click = _use((0, y, 0), 'up', stand)
    cooked = int(item.get('cook_ticks', 900))
    steps.append((yield from act(
        b, 'raw_beef', cells, cooked, use=click,
        until=lambda s, t, log: any(e['event'] == 'appeared' and e['type'] == 'minecraft:item' for e in log))))
    yield 10
    yield from b.hold('minecraft:wooden_shovel', 1)
    steps.append((yield from act(b, 'wooden_shovel', cells, 10, use=click)))
    return steps


def beehive(b: Blocks, item: dict):
    """Full beehives (honey level 5) used with shears and with a glass
    bottle."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    yield from _ureset(b)
    hives = {'shears': (0, y, 0), 'glass_bottle': (3, y, 0)}
    for pos in hives.values():
        _setblock(ctx, pos, 'beehive', {'direction': 0, 'honey_level': 5})
    yield 2
    for tool, pos in hives.items():
        yield from b.hold(f'minecraft:{tool}', 1)
        steps.append((yield from act(b, tool, {'beehive': pos}, 10,
                                     use=_use(pos, 'south', (pos[0] + 0.5, y, 2.5)))))
    return steps


def bell(b: Blocks, item: dict):
    """A standing bell (direction 0) clicked on each side face, then a lever
    on a stone block west of it."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    yield from _ureset(b)
    pos = (0, y, 0)
    _setblock(ctx, pos, 'bell', {'attachment': 'standing', 'direction': 0})
    _setblock(ctx, (-1, y, 0), 'stone')
    yield 2
    cells = {'bell': pos}
    yield from b.hold(None)
    stands = {'south': (0.5, y, 2.5), 'north': (0.5, y, -1.5), 'east': (2.5, y, 0.5), 'west': (-1.5, y, 1.5)}
    for face in ('south', 'north', 'east', 'west'):
        steps.append((yield from act(b, f'use_{face}_face', cells, 20, use=_use(pos, face, stands[face]))))
    stand = (-0.5, y, 2.5)
    lever_step, lever = yield from _lever(b, 'lever_place', (-1, y, 0), stand, cells)
    steps.append(lever_step)
    cells['lever'] = lever
    yield from b.hold(None)
    for state in ('on', 'off'):
        steps.append((yield from act(b, f'lever_{state}', cells, 30, use=_use(lever, 'up', stand))))
    return steps


def jukebox(b: Blocks, item: dict):
    """A jukebox given music disc 13, then ejected with an empty hand."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    yield from _ureset(b)
    _setblock(ctx, (0, y, 0), 'jukebox')
    yield 2
    cells = {'jukebox': (0, y, 0)}
    click = _use((0, y, 0), 'up', (0.5, y, 2.5))
    yield from b.hold('minecraft:music_disc_13', 1)
    steps.append((yield from act(b, 'insert_disc', cells, 20, use=click)))
    yield from b.hold(None)
    steps.append((yield from act(b, 'eject', cells, 20, use=click)))
    return steps


def flower_pot(b: Blocks, item: dict):
    """A flower pot given a poppy, then used with an empty hand."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    yield from _ureset(b)
    _setblock(ctx, (0, y, 0), 'flower_pot')
    yield 2
    cells = {'flower_pot': (0, y, 0)}
    click = _use((0, y, 0), 'up', (0.5, y, 2.5))
    yield from b.hold('minecraft:poppy', 1)
    steps.append((yield from act(b, 'poppy', cells, 6, use=click)))
    yield from b.hold(None)
    steps.append((yield from act(b, 'empty_hand', cells, 6, use=click)))
    return steps


def cake(b: Blocks, item: dict):
    """Cake clicked at full hunger, then after a hunger effect (easy
    difficulty) twice."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    yield from _ureset(b)
    ctx.cmd('difficulty easy')
    _setblock(ctx, (0, y, 0), 'cake')
    yield 2
    cells = {'cake': (0, y, 0)}
    click = _use((0, y, 0), 'up', (0.5, y, 2.5))
    yield from b.hold(None)
    steps.append((yield from act(b, 'use_full_hunger', cells, 10, use=click)))
    steps.append((yield from act(b, 'hunger_effect', cells, 50,
                                 commands=[f'effect "{b.name}" hunger 2 255 true'])))
    ctx.cmd(f'effect "{b.name}" clear')
    ctx.cmd(f'effect "{b.name}" resistance 99999 4 true')
    yield 5
    for bite in (1, 2):
        steps.append((yield from act(b, f'use_hungry_{bite}', cells, 10, use=click)))
    ctx.cmd('difficulty peaceful')
    return steps


INWARD = {'north_edge': 'south', 'south_edge': 'north', 'west_edge': 'east', 'east_edge': 'west'}
OUTWARD = {'north_edge': 'north', 'south_edge': 'south', 'west_edge': 'west', 'east_edge': 'east'}


def end_portal(b: Blocks, item: dict):
    """Twelve end portal frames around a 3x3 interior (/setblock, eleven
    with eyes); the player inserts the twelfth eye into the south edge's
    middle frame. Once with every frame's cardinal_direction pointing at the
    interior, once pointing away."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    for variant, directions in (('inward', INWARD), ('outward', OUTWARD)):
        yield from _ureset(b)
        frames = {}
        for i in (1, 2, 3):
            frames[f'frame_n{i}'] = ((i, y, 0), directions['north_edge'])
            frames[f'frame_s{i}'] = ((i, y, 4), directions['south_edge'])
            frames[f'frame_w{i}'] = ((0, y, i), directions['west_edge'])
            frames[f'frame_e{i}'] = ((4, y, i), directions['east_edge'])
        last = (2, y, 4)
        for pos, direction in frames.values():
            _setblock(ctx, pos, 'end_portal_frame', {'minecraft:cardinal_direction': direction,
                                                     'end_portal_eye_bit': pos != last})
        yield 4
        cells = {'last_frame': last, **_region(range(1, 4), (y,), range(1, 4))}
        yield from b.hold('minecraft:ender_eye', 2)
        steps.append((yield from act(b, f'{variant}_last_eye', cells, 20, use=_use(last, 'up', (2.5, y, 6.5)))))
    return steps


def nether_portal(b: Blocks, item: dict):
    """Obsidian 4x5 frame in the x plane at z=0 (inner x 1..2, three high);
    flint and steel on the top of the inner bottom obsidian; then the west
    side's middle obsidian set to air."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    yield from _ureset(b)
    for x in range(4):
        for dy in range(5):
            if x in (0, 3) or dy in (0, 4):
                _setblock(ctx, (x, y + dy, 0), 'obsidian')
    yield 4
    cells = {'removed_frame': (0, y + 2, 0), **_region((1, 2), range(y + 1, y + 4), (0,))}
    yield from b.hold('minecraft:flint_and_steel', 1)
    steps.append((yield from act(b, 'flint_and_steel', cells, 40, use=_use((1, y, 0), 'up', (1.5, y, -2.5)))))
    steps.append((yield from act(b, 'frame_removed', cells, 20, commands=[f'setblock 0 {y + 2} 0 air'])))
    return steps


def respawn_anchor(b: Blocks, item: dict):
    """Overworld respawn anchor: charged with glowstone, then used with an
    empty hand; the 7x3x7 cells around it are watched."""
    ctx = b.ctx
    floor = b.arena['floor']
    y = floor + 1
    steps = []
    yield from _ureset(b)
    pos = (0, y, 0)
    _setblock(ctx, pos, 'respawn_anchor')
    yield 2
    cells = {'anchor': pos, **_region(range(-3, 4), range(floor - 1, floor + 3), range(-3, 4))}
    click = _use(pos, 'south', (0.5, y, 3.5))
    yield from b.hold('minecraft:glowstone', 4)
    steps.append((yield from act(b, 'glowstone', cells, 6, use=click)))
    yield from b.hold(None)
    steps.append((yield from act(b, 'empty_hand', cells, 40, use=click,
                                 schedule=[(5, f'tp "{b.name}" 0.5 {y} 18.5')])))
    return steps


def bed(b: Blocks, item: dict):
    """A bed placed looking north (foot at the origin); used at noon, then
    at night (`time set 18000`); the level time is read before and after."""
    ctx = b.ctx
    y = b.arena['floor'] + 1
    steps = []
    yield from _ureset(b)
    cells = {'foot': (0, y, 0), 'head': (0, y, -1)}
    stand = (0.5, y, 1.8)
    steps.append((yield from _place(b, 'place', 'minecraft:bed', (0, y - 1, 0), 'up', stand, cells, yaw=180,
                                    count=1)))
    yield from b.hold(None)
    click = _use((0, y, 0), 'up', stand)
    steps.append((yield from act(b, 'use_noon', cells, 20, use=click)))
    ctx.must('time set 18000')
    yield 5
    time_before = ctx.player.level.time
    steps.append((yield from act(b, 'use_night', cells, int(item.get('sleep_ticks', 240)), use=click)))
    steps[-1].update(level_time_before=time_before, level_time_after=ctx.player.level.time)
    ctx.must('time set 6000')
    yield 5
    return steps


UTILITY_SCENARIOS = {'observer': observer, 'dispensers': dispensers, 'tnt': tnt, 'note_block_use': note_block_use,
                     'rails': rails, 'composter': composter, 'cauldron': cauldron, 'campfire': campfire,
                     'beehive': beehive, 'bell': bell, 'jukebox': jukebox, 'flower_pot': flower_pot, 'cake': cake,
                     'end_portal': end_portal, 'nether_portal': nether_portal, 'respawn_anchor': respawn_anchor,
                     'bed': bed}



REDSTONE_SCENARIOS = {'redstone_dust': redstone_dust, 'repeaters': repeaters, 'redstone_torch': redstone_torch,
                      'comparators': comparators, 'comparator_container': comparator_container, 'buttons': buttons,
                      'pressure_plates': pressure_plates, 'weighted_plates': weighted_plates,
                      'iron_door_lever': iron_door_lever, 'note_blocks': note_blocks, 'pistons': pistons,
                      'fire': fire}

BLOCK_SCENARIOS = {'doors': doors, 'trapdoors': trapdoors, 'fence_gates': fence_gates, 'torches': torches,
                   'attachables': attachables, 'signs': signs, 'beds': beds, 'stairs': stairs, 'slabs': slabs,
                   'connections': connections, 'crops': crops, 'crop_care': crop_care,
                   'plant_survival': plant_survival, 'falling_blocks': falling_blocks}

SCENARIOS = {**BLOCK_SCENARIOS, **REDSTONE_SCENARIOS, **UTILITY_SCENARIOS}

JOBS = {'drops': drops, 'armor_slots': armor_slots, 'armor_values': armor_values, 'attack': attack,
        'fuel': fuel, 'mining': mining, 'blocks': blocks}
