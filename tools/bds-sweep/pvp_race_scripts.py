"""Scripts of the PvP race matrix for `order_probe` (see `pvp_race.py`).

Every trial starts from the same arena: a flat survival world, A at
(0.5,-60,0.5) facing east, B two blocks east facing west, C two blocks south
of B facing north, everyone at full health with no effects. A trial is
marked `case=CASE variant=VARIANT trial=N`; its race happens on the marked
tick, and the next trial starts 40 ticks later (well past every hurt window
and knockback).
"""

from __future__ import annotations

A_POS = (0.5, -60, 0.5)
B_POS = (2.5, -60, 0.5)
C_POS = (2.5, -60, 2.5)
# Ticks from a trial's reset to its race; long enough for every teleport to
# be acknowledged and every motion to settle.
SETTLE = 25
TRIAL = 45

SETUP = [
    'cmd /gamerule sendcommandfeedback false',
    'cmd /gamerule naturalregeneration false',
    'cmd /gamerule domobspawning false',
    'cmd /gamerule dodaylightcycle false',
    'cmd /gamerule doweathercycle false',
    'cmd /time set 6000',
    'cmd /difficulty normal',
]


class Script:
    def __init__(self) -> None:
        self.lines: list[tuple[int, str]] = []
        self.tick = 40
        for line in SETUP:
            self.at(self.tick, 'A', line)
            self.tick += 1
        self.tick += 10

    def at(self, tick: int, who: str, action: str) -> None:
        self.lines.append((tick, f'{who} {action}'))

    def reset(self, start: int, a=A_POS, b=B_POS, c=C_POS, a_yaw=-90, b_yaw=90, c_yaw=180) -> None:
        """Teleports, heals and clears everyone at `start`."""
        self.at(start, 'A', 'cmd /effect @a clear')
        self.at(start + 1, 'A', 'cmd /effect @a instant_health 1 10 true')
        self.at(start + 2, 'A', f'cmd /tp RaceA {a[0]} {a[1]} {a[2]} {a_yaw} 0')
        self.at(start + 2, 'A', f'cmd /tp RaceB {b[0]} {b[1]} {b[2]} {b_yaw} 0')
        self.at(start + 2, 'A', f'cmd /tp RaceC {c[0]} {c[1]} {c[2]} {c_yaw} 0')
        self.at(start + 3, 'A', 'cmd /effect @a instant_health 1 10 true')

    def trial(self, case: str, variant: str, n: int, build, **reset) -> None:
        """One trial: reset, settle, then `build(script, race_tick)`."""
        start = self.tick
        self.reset(start, **reset)
        race = start + SETTLE
        self.lines.append((race, f'* mark case={case} variant={variant} trial={n}'))
        build(self, race)
        self.tick = start + TRIAL + getattr(build, 'extra', 0)

    def text(self) -> str:
        self.lines.append((self.tick + 20, '* exit'))
        # Stable sort keeps the order lines were added within one tick.
        ordered = sorted(enumerate(self.lines), key=lambda item: (item[1][0], item[0]))
        return ''.join(f'{tick} {line}\n' for _, (tick, line) in ordered)


def variants(spec: dict[str, object]):
    return spec.items()


# ── Generic order (no combat): a burst and cross-player order ──────────────

def order_cases():
    """Where A is when a command runs: the command copies A's position onto
    C (C lands two blocks south of A). A walks east with `move`."""
    probe = 'cmd /execute as RaceA at @s run tp RaceC ~ ~ ~2'
    walk = 'input move=0,1 flags=up'

    def walk_then_read(s, t):
        for k in range(4):
            s.at(t + k, 'A', walk)
            s.at(t + k, 'A', probe)

    def read_then_walk(s, t):
        for k in range(4):
            s.at(t + k, 'A', probe)
            s.at(t + k, 'A', walk)

    def walk_batch_read(s, t):
        for k in range(4):
            s.at(t + k, 'A', f'{walk} ; {probe}')

    def cross_walk_then_read(s, t):
        for k in range(4):
            s.at(t + k, 'A', walk)
            s.at(t + k, 'B', probe)

    def cross_read_then_walk(s, t):
        for k in range(4):
            s.at(t + k, 'B', probe)
            s.at(t + k, 'A', walk)

    def burst(s, t):
        # Three walking inputs at once, a read between the first two.
        s.at(t - 1, 'A', 'noinput')
        s.at(t - 2, 'A', 'noinput')
        s.at(t, 'A', walk)
        s.at(t, 'A', probe)
        s.at(t, 'A', walk)
        s.at(t, 'A', walk)
        s.at(t + 1, 'A', probe)

    return {
        'walk-then-read': walk_then_read,
        'read-then-walk': read_then_walk,
        'walk-batch-read': walk_batch_read,
        'cross-walk-then-read': cross_walk_then_read,
        'cross-read-then-walk': cross_read_then_walk,
        'burst': burst,
    }


