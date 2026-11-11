"""Items of the `pvp` sweep: player-versus-player hits between three
lockstep clients (A = the sweep player, B = the victim, C = a second
attacker; see `jobs.pvp` and the client's `lockstep` module).

Every item puts A at (0.5, -60, 0.5) facing east (+x), B east of it facing
west and C out of the way 12 blocks south unless the item places it. Each
lockstep tick sends `i0 i1 i2` (every client's input, in login order)
unless the item's `script` or `when` says otherwise. A plain hit is
`a0>1 i0 i1 i2`: the attack, then the attacker's input of that tick, as a
vanilla client sends them.

Sets (`--items`): `numbers` (damage, effects, armour, enchantments,
difficulty, game modes, the pvp rule, crits, knockback), `reach` (distance,
claimed position, facing, walls), `hurt` (the hurt window, bursts, two
attackers, the kill race, mutual hits, victim input order, sprint toggles,
the swing), `all` (those three), `extra` (follow-ups of the first run), or
item keys; `--repeats N` runs each item N times.
"""

from __future__ import annotations

Y = -60
A_AT = [0.5, Y, 0.5, -90, 0]
C_IDLE = [0.5, Y, 12.5, 180, 0]
# C two blocks south of B (B at distance 2), facing north at it.
C_SIDE = [2.5, Y, 2.5, 180, 0]
INPUTS = 'i0 i1 i2'
HIT = 'a0>1 ' + INPUTS
# Weapons by damage (BDS 1.26.51 attack sweep): 3 to 8.
LADDER = {'1': 'minecraft:copper_shovel', '2': 'minecraft:copper_pickaxe', '3': 'minecraft:copper_axe',
          '4': 'minecraft:copper_sword', '5': 'minecraft:diamond_axe', '6': 'minecraft:diamond_sword'}
ARMOUR_SLOTS = ('helmet', 'chestplate', 'leggings', 'boots')
PIECES = {'helmet': 'helmet', 'chestplate': 'chestplate', 'leggings': 'leggings', 'boots': 'boots'}


def b_at(distance: float, z: float = 0.5) -> list:
    return [round(0.5 + distance, 4), Y, z, 90, 0]


def spec(item: str | None, **enchants: int) -> dict | None:
    if item is None:
        return None
    out = {'item': item if ':' in item else f'minecraft:{item}'}
    if enchants:
        out['enchants'] = {f'minecraft:{name}': level for name, level in enchants.items()}
    return out


def suit(material: str, **enchants: int) -> dict:
    return {slot: spec(f'{material}_{piece}', **enchants) for slot, piece in PIECES.items()}


def item(key: str, *, a=None, b=None, c=None, held=(None, None, None), hotbar=None, armour=None,
         script=None, when=None, ticks=40, **extra) -> dict:
    out = {'key': key, 'at': [a or A_AT, b or b_at(2.0), c or C_IDLE], 'held': list(held),
           'ticks': ticks, 'script': {str(t): steps for t, steps in (script or {5: HIT}).items()}}
    if hotbar:
        out['hotbar'] = hotbar
    if armour:
        out['armour'] = armour
    if when:
        out['when'] = when
    out.update(extra)
    return out


