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
            setattr(inventory, ARMOR_ATTRIBUTES[item['slot']], ctx.stack({'item': item['item']}))
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
        for _ in range(int(item.get('repeats', 3))):
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
            trials.append({'damage': damage, 'causes': causes})
            for actor in ctx.dimension.actors:
                if actor.runtime_id == target:
                    actor.remove()
            ctx.take_items()
            yield 2
        ctx.record({'key': item['key'], 'tool': item['tool'], 'target': job['target'], 'trials': trials})
    ctx.player.inventory.item_in_main_hand = None


JOBS = {'drops': drops, 'armor_slots': armor_slots, 'armor_values': armor_values, 'attack': attack,
        'fuel': fuel, 'mining': mining}