# ── H1/H2: do attacks wait for their player's next input? ──────────────────

RESIST = 'cmd /effect RaceB resistance 2 4 true'


def queue_cases():
    def attack_no_input(s, t):
        s.at(t, 'A', 'attack B')
        for k in range(5):
            s.at(t + k + 1, 'A', 'noinput')

    def attack_chat_no_input(s, t):
        s.at(t, 'A', 'attack B')
        s.at(t, 'A', 'say order-probe')
        for k in range(5):
            s.at(t + k + 1, 'A', 'noinput')

    def swing_attack_no_input(s, t):
        s.at(t, 'A', 'swing')
        s.at(t, 'A', 'attack B')
        for k in range(5):
            s.at(t + k + 1, 'A', 'noinput')

    def attack_then_resist(s, t):
        # Applied on arrival, the hit lands before the resistance.
        s.at(t, 'A', 'attack B')
        s.at(t, 'A', RESIST)

    def resist_then_attack(s, t):
        s.at(t, 'A', RESIST)
        s.at(t, 'A', 'attack B')

    def attack_then_victim_resist(s, t):
        # The victim's own command, after the attack, A silent for 3 ticks.
        s.at(t, 'A', 'attack B')
        s.at(t, 'B', 'cmd /effect @s resistance 2 4 true')
        for k in range(3):
            s.at(t + k + 1, 'A', 'noinput')

    def attack_then_turn(s, t):
        # The input after the attack turns A away from B.
        s.at(t, 'A', 'attack B')
        s.at(t + 1, 'A', 'input rot=0,90')

    def turn_then_attack(s, t):
        s.at(t, 'A', 'input rot=0,90')
        s.at(t, 'A', 'attack B')

    def attack_input_batch(s, t):
        s.at(t, 'A', 'attack B ; input')
        for k in range(3):
            s.at(t + k + 1, 'A', 'noinput')

    return {
        'attack-no-input': attack_no_input,
        'attack-chat-no-input': attack_chat_no_input,
        'swing-attack-no-input': swing_attack_no_input,
        'attack-then-resist': attack_then_resist,
        'resist-then-attack': resist_then_attack,
        'attack-then-victim-resist': attack_then_victim_resist,
        'attack-then-turn': attack_then_turn,
        'turn-then-attack': turn_then_attack,
        'attack-input-batch': attack_input_batch,
    }


# ── H5: a lagging player's inputs arriving together ────────────────────────

def lag_cases():
    def silent(s, t, ticks):
        for k in range(ticks):
            s.at(t - ticks + k, 'A', 'noinput')

    def burst_two_hits(s, t):
        # Twelve inputs at once, a hit after the first and after the eleventh.
        silent(s, t, 12)
        lines = ['input', 'attack B'] + ['input'] * 10 + ['attack B', 'input']
        for line in lines:
            s.at(t, 'A', line)

    def burst_two_hits_batch(s, t):
        silent(s, t, 12)
        s.at(t, 'A', ' ; '.join(['input', 'attack B'] + ['input'] * 10 + ['attack B', 'input']))

    def hit_then_burst(s, t):
        # The hit before the burst of inputs.
        silent(s, t, 6)
        s.at(t, 'A', 'attack B')
        for _ in range(6):
            s.at(t, 'A', 'input')

    return {'burst-two-hits': burst_two_hits, 'burst-two-hits-batch': burst_two_hits_batch,
            'hit-then-burst': hit_then_burst}


# ── H4: both at one health, hitting each other in the same tick ────────────

