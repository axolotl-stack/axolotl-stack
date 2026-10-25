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
        if self.results_path.exists():
            for line in self.results_path.read_text(encoding='utf-8').splitlines():
                if line.strip():
                    self.done.add(json.loads(line)['key'])
        self.results = self.results_path.open('a', encoding='utf-8', buffering=1)

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
        """`spec` is null (empty hand) or {item, count?, enchants?: {id: level}}."""
        if not spec:
            return None
        from endstone.inventory import ItemStack
        stack = ItemStack(spec['item'], int(spec.get('count', 1)))
        enchants = spec.get('enchants') or {}
        if enchants:
            meta = stack.item_meta
            for enchant, level in enchants.items():
                if not meta.add_enchant(enchant, int(level), True):
                    raise RuntimeError(f'cannot enchant {spec["item"]} with {enchant} {level}')
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
        if not pending:
            slots[index] = None
            return
        item = pending.pop(0)
        ctx.must(f'setblock {pos[0]} {pos[1]} {pos[2]} {job["furnace"]}')
        ctx.must(f'replaceitem block {pos[0]} {pos[1]} {pos[2]} slot.container 0 {job["input"]} 1')
        ctx.must(f'replaceitem block {pos[0]} {pos[1]} {pos[2]} slot.container 1 {item["item"]} 1')
        slots[index] = {'item': item, 'loaded': tick, 'lit_at': None}

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
    keeps sending ContinueDestroyBlock; the time is the number of server ticks
    from its LEFT_CLICK_BLOCK interaction to the BlockBreakEvent. With
    `predict_after` > 0 the client predicts the break after that many ticks
    instead (the server then accepts or rejects it)."""
    name = job['player']
    miner = job['arena']['miner']
    pos = tuple(job['arena']['mine_block'])
    face = int(job['arena']['mine_face'])
    ctx.must(f'gamemode survival "{name}"')
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        ctx.must(f'tp "{name}" {miner[0]} {miner[1]} {miner[2]} {miner[3]} {miner[4]}')
        ctx.must(f'effect "{name}" clear')
        for line in item.get('commands', []):
            ctx.must(line.replace('{player}', f'"{name}"'))
        ctx.set_state(pos, item['block'], item['states'])
        ctx.player.inventory.item_in_main_hand = ctx.stack(item['tool'])
        yield 5
        ctx.plugin.events.clear()
        predict = int(item.get('predict_after', 0))
        ctx.player.send_message(f'bds-sweep:mine {pos[0]} {pos[1]} {pos[2]} {face} {predict}')
        start = broke = None
        limit = int(item.get('max_ticks', 1200))
        for _ in range(limit):
            yield 1
            for tick, kind, where in ctx.plugin.events:
                if where != pos:
                    continue
                if kind == 'left_click_block' and start is None:
                    start = tick
                if kind == 'break' and broke is None:
                    broke = tick
            if broke is not None:
                break
        if broke is None:
            ctx.player.send_message('bds-sweep:stop')
            yield 2
        ctx.record({
            'key': item['key'],
            'block': item['block'],
            'states': item['states'],
            'tool': item['tool'],
            'commands': item.get('commands', []),
            'predict_after': predict,
            'start_tick': start,
            'break_tick': broke,
            'ticks': broke - start if start is not None and broke is not None else None,
            'events': [[t, k] for t, k, w in ctx.plugin.events if w == pos][:8],
        })
        ctx.set_state(pos, 'minecraft:air', {})
        ctx.take_items()
    ctx.must(f'gamemode creative "{name}"')


JOBS = {'drops': drops, 'armor_slots': armor_slots, 'fuel': fuel, 'mining': mining}
