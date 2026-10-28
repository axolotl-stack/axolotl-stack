"""Inert unless ENTITY_MOTION_PROBE_OUT is set; run only in a disposable world."""

import json
import os
import traceback
from pathlib import Path

from endstone.event import (
    ActorSpawnEvent, PlayerDropItemEvent, PlayerJoinEvent,
    PlayerPickupItemEvent, event_handler,
)
from endstone.inventory import ItemStack
from endstone.level import Location
from endstone.plugin import Plugin


class EntityMotionProbe(Plugin):
    api_version = '0.11'
    load = 'POSTWORLD'

    def on_enable(self):
        path = os.environ.get('ENTITY_MOTION_PROBE_OUT')
        self.out = Path(path) if path else None
        self.task = None
        if self.out is None:
            self.logger.info('inactive: ENTITY_MOTION_PROBE_OUT is not set')
            return
        self.out.mkdir(parents=True, exist_ok=True)
        self.register_events(self)

    @event_handler
    def on_join(self, event: PlayerJoinEvent):
        if event.player.name != 'BDSSweep' or self.task is not None:
            return
        mode = os.environ.get('ENTITY_MOTION_PROBE_MODE', 'motion')
        self.steps = {'motion': self.run, 'fluid-contact': self.run_contact,
                      'kinematics': self.run_kinematics,
                      'flow-corners': lambda: self.run_kinematics(flow_corners=True),
                      'ground-liquid': self.run_ground_liquid,
                      'player-drop': self.run_player_drop}[mode]()
        self.tick_number = 0
        self.task = self.server.scheduler.run_task(self, self.tick, delay=60, period=1)

    def tick(self):
        self.tick_number += 1
        try:
            next(self.steps)
        except StopIteration:
            self.task.cancel()
            (self.out / 'done.json').write_text(json.dumps({
                'ticks': self.tick_number,
                'minecraft_version': self.server.minecraft_version,
                'protocol_version': self.server.protocol_version,
                'endstone_version': self.server.version,
            }, indent=2), encoding='utf-8')
            if os.environ.get('ENTITY_MOTION_PROBE_MODE') == 'player-drop':
                for name in ('BDSSweep', 'BDSOther'):
                    player = self.server.get_player(name)
                    if player is not None:
                        player.kick('entity player-drop capture complete')
        except Exception:
            self.task.cancel()
            (self.out / 'error.txt').write_text(traceback.format_exc(), encoding='utf-8')

    def command(self, text):
        if not self.server.dispatch_command(self.server.command_sender, text):
            raise RuntimeError(f'command failed: {text}')

    def location(self, x, y, z):
        return Location(self.dimension, x, y, z)

    def record(self, case, actor, **metadata):
        value = {'tick': self.tick_number, 'case': case, 'valid': actor is not None, **metadata}
        if actor is not None:
            p, v = actor.location, actor.velocity
            value.update(runtime_id=actor.runtime_id, actor_type=actor.type,
                         position=[p.x, p.y, p.z], velocity=[v.x, v.y, v.z],
                         on_ground=actor.is_on_ground, in_water=actor.is_in_water,
                         in_lava=actor.is_in_lava)
            if hasattr(actor, 'item_stack'):
                value['item'] = actor.item_stack.type.id
                value['count'] = actor.item_stack.amount
                value['pickup_delay'] = actor.pickup_delay
                value['thrower'] = actor.thrower
        with (self.out / 'samples.jsonl').open('a', encoding='utf-8') as stream:
            stream.write(json.dumps(value) + '\n')
        return value

    def run(self):
        self.player = self.server.get_player('BDSSweep')
        if self.player is None:
            raise RuntimeError('probe player left before sampling')
        self.dimension = self.player.dimension
        for command in [
            'gamerule domobspawning false', 'gamerule randomtickspeed 0',
            'gamerule doweathercycle false', 'weather clear',
            'tickingarea add circle 0 -60 0 2 entity_motion_probe',
            'fill -25 -61 -25 25 -61 25 stone',
            'tickingarea add circle 32 -40 32 1 entity_motion_water true',
            'tickingarea add circle -32 -40 32 1 entity_motion_lava true',
        ]:
            self.command(command)
        for _ in range(40):
            yield
        for command in [
            'fill 24 -61 24 40 -61 40 stone',
            'fill -40 -61 24 -24 -61 40 stone',
            'fill 24 -60 24 40 -20 40 water',
            'fill -40 -60 24 -24 -20 40 lava',
            'fill 16 -60 -2 20 -60 2 minecraft:oak_slab ["minecraft:vertical_half"="bottom"]',
        ]:
            self.command(command)
        for _ in range(10):
            yield
        self.player.teleport(self.location(0.5, -60, 0.5))
        self.player.inventory.clear()
        actors = {
            'item_air': self.dimension.drop_item(self.location(0.5, -30, 12.5), ItemStack('minecraft:stone', 1)),
            'item_water': self.dimension.drop_item(self.location(32.5, -40, 32.5), ItemStack('minecraft:stone', 1)),
            'item_lava': self.dimension.drop_item(self.location(-31.5, -40, 32.5), ItemStack('minecraft:netherite_scrap', 1)),
            'item_water_entry': self.dimension.drop_item(self.location(32.5, 0, 33.5), ItemStack('minecraft:cobblestone', 1)),
            'item_lava_entry': self.dimension.drop_item(self.location(-31.5, 0, 33.5), ItemStack('minecraft:netherite_ingot', 1)),
            'item_bottom_slab': self.dimension.drop_item(self.location(18.5, -59.4, 0.5), ItemStack('minecraft:stone', 1)),
        }
        actors = {case: actor.runtime_id for case, actor in actors.items()}
        self.command('setblock -12 -30 -12 sand')
        falling = None
        for index in range(100):
            # Endstone wrappers do not own actors. Never dereference a wrapper
            # retained across ticks after BDS may have destroyed its actor.
            live = {actor.runtime_id: actor for actor in self.dimension.actors}
            if falling is None:
                for actor in live.values():
                    if actor.type == 'minecraft:falling_block' and actor.location.x < -10:
                        falling = actor.runtime_id
                        actors['falling_sand'] = falling
                        break
            for case, runtime_id in actors.items():
                self.record(case, live.get(runtime_id))
            yield
        if falling is None:
            raise RuntimeError('sand never produced a falling-block actor')
        for actor in self.dimension.actors:
            if actor.runtime_id in actors.values():
                actor.remove()
        # Let each item settle away from the player before moving it to an exact
        # horizontal boundary. Record the actual position before and after pickup.
        for distance in [1.30, 1.40, 1.424, 1.426, 1.45, 1.50, 2.0]:
            actor = self.dimension.drop_item(self.location(0.5, -59.9, 20.5), ItemStack('minecraft:stone', 1))
            actor.pickup_delay = 10000
            runtime_id = actor.runtime_id
            for _ in range(60):
                yield
            actor = next((a for a in self.dimension.actors if a.runtime_id == runtime_id), None)
            if actor is None:
                raise RuntimeError('delayed item disappeared before pickup probe')
            self.player.inventory.clear()
            self.player.teleport(self.location(0.5, -60, 0.5))
            actor.teleport(self.location(0.5 + distance, -60, 0.5))
            actor.pickup_delay = 0
            case = f'pickup_x_{distance}'
            for _ in range(6):
                actor = next((a for a in self.dimension.actors if a.runtime_id == runtime_id), None)
                self.record(case, actor)
                yield
            actor = next((a for a in self.dimension.actors if a.runtime_id == runtime_id), None)
            if actor is not None:
                actor.remove()
        self.command('tickingarea remove entity_motion_water')
        self.command('tickingarea remove entity_motion_lava')
        self.command('tickingarea remove entity_motion_probe')

    def contact_block(self, x, z):
        data = self.dimension.get_block_at(x, -60, z).data
        return {'id': data.type, 'runtime_id': data.runtime_id, 'states': data.block_states}

    def record_impulse(self, state):
        live = {actor.runtime_id: actor for actor in self.dimension.actors}
        for case, runtime_id in state['sleep'].items():
            self.record(case, live.get(runtime_id))
        tnt = next((actor for actor in live.values() if actor.type == 'minecraft:tnt'), None)
        state['seen_tnt'] |= tnt is not None
        if state['sand'] is None:
            sand = next((actor for actor in live.values()
                         if actor.type == 'minecraft:falling_block' and actor.location.x < -16), None)
            if sand is not None:
                state['sand'] = sand.runtime_id
        sand = live.get(state['sand'])
        state['released'] |= state['seen_tnt'] and tnt is None
        self.record('falling_sand_impulse', sand, released=state['released'])
        if sand is not None and tnt is not None and not state['released']:
            # Keep the falling actor in blast range until this external impulse.
            # Only the uninterrupted post-release trajectory is a physics golden.
            p = tnt.location
            sand.teleport(self.location(p.x + 2, p.y, p.z))

    def run_contact(self):
        self.player = self.server.get_player('BDSSweep')
        if self.player is None:
            raise RuntimeError('probe player left before sampling')
        self.dimension = self.player.dimension
        for command in [
            'gamerule domobspawning false', 'gamerule randomtickspeed 0',
            'gamerule doweathercycle false', 'weather clear',
            'tickingarea add circle 0 -60 0 3 entity_motion_contact',
        ]:
            self.command(command)
        for _ in range(40):
            yield
        self.player.teleport(self.location(0.5, -60, 0.5))
        cells = []
        for index in range(32):
            x, z = -14 + (index % 8) * 4, -6 + (index // 8) * 4
            fluid = 'water' if index < 16 else 'lava'
            self.command(f'fill {x-1} -61 {z-1} {x+1} -60 {z+1} stone')
            self.command(f'setblock {x} -60 {z} air')
            cells.append({'x': x, 'z': z, 'fluid': fluid, 'depth': index % 16})
        self.command('setblock -18 -30 -18 sand')
        self.command('summon tnt -20 -30 -18 0 0 from_explosion')
        self.command('fill -22 -61 8 22 -61 32 ice')
        sleep = {}
        for index in range(32):
            stack = ItemStack('minecraft:stone', 1)
            meta = stack.item_meta
            meta.display_name = f'independent sleep probe {index}'
            if not stack.set_item_meta(meta):
                raise RuntimeError('could not prevent probe stacks merging')
            actor = self.dimension.drop_item(
                self.location(-13.5 + (index % 8) * 4, -59.99, 14.5 + (index // 8) * 4), stack)
            actor.pickup_delay = 10000
            case = f'sleep_ice_{index}'
            sleep[case] = actor.runtime_id
            self.record(case, actor)
        self.command('fill -22 -58 -2 -18 -58 2 oak_slab ["minecraft:vertical_half"="bottom"]')
        actor = self.dimension.drop_item(self.location(-19.5, -57.4, 0.5),
                                         ItemStack('minecraft:stone', 1))
        actor.pickup_delay = 10000
        sleep['item_floating_bottom_slab'] = actor.runtime_id
        self.record('item_floating_bottom_slab', actor)
        impulse = {'sand': None, 'seen_tnt': False, 'released': False, 'sleep': sleep}
        merge = []
        self.command('fill -29 -61 -26 -22 -61 2 stone')
        for index, distance in enumerate([0.70, 0.749, 0.751, 0.80]):
            pair = []
            for x in [-27.5, -23.5]:
                actor = self.dimension.drop_item(
                    self.location(x, -59.9, -23.5 + index * 8), ItemStack('minecraft:stone', 1))
                actor.pickup_delay = 10000
                pair.append(actor.runtime_id)
            merge.append((distance, -23.5 + index * 8, pair))
        results = []
        for profile in ['contact', 'vertical-response']:
            probes = [dict(cell, count=0, lower=0.0, upper=1.25, floor_contact=None)
                      for cell in cells]
            for attempt in range(80):
                pending = []
                for cell in probes:
                    count = cell['count']
                    if count >= 14 or (count >= 2 and not cell['floor_contact']):
                        continue
                    h = (0.0 if count == 0 else 1.25 if count == 1
                         else (cell['lower'] + cell['upper']) / 2)
                    x, z, fluid, depth = (cell[k] for k in ['x', 'z', 'fluid', 'depth'])
                    current = self.contact_block(x, z)
                    if (current['id'] != f'minecraft:{fluid}'
                            or current['states'].get('liquid_depth') != depth):
                        self.command(f'setblock {x} -60 {z} {fluid} ["liquid_depth"={depth}]')
                    actor = self.dimension.drop_item(self.location(x + 0.5, -60 + h, z + 0.5),
                                                     ItemStack('minecraft:netherite_scrap', 1))
                    actor.pickup_delay = 10000
                    case = f'{profile}_{fluid}_{depth}'
                    before = self.record(case, actor, stage='before', attempt=attempt, height=h,
                                         block=self.contact_block(x, z))
                    pending.append((cell, case, before, actor.runtime_id))
                if not pending:
                    break
                yield
                self.record_impulse(impulse)
                live = {actor.runtime_id: actor for actor in self.dimension.actors}
                for cell, case, before, runtime_id in pending:
                    actor = live.get(runtime_id)
                    after = self.record(case, actor, stage='after', attempt=attempt,
                                        block=self.contact_block(cell['x'], cell['z']))
                    if actor is None:
                        raise RuntimeError(f'contact actor disappeared: {case}')
                    actor.remove()
                    # A queued liquid update may change the requested state.
                    # Retain that observation, but never fit a different state.
                    if any(row['block']['states'].get('liquid_depth') != cell['depth']
                           for row in [before, after]):
                        continue
                    if profile == 'contact':
                        inside = before['in_water'] if cell['fluid'] == 'water' else before['in_lava']
                    else:
                        vy = before['velocity'][1]
                        air = (vy - 0.04) * 0.98
                        actual = after['velocity'][1]
                        # Locate any departure from the independently measured
                        # air recurrence; retain the raw response rather than
                        # assuming deep-fluid buoyancy also fits the surface.
                        inside = abs(actual - air) >= 0.000001
                    h = before['position'][1] + 60
                    if cell['count'] == 0:
                        cell['floor_contact'] = inside
                    elif cell['count'] == 1:
                        if inside:
                            raise RuntimeError(f'fluid contact exceeds search interval: {case}')
                    elif inside:
                        cell['lower'] = h
                    else:
                        cell['upper'] = h
                    cell['count'] += 1
            else:
                raise RuntimeError(f'liquid states did not stabilize for {profile}')
            results.extend(dict(cell, profile=profile) for cell in probes)
        # An overlying fluid can change a shallow cell's effective surface.
        for index, fluid in enumerate(['water', 'lava']):
            x, z = -20 + index * 40, -20
            self.command(f'fill {x-1} -61 {z-1} {x+1} -59 {z+1} stone')
            self.command(f'setblock {x} -60 {z} {fluid} ["liquid_depth"=7]')
            self.command(f'setblock {x} -59 {z} {fluid}')
            actor = self.dimension.drop_item(self.location(x + 0.5, -59.5, z + 0.5),
                                             ItemStack('minecraft:netherite_scrap', 1))
            actor.pickup_delay = 10000
            self.record(f'stacked_{fluid}', actor, stage='before', block=self.contact_block(x, z))
            runtime_id = actor.runtime_id
            yield
            self.record_impulse(impulse)
            actor = next((a for a in self.dimension.actors if a.runtime_id == runtime_id), None)
            self.record(f'stacked_{fluid}', actor, stage='after', block=self.contact_block(x, z))
            if actor is None:
                raise RuntimeError(f'stacked {fluid} actor disappeared')
            actor.remove()
        for _ in range(110):
            yield
            self.record_impulse(impulse)
        if impulse['sand'] is None or not impulse['released']:
            raise RuntimeError(f'falling-block impulse was not observed: {impulse}')
        runtime_id = next(iter(sleep.values()))
        actor = next((a for a in self.dimension.actors if a.runtime_id == runtime_id), None)
        if actor is None or not actor.is_on_ground:
            raise RuntimeError('sleep probe did not settle before support removal')
        p = actor.location
        x, z = int(p.x // 1), int(p.z // 1)
        self.record('sleep_support_removed', actor, stage='before')
        self.command(f'fill {x-1} -61 {z-1} {x+1} -61 {z+1} air')
        for _ in range(12):
            yield
            actor = next((a for a in self.dimension.actors if a.runtime_id == runtime_id), None)
            self.record('sleep_support_removed', actor, stage='after')
        live = {actor.runtime_id: actor for actor in self.dimension.actors}
        for distance, z, pair in merge:
            for index, runtime_id in enumerate(pair):
                actor = live.get(runtime_id)
                if actor is None:
                    raise RuntimeError(f'merge actor disappeared before boundary probe: {distance}')
                actor.teleport(self.location(-25.5 + index * distance, -60, z))
                self.record(f'merge_{distance}_{index}', actor)
        for _ in range(40):
            yield
            live = {actor.runtime_id: actor for actor in self.dimension.actors}
            for distance, _, pair in merge:
                for index, runtime_id in enumerate(pair):
                    self.record(f'merge_{distance}_{index}', live.get(runtime_id))
        (self.out / 'contact-results.json').write_text(json.dumps(results, indent=2), encoding='utf-8')
        self.command('tickingarea remove entity_motion_contact')

    def probe_cube(self, x, z):
        result = []
        for y in range(-61, -58):
            for dx in range(-2, 3):
                for dz in range(-2, 3):
                    data = self.dimension.get_block_at(x + dx, y, z + dz).data
                    result.append({'position': [x + dx, y, z + dz], 'id': data.type,
                                   'states': data.block_states})
        return result

    def record_named(self, actors):
        live = {actor.runtime_id: actor for actor in self.dimension.actors}
        for case, (runtime_id, birth) in actors.items():
            self.record(case, live.get(runtime_id), spawn_tick=birth)

    def run_kinematics(self, flow_corners=False):
        self.player = self.server.get_player('BDSSweep')
        if self.player is None:
            raise RuntimeError('probe player left before sampling')
        self.dimension = self.player.dimension
        for command in [
            'gamerule domobspawning false', 'gamerule randomtickspeed 0',
            'tickingarea add circle 0 -60 0 3 entity_kinematics',
        ]:
            self.command(command)
        for _ in range(40):
            yield
        self.player.teleport(self.location(0.5, -60, 0.5))
        configurations = []
        for fluid in ['water', 'lava']:
            configurations.extend([
                (f'{fluid}_fall_east', fluid, 8, None, 'fall_east'),
                (f'{fluid}_fall_nine_east', fluid, 9, None, 'fall_east'),
                (f'{fluid}_fall_east_open', fluid, 8, None, 'fall_east_open'),
                (f'{fluid}_fall_above_wall', fluid, 8, None, 'above_wall'),
                (f'{fluid}_cliff_diagonal', fluid, 1, None, 'cliff_diagonal'),
                (f'{fluid}_boundary_cancel', fluid, 1, None, 'boundary_cancel'),
                (f'{fluid}_boundary_diagonal', fluid, 0, None, 'boundary_diagonal'),
                (f'{fluid}_fall_stack_open', fluid, 8, 0, 'open'),
                (f'{fluid}_fall_stack_wall', fluid, 8, 0, 'closed'),
            ] if flow_corners else [
                (f'{fluid}_single', fluid, 0, None, 'closed'),
                (f'{fluid}_shallow_under_source', fluid, 7, 0, 'closed'),
                (f'{fluid}_source_under_shallow', fluid, 0, 7, 'closed'),
                (f'{fluid}_shallow_stack', fluid, 7, 7, 'closed'),
                (f'{fluid}_fall_open', fluid, 8, None, 'open'),
                (f'{fluid}_fall_wall', fluid, 8, None, 'east_wall'),
                (f'{fluid}_east_source', fluid, 1, None, 'east_source'),
                (f'{fluid}_east_shallow', fluid, 1, None, 'east_shallow'),
                (f'{fluid}_diagonal', fluid, 0, None, 'diagonal'),
            ])
        heights = [0.0, 0.70, 0.90] if flow_corners else [0.0, 0.62, 0.64, 0.70, 0.76, 0.86, 0.88, 0.99, 1.10]
        for h in heights:
            pending = []
            for index, (case, fluid, depth, above, sides) in enumerate(configurations):
                x, z = -12 + (index % 5) * 6, -26 + (index // 5) * 6
                self.command(f'fill {x-2} -61 {z-2} {x+2} -59 {z+2} stone')
                self.command(f'fill {x-1} -60 {z-1} {x+1} -59 {z+1} air')
                for dx, dz in [(1, 0), (-1, 0), (0, 1), (0, -1)]:
                    solid = (sides not in ['open', 'east_wall', 'fall_east_open', 'above_wall']
                             or (sides == 'east_wall' and dx == 1))
                    if solid:
                        self.command(f'fill {x+dx} -60 {z+dz} {x+dx} -59 {z+dz} stone')
                self.command(f'setblock {x} -60 {z} {fluid} ["liquid_depth"={depth}]')
                if above is not None:
                    self.command(f'setblock {x} -59 {z} {fluid} ["liquid_depth"={above}]')
                if sides in ['east_source', 'east_shallow', 'diagonal', 'fall_east',
                             'fall_east_open', 'above_wall', 'boundary_cancel', 'boundary_diagonal']:
                    neighbour_depth = (0 if sides == 'east_source' else 1
                                       if sides in ['diagonal', 'boundary_diagonal'] else 7)
                    self.command(f'setblock {x+1} -60 {z} {fluid} ["liquid_depth"={neighbour_depth}]')
                    if sides in ['diagonal', 'boundary_diagonal']:
                        self.command(f'setblock {x} -60 {z+1} {fluid} ["liquid_depth"=1]')
                if sides == 'above_wall':
                    self.command(f'setblock {x+1} -59 {z} stone')
                if sides == 'cliff_diagonal':
                    self.command(f'setblock {x+1} -60 {z} air')
                    self.command(f'setblock {x+1} -61 {z} {fluid} ["liquid_depth"=0]')
                    self.command(f'setblock {x} -60 {z+1} {fluid} ["liquid_depth"=7]')
                offset = 0.4 if sides.startswith('boundary_') else 0.0
                actor = self.dimension.drop_item(self.location(x + 0.5 + offset, -60 + h, z + 0.5),
                                                 ItemStack('minecraft:netherite_scrap', 1))
                actor.pickup_delay = 10000
                self.record(case, actor, stage='before', height=h, cube=self.probe_cube(x, z))
                pending.append((case, actor.runtime_id, x, z))
            yield
            live = {actor.runtime_id: actor for actor in self.dimension.actors}
            for case, runtime_id, x, z in pending:
                actor = live.get(runtime_id)
                self.record(case, actor, stage='after', height=h, cube=self.probe_cube(x, z))
                if actor is None:
                    raise RuntimeError(f'fluid kinematics actor disappeared: {case}')
                actor.remove()
        if flow_corners:
            yield from self.run_vertical_pickup()
            self.command('tickingarea remove entity_kinematics')
            return
        self.command('fill -22 -61 8 22 -61 32 ice')
        actors, wakes = {}, []
        for wave in range(4):
            for index in range(4):
                case = f'cadence_ice_{wave}_{index}'
                stack = ItemStack('minecraft:stone', 1)
                meta = stack.item_meta
                meta.display_name = case
                if not stack.set_item_meta(meta):
                    raise RuntimeError('could not isolate cadence probe stack')
                actor = self.dimension.drop_item(
                    self.location(-11.5 + index * 8, -59.99, 12.5 + wave * 4), stack)
                actor.pickup_delay = 10000
                actors[case] = (actor.runtime_id, self.tick_number)
                self.record(case, actor, spawn_tick=self.tick_number)
                if index == 0:
                    wakes.append((case, actor.runtime_id, self.tick_number))
            case = f'air_zero_{wave}'
            actor = self.dimension.drop_item(
                self.location(-11.5 + wave * 8, -30, -31.5), ItemStack('minecraft:stone', 1))
            actor.pickup_delay = 10000
            runtime_id = actor.runtime_id
            actors[case] = (runtime_id, self.tick_number)
            yield
            self.record_named(actors)
            actor = next((a for a in self.dimension.actors if a.runtime_id == runtime_id), None)
            if actor is None:
                raise RuntimeError('air actor disappeared before zero-velocity control')
            p = actor.location
            if not actor.teleport(self.location(p.x, p.y + 0.001, p.z)):
                raise RuntimeError('air zero-velocity teleport failed')
            self.record(case, actor, stage='zero_reset', spawn_tick=actors[case][1])
        for _ in range(120):
            yield
            self.record_named(actors)
        live = {actor.runtime_id: actor for actor in self.dimension.actors}
        for case, runtime_id, birth in wakes:
            actor = live.get(runtime_id)
            if actor is None or not actor.is_on_ground:
                raise RuntimeError(f'cadence actor did not settle: {case}')
            p = actor.location
            x, z = int(p.x // 1), int(p.z // 1)
            self.record(f'wake_{case}', actor, stage='before', spawn_tick=birth)
            if any(self.dimension.get_block_at(bx, -61, bz).type != 'minecraft:air'
                   for bx in range(x-1, x+2) for bz in range(z-1, z+2)):
                self.command(f'fill {x-1} -61 {z-1} {x+1} -61 {z+1} air')
            self.record(f'wake_{case}', actor, stage='removed', spawn_tick=birth)
        for _ in range(12):
            yield
            live = {actor.runtime_id: actor for actor in self.dimension.actors}
            for case, runtime_id, birth in wakes:
                self.record(f'wake_{case}', live.get(runtime_id), stage='after', spawn_tick=birth)
        self.command('tickingarea remove entity_kinematics')

    def run_vertical_pickup(self):
        # The item stays on a one-block floating platform. Move the flying
        # player instead, so gravity cannot change a boundary before pickup.
        # The player is horizontally clear of the platform's collision box.
        self.command('setblock 0 -30 16 stone')
        self.player.allow_flight = True
        self.player.is_flying = True
        for distance in [2.299, 2.301, -0.749, -0.751]:
            self.player.teleport(self.location(1.31, -20, 16.5))
            self.player.is_flying = True
            self.player.inventory.clear()
            actor = self.dimension.drop_item(self.location(0.5, -28.9, 16.5),
                                             ItemStack('minecraft:stone', 1))
            actor.pickup_delay = 10000
            if not actor.teleport(self.location(0.5, -29, 16.5)):
                raise RuntimeError('stationary pickup item teleport failed')
            runtime_id = actor.runtime_id
            for _ in range(8):
                yield
            actor = next((a for a in self.dimension.actors if a.runtime_id == runtime_id), None)
            if actor is None or abs(actor.location.y + 29) > 0.00001 or not actor.is_on_ground:
                raise RuntimeError('vertical pickup item did not settle on its platform')
            self.player.teleport(self.location(1.31, -29 - distance, 16.5))
            self.player.is_flying = True
            actor.pickup_delay = 0
            for _ in range(6):
                actor = next((a for a in self.dimension.actors if a.runtime_id == runtime_id), None)
                p = self.player.location
                self.record(f'pickup_y_{distance}', actor, player_position=[p.x, p.y, p.z],
                            flying=self.player.is_flying)
                yield
            actor = next((a for a in self.dimension.actors if a.runtime_id == runtime_id), None)
            if actor is not None:
                actor.remove()

    def run_ground_liquid(self):
        self.player = self.server.get_player('BDSSweep')
        if self.player is None:
            raise RuntimeError('probe player left before sampling')
        self.dimension = self.player.dimension
        for command in [
            'gamerule domobspawning false', 'gamerule randomtickspeed 0',
            'tickingarea add circle 0 -60 0 3 entity_ground_liquid',
        ]:
            self.command(command)
        for _ in range(40):
            yield
        self.player.teleport(self.location(0.5, -60, 0.5))
        self.command('fill -18 -61 8 18 -61 24 ice')
        actors = []
        for case, x, fluid, depth in [
            ('ground_air', -11.5, None, None),
            ('ground_thin_water', 0.5, 'water', 7),
            ('ground_lava_depth8_input', 12.5, 'lava', 8),
        ]:
            actor = self.dimension.drop_item(self.location(x, -59.99, 16.5),
                                             ItemStack('minecraft:netherite_scrap', 1))
            actor.pickup_delay = 10000
            actors.append((case, actor.runtime_id, fluid, depth, self.tick_number))
        for _ in range(121):
            yield
        for attempt in range(16):
            pending = []
            live = {actor.runtime_id: actor for actor in self.dimension.actors}
            for case, runtime_id, fluid, depth, birth in actors:
                actor = live.get(runtime_id)
                if actor is None or (attempt == 0 and not actor.is_on_ground):
                    raise RuntimeError(f'ground-liquid actor missing or initially unsettled: {case}')
                p = actor.location
                x, z = int(p.x // 1), int(p.z // 1)
                if fluid is not None:
                    needs_reset = False
                    for bx in range(x-2, x+3):
                        for bz in range(z-2, z+3):
                            data = self.dimension.get_block_at(bx, -60, bz).data
                            needs_reset |= (data.type != f'minecraft:{fluid}'
                                            or data.block_states.get('liquid_depth') != depth)
                    if needs_reset:
                        self.command(f'fill {x-2} -60 {z-2} {x+2} -60 {z+2} '
                                     f'{fluid} ["liquid_depth"={depth}]')
                self.record(case, actor, stage='before', attempt=attempt, spawn_tick=birth,
                            cube=self.probe_cube(x, z))
                pending.append((case, runtime_id, birth, x, z))
            yield
            live = {actor.runtime_id: actor for actor in self.dimension.actors}
            for case, runtime_id, birth, x, z in pending:
                actor = live.get(runtime_id)
                self.record(case, actor, stage='after', attempt=attempt, spawn_tick=birth,
                            cube=self.probe_cube(x, z))
                if actor is None:
                    raise RuntimeError(f'ground-liquid actor disappeared: {case}')
        self.command('tickingarea remove entity_ground_liquid')

    def drop_player_state(self, player):
        p, v = player.location, player.velocity
        return {'name': player.name, 'runtime_id': player.runtime_id,
                'position': [p.x, p.y, p.z], 'velocity': [v.x, v.y, v.z],
                'yaw': p.yaw, 'pitch': p.pitch}

    @event_handler
    def on_drop_spawn(self, event: ActorSpawnEvent):
        active = getattr(self, 'drop_active', None)
        if active is None or event.actor.type != 'minecraft:item':
            return
        actor = event.actor
        self.drop_actor_cases[actor.runtime_id] = active['case']
        active['ids'].add(actor.runtime_id)
        self.record(active['case'], actor, stage='actor_spawn',
                    player=self.drop_player_state(self.player))

    @event_handler
    def on_player_drop(self, event: PlayerDropItemEvent):
        active = getattr(self, 'drop_active', None)
        if active is None or event.player.name != 'BDSSweep':
            return
        self.record(active['case'], None, stage='player_drop_event',
                    item=event.item.type.id, count=event.item.amount,
                    player=self.drop_player_state(event.player))

    @event_handler
    def on_player_pickup(self, event: PlayerPickupItemEvent):
        case = getattr(self, 'drop_actor_cases', {}).get(event.item.runtime_id)
        if case is not None:
            self.record(case, event.item, stage='pickup_event',
                        player=self.drop_player_state(event.player))

    def run_player_drop(self):
        self.player = self.server.get_player('BDSSweep')
        if self.player is None:
            raise RuntimeError('dropper disconnected before player-drop probe')
        self.dimension = self.player.dimension
        self.drop_actor_cases = {}
        self.drop_active = None
        for _ in range(200):
            other = self.server.get_player('BDSOther')
            if other is not None:
                break
            yield
        else:
            raise RuntimeError('second scripted player BDSOther did not join')
        for command in [
            'gamerule domobspawning false', 'gamerule randomtickspeed 0',
            'gamerule doweathercycle false', 'weather clear',
            'tickingarea add circle 0 -60 0 2 entity_player_drop',
            'fill -24 -61 -8 24 -61 24 stone',
        ]:
            self.command(command)
        for _ in range(40):
            yield
        modes = [('hotbar_q', 'hotbar', 0, 1),
                 ('inventory_q', 'inventory', 9, 1),
                 ('ctrl_stack', 'hotbar', 0, 64)]
        # No API exposes the internal throw-time counter. Separate the global
        # pickup delay from any thrower-only exclusion through native controls.
        experiments = [
            (name, container, slot, count, target, 0.0, 0.0)
            for name, container, slot, count in modes
            for target in ('thrower', 'other', 'thrower_zero_delay', 'other_zero_delay')
        ]
        # Preserve unmodified impulses at several orientations. All players
        # are stationary; this does not assert a moving-player inheritance rule.
        experiments += [
            (name, container, slot, count, f'motion_{sample}', yaw, pitch)
            for name, container, slot, count in modes
            for yaw, pitch in [(0.0, 0.0), (90.0, -30.0), (-135.0, 45.0)]
            for sample in range(4)
        ]
        experiments += [('hotbar_q', 'hotbar', 0, 1, 'owner_retention', 0.0, 0.0)]
        if os.environ.get('ENTITY_MOTION_PROBE_ANGLE_CONTROLS') == '1':
            experiments = [
                ('hotbar_q', 'hotbar', 0, 1, f'motion_angle_{index}', yaw, pitch)
                for index, (yaw, pitch) in enumerate([
                    (0.0, -89.0), (17.25, -60.0), (90.0, -45.25),
                    (-135.0, -30.0), (123.456, -29.999), (-179.99, -1.0),
                    (179.99, -0.5), (270.0, 0.5), (-270.0, 1.0),
                    (-72.25, 17.25), (45.0, 30.0), (60.0, 45.25),
                    (-90.0, 60.0), (-123.456, 89.0), (359.99, -17.25),
                    (-359.99, 17.25),
                ])
            ]
        for attempt, (name, container, slot, count, target, yaw, pitch) in enumerate(experiments):
            case = f'{name}_{target}_yaw{yaw}_pitch{pitch}_{attempt}'
            self.player.send_message('bds-sweep:close-inventory')
            self.player.inventory.clear()
            other.inventory.clear()
            self.player.inventory.held_item_slot = 0
            if not self.player.teleport(Location(self.dimension, 0.5, -60, 0.5, pitch, yaw)):
                raise RuntimeError('dropper teleport failed')
            if not other.teleport(self.location(10.5, -60, 0.5)):
                raise RuntimeError('observer teleport failed')
            # Endstone's direct inventory mutation produced no native slot
            # update in the v3 wire trace. Use the vanilla command path.
            command_slot = 'slot.hotbar 0' if slot == 0 else 'slot.inventory 0'
            self.command(f'replaceitem entity "BDSSweep" {command_slot} minecraft:stone 64')
            stored = self.player.inventory.get_item(slot)
            if stored is None or stored.type.id != 'minecraft:stone' or stored.amount != 64:
                raise RuntimeError(f'vanilla command did not prepare native source slot {slot}')
            for _ in range(8):
                yield
            self.drop_active = {'case': case, 'ids': set()}
            self.record(case, None, stage='before_request', attempt=attempt,
                        container=container, slot=slot, count=count, target=target,
                        player=self.drop_player_state(self.player))
            self.player.send_message(f'bds-sweep:drop {container} {slot} {count}')
            for _ in range(30):
                yield
                live = {a.runtime_id: a for a in self.dimension.actors}
                actors = [live[rid] for rid in self.drop_active['ids'] if rid in live]
                if actors:
                    break
            else:
                raise RuntimeError(f'no native item spawned after client Drop: {case}')
            ids = set(self.drop_active['ids'])
            for actor in actors:
                self.record(case, actor, stage='first_observation')
            # Let the initial AddItemActor/motion leave the native viewer
            # pipeline before any timing-control teleport resets velocity.
            for initial_step in range(3):
                yield
                live = {a.runtime_id: a for a in self.dimension.actors}
                for rid in ids:
                    if rid not in live:
                        raise RuntimeError(f'item vanished before initial wire sample: {case}')
                    self.record(case, live[rid], stage='unmodified_initial', initial_step=initial_step)
            if target.endswith('zero_delay'):
                for rid in ids:
                    live[rid].pickup_delay = 0
                    self.record(case, live[rid], stage='zero_delay_applied')
            limit = 2 if target.startswith('motion_') else 60
            for elapsed in range(limit):
                live = {a.runtime_id: a for a in self.dimension.actors}
                remaining = [live[rid] for rid in ids if rid in live]
                self.record(case, None, stage='remaining', elapsed=elapsed,
                            runtime_ids=[actor.runtime_id for actor in remaining])
                if not remaining:
                    break
                for actor in remaining:
                    self.record(case, actor, stage='tick', elapsed=elapsed)
                    if target == 'owner_retention' or target.startswith('motion_'):
                        position = self.location(0.5, -30, 16.5)
                    else:
                        position = (other if target.startswith('other') else self.player).location
                    if not actor.teleport(position):
                        raise RuntimeError(f'item timing-control teleport failed: {case}')
                yield
            live = {a.runtime_id: a for a in self.dimension.actors}
            for rid in ids:
                if rid in live:
                    self.record(case, live[rid], stage='final_uncollected')
                    live[rid].remove()
            self.drop_active = None
            self.player.send_message('bds-sweep:close-inventory')
            for _ in range(2):
                yield
        self.command('tickingarea remove entity_player_drop')