def both_low_cases():
    def weaken(s, t):
        s.at(t - 20, 'A', 'cmd /damage RaceA 19')
        s.at(t - 20, 'A', 'cmd /damage RaceB 19')

    def a_first(s, t):
        weaken(s, t)
        s.at(t, 'A', 'attack B')
        s.at(t, 'B', 'attack A')

    def b_first(s, t):
        weaken(s, t)
        s.at(t, 'B', 'attack A')
        s.at(t, 'A', 'attack B')

    def a_first_b_inputs_first(s, t):
        # Attacks A then B; in the next tick B's input arrives before A's.
        weaken(s, t)
        s.at(t, 'A', 'attack B')
        s.at(t, 'B', 'attack A')
        s.at(t + 1, 'B', 'input')
        s.at(t + 1, 'A', 'input')

    def b_first_a_inputs_first(s, t):
        weaken(s, t)
        s.at(t, 'B', 'attack A')
        s.at(t, 'A', 'attack B')
        s.at(t + 1, 'A', 'input')
        s.at(t + 1, 'B', 'input')

    def batched_b_first(s, t):
        # Each attack batched with its player's input, B's batch first.
        weaken(s, t)
        s.at(t, 'B', 'attack A ; input')
        s.at(t, 'A', 'attack B ; input')

    def batched_a_first(s, t):
        weaken(s, t)
        s.at(t, 'A', 'attack B ; input')
        s.at(t, 'B', 'attack A ; input')

    return {'a-first': a_first, 'b-first': b_first,
            'a-first-b-inputs-first': a_first_b_inputs_first,
            'b-first-a-inputs-first': b_first_a_inputs_first,
            'batched-a-first': batched_a_first, 'batched-b-first': batched_b_first}


# ── M1 or M2: applied on arrival, or with the next input? ───────────────────

def model_cases():
    def silent(s, t, ticks=6):
        for k in range(ticks):
            s.at(t + k + 1, 'A', 'noinput')

    def snowballs(s, t):
        s.at(t - 20, 'A', 'cmd /replaceitem entity RaceA slot.hotbar 0 snowball 16')

    def use_no_input(s, t):
        snowballs(s, t)
        s.at(t, 'A', 'useitem')
        s.at(t, 'A', 'say order-probe')
        silent(s, t)

    def interact_no_input(s, t):
        s.at(t, 'A', 'interact')
        s.at(t, 'A', 'say order-probe')
        silent(s, t)
        # Close it again so the next trial can open it.
        s.at(t + 12, 'A', 'close')

    def attack_chat_input(s, t):
        # Attack, chat, then the input, all in one tick.
        s.at(t, 'A', 'attack B')
        s.at(t, 'A', 'say order-probe')
        s.at(t, 'A', 'input')
        silent(s, t)

    def attack_then_face_away(s, t):
        s.at(t, 'A', 'attack B')
        s.at(t, 'A', 'input rot=0,90')

    def face_away_then_attack(s, t):
        s.at(t, 'A', 'input rot=0,90')
        s.at(t, 'A', 'attack B')

    def away_attack_then_face(s, t):
        # A faces away for a while; the input after the attack faces B.
        for k in range(1, 4):
            s.at(t - k, 'A', 'input rot=0,90')
        s.at(t, 'A', 'attack B')
        s.at(t, 'A', 'input rot=0,-90')

    def away_face_then_attack(s, t):
        for k in range(1, 4):
            s.at(t - k, 'A', 'input rot=0,90')
        s.at(t, 'A', 'input rot=0,-90')
        s.at(t, 'A', 'attack B')

    return {'use-no-input': use_no_input, 'interact-no-input': interact_no_input,
            'attack-chat-input': attack_chat_input,
            'attack-then-face-away': attack_then_face_away,
            'face-away-then-attack': face_away_then_attack,
            'away-attack-then-face': away_attack_then_face,
            'away-face-then-attack': away_face_then_attack}