def numbers_items() -> list[dict]:
    items = []
    weapons = [None, 'wooden_sword', 'stone_sword', 'iron_sword', 'golden_sword', 'diamond_sword',
               'netherite_sword', 'diamond_axe', 'netherite_axe', 'iron_pickaxe', 'copper_shovel', 'trident',
               'mace', 'stick']
    for weapon in weapons:
        items.append(item(f'weapon|{weapon or "hand"}', held=(spec(weapon), None, None)))
    for level in (1, 3, 5):
        items.append(item(f'sharpness|{level}', held=(spec('diamond_sword', sharpness=level), None, None)))
    for weapon in (None, 'diamond_sword'):
        name = weapon or 'hand'
        for effect, level in (('strength', 0), ('strength', 1), ('weakness', 0)):
            items.append(item(f'effect|{effect}{level + 1}|{name}', held=(spec(weapon), None, None),
                              effects=[f'effect "{{A}}" {effect} 100 {level} true']))
    for level in range(5):
        items.append(item(f'resistance|{level + 1}', held=(spec('diamond_sword'), None, None),
                          effects=[f'effect "{{B}}" resistance 100 {level} true']))
    for material in ('leather', 'iron', 'diamond', 'netherite'):
        items.append(item(f'armour|{material}', held=(spec('diamond_sword'), None, None),
                          armour=[None, suit(material), None]))
    for level in (1, 4):
        items.append(item(f'armour|diamond+protection{level}', held=(spec('diamond_sword'), None, None),
                          armour=[None, suit('diamond', protection=level), None]))
    items.append(item('armour|netherite+protection4', held=(spec('diamond_sword'), None, None),
                      armour=[None, suit('netherite', protection=4), None]))
    items.append(item('armour|diamond_chestplate', held=(spec('diamond_sword'), None, None),
                      armour=[None, {'chestplate': spec('diamond_chestplate')}, None]))
    items.append(item('armour|diamond+resistance2', held=(spec('diamond_sword'), None, None),
                      armour=[None, suit('diamond', protection=4), None],
                      effects=['effect "{B}" resistance 100 1 true']))
    for difficulty in ('peaceful', 'easy', 'normal', 'hard'):
        items.append(item(f'difficulty|{difficulty}', held=(spec('diamond_sword'), None, None),
                          difficulty=difficulty))
    items.append(item('rule|pvp_false', held=(spec('diamond_sword'), None, None),
                      setup=['gamerule pvp false'], cleanup=['gamerule pvp true']))
    for name, modes in (('victim_creative', ['survival', 'creative', 'survival']),
                        ('victim_spectator', ['survival', 'spectator', 'survival']),
                        ('victim_adventure', ['survival', 'adventure', 'survival']),
                        ('attacker_creative', ['creative', 'survival', 'survival']),
                        ('attacker_adventure', ['adventure', 'survival', 'survival'])):
        items.append(item(f'mode|{name}', held=(spec('diamond_sword'), None, None), modes=modes))
    # Crits: A jumps on t 3 and hits k ticks later (rising, apex, falling,
    # landed); a sprint jump; under Blindness; B is then 1.5 away so the
    # jump stays in reach.
    for k in range(1, 13):
        items.append(item(f'crit|jump+{k}', held=(spec('diamond_sword'), None, None), b=b_at(1.5),
                          script={3: 'o0=jump:1 ' + INPUTS, 3 + k: HIT}))
    items.append(item('crit|blind_jump+8', held=(spec('diamond_sword'), None, None), b=b_at(1.5),
                      effects=['effect "{A}" blindness 100 0 true'],
                      script={3: 'o0=jump:1 ' + INPUTS, 11: HIT}))
    items.append(item('crit|slow_falling_jump+8', held=(spec('diamond_sword'), None, None), b=b_at(1.5),
                      effects=['effect "{A}" slow_falling 100 0 true'],
                      script={3: 'o0=jump:1 ' + INPUTS, 11: HIT}))
    items.append(item('crit|sprint_jump', held=(spec('diamond_sword'), None, None), b=b_at(7.0), ticks=50,
                      script={0: 'o0=move:1:0 o0=sprint:on ' + INPUTS},
                      when=[{'after': 6, 'a': 0, 'b': 1, 'close': 4.6, 'steps': 'o0=jump:1 ' + INPUTS},
                            {'after': 6, 'a': 0, 'b': 1, 'close': 2.4, 'steps': HIT}]))
    items.append(item('crit|mace_fall', held=(spec('mace'), None, None), b=b_at(1.5),
                      script={3: 'o0=jump:1 ' + INPUTS, 11: HIT}))
    # Knockback.
    for name, weapon in (('hand', None), ('diamond_sword', spec('diamond_sword')),
                         ('knockback1', spec('iron_sword', knockback=1)),
                         ('knockback2', spec('iron_sword', knockback=2))):
        items.append(item(f'knockback|{name}', held=(weapon, None, None)))
        items.append(item(f'knockback|sprint|{name}', held=(weapon, None, None), b=b_at(7.0), ticks=50,
                          script={0: 'o0=move:1:0 o0=sprint:on ' + INPUTS},
                          when=[{'after': 6, 'a': 0, 'b': 1, 'close': 2.6, 'steps': HIT}]))
    items.append(item('knockback|walk|hand', b=b_at(5.0), ticks=50,
                      script={0: 'o0=move:1:0 ' + INPUTS},
                      when=[{'after': 4, 'a': 0, 'b': 1, 'close': 2.6, 'steps': HIT}]))
    items.append(item('knockback|victim_netherite', armour=[None, suit('netherite'), None]))
    items.append(item('knockback|victim_netherite_chestplate',
                      armour=[None, {'chestplate': spec('netherite_chestplate')}, None]))
    for k in (2, 5, 8):
        items.append(item(f'knockback|victim_jump+{k}', script={3: 'o1=jump:1 ' + INPUTS, 3 + k: HIT}))
    items.append(item('knockback|victim_sneaking', script={0: 'o1=sneak:on ' + INPUTS, 8: HIT}))
    items.append(item('knockback|victim_walking_away', b=[2.5, Y, 0.5, -90, 0],
                      script={0: 'o1=move:1:0 ' + INPUTS, 6: HIT}))
    items.append(item('knockback|diagonal', b=[2.0, Y, 1.7, 90, 0]))
    return items


