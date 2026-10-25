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
    """Furnace burn time per fuel: furnaces in a row, one smeltable input and
    one fuel item each; counts server ticks from the first tick the furnace
    is lit to the first tick it is unlit again."""
    origin = job['arena']['furnaces']
    pending = [item for item in job['items'] if item['key'] not in ctx.done]
    batch = int(job.get('batch', 32))
    while pending:
        group, pending = pending[:batch], pending[batch:]
        positions = []
        for index, item in enumerate(group):
            pos = (origin[0] + 2 * (index % 16), origin[1], origin[2] + 2 * (index // 16))
            ctx.set_state(pos, 'minecraft:air', {})
            ctx.must(f'setblock {pos[0]} {pos[1]} {pos[2]} {job["furnace"]}')
            positions.append(pos)
        yield 2
        for item, pos in zip(group, positions):
            ctx.must(f'replaceitem block {pos[0]} {pos[1]} {pos[2]} slot.container 0 {job["input"]} 1')
            ctx.must(f'replaceitem block {pos[0]} {pos[1]} {pos[2]} slot.container 1 {item["item"]} 1')
        state = {item['key']: {'lit_at': None, 'unlit_at': None} for item in group}
        tick = 0
        limit = int(job.get('max_ticks', 24000))
        lit_type = job['lit_furnace']
        while tick < limit and any(s['unlit_at'] is None for s in state.values()):
            yield 1
            tick += 1
            for item, pos in zip(group, positions):
                s = state[item['key']]
                lit = ctx.block(pos).type == lit_type
                if lit and s['lit_at'] is None:
                    s['lit_at'] = tick
                elif not lit and s['lit_at'] is not None and s['unlit_at'] is None:
                    s['unlit_at'] = tick
            # Nothing ever lit after 40 ticks: not a fuel.
            if tick == 40:
                for s in state.values():
                    if s['lit_at'] is None:
                        s['unlit_at'] = -1
        for item, pos in zip(group, positions):
            s = state[item['key']]
            ctx.record({
                'key': item['key'],
                'item': item['item'],
                'furnace': job['furnace'],
                'lit_at': s['lit_at'],
                'unlit_at': s['unlit_at'] if s['unlit_at'] != -1 else None,
                'burn_ticks': (s['unlit_at'] - s['lit_at'])
                if s['lit_at'] is not None and s['unlit_at'] not in (None, -1) else None,
                'timed_out': s['lit_at'] is not None and s['unlit_at'] is None,
            })
            ctx.set_state(pos, 'minecraft:air', {})
        yield 2


JOBS = {'drops': drops, 'armor_slots': armor_slots, 'fuel': fuel}