def follow_up_cases():
    def sword_c(s, t):
        s.at(t - 20, 'A', 'cmd /replaceitem entity RaceC slot.weapon.mainhand 0 wooden_sword')

    def fist_then_sword(s, t):
        sword_c(s, t)
        s.at(t, 'A', 'attack B')
        s.at(t, 'C', 'attack B')

    def sword_then_fist(s, t):
        sword_c(s, t)
        s.at(t, 'C', 'attack B')
        s.at(t, 'A', 'attack B')

    def fist_then_sword_later(s, t):
        sword_c(s, t)
        s.at(t, 'A', 'attack B')
        s.at(t + 5, 'C', 'attack B')

    def attacker_teleport_then_hit(s, t):
        # A's own teleport, then A's attack in the same tick.
        s.at(t, 'A', 'cmd /tp RaceA 0.5 -60 0.6 -90 0')
        s.at(t, 'A', 'attack B')

    return {'fist-then-sword': fist_then_sword, 'sword-then-fist': sword_then_fist,
            'fist-then-sword-5-ticks': fist_then_sword_later,
            'attacker-teleport-then-hit': attacker_teleport_then_hit}


# ── Order across players: every pair at one health hits each other ───────

def pair_cases():
    names = {'A': 'RaceA', 'B': 'RaceB', 'C': 'RaceC'}

    def pair(first, second, batched):
        def build(s, t):
            s.at(t - 20, 'A', f'cmd /damage {names[first]} 19')
            s.at(t - 20, 'A', f'cmd /damage {names[second]} 19')
            if batched:
                s.at(t, first, f'attack {second} ; input')
                s.at(t, second, f'attack {first} ; input')
            else:
                s.at(t, first, f'attack {second}')
                s.at(t, second, f'attack {first}')
        return build

    cases = {}
    for first, second in [('A', 'B'), ('B', 'C'), ('A', 'C')]:
        for a, b in [(first, second), (second, first)]:
            cases[f'{a}{b}'] = pair(a, b, False)
            cases[f'{a}{b}-batched'] = pair(a, b, True)
    return cases


# ── 1. Mutual hit ──────────────────────────────────────────────────────────

def mutual_cases():
    def a_first(s, t):
        s.at(t, 'A', 'attack B')
        s.at(t, 'B', 'attack A')

    def b_first(s, t):
        s.at(t, 'B', 'attack A')
        s.at(t, 'A', 'attack B')

    def batched_a_first(s, t):
        s.at(t, 'A', 'input ; attack B')
        s.at(t, 'B', 'input ; attack A')

    def batched_b_first(s, t):
        s.at(t, 'B', 'input ; attack A')
        s.at(t, 'A', 'input ; attack B')

    return {'a-first': a_first, 'b-first': b_first,
            'batched-a-first': batched_a_first, 'batched-b-first': batched_b_first}


# ── 2. Victim input vs hit ─────────────────────────────────────────────────

def victim_input_cases():
    def input_first(s, t):
        s.at(t, 'B', 'input')
        s.at(t, 'A', 'attack B')

    def hit_first(s, t):
        s.at(t, 'A', 'attack B')
        s.at(t, 'B', 'input')

    def hit_no_input(s, t):
        # B sends nothing for three ticks after the hit.
        s.at(t, 'A', 'attack B')
        for k in range(0, 3):
            s.at(t + k, 'B', 'noinput')

    def attacker_batch(s, t):
        # A's input and attack in one batch, B's input after.
        s.at(t, 'A', 'input ; attack B')
        s.at(t, 'B', 'input')

    return {'input-first': input_first, 'hit-first': hit_first,
            'hit-no-input': hit_no_input, 'attacker-batch': attacker_batch}


# ── 3. Attacker sprint toggle ──────────────────────────────────────────────

SPRINT = 'flags=startsprinting|sprinting|sprintdown|up move=0,1'
SPRINTING = 'flags=sprinting|sprintdown|up move=0,1'