def reach_items(strict: bool) -> list[dict]:
    items = []
    for distance in (3.0, 3.5, 4.0, 4.5, 5.0, 5.5, 6.0, 6.5, 6.6, 6.7, 6.8, 6.9, 7.0, 7.5):
        items.append(item(f'reach|survival|{distance}', b=b_at(distance)))
    for distance in (7.0, 8.0, 10.0, 11.5, 12.0, 12.3, 12.5, 12.7, 13.0, 13.5, 14.0):
        items.append(item(f'reach|creative|{distance}', b=b_at(distance),
                          modes=['creative', 'survival', 'survival']))
    # Height: B on a pillar, so the eye-to-box distance differs from the
    # feet distance.
    for distance, height in ((4.0, 4), (5.0, 5)):
        items.append(item(f'reach|survival|{distance}+up{height}', b=[0.5 + distance, Y + height, 0.5, 90, 0],
                          setup=[f'fill {int(0.5 + distance)} {Y} 0 {int(0.5 + distance)} {Y + height - 1} 0 stone'],
                          cleanup=[f'fill {int(0.5 + distance)} {Y} 0 {int(0.5 + distance)} {Y + height - 1} 0 air']))
    # The claimed position: off by this much along x (away from B) or up.
    for offset in (1.0, 3.0, 5.9, 6.0, 6.1, 7.0):
        items.append(item(f'claim|x-{offset}', script={4: f'o0=claim:-{offset}:0:0 ' + INPUTS, 5: HIT,
                                                        6: 'o0=claim:0:0:0 ' + INPUTS}))
        items.append(item(f'claim|y+{offset}', script={4: f'o0=claim:0:{offset}:0 ' + INPUTS, 5: HIT,
                                                        6: 'o0=claim:0:0:0 ' + INPUTS}))
    # Facing: A turned away, sideways, or looking straight down.
    for name, yaw, pitch in (('away', 90, 0), ('side', 0, 0), ('down', -90, 90), ('up', -90, -90)):
        items.append(item(f'facing|{name}', a=[0.5, Y, 0.5, yaw, pitch]))
    # Walls between A (0.5) and B (3.5): full, eye level only, feet only.
    for name, ys in (('full', (Y, Y + 1)), ('eye', (Y + 1,)), ('feet', (Y,))):
        setup = [f'setblock 2 {y} 0 stone' for y in ys]
        cleanup = [f'setblock 2 {y} 0 air' for y in ys]
        items.append(item(f'wall|{name}', b=b_at(3.0), setup=setup, cleanup=cleanup))
    if strict:
        for item_ in items:
            item_['key'] = 'strict|' + item_['key']
    return items


