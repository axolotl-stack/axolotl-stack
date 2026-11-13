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

    def held(self) -> dict | None:
        """The main-hand stack as `{item, damage}` (`damage`: the durability
        damage BDS stores, 0 when none), `None` for an empty hand."""
        stack = self.player.inventory.item_in_main_hand
        if stack is None or stack.type.id == 'minecraft:air':
            return None
        meta = stack.item_meta
        return {'item': stack.type.id, 'damage': meta.damage if meta.has_damage else 0}


def run(ctx: Context, job: dict):
    arena = job['arena']
    ctx.must(f'tp "{job["player"]}" {arena["player"][0]} {arena["player"][1]} {arena["player"][2]}')
    for line in job.get('setup_commands', []):
        ctx.must(line)
    # A second client (`sweep.py --observer`) joins after the sweep player.
    if job.get('observer'):
        for _ in range(1200):
            if ctx.server.get_player(job['observer']) is not None:
                break
            yield 1
        else:
            raise RuntimeError(f'observer {job["observer"]} never joined')
        # Joined is not yet spawned: commands naming it fail for a while.
        yield 60
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
    that many ticks instead. The held stack is read back after every trial
    (`held_after`: item and durability damage, `None` once it broke), so the
    wear one break costs is the difference to `held_before`."""
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
                'held_after': ctx.held(),
                'ticks': broke - start if start is not None and broke is not None else None}

    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        ctx.must(f'tp "{name}" {miner[0]} {miner[1]} {miner[2]} {miner[3]} {miner[4]}')
        # Fails when there is nothing to clear.
        ctx.cmd(f'effect "{name}" clear')
        for line in item.get('commands', []):
            ctx.must(line.replace('{player}', f'"{name}"'))
        try:
            ctx.player.inventory.item_in_main_hand = ctx.stack(item['tool'])
        except Exception as error:  # an identifier Endstone cannot make a stack of
            ctx.record({'key': item['key'], 'block': item['block'], 'states': item['states'],
                        'tool': item['tool'], 'stack_error': str(error), 'ticks': [], 'trials': []})
            continue
        yield 1
        held_before = ctx.held()
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
            'held_before': held_before,
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


def armor_wear(ctx: Context, job: dict):
    """Durability a hit costs worn armour: the sweep player (survival, raised
    max health, no natural regeneration) wears one breakable piece and takes
    one `/damage <player> D entity_attack` per configured D, spaced past the
    hurt cooldown; the piece is read back before and after every hit."""
    name = job['player']
    inventory = ctx.player.inventory
    for line in ('gamerule naturalregeneration false', 'difficulty normal', f'gamemode survival "{name}"',
                 f'effect "{name}" saturation 1000000 255 true'):
        ctx.must(line)
    ctx.player.max_health = int(job['max_health'])

    def worn(attribute):
        stack = getattr(inventory, attribute)
        if stack is None or stack.type.id == 'minecraft:air':
            return None
        meta = stack.item_meta
        return meta.damage if meta.has_damage else 0

    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        for attribute in ARMOR_ATTRIBUTES.values():
            setattr(inventory, attribute, None)
        attribute = ARMOR_ATTRIBUTES[item['slot']]
        setattr(inventory, attribute, ctx.stack({'item': item['item']}))
        yield 2
        hits = []
        for amount in job['amounts']:
            ctx.player.health = ctx.player.max_health
            before = worn(attribute)
            ctx.must(f'damage "{name}" {amount} entity_attack')
            yield int(job.get('cooldown_ticks', 15))
            after = worn(attribute)
            hits.append({'damage': amount, 'before': before, 'after': after,
                         'wear': after - before if before is not None and after is not None else None})
        ctx.record({'key': item['key'], 'item': item['item'], 'slot': item['slot'], 'hits': hits})
    for attribute in ARMOR_ATTRIBUTES.values():
        setattr(inventory, attribute, None)
    ctx.player.health = ctx.player.max_health
    ctx.must(f'gamemode creative "{name}"')


def attack(ctx: Context, job: dict):
    """Melee damage per held item: a fresh target mob is spawned in front of
    the sweep player (creative unless the job sets `gamemode`, on the ground,
    not sprinting), the client gets a `bds-sweep:attack <runtime id>` order,
    and the damage events of that mob are recorded; `repeats` trials per
    item, `gap` extra ticks between them (items with an attack cooldown).
    The held stack is read back after every trial (`held_after`), so in
    survival the wear of each hit is the change of its durability damage."""
    from endstone.level import Location
    spot = job['arena']['target']
    miner = job['arena']['miner']
    gamemode = job.get('gamemode')
    if gamemode:
        ctx.must(f'gamemode {gamemode} "{job["player"]}"')
    ctx.must(f'tp "{job["player"]}" {miner[0]} {miner[1]} {miner[2]} {miner[3]} {miner[4]}')
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        ctx.player.inventory.held_item_slot = 0
        ctx.player.inventory.item_in_main_hand = ctx.stack(item['tool'])
        yield 5
        held_before = ctx.held()
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
            trials.append({'damage': damage, 'causes': causes, 'warmup': index == 0,
                           'held_after': ctx.held()})
            for actor in ctx.dimension.actors:
                if actor.runtime_id == target:
                    actor.remove()
            ctx.take_items()
            yield 2 + int(item.get('gap', 0))
        row = {'key': item['key'], 'tool': item['tool'], 'target': job['target'], 'trials': trials,
               'held_before': held_before}
        if gamemode:
            row['gamemode'] = gamemode
        ctx.record(row)
    ctx.player.inventory.item_in_main_hand = None
    if gamemode:
        ctx.must(f'gamemode creative "{job["player"]}"')


def actor_state(actor) -> dict:
    """Position (feet), rotation (pitch, yaw), velocity and ground contact of
    a live actor."""
    loc = actor.location
    vel = actor.velocity
    return {'pos': [loc.x, loc.y, loc.z], 'rot': [loc.pitch, loc.yaw], 'vel': [vel.x, vel.y, vel.z],
            'ground': actor.is_on_ground}


def find_actor(ctx: Context, runtime_id: int):
    for actor in ctx.dimension.actors:
        if actor.runtime_id == runtime_id and actor.is_valid:
            return actor
    return None


def spawned_by(ctx: Context, actor_type: str, commands: list[str]):
    """Runs `commands` (a powered dispenser, say) and waits for the first new
    actor of `actor_type` they make; returns it."""
    before = {actor.runtime_id for actor in ctx.dimension.actors}
    for line in commands:
        ctx.must(line)
    for _ in range(40):
        for actor in ctx.dimension.actors:
            if actor.type == actor_type and actor.runtime_id not in before and actor.is_valid:
                return actor
        yield 1
    raise RuntimeError(f'no {actor_type} came out of {commands}')


def knockback(ctx: Context, job: dict):
    """Knockback and movement of a mob the sweep player hits. Per item: the
    `setup` commands run, a fresh `mob` is spawned at `at` (x, y, z, yaw)
    (or, with `spawn_commands`, the first new actor of that type those
    commands make is followed instead),
    with `frozen` given Slowness 255 so its AI cannot accelerate it and then
    the `effects` commands run; it settles for `settle` ticks; then each of
    `hits` lands `tick` ticks later: three ticks before it the player
    (holding `tool`) is teleported to the mob's position + `from` (x, z) on
    the ground, facing `yaw`, and on the tick the client gets a
    `bds-sweep:attack` order. Without hits the player stands at `player`
    (x, y, z, yaw). Every tick until `tail` ticks after the last hit (or the
    settle) records the mob's and the player's state; every
    ActorKnockbackEvent and ActorDamageEvent is kept with its tick; the
    `cleanup` commands run last. `--client-capture` on sweep.py records the
    packets the player received alongside.

    With an observer client (`sweep.py --observer`), it stands at the item's
    `observer` (x, y, z, yaw) from before the spawn and its state is sampled
    too. `actions` run commands on given ticks: `[{t, commands}]`, `t`
    counted like the samples (0 = the spawn tick)."""
    from endstone.level import Location
    name = job['player']
    observer = job.get('observer')
    ground = job['arena']['player'][1]
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        ctx.begin(item)
        for line in item.get('setup', []):
            ctx.must(line)
        ctx.player.inventory.held_item_slot = 0
        ctx.player.inventory.item_in_main_hand = ctx.stack(item.get('tool'))
        at = item['at']
        if item['hits']:
            first = item['hits'][0]
            start = [at[0] + first['from'][0], ground, at[2] + first['from'][1], first['yaw']]
        else:
            start = item['player']
        ctx.must(f'tp "{name}" {start[0]} {start[1]} {start[2]} {start[3]} 0')
        watcher = ctx.server.get_player(observer) if observer else None
        if watcher is not None and item.get('observer'):
            spot = item['observer']
            ctx.must(f'tp "{observer}" {spot[0]} {spot[1]} {spot[2]} {spot[3]} 0')
        yield 5
        if item.get('spawn_commands'):
            mob = yield from spawned_by(ctx, item['mob'], item['spawn_commands'])
        else:
            mob = ctx.dimension.spawn_actor(Location(ctx.dimension, at[0], at[1], at[2], 0.0, at[3]), item['mob'])
        runtime_id = mob.runtime_id
        if item.get('frozen'):
            ctx.must(f'effect @e[type={item["mob"]}] slowness 100000 255 true')
        for line in item.get('effects', []):
            ctx.must(line)
        ctx.plugin.knockbacks.clear()
        ctx.plugin.damage.clear()
        hits = {int(item['settle']) + int(hit['tick']): hit for hit in item['hits']}
        moves = {tick - 3: hit for tick, hit in hits.items()}
        actions = {int(action['t']): action['commands'] for action in item.get('actions', [])}
        end = max([*hits, *actions], default=int(item['settle'])) + int(item['tail'])
        first_tick = ctx.plugin.ticks
        samples, orders = [], []
        for t in range(end + 1):
            actor = find_actor(ctx, runtime_id)
            if actor is None:
                break
            sample = {'t': t, 'tick': ctx.plugin.ticks, 'mob': actor_state(actor), 'player': actor_state(ctx.player)}
            if watcher is not None:
                sample['observer'] = actor_state(watcher)
            samples.append(sample)
            for line in actions.get(t, []):
                ctx.must(line)
            if t in moves:
                hit, loc = moves[t], actor.location
                ctx.must(f'tp "{name}" {loc.x + hit["from"][0]:.4f} {ground} {loc.z + hit["from"][1]:.4f} '
                         f'{hit["yaw"]} 0')
            if t in hits:
                ctx.player.send_message(f'bds-sweep:attack {runtime_id}')
                orders.append({'t': t, 'tick': ctx.plugin.ticks})
            yield 1
        actor = find_actor(ctx, runtime_id)
        if actor is not None:
            actor.remove()
        # Not checked: an action may already have cleared what it restores.
        for line in item.get('cleanup', []):
            ctx.cmd(line)
        ctx.record({
            'key': item['key'],
            'mob': item['mob'],
            'runtime_id': runtime_id,
            'frozen': bool(item.get('frozen')),
            'tool': item.get('tool'),
            'spec': {k: item[k] for k in ('at', 'hits', 'settle', 'tail', 'player', 'setup', 'effects',
                                          'observer', 'actions', 'spawn_commands') if k in item},
            'first_tick': first_tick,
            'orders': orders,
            'knockbacks': [k for k in ctx.plugin.knockbacks if k['actor'] == runtime_id],
            'damage': [list(d) for d in ctx.plugin.damage if d[1] == item['mob']],
            'samples': samples,
        })
        yield 5
    ctx.player.inventory.item_in_main_hand = None


def find_named(ctx: Context, name_tag: str):
    for actor in ctx.dimension.actors:
        if actor.is_valid and actor.name_tag == name_tag:
            return actor
    return None


def knockback_reload(ctx: Context, job: dict):
    """Whether a mob's knockback response survives saving and loading, over
    two BDS runs on one world (`sweep.py knockback-reload --items spawn
    --keep-world`, then `--items reload --world-from <first run>/world`).
    Per item, `spawn`: a `mob` named `kb-<key>` is spawned at `at` (x, y, z,
    yaw), frozen, and hit once by the player (holding nothing) from `from`
    (x, z offset) facing `yaw`; it stays in the world, which BDS saves when
    it stops. `reload`: the player goes back there, waits for the loaded mob
    with that name (a new runtime id) and hits it the same way. Every hit's
    ActorKnockbackEvent is recorded with the runtime id it hit."""
    from endstone.level import Location
    name = job['player']
    ground = job['arena']['player'][1]

    def hit(actor, item):
        loc = actor.location
        ctx.must(f'tp "{name}" {loc.x + item["from"][0]:.4f} {ground} {loc.z + item["from"][1]:.4f} '
                 f'{item["yaw"]} 0')
        yield 3
        runtime_id = actor.runtime_id
        before = len(ctx.plugin.knockbacks)
        ctx.player.send_message(f'bds-sweep:attack {runtime_id}')
        yield 10
        return {'runtime_id': runtime_id, 'pos': actor_state(actor)['pos'],
                'knockbacks': [k for k in ctx.plugin.knockbacks[before:] if k['actor'] == runtime_id]}

    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        ctx.begin(item)
        ctx.player.inventory.held_item_slot = 0
        ctx.player.inventory.item_in_main_hand = None
        at = item['at']
        tag = f'kb-{item["key"]}'
        ctx.must(f'tp "{name}" {at[0] + item["from"][0]} {ground} {at[2] + item["from"][1]} {item["yaw"]} 0')
        yield 5
        if item['phase'] == 'spawn':
            actor = ctx.dimension.spawn_actor(Location(ctx.dimension, at[0], at[1], at[2], 0.0, at[3]),
                                              item['mob'])
            actor.name_tag = tag
            ctx.must(f'effect @e[type={item["mob"]}] slowness 100000 255 true')
        else:
            actor = None
            for _ in range(400):
                actor = find_named(ctx, tag)
                if actor is not None:
                    break
                yield 1
            if actor is None:
                raise RuntimeError(f'{tag} is not in the loaded world')
        yield 20
        ctx.record({'key': item['key'], 'mob': item['mob'], 'phase': item['phase'],
                    **(yield from hit(actor, item))})
        yield 5


def spawn_marker(ctx: Context, marker: dict):
    """A still actor for `send_range`: an item (`minecraft:item`, a stick)
    is dropped, anything else is spawned facing `at[3]`."""
    from endstone.inventory import ItemStack
    from endstone.level import Location
    at = marker['at']
    where = Location(ctx.dimension, at[0], at[1], at[2], 0.0, at[3] if len(at) > 3 else 0.0)
    if marker['type'] == 'minecraft:item':
        return ctx.dimension.drop_item(where, ItemStack('minecraft:stick', 1))
    return ctx.dimension.spawn_actor(where, marker['type'])


def send_range(ctx: Context, job: dict):
    """Which actors BDS sends to a viewer, by where it stands. Per item: the
    sweep player stands at `center` (it is a marker too: the observer sees
    it as a player), the `markers` are spawned (cows frozen) and settle for
    `settle` ticks, then the observer walks `path` ([x, z] per step, on the
    ground) and stays `ticks` ticks on each step. Every tick it gets a chat
    line `bds-sweep:mark <key> <step> <tick>`, so each actor packet in its
    capture can be put on a tick; each step's observer state is recorded.
    The markers are removed at the end of the item. Needs `--observer`."""
    name, observer = job['player'], job['observer']
    ground = job['arena']['player'][1]
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        ctx.begin(item)
        watcher = ctx.server.get_player(observer)
        if watcher is None:
            raise RuntimeError(f'observer {observer} left')
        center, path = item['center'], item['path']
        ctx.must(f'tp "{name}" {center[0]} {ground} {center[1]} 0 0')
        ctx.must(f'tp "{observer}" {path[0][0]} {ground} {path[0][1]} 0 0')
        yield 20
        markers = [(marker, spawn_marker(ctx, marker)) for marker in item['markers']]
        ctx.cmd('effect @e[type=minecraft:cow] slowness 100000 255 true')
        yield int(item.get('settle', 40))
        placed = []
        for marker, actor in markers:
            placed.append({**marker, 'runtime_id': actor.runtime_id,
                           'unique_id': getattr(actor, 'id', None), 'state': actor_state(actor)})
        steps = []
        for step, (x, z) in enumerate(path):
            ctx.must(f'tp "{observer}" {x} {ground} {z} 0 0')
            first_tick = ctx.plugin.ticks
            for tick in range(int(item['ticks'])):
                watcher.send_message(f'bds-sweep:mark {item["key"]} {step} {ctx.plugin.ticks}')
                yield 1
            steps.append({'step': step, 'tick': first_tick, 'observer': actor_state(watcher)})
        ended = []
        for marker, actor in markers:
            ended.append(actor_state(actor) if actor.is_valid else None)
            if actor.is_valid:
                actor.remove()
        ctx.record({'key': item['key'], 'center': center, 'player': actor_state(ctx.player),
                    'player_runtime_id': ctx.player.runtime_id, 'observer_runtime_id': watcher.runtime_id,
                    'markers': placed, 'ended': ended, 'steps': steps, 'ticks': item['ticks']})
        yield 5


def spawn_bonus(ctx: Context, job: dict):
    """Spawns `count` of each item's `mob` on a grid around the sweep player
    (with `via: summon`, by `/summon`), frozen (slowness 255) so they stay
    near it, and leaves them in the world:
    run with `--keep-world`, then read their saved attributes from the kept
    world's actor records. Each spawned actor's runtime and unique ids are
    recorded with its grid spot."""
    from endstone.level import Location
    center = job['arena']['player']
    index = 0
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        ctx.begin(item)
        spawned = []
        for n in range(int(item['count'])):
            gx, gz = index % 40, index // 40
            index += 1
            x, z = center[0] - 20 + gx, center[2] + 3 + gz
            spot = [x + 0.5, center[1], z + 0.5]
            if item.get('via') == 'summon':
                ctx.must(f'summon {item["mob"]} {spot[0]} {spot[1]} {spot[2]}')
                spawned.append({'at': spot})
            else:
                actor = ctx.dimension.spawn_actor(Location(ctx.dimension, *spot, 0.0, 0.0), item['mob'])
                spawned.append({'runtime_id': actor.runtime_id, 'unique_id': getattr(actor, 'id', None),
                                'at': spot})
            if n % 20 == 19:
                yield 1
        ctx.cmd(f'effect @e[type={item["mob"]}] slowness 100000 255 true')
        yield 2
        ctx.record({'key': item['key'], 'mob': item['mob'], 'spawned': spawned})
    yield int(job.get('linger', 100))


def walk(ctx: Context, job: dict):
    """How BDS relays a player's movement to another player. Per item: the
    sweep player stands at `start` (x, y, z, yaw) and the observer at
    `observer` (x, y, z, yaw); a frozen cow with an odd yaw (`cow`, x, y, z,
    yaw) is spawned for its `AddActor` rotation; then the sweep player gets
    `bds-sweep:walk <order>` and both players are sampled for `ticks` ticks,
    the observer getting a `bds-sweep:mark` chat line every tick."""
    from endstone.level import Location
    name, observer = job['player'], job['observer']
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        ctx.begin(item)
        watcher = ctx.server.get_player(observer)
        if watcher is None:
            raise RuntimeError(f'observer {observer} left')
        start, spot = item['start'], item['observer']
        ctx.must(f'tp "{name}" {start[0]} {start[1]} {start[2]} {start[3]} 0')
        ctx.must(f'tp "{observer}" {spot[0]} {spot[1]} {spot[2]} {spot[3]} 0')
        yield 20
        cow = None
        if item.get('cow'):
            at = item['cow']
            cow = ctx.dimension.spawn_actor(Location(ctx.dimension, at[0], at[1], at[2], 0.0, at[3]),
                                            'minecraft:cow')
            ctx.cmd('effect @e[type=minecraft:cow] slowness 100000 255 true')
        yield 10
        samples = []
        ctx.player.send_message(f'bds-sweep:walk {item["order"]}')
        for t in range(int(item['ticks'])):
            watcher.send_message(f'bds-sweep:mark {item["key"]} {t} {ctx.plugin.ticks}')
            samples.append({'t': t, 'tick': ctx.plugin.ticks, 'player': actor_state(ctx.player),
                            'observer': actor_state(watcher)})
            yield 1
        if cow is not None and cow.is_valid:
            cow_state = actor_state(cow)
            cow_runtime = cow.runtime_id
            cow.remove()
        else:
            cow_state, cow_runtime = None, None
        ctx.record({'key': item['key'], 'order': item['order'], 'start': start, 'observer': spot,
                    'player_runtime_id': ctx.player.runtime_id, 'cow': cow_state, 'cow_runtime_id': cow_runtime,
                    'samples': samples})
        yield 10


def living_state(actor) -> dict:
    """`actor_state` plus the health Endstone reports (whole points)."""
    state = actor_state(actor)
    health = getattr(actor, 'health', None)
    if health is not None:
        state['health'] = health
    return state


def effects(ctx: Context, job: dict):
    """What effects do to a mob or to the sweep player, tick by tick. Per
    item: the `setup` commands run (a barrier cell, say); the sweep player
    stands at `player` (x, y, z, yaw); unless `mob` is null a fresh `mob` is
    spawned at `at` (x, y, z, yaw), with `frozen` given hidden Slowness 255;
    then for `ticks` ticks the mob and the player are sampled every tick
    (position, rotation, velocity, ground contact, health) and each
    `actions` entry `{t, commands, order?}` runs its commands on tick `t`
    (0 = the spawn tick) and then, with `order` (`interact`, `attack`),
    sends the client that order on the mob; the player holds `hold` (an
    item spec, or nothing). Every ActorDamageEvent of the item is kept with its tick,
    actor type, damage and cause. The `cleanup` commands run last; run with
    `--client-capture` for the packets the player got (its own `MobEffect`
    and attributes, and the mob's actor data, attributes and events)."""
    from endstone.level import Location
    name = job['player']
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        ctx.begin(item)
        for line in item.get('setup', []):
            ctx.must(line)
        spot = item['player']
        ctx.must(f'tp "{name}" {spot[0]} {spot[1]} {spot[2]} {spot[3]} 0')
        ctx.player.inventory.held_item_slot = 0
        ctx.player.inventory.item_in_main_hand = ctx.stack(item.get('hold'))
        yield 10
        runtime_id = None
        if item.get('mob'):
            at = item['at']
            mob = ctx.dimension.spawn_actor(Location(ctx.dimension, at[0], at[1], at[2], 0.0, at[3]), item['mob'])
            runtime_id = mob.runtime_id
            if item.get('frozen'):
                ctx.must(f'effect @e[type={item["mob"]}] slowness 100000 255 true')
        ctx.plugin.damage.clear()
        actions = {int(action['t']): action for action in item.get('actions', [])}
        first_tick = ctx.plugin.ticks
        samples, failed = [], []
        for t in range(int(item['ticks']) + 1):
            # The mark lands in this tick's packet batch, so every packet of
            # the capture can be put on a sample.
            ctx.player.send_message(f'bds-sweep:mark {item["key"]} {t}')
            sample = {'t': t, 'tick': ctx.plugin.ticks, 'player': living_state(ctx.player)}
            if runtime_id is not None:
                actor = find_actor(ctx, runtime_id)
                sample['mob'] = living_state(actor) if actor is not None else None
            samples.append(sample)
            action = actions.get(t)
            if action is not None:
                for line in action['commands']:
                    if not ctx.cmd(line):
                        failed.append({'t': t, 'command': line})
                if action.get('order') and runtime_id is not None:
                    ctx.player.send_message(f'bds-sweep:{action["order"]} {runtime_id}')
            yield 1
        if runtime_id is not None:
            actor = find_actor(ctx, runtime_id)
            if actor is not None:
                actor.remove()
        for line in item.get('cleanup', []):
            ctx.cmd(line)
        ctx.player.inventory.item_in_main_hand = None
        ctx.record({
            'key': item['key'],
            'mob': item.get('mob'),
            'runtime_id': runtime_id,
            'player_runtime_id': ctx.player.runtime_id,
            'spec': {k: item[k] for k in ('at', 'player', 'frozen', 'setup', 'actions', 'ticks', 'cleanup',
                                          'hold') if k in item},
            'first_tick': first_tick,
            'failed_commands': failed,
            'damage': [list(d) for d in ctx.plugin.damage],
            'samples': samples,
        })
        yield 10


def entity_loot(ctx: Context, job: dict):
    """`/loot spawn … kill <mob>` as the sweep player, `samples` times per
    item: a frozen `mob` stands at `at` and every outcome (item type, data,
    count of each dropped stack) is counted, like `drops` does for blocks."""
    from endstone.level import Location
    name = job['player']
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        ctx.begin(item)
        at = item['at']
        ctx.take_items()
        mob = ctx.dimension.spawn_actor(Location(ctx.dimension, at[0], at[1], at[2], 0.0, 0.0), item['mob'])
        ctx.must(f'effect @e[type={item["mob"]}] slowness 100000 255 true')
        ctx.must(f'effect @e[type={item["mob"]}] resistance 100000 255 true')
        yield 5
        drop = item['drop']
        loot = (f'execute as @a[name="{name}"] run loot spawn {drop[0]} {drop[1]} {drop[2]} '
                f'kill @e[type={item["mob"]},c=1] mainhand')
        outcomes: Counter[str] = Counter()
        failures = 0
        for _ in range(int(item['samples'])):
            if not ctx.cmd(loot):
                failures += 1
            stacks = sorted(ctx.take_items())
            outcomes[';'.join(f'{i}@{d}x{n}' for i, d, n in stacks)] += 1
            yield None
        if mob.is_valid:
            mob.remove()
        ctx.record({'key': item['key'], 'mob': item['mob'], 'samples': int(item['samples']),
                    'command_failures': failures, 'outcomes': dict(sorted(outcomes.items()))})
        yield 2


# Superflat layers under the scenario cells, restored after every wave.
FLAT_LAYERS = {-61: 'minecraft:grass_block', -62: 'minecraft:dirt', -63: 'minecraft:dirt',
               -64: 'minecraft:bedrock'}


def command_states(states: dict) -> str:
    """A block-state list in command syntax: `["a"=1,"b"=true,"c"="x"]`."""
    parts = []
    for key, value in sorted(states.items()):
        if isinstance(value, bool):
            text = 'true' if value else 'false'
        elif isinstance(value, (int, float)):
            text = str(value)
        else:
            text = json.dumps(value)
        parts.append(f'{json.dumps(key)}={text}')
    return '[' + ','.join(parts) + ']'


def step_signature(item: dict) -> tuple:
    """Items of one wave run step by step together, so their step kinds and
    waits must line up."""
    return tuple(f'wait{step[1]}' if step[0] == 'wait' else step[0] for step in item['steps'])


def scenarios(ctx: Context, job: dict):
    """Block scenarios in waves: up to `wave_size` items with the same step
    signature run side by side, each in its own cell (`spacing` blocks apart
    on a grid of `columns` from `origin`), so one wait covers the wave.

    Steps (offsets relative to the cell origin):

    * `['set', [dx, dy, dz], block, states, physics]`: Endstone `set_data`,
      with or without the block update a placement triggers;
    * `['setblock', [dx, dy, dz], block, states, mode]`: the `/setblock`
      command (`replace`, `destroy` or `keep`);
    * `['wait', ticks]`;
    * `['read', label, [[dx, dy, dz], ...]]`: block type and states.

    Records every read, the item actors that appeared in the cell (popped
    blocks drop items) and failed commands. Afterwards each touched position
    is cleared without a block update (the superflat layers are restored
    under the cell) and every item and falling block actor is removed."""
    arena = job['arena']['scenarios']
    origin = arena['origin']
    spacing = int(arena['spacing'])
    columns = int(arena['columns'])
    wave_size = int(job.get('wave_size', columns * columns))
    pending = [item for item in job['items'] if item['key'] not in ctx.done]

    def cell(index: int) -> tuple[int, int, int]:
        return (origin[0] + (index % columns) * spacing, origin[1],
                origin[2] + (index // columns) * spacing)

    def absolute(base, offset) -> tuple[int, int, int]:
        return (base[0] + offset[0], base[1] + offset[1], base[2] + offset[2])

    def clear(positions) -> None:
        for pos in positions:
            layer = FLAT_LAYERS.get(pos[1], 'minecraft:air')
            ctx.set_state(pos, layer, {})

    def sweep_actors(kinds: tuple[str, ...]) -> list[tuple[str, tuple[float, float, float], tuple]]:
        found = []
        for actor in ctx.dimension.actors:
            if actor.type in kinds:
                loc = actor.location
                stack = None
                if actor.type == 'minecraft:item':
                    item = actor.item_stack
                    stack = (item.type.id, item.data, item.amount)
                found.append((actor.type, (loc.x, loc.y, loc.z), stack))
                actor.remove()
        return found

    sweep_actors(('minecraft:item', 'minecraft:falling_block'))
    while pending:
        wave = [pending.pop(0)]
        signature = step_signature(wave[0])
        while pending and len(wave) < wave_size and step_signature(pending[0]) == signature:
            wave.append(pending.pop(0))
        bases = [cell(i) for i in range(len(wave))]
        touched: list[set] = [set() for _ in wave]
        reads: list[dict] = [{} for _ in wave]
        failures: list[list] = [[] for _ in wave]
        for index, step in enumerate(wave[0]['steps']):
            if step[0] == 'wait':
                yield int(step[1])
                continue
            for i, item in enumerate(wave):
                own = item['steps'][index]
                if own[0] == 'set':
                    pos = absolute(bases[i], own[1])
                    touched[i].add(pos)
                    data = ctx.server.create_block_data(own[2], own[3])
                    ctx.block(pos).set_data(data, bool(own[4]))
                elif own[0] == 'setblock':
                    pos = absolute(bases[i], own[1])
                    touched[i].add(pos)
                    line = f'setblock {pos[0]} {pos[1]} {pos[2]} {own[2]} {command_states(own[3])} {own[4]}'
                    if not ctx.cmd(line):
                        failures[i].append(line)
                elif own[0] == 'read':
                    values = []
                    for offset in own[2]:
                        pos = absolute(bases[i], offset)
                        touched[i].add(pos)
                        data = ctx.block(pos).data
                        values.append([data.type, data.block_states])
                    reads[i][own[1]] = values
                else:
                    raise RuntimeError(f'unknown scenario step {own!r}')
            yield None
        actors = sweep_actors(('minecraft:item', 'minecraft:falling_block'))
        for i, item in enumerate(wave):
            half = spacing / 2
            base = bases[i]
            mine = [(kind, stack) for kind, (x, y, z), stack in actors
                    if abs(x - (base[0] + 0.5)) < half and abs(z - (base[2] + 0.5)) < half]
            ctx.record({'key': item['key'], 'reads': reads[i],
                        'items': sorted(stack for kind, stack in mine if kind == 'minecraft:item'),
                        'falling_blocks': sum(1 for kind, _ in mine if kind == 'minecraft:falling_block'),
                        'command_failures': failures[i]})
        for positions in touched:
            clear(sorted(positions))
        yield 1
        sweep_actors(('minecraft:item', 'minecraft:falling_block'))


def placements(ctx: Context, job: dict):
    """Survival block clicks by the scripted player. Per item: the arena is
    cleared (superflat layers restored), its blocks are set with block
    updates, the item goes into the player's hand (hotbar slot 0) and the
    player is teleported with a look direction; a chat marker
    `bds-sweep:mark <key>` lets the client's capture be cut per item; then
    the client clicks (`bds-sweep:use`) and the arena and the held stack are
    read back. Offsets are relative to `arena.placements.origin`.

    With `job.watcher`, a second scripted player stands at the item's
    `watcher` spot (default `arena.placements.watcher`) and records what
    another player is sent; it gets the same markers.

    `after` steps follow the first click: `['wait', n]`, `['hand', who,
    item, count]` (`/replaceitem`; `item` null empties the hand), `['use',
    who, click]` and `['sign', who, offset, front, text]` (a sign text edit
    sent by that client). `timeline` records the tick (relative to the
    first click) of every click and edit, `read_tick` the tick of the reads;
    `read_later` = [ticks, offsets] reads again that many ticks later."""
    arena = job['arena']['placements']
    origin = arena['origin']
    low, high = arena['clear']
    name = job['player']
    watcher_name = job.get('watcher')
    players = {'player': (name, ctx.player)}
    if watcher_name:
        watcher = ctx.server.get_player(watcher_name)
        if watcher is None:
            raise RuntimeError(f'watcher {watcher_name} is not online')
        players['watcher'] = (watcher_name, watcher)
    for player_name, _ in players.values():
        ctx.must(f'gamemode survival "{player_name}"')
        ctx.cmd(f'clear "{player_name}"')
    yield 10

    def at(offset) -> tuple[int, int, int]:
        return (origin[0] + offset[0], origin[1] + offset[1], origin[2] + offset[2])

    def mark(text: str) -> None:
        for _, player in players.values():
            player.send_message(text)

    def give(who: str, spec) -> None:
        player_name = players[who][0]
        # `/clear` fails (and says so) when there is nothing to clear.
        ctx.cmd(f'clear "{player_name}"')
        if spec is not None:
            ctx.must(f'replaceitem entity "{player_name}" slot.hotbar 0 {spec["item"]} {spec.get("count", 1)}')

    def read(offsets) -> list:
        return [[list(offset), ctx.block(at(offset)).data.type, ctx.block(at(offset)).data.block_states]
                for offset in offsets]

    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        for x in range(low[0], high[0] + 1):
            for y in range(low[1], high[1] + 1):
                for z in range(low[2], high[2] + 1):
                    pos = at((x, y, z))
                    ctx.set_state(pos, FLAT_LAYERS.get(pos[1], 'minecraft:air'), {})
        ctx.take_items()
        for x, y, z, block, states in item['setup']:
            ctx.block(at((x, y, z))).set_data(ctx.server.create_block_data(block, states), True)
        # `/replaceitem` (not the inventory API) so BDS syncs the stack to the
        # client, which must click with the stack the server holds.
        give('player', item['hand'])
        px, py, pz, yaw, pitch = item['player']
        ctx.must(f'tp "{name}" {origin[0] + px} {origin[1] + py} {origin[2] + pz} {yaw} {pitch}')
        if 'watcher' in players:
            give('watcher', item.get('watcher_hand'))
            wx, wy, wz, wyaw, wpitch = item.get('watcher') or arena['watcher']
            ctx.must(f'tp "{watcher_name}" {origin[0] + wx} {origin[1] + wy} {origin[2] + wz} {wyaw} {wpitch}')
        # The new hand stack and the teleport reach the client over the next
        # ticks; the click must carry the stack the server holds.
        yield 20
        mark(f'bds-sweep:mark {item["key"]}')
        yield 2
        start = ctx.plugin.ticks
        timeline = []
        for step in [['use', 'player', item['click']]] + item.get('after', []):
            kind = step[0]
            if kind == 'wait':
                yield int(step[1])
            elif kind == 'hand':
                give(step[1], None if step[2] is None else {'item': step[2], 'count': step[3]})
                yield 20
            elif kind == 'use':
                cx, cy, cz, face, ux, uy, uz = step[2]
                clicked = at((cx, cy, cz))
                target = ctx.block(clicked).data.runtime_id
                timeline.append([ctx.plugin.ticks - start, step[1], 'use'])
                players[step[1]][1].send_message(f'bds-sweep:use {clicked[0]} {clicked[1]} {clicked[2]} {face} '
                                                 f'{ux} {uy} {uz} {target}')
                yield 10
            elif kind == 'sign':
                pos = at(step[2])
                text = step[4].encode('utf-8').hex() or '-'
                timeline.append([ctx.plugin.ticks - start, step[1], 'sign'])
                players[step[1]][1].send_message(f'bds-sweep:sign {pos[0]} {pos[1]} {pos[2]} '
                                                 f'{1 if step[3] else 0} {text}')
                yield 10
            else:
                raise RuntimeError(f'unknown placement step {step!r}')
        held = ctx.player.inventory.item_in_main_hand
        row = {
            'key': item['key'],
            'reads': read(item['read']),
            'held': None if held is None else [held.type.id, held.amount],
            'items': sorted(ctx.take_items()),
            'timeline': timeline,
            'read_tick': ctx.plugin.ticks - start,
        }
        if item.get('read_later'):
            wait, offsets = item['read_later']
            yield int(wait)
            row['reads_later'] = read(offsets)
            row['read_later_tick'] = ctx.plugin.ticks - start
            row['items_later'] = sorted(ctx.take_items())
        ctx.record(row)
        mark('bds-sweep:mark end')
        yield 2


def item_blocks(ctx: Context, job: dict):
    """Every listed item goes into the scripted player's hotbar slot 0 by
    `/replaceitem`; the client's capture records how BDS describes the stack
    (its item network id and block runtime id). A chat marker names the item
    before each command so the capture can be cut per item."""
    name = job['player']
    for item in job['items']:
        if item['key'] in ctx.done:
            continue
        ctx.player.send_message(f'bds-sweep:mark {item["key"]}')
        given = ctx.cmd(f'replaceitem entity "{name}" slot.hotbar 0 {item["item"]} 1')
        yield 2
        ctx.record({'key': item['key'], 'item': item['item'], 'given': given})
    ctx.player.send_message('bds-sweep:mark end')
    yield 2


JOBS = {'drops': drops, 'armor_slots': armor_slots, 'armor_values': armor_values, 'armor_wear': armor_wear,
        'attack': attack, 'fuel': fuel, 'mining': mining, 'scenarios': scenarios, 'placements': placements,
        'item_blocks': item_blocks, 'knockback': knockback, 'knockback_reload': knockback_reload,
        'send_range': send_range, 'spawn_bonus': spawn_bonus, 'walk': walk, 'effects': effects,
        'entity_loot': entity_loot}