def sprint_cases():
    def sprint_up(s, t):
        s.at(t - 3, 'A', f'input {SPRINT}')
        s.at(t - 2, 'A', f'input {SPRINTING}')
        s.at(t - 1, 'A', f'input {SPRINTING}')

    def stop_then_hit(s, t):
        sprint_up(s, t)
        s.at(t, 'A', 'input flags=stopsprinting')
        s.at(t, 'A', 'attack B')

    def hit_then_stop(s, t):
        sprint_up(s, t)
        s.at(t, 'A', 'attack B')
        s.at(t, 'A', 'input flags=stopsprinting')

    def sprint_hit(s, t):
        sprint_up(s, t)
        s.at(t, 'A', f'input {SPRINTING}')
        s.at(t, 'A', 'attack B')

    def start_then_hit(s, t):
        s.at(t, 'A', f'input {SPRINT}')
        s.at(t, 'A', 'attack B')

    def hit_then_start(s, t):
        s.at(t, 'A', 'attack B')
        s.at(t, 'A', f'input {SPRINT}')

    def two_sprint_hits(s, t):
        # Sprint reset: a second hit (after the hurt window) while still
        # holding sprint, without a new start.
        sprint_up(s, t)
        s.at(t, 'A', f'input {SPRINTING}')
        s.at(t, 'A', 'attack B')
        for k in range(1, 13):
            s.at(t + k, 'A', f'input {SPRINTING}')
        s.at(t + 12, 'A', 'attack B')

    def stop_batch_hit(s, t):
        sprint_up(s, t)
        s.at(t, 'A', 'input flags=stopsprinting ; attack B')

    def hit_batch_stop(s, t):
        sprint_up(s, t)
        s.at(t, 'A', 'attack B ; input flags=stopsprinting')

    def start_batch_hit(s, t):
        s.at(t, 'A', f'input {SPRINT} ; attack B')

    def hit_batch_start(s, t):
        s.at(t, 'A', f'attack B ; input {SPRINT}')

    def sneak_then_hit(s, t):
        s.at(t, 'A', 'input flags=startsneaking|sneaking')
        s.at(t, 'A', 'attack B')

    def hit_then_sneak(s, t):
        s.at(t, 'A', 'attack B')
        s.at(t, 'A', 'input flags=startsneaking|sneaking')

    def sneaking_hit(s, t):
        s.at(t - 2, 'A', 'input flags=startsneaking|sneaking')
        s.at(t - 1, 'A', 'input flags=sneaking')
        s.at(t, 'A', 'input flags=sneaking')
        s.at(t, 'A', 'attack B')

    def victim_sneak_then_hit(s, t):
        s.at(t, 'B', 'input flags=startsneaking|sneaking')
        s.at(t, 'A', 'attack B')

    def hit_then_victim_sneak(s, t):
        s.at(t, 'A', 'attack B')
        s.at(t, 'B', 'input flags=startsneaking|sneaking')

    # A starts 1.5 blocks further west so its sprint stays in reach.
    return {'stop-batch-hit': stop_batch_hit, 'hit-batch-stop': hit_batch_stop,
            'start-batch-hit': start_batch_hit, 'hit-batch-start': hit_batch_start,
            'sneak-then-hit': sneak_then_hit, 'hit-then-sneak': hit_then_sneak,
            'sneaking-hit': sneaking_hit, 'victim-sneak-then-hit': victim_sneak_then_hit,
            'hit-then-victim-sneak': hit_then_victim_sneak,'stop-then-hit': stop_then_hit, 'hit-then-stop': hit_then_stop,
            'sprint-hit': sprint_hit, 'start-then-hit': start_then_hit,
            'hit-then-start': hit_then_start, 'two-sprint-hits': two_sprint_hits}


# ── 4. Reach by order ──────────────────────────────────────────────────────

def reach_sweep_cases(low: float = 2.5, high: float = 4.5):
    """B at increasing distance east of A, in tenths from `low` to `high`;
    one attack each."""
    cases = {}
    for tenths in range(round(low * 10), round(high * 10) + 1):
        def build(s, t):
            s.at(t, 'A', 'attack B')
        cases[f'd={tenths / 10:.1f}'] = (build, tenths / 10)
    return cases


