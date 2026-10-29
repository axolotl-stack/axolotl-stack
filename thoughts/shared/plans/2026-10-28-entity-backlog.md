# Entity backlog (2026-10-28)

Everything the vanilla behaviour pack asks for that the server does not do yet,
grouped by the engine feature that unlocks it. The mob runtime refuses a
definition that uses an unimplemented component, so each group lists the mobs it
unblocks. Counts come from the 128 vanilla actor definitions (bedrock-samples
1.26.50.4) against the implemented component kinds after milestone 3's zombie
slice.

Refresh this list from the runtime itself, not by hand:
`MobRegistry::unsupported()` returns every definition the server cannot spawn
and the first component, filter or event that blocks it.

## 1. Accepted without behaviour (the definition compiles; the behaviour is missing)

These are listed in `entity/mob/components/mod.rs` `ACCEPTED_WITHOUT_BEHAVIOUR`.
Each one disappears from that list when its system lands.

| Component(s) | Missing system | Effect today |
|---|---|---|
| `rideable`, `behavior.mount_pathing`, `behavior.controlled_by_player`, `boostable`, `item_controllable`, `rotation_locked_to_vehicle`, `behavior.find_mount` | Riding (passengers, seats, steering, boost items) | Nothing can be ridden; baby zombies never become jockeys; pigs can be saddled but not ridden |
| `leashable`, `leashable_to` | Leads (leash knot, lead physics, leash holder actor data) | Leads do nothing on mobs |
| `transformation` | Lightning (pig → zombified piglin) and drowned conversion (target not spawnable) | Struck pigs stay pigs; underwater zombies shake but never convert |
| `behavior.stomp_turtle_egg` | Turtle eggs (block, hatching, turtles) | Zombies never seek turtle eggs |
| `behavior.use_kinetic_weapon` | Kinetic (spear) weapons: the item component and charge attacks | Zombies holding spears melee-attack normally |
| `game_event_movement_tracking` | Vibrations / game events (sculk sensors, wardens, allays) | No movement vibrations are emitted |
| `balloonable` | Balloons (Education Edition) | Out of vanilla survival scope |
| `conditional_bandwidth_optimization` | Network send-rate tuning | Every significant change is replicated |

## 2. Unimplemented systems, by what they unlock