def burst(order: list[str], batch: bool) -> str:
    acts = []
    for slot in order:
        acts += [f'o0=slot:{slot}', 'a0>1']
    if batch:
        return '[' + ','.join(acts + ['i0']) + '] i1 i2'
    return ' '.join(acts + ['i0', 'i1', 'i2'])


def hurt_items() -> list[dict]:
    items = []
    fist_hits = lambda *ticks: {t: HIT for t in ticks}  # noqa: E731
    # Equal hits k ticks apart: where the window ends.
    for k in (1, 5, 8, 9, 10, 11, 12):
        items.append(item(f'window|equal+{k}', held=(spec('copper_sword'), None, None),
                          script=fist_hits(5, 5 + k)))
    # Stronger, equal and weaker hits inside the window (slot 1: 3 ... 6: 8).
    ladder = [{slot: spec(weapon) for slot, weapon in LADDER.items()}, None, None]

    def slot_hit(slot):
        return f'o0=slot:{slot} a0>1 ' + INPUTS

    sequences = {
        # 4, then 7 two ticks later, 7 again three later, then 8 nine ticks
        # after the 7 (inside a window restarted by it, outside the first).
        'h9|4-7-7-8@+9': {5: slot_hit(2), 7: slot_hit(5), 10: slot_hit(5), 16: slot_hit(6)},
        'h9|4-7-8@+10': {5: slot_hit(2), 7: slot_hit(5), 17: slot_hit(6)},
        'h9|4-7-8@+8': {5: slot_hit(2), 7: slot_hit(5), 15: slot_hit(6)},
        'h9|7-4': {5: slot_hit(5), 7: slot_hit(2)},
        'h9|4-4': {5: slot_hit(2), 7: slot_hit(2)},
    }
    for key, script in sequences.items():
        items.append(item(key, hotbar=ladder, script=script))
    # Bursts in one tick (H13 / race case 5): ascending and descending
    # damage, separate packets or one batch with the input; five equal hits.
    for name, order in (('up', ['1', '2', '3', '4', '5']), ('down', ['5', '4', '3', '2', '1'])):
        for batch in (False, True):
            items.append(item(f'burst|{name}|{"batch" if batch else "packets"}', hotbar=ladder,
                              script={5: burst(order, batch)}))
    items.append(item('burst|equal5|packets', held=(spec('copper_sword'), None, None),
                      script={5: 'a0>1 a0>1 a0>1 a0>1 a0>1 ' + INPUTS}))
    items.append(item('burst|equal5|batch', held=(spec('copper_sword'), None, None),
                      script={5: '[a0>1,a0>1,a0>1,a0>1,a0>1,i0] i1 i2'}))
    # Two attackers in one tick (race case 6).
    for name, held, steps in (
            ('hand_then_hand', (None, None, None), 'a0>1 i0 a2>1 i2 i1'),
            ('hand_then_sword', (None, None, spec('diamond_sword')), 'a0>1 i0 a2>1 i2 i1'),
            ('sword_then_hand', (None, None, spec('diamond_sword')), 'a2>1 i2 a0>1 i0 i1'),
            ('sword_then_sword', (spec('diamond_sword'), None, spec('diamond_sword')), 'a0>1 i0 a2>1 i2 i1'),
            ('hand_then_sword_next_tick', (None, None, spec('diamond_sword')), None)):
        script = {5: steps} if steps else {5: 'a0>1 ' + INPUTS, 6: 'a2>1 ' + INPUTS}
        items.append(item(f'two|{name}', held=held, c=C_SIDE, script=script))
    # Kill race (case 8): B at 1 health; A's hit kills B, B's hit on A in
    # the same tick after it, before it, and batched.
    for name, steps in (('a_then_b', 'a0>1 i0 a1>0 i1 i2'), ('b_then_a', 'a1>0 i1 a0>1 i0 i2'),
                        ('a_then_b_batched', '[a0>1,i0] [a1>0,i1] i2')):
        items.append(item(f'kill|{name}', held=(spec('diamond_sword'), spec('diamond_sword'), None),
                          damage_before=[0, 19, 0], script={5: steps}))
    # Mutual hits (case 1) and the victim's input around the hit (case 2):
    # the knockback values.
    for name, steps in (('a_first', 'a0>1 i0 a1>0 i1 i2'), ('b_first', 'a1>0 i1 a0>1 i0 i2'),
                        ('batched', '[a0>1,i0] [a1>0,i1] i2')):
        items.append(item(f'mutual|{name}', script={5: steps}))
    for name, steps in (('input_before', 'i1 a0>1 i0 i2'), ('input_after', 'a0>1 i0 i1 i2'),
                        ('input_after_attack_after_own_input', 'i0 a0>1 i1 i2')):
        items.append(item(f'victim|{name}', script={5: steps}))
    # Sprint toggles around the hit (case 3) and the attacker afterwards.
    sprinting = {0: 'o0=move:1:0 o0=sprint:on ' + INPUTS}
    walking = {0: 'o0=move:1:0 ' + INPUTS}
    for name, start, steps in (
            ('stop_then_hit', sprinting, 'o0=move:0:0 i0 a0>1 i1 i2'),
            ('hit_then_stop', sprinting, 'a0>1 o0=move:0:0 i0 i1 i2'),
            ('start_then_hit', walking, 'o0=sprint:on i0 a0>1 i1 i2'),
            ('hit_then_start', walking, 'a0>1 o0=sprint:on i0 i1 i2'),
            ('sprint_hit_keep_moving', sprinting, HIT)):
        items.append(item(f'sprint|{name}', b=b_at(7.0), ticks=50, script=dict(start),
                          when=[{'after': 6, 'a': 0, 'b': 1, 'close': 2.6, 'steps': steps}]))
    # The swing (H14): with an Animate before the attack, and the swing alone.
    items.append(item('swing|with_attack', script={5: 'o0=swing a0>1 ' + INPUTS}))
    items.append(item('swing|alone', script={5: 'o0=swing ' + INPUTS}))
    return items