def reach_order_cases(edge: float):
    """`edge`: the largest distance at which a standing hit lands."""
    out = edge + 0.05

    def move_then_hit(s, t):
        s.at(t, 'A', 'input move=0,1 flags=up')
        s.at(t, 'A', 'attack B')

    def hit_then_move(s, t):
        s.at(t, 'A', 'attack B')
        s.at(t, 'A', 'input move=0,1 flags=up')

    def victim_leaves_then_hit(s, t):
        s.at(t, 'B', 'input move=0,-1 flags=down')
        s.at(t, 'A', 'attack B')

    def hit_then_victim_leaves(s, t):
        s.at(t, 'A', 'attack B')
        s.at(t, 'B', 'input move=0,-1 flags=down')

    def spoofed_from(s, t):
        # Out of reach, but the transaction claims a position next to B.
        s.at(t, 'A', f'attack B from={B_POS[0] - 1.0},{-60 + 1.62},{B_POS[2]}')

    # The same, with the input also claiming the step (0.1 block, a little more than
    # one walking tick), since BDS takes a client position close to its own.
    def claim_then_hit(s, t):
        s.at(t, 'A', 'input move=0,1 flags=up dpos=0.1,0,0')
        s.at(t, 'A', 'attack B')

    def hit_then_claim(s, t):
        s.at(t, 'A', 'attack B')
        s.at(t, 'A', 'input move=0,1 flags=up dpos=0.1,0,0')

    def victim_claims_leave_then_hit(s, t):
        s.at(t, 'B', 'input move=0,-1 flags=down dpos=0.1,0,0')
        s.at(t, 'A', 'attack B')

    def hit_then_victim_claims_leave(s, t):
        s.at(t, 'A', 'attack B')
        s.at(t, 'B', 'input move=0,-1 flags=down dpos=0.1,0,0')

    return {
        'claim-then-hit': (claim_then_hit, out),
        'hit-then-claim': (hit_then_claim, out),
        'victim-claims-leave-then-hit': (victim_claims_leave_then_hit, edge - 0.02),
        'hit-then-victim-claims-leave': (hit_then_victim_claims_leave, edge - 0.02),
        'move-then-hit': (move_then_hit, out),
        'hit-then-move': (hit_then_move, out),
        'victim-leaves-then-hit': (victim_leaves_then_hit, edge - 0.02),
        'hit-then-victim-leaves': (hit_then_victim_leaves, edge - 0.02),
        'spoofed-from': (spoofed_from, out + 1.0),
    }


# ── 5. Autoclicker burst ───────────────────────────────────────────────────

def burst_cases():
    def separate(s, t):
        for _ in range(5):
            s.at(t, 'A', 'attack B')

    def one_batch(s, t):
        s.at(t, 'A', ' ; '.join(['attack B'] * 5))

    def across_ticks(s, t):
        for k in range(5):
            s.at(t + k, 'A', 'attack B')

    return {'separate': separate, 'one-batch': one_batch, 'across-ticks': across_ticks}


# ── 6. Two attackers ───────────────────────────────────────────────────────

def two_attacker_cases():
    def a_then_c(s, t):
        s.at(t, 'A', 'attack B')
        s.at(t, 'C', 'attack B')

    def c_then_a(s, t):
        s.at(t, 'C', 'attack B')
        s.at(t, 'A', 'attack B')

    def sword_c(s, t):
        s.at(t - 20, 'A', 'cmd /replaceitem entity RaceC slot.weapon.mainhand 0 wooden_sword')

    def fist_then_sword(s, t):
        sword_c(s, t)
        s.at(t, 'A', 'attack B')
        s.at(t, 'C', 'attack B')

    def sword_then_fist(s, t):
        sword_c(s, t)
        s.at(t, 'C', 'attack B')
        s.at(t, 'A', 'attack B')

    def fist_then_sword_later(s, t):
        sword_c(s, t)
        s.at(t, 'A', 'attack B')
        s.at(t + 5, 'C', 'attack B')

    return {'a-then-c': a_then_c, 'c-then-a': c_then_a,
            'fist-then-sword': fist_then_sword, 'sword-then-fist': sword_then_fist,
            'fist-then-sword-5-ticks': fist_then_sword_later}


# ── 7. Hurt window edge ────────────────────────────────────────────────────

def window_cases():
    cases = {}
    for k in range(8, 13):
        def build(s, t, k=k):
            s.at(t, 'A', 'attack B')
            s.at(t + k, 'A', 'attack B')
        cases[f'second-at+{k}'] = build
    return cases


# ── 8. Kill race ───────────────────────────────────────────────────────────

def kill_cases():
    def weaken(s, t):
        s.at(t - 20, 'A', 'cmd /damage RaceB 19')

    def kill_then_hit(s, t):
        weaken(s, t)
        s.at(t, 'A', 'attack B')
        s.at(t, 'B', 'attack A')

    def hit_then_kill(s, t):
        weaken(s, t)
        s.at(t, 'B', 'attack A')
        s.at(t, 'A', 'attack B')

    def kill_then_hit_next_tick(s, t):
        weaken(s, t)
        s.at(t, 'A', 'attack B')
        s.at(t + 1, 'B', 'attack A')

    return {'kill-then-hit': kill_then_hit, 'hit-then-kill': hit_then_kill,
            'kill-then-hit-next-tick': kill_then_hit_next_tick}