| System | Components | Unlocks (examples) |
|---|---|---|
| Ranged attacks and arrows | `behavior.ranged_attack`, `shooter`, arrow projectile features (`anchor`, `should_bounce`, `uncertainty_*`, `arrow_effect`, `stick_in_ground`, `power_multiplier`), `dimension_bound` | skeleton, stray, bogged, parched, pillager, drowned (trident), blaze, ghast, llama spit |
| Sun avoidance | `behavior.flee_sun`, `freezing_immune` | skeleton family, zombie villager |
| Swimming | `navigation.generic`, `underwater_movement`, `movement.sway`, `behavior.random_swim`, `behavior.swim_wander`, `behavior.swim_idle`, `flocking`, `movement.amphibious`, `behavior.move_to_water`/`move_to_land` | fish, squid, dolphin, axolotl, turtle, guardian, drowned |
| Flying / hovering | `can_fly`, `navigation.fly`/`hover`/`float`, `movement.fly`/`hover`/`glide`, `flying_speed`, `behavior.random_fly`/`random_hover`/`float_wander`/`swoop_attack`/`circle_around_anchor` | bat, bee, parrot, allay, ghast, phantom |
| Anger and target events | `angry`, `on_target_acquired`, `on_target_escape`, `on_friendly_anger`, `on_hurt`, `on_hurt_by_player`, `follow_range`, `behavior.nearest_prioritized_attackable_target`, `target_nearby_sensor` | bee, spider, enderman, wolf, piglin, llama, polar bear, creeper |
| Fear and avoidance | `behavior.avoid_mob_type`, `behavior.avoid_block` | cat/ocelot vs creeper, fox, hoglin, fish |
| Variants | `variant`, `mark_variant`, `skin_id`, `color2`, `genetics` | cat, horse, llama, fox, frog, goat, tropical fish, villagers, panda |
| Taming and owners | `tameable`, `is_tamed`, `tamemount`, `sittable`, `healable`, `behavior.follow_owner`/`stay_while_sitting`/`teleport_to_owner`/`owner_hurt_target`/`owner_hurt_by_target`/`beg`, `trust`, `trusting` | wolf, cat, parrot, horse family, ocelot, fox |
| Inventories and mount gear | `inventory`, `equippable`, `is_chested`, `strength`, `horse.jump_strength`, `can_power_jump`, `input_ground_controlled`, `dash_action`, `behavior.player_ride_tamed`, `behavior.run_around_like_crazy` | horse family, llama, camel, chest boats/minecarts |
| Explosions | `explode`, `behavior.swell`, `is_charged`, `is_ignited` | creeper, TNT, fireballs, wither skulls, wind charges |
| Villages and raids | `dweller`, `preferred_path`, `home`, `scheduler`, `trade_table`/`economy_trade_table`, `behavior.trade_*`, `behavior.work*`, `behavior.sleep`, `behavior.move_to_village`, `can_join_raid`, `is_illager_captain`, `behavior.celebrate*`, `raid_trigger` | villagers, wandering trader, iron golem, illagers, witch, ravager |
| Area and special attacks | `area_attack`, `behavior.leap_at_target`, `behavior.slime_*`, `movement.jump`, `behavior.ram_attack`, `behavior.ocelotattack`, `behavior.stomp_attack`, `behavior.charge_attack`, `behavior.guardian_attack`, `attack_cooldown`, `knockback_resistance` | spider, slime, magma cube, goat, guardian, polar bear, vex |
| Block interaction | `annotation.open_door`, `behavior.open_door`, `behavior.place_block`/`take_block`, `behavior.raid_garden`, `behavior.harvest_farm_block`, `grows_crop`, `break_blocks`, `block_climber`, `navigation.climb` | villagers, enderman, fox, rabbit, bee, ravager, spider |
| Effects | `mob_effect`, `mob_effect_immunity`, `spell_effects`, `damage_over_time`, `fire_immune`, `freezing_vulnerable`, `behavior.drink_potion` | pufferfish, warden, wither, blaze, witch |
| Persistence and misc state | `persistent`, `instant_despawn`, `transient`, `is_stackable`, `is_collidable`, `renders_when_invisible`, `buoyant`, `variable_max_auto_step`, `custom_hit_test`, `cannot_be_attacked`, `looked_at`, `teleport`, `peek` | many |
| Vehicles | `rail_movement`, `rail_sensor`, `out_of_control`, `inside_block_notifier`, `item_hopper` | boats, minecarts |
| Bosses | `boss`, `behavior.dragon*`, `behavior.wither_*`, `on_start_takeoff`/`landing`, `on_death` | ender dragon, wither |
| Unique AI | warden (`anger_level`, `vibration_listener`, `behavior.sonic_boom`/`roar`/`dig`/`emerge`/`sniff`/`investigate_suspicious_location`), panda, frog, sniffer, allay, copper golem, breeze, creaking, happy ghast, nautilus | one mob each |
| Player definition | `player.*` attributes, `exhaustion_values`, `insomnia`, `block_climber`, `spell_effects`, `raid_trigger` | the player actor definition (players are not spawned through the mob runtime) |

## 3. Loot

Decoded and evaluated: `set_count`, `set_data`, `looting_enchant`,
`apply_bonus`, `limit_count`, `explosion_decay`, `furnace_smelt`,
`set_data_from_color_index`; conditions `random_chance`,
`random_chance_with_looting`, `killed_by_player`, `killed_by_player_or_pets`
(treated as player-only until pets exist), `match_tool`, `table_bonus`,
`block_state_property`, `inverted`; entry kinds `item`, `loot_table`, `empty`,
`alternatives`. Milestone 3's zombie slice adds `passenger_of_entity`, `is_baby`,
`random_difficulty_chance`, `random_regional_difficulty_chance`,
`enchant_random_gear` and pool `tiers`.

Still missing, used by vanilla tables:
- functions: `enchant_book_for_trading`, `enchant_randomly`, `enchant_with_levels`, `exploration_map`, `random_aux_value`, `set_armor_trim`, `set_banner_details`, `set_damage`, `set_ominous_bottle_amplifier`, `set_potion`, `set_stew_effect`, `specific_enchants`;
- conditions: `biome_has_tag`, `bool_property`, `damaged_by_entity`, `entity_killed`, `entity_properties` (cow on fire works through `furnace_smelt`'s condition), `has_mark_variant`, `has_variant`, `killed_by_entity`.

An unsupported function or condition makes the roll fail with an error and drop
nothing; it is never silently skipped.

## 4. Natural spawning

Rules with these conditions are logged once at startup and never spawn:
`delay_filter`, `mob_event_filter`, `player_in_village_filter`, `spawn_event`,
`is_experimental`, and biome filters other than `has_biome_tag`
(`is_snow_covered`). Weather (`adjust_for_weather`) has no effect because
there is no weather.

## 5. Engine gaps that affect entities

- No weather (rain, thunder, lightning).
- No block light; brightness is sky light only.
- Block identities are resolved by name string at startup in pathing, fall
  damage and spawning; a generated typed block identity (like `ActorType`)
  would remove those strings.
- No villages, POIs or raids.
- No game events or vibrations.
- No riding or leads (see section 1).
