# Block behaviour scenarios

Each folder is one block-behaviour scenario measured on BDS 1.26.51 with the
bds-sweep blocks job (`tools/bds-sweep`, `sweep.py blocks --scenarios ...`):
a survival player driven by the scripted `bds-sweep-client` on loopback, the
world set up and the steps sequenced by the Endstone sweep plugin. A block
behaviour counts as done when replaying `steps.json` on unastar produces the
same packets as `packets.jsonl.gz`.

* `packets.jsonl.gz`: both directions of the client session from the
  scenario's begin marker to its end marker (the chat lines
  `bds-sweep:scenario begin|end NAME`, both included), in the client-diff
  capture format (`seq`, `t_ms` since the session start, `dir`, `id`, `name`,
  `hex` of the decompressed, decrypted packet). The scenario's last step is
  followed by `settle_ticks` idle ticks before the end marker.
* `steps.json`: the world (`world`), the commands run before the begin
  marker to reach the starting state (`prelude`: game rules, survival,
  peaceful, a reset stone arena, the player at the arena start), and every
  step after it with `tick` = server ticks since the begin marker:
  `command` (with its success), `use` (block, face, click position, held
  item, the player's position and look), `mine_start`/`mine_stop`, `fall`,
  `select_slot`, `remove_item_actors` (item actors the plugin removed through
  the server API, listed with their stacks), `mark` (the `bds-sweep:mark
  SCENARIO/STEP` chat line sent to the client, also in the capture) and
  `wait`. `error` is set when the scenario failed part-way; its capture
  stops being meaningful at that point.

Replay notes:

* Align by the chat lines: the `marker` and `mark` steps appear in the
  capture as server `TextPacket`s in the same order.
* Actor runtime/unique ids depend on everything spawned earlier in the same
  BDS run (scenarios ran back to back); map them by first appearance.
* Item pickup by the player is cancelled by the plugin; drops stay on the
  ground until the plugin removes them (`remove_item_actors`) or a reset
  kills them.
* Random outcomes: item drop spread and velocity, tool/crop drop counts,
  bone meal growth, random-tick driven steps (fire spread, crop growth,
  farmland drying, sugar cane/cactus checks that wait for a random tick)
  and explosion block selection differ between runs; compare those by
  outcome class, not byte for byte.
* The client's `PlayerAuthInput` is sent every 50 ms on wall-clock time, not
  in step with server ticks.

| Scenario | Covers | Recorded (UTC) | Steps | Packets | Size | Status |
|---|---|---|---|---|---|---|
| `attachables` | Ladders, vines, stone buttons and levers clicked onto the north face of a stone block and onto the floor top (ladder and vine refused); buttons and levers under an overhanging stone; then every support removed with `/setblock` (the attached block breaks). | 2026-10-30T16:49:08Z | 48 | 1029 | 34 KiB | ok |
| `bed` | A bed placed looking north (foot at the origin); used at noon, then at night (`time set 18000`); the level time is read before and after. | 2026-10-31T00:35:26Z | 15 | 538 | 15 KiB | ok |
| `beds` | A bed placed looking north (foot at the origin, head north of it); the placement refused with a stone where the head would go; the foot and then the head half of a placed bed broken by hand (the other half goes too, one bed drops). | 2026-10-30T16:49:35Z | 31 | 513 | 20 KiB | ok |
| `beehive` | Full beehives (honey level 5) used with shears and with a glass bottle. | 2026-10-31T00:34:16Z | 13 | 183 | 10 KiB | ok |
| `bell` | A standing bell (direction 0) clicked on each side face, then a lever on a stone block west of it. | 2026-10-31T00:34:30Z | 24 | 394 | 17 KiB | ok |
| `buttons` | A button clicked onto the east face of a stone block, a lamp on the stone's west side; one press each. | 2026-10-31T00:18:58Z | 22 | 367 | 15 KiB | ok |
| `cake` | Cake clicked at full hunger, then after a hunger effect (easy difficulty) twice. | 2026-10-31T00:34:48Z | 18 | 246 | 12 KiB | ok |
| `campfire` | A campfire placed looking north; raw beef put on it (watched until the cooked item pops out); extinguished with a wooden shovel. | 2026-10-31T00:34:11Z | 14 | 869 | 19 KiB | ok |
| `cauldron` | A cauldron filled with a water bucket, emptied by three glass bottles, filled with a lava bucket and emptied with a bucket. | 2026-10-31T00:33:35Z | 22 | 358 | 16 KiB | ok |
| `comparator_container` | A comparator clicked into place looking east with a chest behind it and two dust after it; the chest given 1 item, then 14 full stacks, then 27 (full), then removed: the comparator output follows the fill level. | 2026-10-31T00:18:46Z | 39 | 205 | 14 KiB | ok |
| `comparators` | Comparator at (8, y, 0) clicked into place looking east; rear dust along -x fed by a redstone block (signal at the comparator = 16 - dust count), side dust along +z the same way, two output dust along +x. | 2026-10-31T00:18:38Z | 153 | 1351 | 55 KiB | ok |
| `composter` | Ten wheat seed uses on an empty composter; then a composter set to level 6 used with seeds until it reaches 7 (watched until 8); then used at 8 with an empty hand. | 2026-10-31T00:33:27Z | 39 | 646 | 25 KiB | ok |
| `connections` | Oak fences, cobblestone walls and glass panes clicked onto the floor one after another along +x; a stone set north of the first; a fourth piece south of the second (a junction); the second removed: the connection states of every piece after each step. | 2026-10-30T16:50:19Z | 36 | 685 | 26 KiB | ok |
| `crop_care` | Wheat (growth 0) on farmland given bone meal three times; ripe wheat (growth 7) and growth 3 wheat broken by hand (drops); the player falling 2 blocks onto farmland (trampled to dirt); bare dry farmland (moisture 7, no water) and farmland under wheat at randomtickspeed `dry_speed` until the bare one turns to dirt. | 2026-10-30T16:52:40Z | 33 | 480 | 19 KiB | ok |
| `crops` | Wheat seeds used on farmland (planted) and on dirt (refused); wheat growth on watered farmland at each `growth_runs` randomtickspeed. | 2026-10-30T16:52:27Z | 56 | 2708 | 40 KiB | ok |
| `dispensers` | A dispenser placed by the player (looking west) holding arrows, a water bucket or stone; a stone block behind it with a lever on top. A dropper the same way with a chest in front. Containers are destroyed with `setblock … air destroy` at the end to show their contents. | 2026-10-31T00:32:32Z | 84 | 1571 | 54 KiB | ok |
| `doors` | Oak doors placed looking north/east/south/west, with the hinge decided by the click half and by stone stacked on either side; a door pair (second door east of the first) toggled; one door toggled lower, upper, lower, upper with dirt in hand; an iron door clicked with an empty hand and with dirt (no-op); the upper and the lower half broken by hand; the floor under a door removed. | 2026-10-30T16:47:55Z | 126 | 2550 | 69 KiB | ok |
| `end_portal` | Twelve end portal frames around a 3x3 interior (/setblock, eleven with eyes); the player inserts the twelfth eye into the south edge's middle frame. Once with every frame's cardinal_direction pointing at the interior, once pointing away. | 2026-10-31T00:34:55Z | 40 | 196 | 16 KiB | ok |
| `falling_blocks` | Sand clicked onto the north face of a stone block two blocks above the floor (falls onto the floor); sand set in the air with `/setblock`; gravel set above a torch (breaks on it, drops as an item); white concrete powder set above a water source in the floor (falls into it) and set beside a second water source in the floor. | 2026-10-31T00:35:42Z | 17 | 476 | 15 KiB | ok |
| `fence_gates` | An oak fence gate placed looking north, opened and closed with an empty hand from the south and then from the north (it opens away from the player); a gate placed between two cobblestone walls; a cobblestone wall set beside a placed gate. | 2026-10-30T16:48:29Z | 24 | 349 | 17 KiB | ok |
| `fire` | `/setblock` fires with doFireTick on at randomtickspeed 1: one on netherrack, eight on the bare stone floor and one beside a 4-high oak planks column (every cell around the column watched); the planks column again on normal difficulty; then the player punches (starts breaking, then stops) the block under a fire on netherrack and on stone. | 2026-10-31T00:23:29Z | 49 | 2710 | 39 KiB | ok |
| `flower_pot` | A flower pot given a poppy, then used with an empty hand. | 2026-10-31T00:34:40Z | 12 | 154 | 9 KiB | ok |
| `iron_door_lever` | Iron door (placed looking north) with a stone block east of its lower half and a lever on that stone's east face. | 2026-10-31T00:20:35Z | 19 | 368 | 15 KiB | ok |
| `jukebox` | A jukebox given music disc 13, then ejected with an empty hand. | 2026-10-31T00:34:36Z | 12 | 203 | 11 KiB | ok |
| `nether_portal` | Obsidian 4x5 frame in the x plane at z=0 (inner x 1..2, three high); flint and steel on the top of the inner bottom obsidian; then the west side's middle obsidian set to air. | 2026-10-31T00:35:01Z | 23 | 168 | 11 KiB | ok |
| `note_block_use` | Note block on dirt: three empty-hand clicks, then a lever on the east face of a stone block east of it, on and off. | 2026-10-31T00:33:01Z | 23 | 260 | 14 KiB | ok |
| `note_blocks` | Note block on dirt / on the stone floor, powered through a stone block east of it by a lever on that stone's east face. | 2026-10-31T00:20:46Z | 28 | 354 | 17 KiB | ok |
| `observer` | Observers set facing east: one with the changed block east of it, one with the changed block west of it; then two changes one tick apart. | 2026-10-31T00:31:55Z | 14 | 107 | 8 KiB | ok |
| `pistons` | A piston and a sticky piston clicked into place from three blocks east looking west (they face east), powered and unpowered by a redstone block set on top: nothing in front, one stone (pushed; the sticky piston pulls it back), twelve stones (pushed), thirteen (refused), obsidian (refused) and a torch in front (broken, drops). | 2026-10-31T00:21:22Z | 79 | 994 | 33 KiB | ok |
| `plant_survival` | Sugar cane clicked onto sand beside water (twice: two high) and onto stone (refused); the sand under one cane removed, the water beside another removed. Cactus clicked onto sand; a stone set beside it; the sand under a second cactus removed. | 2026-10-30T16:53:00Z | 30 | 711 | 24 KiB | ok |
| `pressure_plates` | Stone and oak plates on the floor, each with a lamp east of it; an item (`/loot spawn` of a mined dirt block, left to settle elsewhere) teleported onto each plate and removed with `/kill`, then the player teleported onto and off each plate (two dwell times). | 2026-10-31T00:19:40Z | 24 | 961 | 21 KiB | ok |
| `rails` | Rails clicked onto the floor: corner (0,0), then east (1,0), then south (0,1); a minecart placed on the south rail. Ten golden rails along +x at z=6 (/setblock, east-west) next to a stone block with a lever. | 2026-10-31T00:33:14Z | 36 | 431 | 19 KiB | ok |
| `redstone_dust` | Lever on a stone block, a dust line along +x starting next to it and a lamp after the line: 15 dust (the lamp gets signal 1), then 16. | 2026-10-31T00:16:10Z | 61 | 564 | 24 KiB | ok |
| `redstone_torch` | A redstone torch on a stone block with a lever on top inverts the lever (lamp beside the torch); a torch feeding its own block through dust burns out (clock). | 2026-10-31T00:17:52Z | 33 | 917 | 24 KiB | ok |
| `repeaters` | Repeaters set to delays 1-4 (clicked) between a lever and a lamp, the lever switched on and off; a repeater locked from the side by a second powered repeater, toggled locked and unlocked. | 2026-10-31T00:17:16Z | 109 | 2032 | 65 KiB | ok |
| `respawn_anchor` | Overworld respawn anchor: charged with glowstone, then used with an empty hand; the 7x3x7 cells around it are watched. | 2026-10-31T00:35:07Z | 13 | 211 | 14 KiB | ok |
| `signs` | Oak signs: standing on the floor top looking north and looking east, on the north face of a stone block (wall sign), hanging under an overhanging stone and on a stone block's north face; then each support removed with `/setblock` (the sign breaks and drops). | 2026-10-30T16:49:23Z | 26 | 564 | 21 KiB | ok |
| `slabs` | Oak slabs: a bottom slab on the floor top and a second one clicked onto its top (double slab); a top slab on the upper half of a stone block's north face and a second one clicked into it from the floor below (double slab); an oak slab clicked onto a smooth stone slab (no merge, placed above). | 2026-10-30T16:50:02Z | 17 | 271 | 14 KiB | ok |
| `stairs` | Oak stairs placed on the floor looking north, east, south and west; upside down on the upper half of a stone block's north face; corner shapes: a second stair placed looking east or west directly in front of or behind a north-facing stair. | 2026-10-30T16:49:54Z | 43 | 775 | 28 KiB | ok |
| `tnt` | TNT on the floor at the origin: lit with flint and steel; lit by a lever on a stone block next to it; primed by the explosion of a summoned primed TNT two blocks away. The player walks off after the click; the 7x3x7 cells around the TNT are watched (floor-1 .. floor+1). | 2026-10-31T00:32:53Z | 32 | 1039 | 29 KiB | ok |
| `torches` | Torches clicked onto the floor top, each side face of a stone block, the underside of an overhanging stone (refused), an oak fence top and a glass top; then the stone with the four side torches removed. | 2026-10-30T16:48:40Z | 25 | 494 | 20 KiB | ok |
| `trapdoors` | Oak trapdoors clicked onto the north face of a stone block (lower and upper click half), its south, west and east faces, the floor top and the underside of an overhanging stone; one toggled open and closed three times with an empty hand; an iron trapdoor clicked with an empty hand (no-op), then opened and closed by a lever on top of its support. | 2026-10-30T16:48:19Z | 58 | 907 | 34 KiB | ok |
| `weighted_plates` | Light and heavy weighted pressure plates on the floor, each with a lamp east of it, the same steps as `pressure_plates`: an item teleported onto each plate and killed, then the player teleported onto and off each plate (two dwell times). | 2026-10-31T00:20:22Z | 24 | 966 | 21 KiB | ok |