# ── 9. Critical hits ───────────────────────────────────────────────────────

def crit_cases():
    """A drops from 1.5 blocks up; it attacks at a tick after the drop, with
    that tick's input before or after the attack."""
    cases = {}
    for k in range(1, 12):
        def before(s, t, k=k):
            s.at(t - k, 'A', 'cmd /tp RaceA 0.5 -58.5 0.5 -90 0')
            s.at(t, 'A', 'input')
            s.at(t, 'A', 'attack B')

        def after(s, t, k=k):
            s.at(t - k, 'A', 'cmd /tp RaceA 0.5 -58.5 0.5 -90 0')
            s.at(t, 'A', 'attack B')
            s.at(t, 'A', 'input')
        cases[f'input-then-hit+{k}'] = before
        cases[f'hit-then-input+{k}'] = after
    return cases


# ── 10. Pending teleport or correction ─────────────────────────────────────

def pending_cases():
    def teleport_then_hit(s, t):
        s.at(t, 'A', 'cmd /tp RaceB 2.5 -60 1.0 90 0')
        s.at(t, 'A', 'attack B')

    def teleport_unacknowledged(s, t):
        s.at(t, 'A', 'cmd /tp RaceB 2.5 -60 1.0 90 0')
        for k in range(0, 4):
            s.at(t + k, 'B', 'noinput')
        s.at(t + 2, 'A', 'attack B')

    def correction_then_hit(s, t):
        # B claims a position half a block off; BDS corrects it.
        s.at(t, 'B', 'input dpos=0,0,0.5')
        s.at(t, 'A', 'attack B')

    def hit_then_correction(s, t):
        s.at(t, 'A', 'attack B')
        s.at(t, 'B', 'input dpos=0,0,0.5')

    return {'teleport-then-hit': teleport_then_hit,
            'teleport-unacknowledged': teleport_unacknowledged,
            'correction-then-hit': correction_then_hit,
            'hit-then-correction': hit_then_correction}


def script(case: str, trials: int, edge: float = 3.0) -> str:
    s = Script()

    def run(cases, **reset):
        for n in range(trials):
            for variant, build in cases.items():
                s.trial(case, variant, n, build, **reset)

    if case == 'order':
        run(order_cases())
    elif case == 'queue':
        run(queue_cases())
    elif case == 'lag':
        run(lag_cases())
    elif case == 'both-low':
        run(both_low_cases())
    elif case == 'pairs':
        run(pair_cases())
    elif case == 'model':
        run(model_cases())
    elif case == 'follow-up':
        run(follow_up_cases())
    elif case == 'mutual':
        run(mutual_cases())
    elif case == 'victim-input':
        run(victim_input_cases())
    elif case == 'sprint':
        run(sprint_cases(), a=(-1.0, -60, 0.5), b=(2.0, -60, 0.5))
    elif case.startswith('reach-sweep'):
        bounds = case.split('=')[1].split(':') if '=' in case else ('2.5', '4.5')
        for n in range(trials):
            for variant, (build, d) in reach_sweep_cases(float(bounds[0]), float(bounds[1])).items():
                s.trial('reach-sweep', variant, n, build, b=(A_POS[0] + d, -60, 0.5))
    elif case.startswith('reach-order'):
        edge = float(case.split('=')[1]) if '=' in case else edge
        for n in range(trials):
            for variant, (build, d) in reach_order_cases(edge).items():
                s.trial('reach-order', variant, n, build, b=(A_POS[0] + d, -60, 0.5))
    elif case == 'burst':
        run(burst_cases())
    elif case == 'two-attackers':
        run(two_attacker_cases())
    elif case == 'window':
        run(window_cases())
    elif case == 'kill':
        run(kill_cases())
    elif case == 'crit':
        run(crit_cases())
    elif case == 'pending':
        run(pending_cases())
    else:
        raise SystemExit(f'unknown case {case}')
    return s.text()