def extra_items() -> list[dict]:
    """Follow-ups of the first run: the reach metric (diagonal and finer
    distances), crits with Sharpness and Strength, the hurt window on an
    armoured victim, extra knockback on a resistant victim, a claimed item
    that is not held, and the hit packets (animation, sound, last hurt by)."""
    items = []
    for distance in (6.70, 6.72, 6.74, 6.76, 6.78, 6.80):
        items.append(item(f'reach|survival|{distance:.2f}', b=b_at(distance)))
    # Along 45 degrees the box edge is 0.3 * sqrt(2) nearer than on an axis.
    for distance in (6.60, 6.70, 6.75, 6.80, 6.85, 6.90):
        offset = distance / 2 ** 0.5
        items.append(item(f'reach|survival|diagonal{distance:.2f}',
                          b=[round(0.5 + offset, 4), Y, round(0.5 + offset, 4), 135, 0],
                          a=[0.5, Y, 0.5, -45, 0]))
    for distance in (12.05, 12.10, 12.15, 12.20, 12.25):
        items.append(item(f'reach|creative|{distance:.2f}', b=b_at(distance),
                          modes=['creative', 'survival', 'survival']))
    for name, held, effects in (('sharpness5', spec('diamond_sword', sharpness=5), []),
                                ('strength1', spec('diamond_sword'), ['effect "{A}" strength 100 0 true'])):
        items.append(item(f'crit|{name}|jump+8', held=(held, None, None), b=b_at(1.5), effects=effects,
                          script={3: 'o0=jump:1 ' + INPUTS, 11: HIT}))
    ladder = [{slot: spec(weapon) for slot, weapon in LADDER.items()}, None, None]

    def slot_hit(slot):
        return f'o0=slot:{slot} a0>1 ' + INPUTS

    items.append(item('h11|diamond|4-7', hotbar=ladder, armour=[None, suit('diamond'), None],
                      script={5: slot_hit(2), 7: slot_hit(5)}))
    items.append(item('h11|resistance2|4-7', hotbar=ladder, effects=['effect "{B}" resistance 100 1 true'],
                      script={5: slot_hit(2), 7: slot_hit(5)}))
    items.append(item('knockback|sprint|victim_netherite', b=b_at(7.0), ticks=50,
                      armour=[None, suit('netherite'), None],
                      script={0: 'o0=move:1:0 o0=sprint:on ' + INPUTS},
                      when=[{'after': 6, 'a': 0, 'b': 1, 'close': 2.6, 'steps': HIT}]))
    items.append(item('knockback|knockback2|victim_netherite', held=(spec('iron_sword', knockback=2), None, None),
                      armour=[None, suit('netherite'), None]))
    # How long the window runs after an excess hit: 4, 7 two ticks later,
    # then 8 k ticks after the 7; and 7 (equal) k ticks after it.
    for k in (11, 12, 14, 16, 20):
        items.append(item(f'h9|4-7-8@+{k}', hotbar=ladder, ticks=45,
                          script={5: slot_hit(2), 7: slot_hit(5), 7 + k: slot_hit(6)}))
    for k in (10, 11, 12):
        items.append(item(f'h9|4-7-7@+{k}', hotbar=ladder, ticks=45,
                          script={5: slot_hit(2), 7: slot_hit(5), 7 + k: slot_hit(5)}))
    # BDS applied C's attacks before A's and B's before A's in one tick:
    # the kill race with C as the killer (C kills B, then B hits C), and
    # C's fist with A's sword in one tick, each in both send orders.
    for name, steps in (('c_then_b', 'a2>1 i2 a1>2 i1 i0'), ('b_then_c', 'a1>2 i1 a2>1 i2 i0')):
        items.append(item(f'kill|{name}', held=(None, spec('diamond_sword'), spec('diamond_sword')), c=C_SIDE,
                          damage_before=[0, 19, 0], script={5: steps}))
    for name, steps in (('c_hand_then_a_sword', 'a2>1 i2 a0>1 i0 i1'), ('a_sword_then_c_hand', 'a0>1 i0 a2>1 i2 i1')):
        items.append(item(f'two|{name}', held=(spec('diamond_sword'), None, None), c=C_SIDE, script={5: steps}))
    # The held slot 0 is empty; the attack names slot 6 (a diamond sword).
    items.append(item('claim|slot6_item_not_held', hotbar=ladder,
                      script={4: 'o0=claim-slot:6 ' + INPUTS, 5: HIT, 6: 'o0=claim-slot:off ' + INPUTS}))
    for key in ('weapon|hand', 'crit|jump+8', 'sharpness|5', 'knockback|knockback1', 'knockback|sprint|hand'):
        found = [one for one in numbers_items() if one['key'] == key]
        items += [{**one, 'key': 'packets|' + one['key']} for one in found]
    return items


def pvp_items(sets: str, repeats: int, strict: bool) -> list[dict]:
    chosen = ['numbers', 'reach', 'hurt'] if sets == 'all' else sets.split(',')
    items = []
    for name in chosen:
        if name == 'numbers':
            items += numbers_items()
        elif name == 'reach':
            items += reach_items(strict)
        elif name == 'hurt':
            items += hurt_items()
        elif name == 'extra':
            items += extra_items()
        else:
            # A single item key, e.g. `weapon|hand`.
            found = [i for i in numbers_items() + reach_items(strict) + hurt_items() if i['key'] == name]
            if not found:
                raise SystemExit(f'unknown pvp set or item {name!r}')
            items += found
    out = []
    for repeat in range(repeats):
        for one in items:
            out.append({**one, 'key': f'{one["key"]}#{repeat}'})
    return out
