# Movement differential: BDS 1.26.51, unastar, bedsim, acacia-physics

- **bds**: BDS 1.26.51, run md-r5-h; random_14, random_135 and random_66 from run md-r5-random
- **unastar**: unastar_rules::movement
- **bedsim**: oomph-ac/bedsim v0.1.7-0.20261007160526-9c12b344afde
- **acacia**: acacia-rs/acacia acacia-physics @ 97ec4c7ce23e5abb7cde1e66026363d3c2e6fbf0

| scenario | ticks | unastar = BDS | bedsim = BDS | acacia = BDS | unastar = bedsim | acacia = bedsim | acacia = unastar |
|---|---|---|---|---|---|---|---|
| analog_half_walk | 45 | equal | equal | equal | equal | equal | equal |
| bed_bounce | 50 | differs t12 velocity.y | differs t12 velocity.y | differs t12 velocity.y | differs t12 velocity.y | differs t42 velocity.y | differs t12 velocity.y |
| bed_walk | 45 | equal | equal | equal | equal | equal | equal |
| blue_ice_sprint_jump | 55 | equal | equal | equal | equal | equal | equal |
| blue_ice_walk | 65 | equal | equal | equal | equal | equal | equal |
| bubble_column_down | 50 | differs t1 position.y | differs t1 position.y (runner world) | differs t1 position.y | differs t1 position.z | differs t1 position.z | differs t1 velocity.y |
| bubble_column_up | 60 | differs t1 position.y | differs t1 position.y (runner world) | differs t1 position.y | differs t1 position.z | differs t1 position.z | differs t1 velocity.y |
| carpet_walk | 60 | equal | equal | equal | equal | equal | equal |
| cave_vines_climb | 65 | equal | differs t1 position.z (runner world) | equal | differs t1 position.z | differs t1 position.z | equal |
| cave_vines_walk_into | 50 | equal | differs t12 position.z (runner world) | equal | differs t12 position.z | differs t12 position.z | equal |
| ceiling_bonk | 56 | equal | equal | equal | equal | equal | equal |
| ceiling_sprint_jump | 55 | equal | equal | equal | equal | equal | equal |
| chain_walk_into | 45 | equal | equal | equal | equal | equal | equal |
| chunk_border_corner | 45 | equal | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x |
| chunk_border_step | 45 | equal | equal | differs t1 position.x | equal | differs t1 position.x | differs t1 position.x |
| chunk_border_wall_slide | 85 | equal | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x |
| cobblestone_wall_jump | 50 | equal | differs t18 position.z (runner world) | equal | differs t18 position.z | differs t18 position.z | equal |
| cobweb_fall | 40 | equal | equal | equal | equal | equal | equal |
| cobweb_jump | 40 | equal | equal | equal | equal | equal | equal |
| cobweb_sprint | 60 | equal | equal | equal | equal | equal | equal |
| cobweb_walk | 55 | equal | equal | equal | equal | equal | equal |
| door_closed_walk_into | 45 | equal | equal | equal | equal | equal | equal |
| door_open_walk_through | 55 | equal | equal | equal | equal | equal | equal |
| elytra_glide | 76 | equal | differs t6 velocity.x | differs t12 position.x | differs t6 velocity.x | differs t6 velocity.x | differs t7 velocity.x |
| elytra_glide_dive | 51 | equal | differs t6 velocity.x | differs t12 velocity.x | differs t6 velocity.x | differs t6 velocity.x | differs t7 velocity.x |
| elytra_glide_yaw | 66 | differs t6 velocity.x | differs t6 velocity.x | equal | differs t6 velocity.x | differs t6 velocity.x | differs t6 velocity.x |
| fall_long | 60 | equal | equal | equal | equal | equal | equal |
| fall_onto_edge | 48 | equal | differs t36 velocity.x | differs t34 position.x | differs t33 velocity.x | differs t33 velocity.x | differs t33 velocity.x |
| fall_short | 40 | equal | equal | equal | equal | equal | equal |
| fall_walking | 45 | equal | equal | equal | equal | equal | equal |
| fence_gate_closed | 45 | equal | equal | equal | equal | equal | equal |
| fence_gate_open | 45 | equal | equal | equal | equal | equal | equal |
| fence_walk_into | 45 | equal | equal | equal | equal | equal | equal |
| flat_back_sneak | 45 | equal | equal | equal | equal | equal | equal |
| flat_odd_yaw_walk | 45 | equal | differs t6 position.z | equal | differs t6 position.z | differs t6 position.z | equal |
| flat_sneak | 50 | equal | equal | equal | equal | equal | equal |
| flat_sneak_diagonal | 40 | equal | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x |
| flat_sprint | 55 | equal | equal | differs t36 position.z | equal | differs t36 position.z | differs t36 position.z |
| flat_sprint_strafe | 45 | equal | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x |
| flat_still | 20 | equal | equal | equal | equal | equal | equal |
| flat_strafe_diagonal | 50 | equal | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x |
| flat_strafe_right_turning | 55 | equal | differs t36 velocity.x | equal | differs t13 velocity.x | differs t13 velocity.x | equal |
| flat_turning_walk | 55 | equal | differs t9 position.x | equal | differs t8 velocity.x | differs t8 velocity.x | equal |
| flat_walk | 50 | equal | equal | equal | equal | equal | equal |
| flat_walk_back | 40 | equal | equal | equal | equal | equal | equal |
| glass_pane_post_diagonal | 55 | equal | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x |
| glass_pane_walk_into | 55 | equal | equal | differs t26 position.x | equal | differs t26 position.x | differs t26 position.x |
| honey_walk_jump | 51 | equal | equal | differs t48 velocity.z | equal | differs t48 velocity.z | differs t48 velocity.z |
| honey_wall_slide | 50 | equal | differs t10 position.z (runner world) | equal | differs t10 position.z | differs t10 position.z | differs t10 velocity.z |
| ice_sneak_turn | 60 | equal | differs t6 velocity.x | equal | differs t6 velocity.x | differs t6 velocity.x | equal |
| ice_sprint_jump | 60 | equal | differs t6 velocity.z | differs t6 velocity.z | differs t6 velocity.z | equal | differs t6 velocity.z |
| ice_strafe_stop | 65 | equal | equal | equal | equal | equal | equal |
| ice_walk | 65 | equal | equal | equal | equal | equal | equal |
| iron_bars_walk_into | 55 | equal | equal | differs t26 position.x | equal | differs t26 position.x | differs t26 position.x |
| jump_boost_2_jump | 35 | equal | equal | equal | equal | equal | equal |
| jump_boost_4_sprint_jump | 50 | equal | equal | equal | equal | equal | equal |
| jump_boost_jump | 36 | equal | equal | equal | equal | equal | equal |
| jump_in_place | 71 | equal | equal | equal | equal | equal | equal |
| knockback_air | 46 | equal | equal | equal | differs t45 velocity.z | differs t45 velocity.x | differs t45 velocity.x |
| knockback_ground | 45 | equal | differs t43 velocity.z | differs t41 position.z | differs t39 velocity.z | differs t39 velocity.x | differs t39 velocity.x |
| knockback_sprint_air | 50 | differs t16 position.z | differs t16 position.z | differs t16 position.z | equal | equal | equal |
| knockback_water | 50 | equal | equal | equal | differs t44 velocity.y | equal | differs t44 velocity.y |
| known_free_fall | 40 | equal | equal | equal | equal | equal | equal |
| known_still | 30 | equal | equal | equal | equal | equal | equal |
| ladder_climb | 75 | equal | equal | equal | equal | equal | equal |
| ladder_descend | 55 | equal | equal | equal | equal | equal | equal |
| ladder_jump | 65 | equal | equal | equal | equal | equal | equal |
| lava_climb_out | 50 | equal | equal | equal | equal | equal | equal |
| lava_fall_in | 40 | equal | equal | equal | differs t13 velocity.y | equal | differs t13 velocity.y |
| lava_sprint | 40 | equal | equal | equal | equal | equal | equal |
| lava_walk | 55 | equal | differs t48 velocity.z | differs t48 velocity.z | differs t47 velocity.z | equal | differs t47 velocity.z |
| ledge_sneak | 75 | equal | equal | differs t46 position.x | equal | differs t46 position.x | differs t46 position.x |
| ledge_walk_off | 50 | equal | equal | equal | equal | equal | equal |
| levitation_ceiling | 55 | equal | differs t1 velocity.y | differs t1 velocity.y | differs t1 velocity.y | differs t1 velocity.y | differs t1 velocity.y |
| levitation_float | 65 | equal | differs t1 on_ground | differs t1 on_ground | differs t1 velocity.y | differs t1 velocity.y | differs t1 velocity.y |
| levitation_jump | 36 | equal | differs t1 on_ground | differs t1 on_ground | differs t1 velocity.y | differs t1 velocity.y | differs t1 velocity.y |
| packed_ice_sprint_jump | 60 | equal | differs t30 velocity.z | differs t30 velocity.z | differs t30 velocity.z | equal | differs t30 velocity.z |
| powder_snow_fall | 50 | equal | differs t13 position.y (runner world) | differs t13 position.y | differs t13 position.y | differs t13 position.y | differs t13 position.y |
| powder_snow_jump | 45 | equal | differs t1 position.x (runner world) | equal | differs t1 position.x | differs t1 position.x | equal |
| powder_snow_walk_into | 55 | differs t18 position.z | differs t17 position.z (runner world) | differs t38 position.z | differs t17 position.z | differs t17 position.z | differs t18 position.z |
| random_135 | 45 | equal | differs t6 velocity.z | differs t4 position.x | differs t6 velocity.z | differs t4 position.x | differs t4 position.x |
| random_14 | 87 | differs t54 velocity.x | differs t2 position.y (runner world) | differs t31 position.x | differs t2 position.y | differs t2 position.y | differs t31 position.x |
| random_66 | 81 | differs t12 position.x | differs t12 position.x (runner world) | differs t60 position.x | differs t36 velocity.z | differs t12 position.x | differs t12 position.x |
| scaffolding_climb | 75 | equal | differs t1 position.z (runner world) | differs t36 velocity.y | differs t1 position.z | differs t1 position.z | differs t36 velocity.y |
| scaffolding_sneak_down | 55 | equal | differs t6 position.y (runner world) | differs t2 position.y | differs t6 position.y | differs t2 position.y | differs t2 position.y |
| scaffolding_walk_into | 50 | equal | differs t12 position.z (runner world) | equal | differs t12 position.z | differs t12 position.z | equal |
| scaffolding_walk_on_top | 45 | equal | differs t18 position.z (runner world) | equal | differs t18 position.z | differs t18 position.z | equal |
| slab_double_walk_into | 45 | equal | equal | equal | equal | equal | equal |
| slab_top_jump_onto | 45 | equal | equal | equal | equal | equal | equal |
| slime_bounce | 80 | equal | differs t14 position.y | equal | differs t13 velocity.y | differs t13 velocity.y | equal |
| slime_sneak_land | 45 | equal | equal | equal | equal | equal | equal |
| slime_walk | 45 | equal | equal | equal | equal | equal | equal |
| slow_falling_fall | 105 | equal | equal | differs t90 velocity.z | equal | differs t89 velocity.z | differs t89 velocity.z |
| slow_falling_jump | 55 | equal | equal | equal | equal | equal | equal |
| slow_falling_walk_off | 65 | equal | equal | equal | equal | equal | equal |
| slowness_sprint | 45 | equal | equal | equal | equal | equal | equal |
| slowness_sprint_jump | 45 | equal | equal | equal | equal | equal | equal |
| slowness_walk | 55 | equal | equal | equal | equal | equal | equal |
| sneak_jump | 51 | equal | equal | equal | equal | equal | equal |
| sneak_off_carpet_diagonal | 50 | equal | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x |
| sneak_off_fence | 60 | equal | equal | differs t60 velocity.z | differs t43 velocity.x | differs t57 velocity.z | differs t43 velocity.x |
| sneak_off_slab | 50 | equal | equal | equal | equal | equal | equal |
| sneak_off_stairs | 50 | equal | equal | equal | equal | equal | equal |
| sneak_off_trapdoor | 50 | equal | differs t2 position.y (runner world) | equal | differs t2 position.y | differs t2 position.y | equal |
| snow_layer_1_walk | 60 | equal | differs t17 position.z (runner world) | equal | differs t17 position.z | differs t17 position.z | equal |
| snow_layer_4_walk | 60 | equal | differs t17 position.y (runner world) | equal | differs t17 position.y | differs t17 position.y | equal |
| snow_layer_7_walk | 60 | equal | equal | equal | equal | equal | equal |
| snow_layer_8_walk | 60 | equal | equal | equal | equal | equal | equal |
| soul_sand_sprint_jump | 55 | equal | differs t2 position.y (runner world) | equal | differs t2 position.y | differs t2 position.y | equal |
| soul_sand_walk | 45 | equal | differs t2 position.y (runner world) | equal | differs t2 position.y | differs t2 position.y | equal |
| speed_sprint_jump | 45 | equal | equal | equal | equal | equal | equal |
| speed_walk | 55 | equal | equal | equal | equal | equal | equal |
| sprint_jump | 60 | equal | equal | differs t46 position.z | equal | differs t46 position.z | differs t46 position.z |
| sprint_jump_onto_block | 42 | equal | equal | equal | equal | equal | equal |
| sprint_sneak_toggle | 55 | equal | differs t31 position.z | equal | differs t31 position.z | differs t31 position.z | equal |
| sprint_turn_jump | 55 | differs t17 position.x | differs t10 position.x | differs t17 position.x | differs t8 velocity.x | differs t8 velocity.x | differs t42 position.x |
| stairs_corner_diagonal | 55 | equal | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x |
| stairs_down | 55 | equal | equal | equal | equal | equal | equal |
| stairs_up | 55 | equal | equal | equal | equal | equal | equal |
| stairs_upside_down_walk_under | 55 | equal | equal | equal | equal | equal | equal |
| step_slab | 45 | equal | equal | equal | equal | equal | equal |
| step_sprint_into_slab | 45 | equal | equal | equal | equal | equal | equal |
| step_top_slab | 40 | equal | equal | equal | equal | equal | equal |
| sweet_berry_jump | 35 | equal | differs t1 position.z (runner world) | equal | differs t1 position.z | differs t1 position.z | equal |
| sweet_berry_walk | 55 | equal | differs t17 position.z (runner world) | equal | differs t17 position.z | differs t17 position.z | equal |
| trapdoor_bottom_walk | 55 | equal | differs t17 position.y (runner world) | equal | differs t17 position.y | differs t17 position.y | equal |
| trapdoor_open_walk_into | 45 | equal | equal | equal | equal | equal | equal |
| trapdoor_top_walk_under | 55 | equal | equal | equal | equal | equal | equal |
| twisting_vines_climb | 65 | equal | differs t1 position.z (runner world) | equal | differs t1 position.z | differs t1 position.z | equal |
| twisting_vines_walk_into | 50 | equal | differs t12 position.z (runner world) | equal | differs t12 position.z | differs t12 position.z | equal |
| vine_climb | 65 | equal | equal | equal | equal | equal | equal |
| walk_jump | 46 | equal | equal | equal | equal | equal | equal |
| wall_corner_diagonal | 45 | equal | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x |
| wall_sprint_jump | 35 | equal | equal | equal | equal | equal | equal |
| wall_walk_into | 55 | equal | equal | differs t26 position.x | equal | differs t26 position.x | differs t26 position.x |
| water_climb_out | 45 | differs t21 position.y | equal | equal | differs t20 velocity.y | equal | differs t20 velocity.y |
| water_exit | 55 | equal | equal | equal | equal | equal | equal |
| water_fall_in | 40 | equal | equal | equal | differs t14 velocity.y | equal | differs t14 velocity.y |
| water_idle | 40 | equal | equal | equal | differs t25 velocity.y | equal | differs t25 velocity.y |
| water_jump | 55 | equal | equal | equal | differs t52 velocity.y | equal | differs t52 velocity.y |
| water_jump_walk | 45 | equal | equal | equal | equal | equal | equal |
| water_sprint_swim | 60 | differs t39 position.y | differs t11 position.y | differs t6 position.y | differs t11 position.y | differs t6 position.y | differs t6 position.y |
| water_surface_sprint | 45 | equal | equal | differs t6 velocity.y | equal | differs t6 velocity.y | differs t6 velocity.y |
| water_swim_deep | 55 | differs t46 position.x | differs t6 position.y | differs t6 velocity.y | differs t6 position.y | differs t6 position.y | differs t6 velocity.y |
| water_walk | 45 | equal | equal | equal | equal | equal | equal |
| weeping_vines_climb | 65 | equal | differs t1 position.z (runner world) | equal | differs t1 position.z | differs t1 position.z | equal |
| weeping_vines_walk_into | 50 | equal | differs t12 position.z (runner world) | equal | differs t12 position.z | differs t12 position.z | equal |
| **equal** | 147 | 135 | 97 | 103 | 90 | 80 | 95 |

## Runner world fidelity

Scenarios whose world a runner builds differently from unastar's (BDS 1.26.51's shapes and friction). Their divergences above are marked "runner world": they come from the runner, not from the library's physics.

- **bedsim**: 38 scenarios
  - `bubble_column_down`: 8 cells, e.g. 0,101,2 minecraft:bubble_column: a plain full cube of friction 0.6 to bedsim only
  - `bubble_column_up`: 9 cells, e.g. 0,100,2 minecraft:soul_sand: a plain full cube of friction 0.6 to bedsim only
  - `cave_vines_climb`: 7 cells, e.g. 0,100,2 minecraft:cave_vines: a plain full cube of friction 0.6 to bedsim only
  - `cave_vines_walk_into`: 7 cells, e.g. 0,100,2 minecraft:cave_vines: a plain full cube of friction 0.6 to bedsim only
  - `cobblestone_wall_jump`: 17 cells, e.g. -1,100,3 minecraft:cobblestone_wall: boxes [[-0.75, 100, 3.25, -0.25, 100.8125, 3.75], [-0.25, 100, 3.25, 0, 100.75, 3.75], [-1, 100, 3.25, -0.75, 100.75, 3.75]] instead of [[-1.0, 100.0, 3.3125, 0.0, 101.5, 3.6875]]
  - `fence_gate_open`: 14 cells, e.g. -1,100,3 minecraft:oak_fence: boxes [[-1, 100, 3.375, -0.375, 101.5, 3.625], [-0.625, 100, 3.375, 0, 101.5, 3.625]] instead of [[-1.0, 100.0, 3.375, 0.0, 101.5, 3.625]]
  - `fence_walk_into`: 15 cells, e.g. -1,100,3 minecraft:oak_fence: boxes [[-1, 100, 3.375, -0.375, 101.5, 3.625], [-0.625, 100, 3.375, 0, 101.5, 3.625]] instead of [[-1.0, 100.0, 3.375, 0.0, 101.5, 3.625]]
  - `glass_pane_walk_into`: 4 cells, e.g. -8,100,3 minecraft:glass_pane: boxes [[-7.5625, 100, 3.4375, -7, 101, 3.5625]] instead of [[-7.5, 100.0, 3.4375, -7.0, 101.0, 3.5625]]
  - `honey_walk_jump`: 833 cells, e.g. -1,99,-1 minecraft:honey_block: boxes [[-1, 99, -1, 0, 100, 0]] instead of [[-0.9375, 99.0, -0.9375, -0.0625, 100.0, -0.0625]]
  - `honey_wall_slide`: 187 cells, e.g. -1,100,2 minecraft:honey_block: boxes [[-1, 100, 2, 0, 101, 3]] instead of [[-0.9375, 100.0, 2.0625, -0.0625, 101.0, 2.9375]]
  - `iron_bars_walk_into`: 4 cells, e.g. -8,100,3 minecraft:iron_bars: boxes [[-7.5625, 100, 3.4375, -7, 101, 3.5625]] instead of [[-7.5, 100.0, 3.4375, -7.0, 101.0, 3.5625]]
  - `powder_snow_fall`: 18 cells, e.g. -1,100,-1 minecraft:powder_snow: a plain full cube of friction 0.6 to bedsim only
  - `powder_snow_jump`: 18 cells, e.g. -1,100,-1 minecraft:powder_snow: a plain full cube of friction 0.6 to bedsim only
  - `powder_snow_walk_into`: 136 cells, e.g. -1,100,3 minecraft:powder_snow: a plain full cube of friction 0.6 to bedsim only
  - `random_14`: 844 cells, e.g. -1,99,-1 minecraft:soul_sand: a plain full cube of friction 0.6 to bedsim only
  - `random_66`: 10 cells, e.g. -1,100,4 minecraft:soul_sand: a plain full cube of friction 0.6 to bedsim only
  - `scaffolding_climb`: 5 cells, e.g. 0,100,2 minecraft:scaffolding: a plain full cube of friction 0.6 to bedsim only
  - `scaffolding_sneak_down`: 5 cells, e.g. 0,100,2 minecraft:scaffolding: a plain full cube of friction 0.6 to bedsim only
  - `scaffolding_walk_into`: 5 cells, e.g. 0,100,2 minecraft:scaffolding: a plain full cube of friction 0.6 to bedsim only
  - `scaffolding_walk_on_top`: 68 cells, e.g. -1,100,3 minecraft:scaffolding: a plain full cube of friction 0.6 to bedsim only
  - `sneak_off_fence`: 27 cells, e.g. 0,100,-1 minecraft:oak_fence: boxes [[0.375, 100, -1, 0.625, 101.5, -0.375], [0.375, 100, -0.625, 0.625, 101.5, 0]] instead of [[0.375, 100.0, -1.0, 0.625, 101.5, 0.0]]
  - `sneak_off_trapdoor`: 187 cells, e.g. -1,100,-1 minecraft:trapdoor: boxes [[-1, 100, -1, 0, 100.1875, 0]] instead of [[-1.0, 100.0, -1.0, 0.0, 100.18250274658203, 0.0]]
  - `snow_layer_1_walk`: 68 cells, e.g. -1,100,3 minecraft:snow_layer: a plain full cube of friction 0.6 to bedsim only
  - `snow_layer_4_walk`: 68 cells, e.g. -1,100,3 minecraft:snow_layer: a plain full cube of friction 0.6 to bedsim only
  - `snow_layer_7_walk`: 68 cells, e.g. -1,100,3 minecraft:snow_layer: a plain full cube of friction 0.6 to bedsim only
  - `snow_layer_8_walk`: 68 cells, e.g. -1,100,3 minecraft:snow_layer: a plain full cube of friction 0.6 to bedsim only
  - `soul_sand_sprint_jump`: 833 cells, e.g. -1,99,-1 minecraft:soul_sand: a plain full cube of friction 0.6 to bedsim only
  - `soul_sand_walk`: 833 cells, e.g. -1,99,-1 minecraft:soul_sand: a plain full cube of friction 0.6 to bedsim only
  - `stairs_corner_diagonal`: 1 cells, e.g. 3,100,3 minecraft:oak_stairs: boxes [[3, 100, 3, 4, 100.5, 4], [3, 100.5, 3, 3.5, 101, 4]] instead of [[3.0, 100.0, 3.0, 4.0, 100.5, 4.0], [3.0, 100.5, 3.0, 3.5, 101.0, 4.0], [3.5, 100.5, 3.5, 4.0, 101.0, 4.0]]
  - `sweet_berry_jump`: 9 cells, e.g. -1,100,1 minecraft:sweet_berry_bush: a plain full cube of friction 0.6 to bedsim only
  - `sweet_berry_walk`: 68 cells, e.g. -1,100,3 minecraft:sweet_berry_bush: a plain full cube of friction 0.6 to bedsim only
  - `trapdoor_bottom_walk`: 68 cells, e.g. -1,100,3 minecraft:trapdoor: boxes [[-1, 100, 3, 0, 100.1875, 4]] instead of [[-1.0, 100.0, 3.0, 0.0, 100.18250274658203, 4.0]]
  - `trapdoor_open_walk_into`: 17 cells, e.g. -1,100,3 minecraft:trapdoor: boxes [[-1, 100, 3, -0.8125, 101, 4]] instead of [[-1.0, 100.0, 3.0, -0.8174999952316284, 101.0, 4.0]]
  - `trapdoor_top_walk_under`: 68 cells, e.g. -1,101,3 minecraft:trapdoor: boxes [[-1, 101.8125, 3, 0, 102, 4]] instead of [[-1.0, 101.81749725341797, 3.0, 0.0, 102.0, 4.0]]
  - `twisting_vines_climb`: 7 cells, e.g. 0,100,2 minecraft:twisting_vines: a plain full cube of friction 0.6 to bedsim only
  - `twisting_vines_walk_into`: 7 cells, e.g. 0,100,2 minecraft:twisting_vines: a plain full cube of friction 0.6 to bedsim only
  - `weeping_vines_climb`: 7 cells, e.g. 0,100,2 minecraft:weeping_vines: a plain full cube of friction 0.6 to bedsim only
  - `weeping_vines_walk_into`: 7 cells, e.g. 0,100,2 minecraft:weeping_vines: a plain full cube of friction 0.6 to bedsim only

## Harness validation

- **Setup, inputs and relays** (the committed run's `validation.json`): 144 of 144 scenarios played exactly as the corpus says (world, effects, chest slot, start teleport; every input's move vector, rotation and keys); 7506 relays for 7506 inputs.
- **repeat** (`BDS 1.26.51, run md-r5-e`, offset [3.0, 3.0, 3.0], flags from simulation): 142 of 144 scenarios bit-identical to the committed run.
  - `bed_bounce` differs: only the last input: this run's client sent the next scenario's lobby teleport right after it, and BDS ran the teleport before the move (fixed since: the client waits 3 inputs, and the setup check rejects a teleport during a scenario)
  - `powder_snow_walk_into` differs: powder snow's freezing slowdown follows BDS's game ticks, not the inputs, so the walk into the snow is slowed by a different amount
- **offset** (`BDS 1.26.51, run md-r5-f`, offset [-5.0, 5.0, -4.0], flags from simulation): 136 of 144 scenarios bit-identical to the committed run.
  - `flat_odd_yaw_walk` differs: only the last input: this run's client sent the next scenario's lobby teleport right after it, and BDS ran the teleport before the move (fixed since: the client waits 3 inputs, and the setup check rejects a teleport during a scenario)
  - `flat_sneak_diagonal` differs: only the last input: this run's client sent the next scenario's lobby teleport right after it, and BDS ran the teleport before the move (fixed since: the client waits 3 inputs, and the setup check rejects a teleport during a scenario)
  - `flat_walk` differs: only the last input: this run's client sent the next scenario's lobby teleport right after it, and BDS ran the teleport before the move (fixed since: the client waits 3 inputs, and the setup check rejects a teleport during a scenario)
  - `knockback_sprint_air` differs: the knockback command ran an input later than in the committed run (its SetActorMotion stamped a tick later), so the push lands a tick later
  - `powder_snow_fall` differs: positions equal; powder snow's freezing updates follow BDS's game ticks, not the inputs, so a different number of them falls within the scenario
  - `powder_snow_walk_into` differs: powder snow's freezing slowdown follows BDS's game ticks, not the inputs, so the walk into the snow is slowed by a different amount
  - `sneak_jump` differs: only the last input: this run's client sent the next scenario's lobby teleport right after it, and BDS ran the teleport before the move (fixed since: the client waits 3 inputs, and the setup check rejects a teleport during a scenario)
  - `walk_jump` differs: only the last input: this run's client sent the next scenario's lobby teleport right after it, and BDS ran the teleport before the move (fixed since: the client waits 3 inputs, and the setup check rejects a teleport during a scenario)
- **flags** (`BDS 1.26.51, run md-r5-g`, offset [3.0, 3.0, 3.0], flags from keys): 137 of 144 scenarios bit-identical to the committed run.
  - `flat_sprint` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `knockback_ground` differs: the knockback command ran an input later than in the committed run (its SetActorMotion stamped a tick later), so the push lands a tick later
  - `knockback_sprint_air` differs: the knockback command ran an input later than in the committed run (its SetActorMotion stamped a tick later), so the push lands a tick later
  - `powder_snow_fall` differs: positions equal; powder snow's freezing updates follow BDS's game ticks, not the inputs, so a different number of them falls within the scenario
  - `powder_snow_walk_into` differs: powder snow's freezing slowdown follows BDS's game ticks, not the inputs, so the walk into the snow is slowed by a different amount
  - `sprint_jump` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sprint_sneak_toggle` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
- **Mutations** (`mutate.py`): 24 of 34 caught by at least one scenario that matches BDS without them.
  - gravity 0.08 one ULP up: caught by 127 scenarios (e.g. `analog_half_walk`)
  - vertical drag 0.98 one ULP down: caught by 128 scenarios (e.g. `analog_half_walk`)
  - air friction 0.91 one ULP up: caught by 46 scenarios (e.g. `bed_walk`)
  - the block friction one ULP up: caught by 9 scenarios (e.g. `blue_ice_sprint_jump`)
  - air acceleration 0.02 one ULP up: caught by 11 scenarios (e.g. `chunk_border_step`)
  - sprinting air acceleration 0.026 one ULP up: caught by 4 scenarios (e.g. `blue_ice_sprint_jump`)
  - jump 0.42 one ULP up: caught by 23 scenarios (e.g. `blue_ice_sprint_jump`)
  - jump boost step 0.1 one ULP up: caught by 1 scenarios (e.g. `jump_boost_4_sprint_jump`)
  - sprint jump push 0.2 one ULP up: caught by 8 scenarios (e.g. `blue_ice_sprint_jump`)
  - move vector scale 0.98 one ULP down: caught by 85 scenarios (e.g. `analog_half_walk`)
  - sneak factor 0.3 one ULP up: caught by 12 scenarios (e.g. `flat_back_sneak`)
  - ground acceleration rounded as ((speed r) r) r: caught by 1 scenarios (e.g. `ice_sprint_jump`)
  - gravity and drag as v drag - g drag: caught by 55 scenarios (e.g. `bed_walk`)
  - the walk turned by the sine table: caught by 4 scenarios (e.g. `flat_odd_yaw_walk`)
  - the sprint jump turned by the exact sine, not the table: **missed**
  - step height 0.5625 up to 0.6: **missed**
  - a slide stops at twice f32::EPSILON: caught by 3 scenarios (e.g. `fall_onto_edge`)
  - touching tolerance 1e-7 up to 1e-6: **missed**
  - sneak edge probe step 0.05 down to 0.04: caught by 1 scenarios (e.g. `sneak_off_fence`)
  - water drag 0.8 one ULP up: caught by 7 scenarios (e.g. `knockback_water`)
  - water sink 0.005 one ULP up: caught by 8 scenarios (e.g. `knockback_water`)
  - swim steering 0.085 one ULP up: **missed**
  - lava drag 0.5 one ULP up: caught by 4 scenarios (e.g. `lava_climb_out`)
  - glide horizontal drag 0.99 one ULP down: caught by 2 scenarios (e.g. `elytra_glide`)
  - levitation lift 0.01 one ULP up: caught by 3 scenarios (e.g. `levitation_ceiling`)
  - slow falling gravity 0.01 one ULP up: **missed**
  - climb speed 0.2 one ULP up: caught by 10 scenarios (e.g. `cave_vines_climb`)
  - soul sand factor 1.225 one ULP up: caught by 2 scenarios (e.g. `soul_sand_sprint_jump`)
  - honey jump 0.6 one ULP up: caught by 1 scenarios (e.g. `honey_walk_jump`)
  - slowest bouncing fall 0.08000012 down to 0.0784: **missed**
  - cobweb horizontal factor 0.25 one ULP up: **missed**
  - bed bounce 0.66 one ULP up: **missed**
  - powder snow factor 0.9 one ULP up: **missed**
  - wire offset 1.62001 one ULP up: **missed**

## Divergences

- `bed_bounce` unastar vs bds: first tick 12 velocity.y: 0.42683694 vs 0.5324077 (diff 0.106, 2998656 ULP); 38 of 50 ticks differ; largest tick 23 position.y: 102.37192 vs 103.423775 (diff 1.05, 137869 ULP)
- `bed_bounce` bedsim vs bds: first tick 12 velocity.y: 0.49573284 vs 0.5324077 (diff 0.0367, 686893 ULP); 37 of 50 ticks differ; largest tick 40 position.y: 102.18251 vs 102.62937 (diff 0.447, 58571 ULP)
- `bed_bounce` acacia vs bds: first tick 12 velocity.y: 0.49573284 vs 0.5324077 (diff 0.0367, 686893 ULP); 37 of 50 ticks differ; largest tick 40 position.y: 102.18251 vs 102.62937 (diff 0.447, 58571 ULP)
- `bed_bounce` unastar vs bedsim: first tick 12 velocity.y: 0.42683694 vs 0.49573284 (diff 0.0689, 2311763 ULP); 30 of 50 ticks differ; largest tick 31 position.y: 102.18251 vs 102.88831 (diff 0.706, 92511 ULP)
- `bed_bounce` acacia vs bedsim: first tick 42 velocity.y: -0.04744069 vs -0.0784 (diff 0.031, 6176515 ULP); 1 of 50 ticks differ; largest tick 42 velocity.y: -0.04744069 vs -0.0784 (diff 0.031, 6176515 ULP)
- `bed_bounce` acacia vs unastar: first tick 12 velocity.y: 0.49573284 vs 0.42683694 (diff 0.0689, 2311763 ULP); 31 of 50 ticks differ; largest tick 31 position.y: 102.88831 vs 102.18251 (diff 0.706, 92511 ULP)
- `bubble_column_down` unastar vs bds: first tick 1 position.y: 107.62001 vs 107.56001 (diff 0.06, 7864 ULP); 50 of 50 ticks differ; largest tick 19 position.y: 107.26819 vs 102.62001 (diff 4.65, 609246 ULP)
- `bubble_column_down` bedsim vs bds: first tick 1 position.y: 107.62001 vs 107.56001 (diff 0.06, 7864 ULP); 50 of 50 ticks differ; largest tick 19 position.y: 107.62001 vs 102.62001 (diff 5, 655360 ULP)
- `bubble_column_down` acacia vs bds: first tick 1 position.y: 107.62001 vs 107.56001 (diff 0.06, 7864 ULP); 19 of 50 ticks differ; largest tick 5 position.y: 106.93032 vs 106.645096 (diff 0.285, 37385 ULP)
- `bubble_column_down` unastar vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 50 of 50 ticks differ; largest tick 50 position.y: 106.49497 vs 107.62001 (diff 1.13, 147461 ULP)
- `bubble_column_down` acacia vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 50 of 50 ticks differ; largest tick 20 position.y: 102.62001 vs 107.62001 (diff 5, 655360 ULP)
- `bubble_column_down` acacia vs unastar: first tick 1 velocity.y: -0.065 vs -0.005 (diff 0.06, 31541166 ULP); 50 of 50 ticks differ; largest tick 20 position.y: 102.62001 vs 107.243546 (diff 4.62, 606016 ULP)
- `bubble_column_up` unastar vs bds: first tick 1 position.y: 102.62001 vs 102.74001 (diff 0.12, 15729 ULP); 60 of 60 ticks differ; largest tick 23 position.y: 102.49501 vs 113.54069 (diff 11, 1447779 ULP)
- `bubble_column_up` bedsim vs bds: first tick 1 position.y: 102.62001 vs 102.74001 (diff 0.12, 15729 ULP); 60 of 60 ticks differ; largest tick 23 position.y: 102.62001 vs 113.54069 (diff 10.9, 1431395 ULP)
- `bubble_column_up` acacia vs bds: first tick 1 position.y: 102.62001 vs 102.74001 (diff 0.12, 15729 ULP); 60 of 60 ticks differ; largest tick 12 position.y: 108.00727 vs 108.752785 (diff 0.746, 97716 ULP)
- `bubble_column_up` unastar vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 60 of 60 ticks differ; largest tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP)
- `bubble_column_up` acacia vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 60 of 60 ticks differ; largest tick 24 position.y: 113.58124 vs 102.62001 (diff 11, 1436710 ULP)
- `bubble_column_up` acacia vs unastar: first tick 1 velocity.y: 0.114999995 vs -0.005 (diff 0.12, 2039438376 ULP); 60 of 60 ticks differ; largest tick 24 position.y: 113.58124 vs 102.49501 (diff 11.1, 1453094 ULP)
- `cave_vines_climb` bedsim vs bds: first tick 1 position.z: 3.3 vs 2.5 (diff 0.8, 3355443 ULP); 65 of 65 ticks differ; largest tick 29 position.y: 101.62001 vs 106.41994 (diff 4.8, 629136 ULP)
- `cave_vines_climb` unastar vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 65 of 65 ticks differ; largest tick 29 position.y: 106.41994 vs 101.62001 (diff 4.8, 629136 ULP)
- `cave_vines_climb` acacia vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 65 of 65 ticks differ; largest tick 29 position.y: 106.41994 vs 101.62001 (diff 4.8, 629136 ULP)
- `cave_vines_walk_into` bedsim vs bds: first tick 12 position.z: 1.7 vs 1.7551675 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 38 position.y: 101.62001 vs 105.5744 (diff 3.95, 518310 ULP)
- `cave_vines_walk_into` unastar vs bedsim: first tick 12 position.z: 1.7551675 vs 1.7 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 38 position.y: 105.5744 vs 101.62001 (diff 3.95, 518310 ULP)
- `cave_vines_walk_into` acacia vs bedsim: first tick 12 position.z: 1.7551675 vs 1.7 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 38 position.y: 105.5744 vs 101.62001 (diff 3.95, 518310 ULP)
- `chunk_border_corner` acacia vs bds: first tick 6 position.x: -0.57071066 vs -0.5692965 (diff 0.00141, 23726 ULP); 40 of 45 ticks differ; largest tick 45 position.z: 18.542704 vs 18.421852 (diff 0.121, 63361 ULP)
- `chunk_border_corner` acacia vs bedsim: first tick 6 position.x: -0.57071066 vs -0.5692965 (diff 0.00141, 23726 ULP); 40 of 45 ticks differ; largest tick 45 position.z: 18.542704 vs 18.421852 (diff 0.121, 63361 ULP)
- `chunk_border_corner` acacia vs unastar: first tick 6 position.x: -0.57071066 vs -0.5692965 (diff 0.00141, 23726 ULP); 40 of 45 ticks differ; largest tick 45 position.z: 18.542704 vs 18.421852 (diff 0.121, 63361 ULP)
- `chunk_border_step` acacia vs bds: first tick 1 position.x: 0.05 vs 0.050000012 (diff 1.12e-08, 3 ULP); 45 of 45 ticks differ; largest tick 45 position.z: 8.702078 vs 8.542798 (diff 0.159, 167017 ULP)
- `chunk_border_step` acacia vs bedsim: first tick 1 position.x: 0.05 vs 0.050000012 (diff 1.12e-08, 3 ULP); 45 of 45 ticks differ; largest tick 45 position.z: 8.702078 vs 8.542798 (diff 0.159, 167017 ULP)
- `chunk_border_step` acacia vs unastar: first tick 1 position.x: 0.05 vs 0.050000012 (diff 1.12e-08, 3 ULP); 45 of 45 ticks differ; largest tick 45 position.z: 8.702078 vs 8.542798 (diff 0.159, 167017 ULP)
- `chunk_border_wall_slide` acacia vs bds: first tick 6 position.x: 0.4712652 vs 0.4718399 (diff 0.000575, 19284 ULP); 80 of 85 ticks differ; largest tick 85 position.z: 26.124298 vs 25.79175 (diff 0.333, 174351 ULP)
- `chunk_border_wall_slide` acacia vs bedsim: first tick 6 position.x: 0.4712652 vs 0.4718399 (diff 0.000575, 19284 ULP); 80 of 85 ticks differ; largest tick 85 position.z: 26.124298 vs 25.79175 (diff 0.333, 174351 ULP)
- `chunk_border_wall_slide` acacia vs unastar: first tick 6 position.x: 0.4712652 vs 0.4718399 (diff 0.000575, 19284 ULP); 80 of 85 ticks differ; largest tick 85 position.z: 26.124298 vs 25.79175 (diff 0.333, 174351 ULP)
- `cobblestone_wall_jump` bedsim vs bds: first tick 18 position.z: 2.95 vs 3.0125 (diff 0.0625, 262144 ULP); 33 of 50 ticks differ; largest tick 50 position.z: 5.2920594 vs 3.0125 (diff 2.28, 6851520 ULP)
- `cobblestone_wall_jump` unastar vs bedsim: first tick 18 position.z: 3.0125 vs 2.95 (diff 0.0625, 262144 ULP); 33 of 50 ticks differ; largest tick 50 position.z: 3.0125 vs 5.2920594 (diff 2.28, 6851520 ULP)
- `cobblestone_wall_jump` acacia vs bedsim: first tick 18 position.z: 3.0125 vs 2.95 (diff 0.0625, 262144 ULP); 33 of 50 ticks differ; largest tick 50 position.z: 3.0125 vs 5.2920594 (diff 2.28, 6851520 ULP)
- `elytra_glide` bedsim vs bds: first tick 6 velocity.x: -4.3278747e-18 vs 3.0895015e-09 (diff 3.09e-09, 1408498357 ULP); 66 of 76 ticks differ; largest tick 69 position.z: 22.801403 vs 22.801416 (diff 1.34e-05, 7 ULP)
- `elytra_glide` acacia vs bds: first tick 12 position.x: 0.5 vs 0.50000006 (diff 5.96e-08, 1 ULP); 65 of 76 ticks differ; largest tick 75 position.x: 0.5 vs 0.50000316 (diff 3.16e-06, 53 ULP)
- `elytra_glide` unastar vs bedsim: first tick 6 velocity.x: 3.0895015e-09 vs -4.3278747e-18 (diff 3.09e-09, 1408498357 ULP); 71 of 76 ticks differ; largest tick 69 position.z: 22.801416 vs 22.801403 (diff 1.34e-05, 7 ULP)
- `elytra_glide` acacia vs bedsim: first tick 6 velocity.x: 3.0895015e-09 vs -4.3278747e-18 (diff 3.09e-09, 1408498357 ULP); 71 of 76 ticks differ; largest tick 69 position.z: 22.801416 vs 22.801403 (diff 1.34e-05, 7 ULP)
- `elytra_glide` acacia vs unastar: first tick 7 velocity.x: 2.8980212e-09 vs 5.650767e-09 (diff 2.75e-09, 8061477 ULP); 70 of 76 ticks differ; largest tick 75 position.x: 0.5 vs 0.50000316 (diff 3.16e-06, 53 ULP)
- `elytra_glide_dive` bedsim vs bds: first tick 6 velocity.x: -3.3684401e-18 vs 2.4045983e-09 (diff 2.4e-09, 1402849837 ULP); 41 of 51 ticks differ; largest tick 17 position.y: 114.882774 vs 114.88277 (diff 7.63e-06, 1 ULP)
- `elytra_glide_dive` acacia vs bds: first tick 12 velocity.x: 3.2385097e-09 vs 1.4841777e-08 (diff 1.16e-08, 18902620 ULP); 40 of 51 ticks differ; largest tick 51 position.x: 0.5 vs 0.50000155 (diff 1.55e-06, 26 ULP)
- `elytra_glide_dive` unastar vs bedsim: first tick 6 velocity.x: 2.4045983e-09 vs -3.3684401e-18 (diff 2.4e-09, 1402849837 ULP); 46 of 51 ticks differ; largest tick 17 position.y: 114.88277 vs 114.882774 (diff 7.63e-06, 1 ULP)
- `elytra_glide_dive` acacia vs bedsim: first tick 6 velocity.x: 2.4045983e-09 vs -3.3684401e-18 (diff 2.4e-09, 1402849837 ULP); 46 of 51 ticks differ; largest tick 17 position.y: 114.88277 vs 114.882774 (diff 7.63e-06, 1 ULP)
- `elytra_glide_dive` acacia vs unastar: first tick 7 velocity.x: 2.5794644e-09 vs 4.7219615e-09 (diff 2.14e-09, 7404645 ULP); 45 of 51 ticks differ; largest tick 51 position.x: 0.5 vs 0.50000155 (diff 1.55e-06, 26 ULP)
- `elytra_glide_yaw` unastar vs bds: first tick 6 velocity.x: -0.021511702 vs -0.021511696 (diff 5.59e-09, 3 ULP); 56 of 66 ticks differ; largest tick 52 position.z: 7.996498 vs 7.996505 (diff 6.68e-06, 14 ULP)
- `elytra_glide_yaw` bedsim vs bds: first tick 6 velocity.x: -0.0215117 vs -0.021511696 (diff 3.73e-09, 2 ULP); 53 of 66 ticks differ; largest tick 52 position.z: 7.996498 vs 7.996505 (diff 6.68e-06, 14 ULP)
- `elytra_glide_yaw` unastar vs bedsim: first tick 6 velocity.x: -0.021511702 vs -0.0215117 (diff 1.86e-09, 1 ULP); 61 of 66 ticks differ; largest tick 56 position.x: -8.793335 vs -8.793333 (diff 1.91e-06, 2 ULP)
- `elytra_glide_yaw` acacia vs bedsim: first tick 6 velocity.x: -0.021511696 vs -0.0215117 (diff 3.73e-09, 2 ULP); 61 of 66 ticks differ; largest tick 52 position.z: 7.996505 vs 7.996498 (diff 6.68e-06, 14 ULP)
- `elytra_glide_yaw` acacia vs unastar: first tick 6 velocity.x: -0.021511696 vs -0.021511702 (diff 5.59e-09, 3 ULP); 61 of 66 ticks differ; largest tick 52 position.z: 7.996505 vs 7.996498 (diff 6.68e-06, 14 ULP)
- `fall_onto_edge` bedsim vs bds: first tick 36 velocity.x: 0 vs -0 (diff 0, 0 ULP); 3 of 48 ticks differ; largest tick 36 velocity.x: 0 vs -0 (diff 0, 0 ULP)
- `fall_onto_edge` acacia vs bds: first tick 34 position.x: 0.4969403 vs 0.49694037 (diff 5.96e-08, 2 ULP); 15 of 48 ticks differ; largest tick 34 position.x: 0.4969403 vs 0.49694037 (diff 5.96e-08, 2 ULP)
- `fall_onto_edge` unastar vs bedsim: first tick 33 velocity.x: -0 vs 0 (diff 0, 0 ULP); 16 of 48 ticks differ; largest tick 33 velocity.x: -0 vs 0 (diff 0, 0 ULP)
- `fall_onto_edge` acacia vs bedsim: first tick 33 velocity.x: -3.6497365e-08 vs 0 (diff 3.65e-08, 857522503 ULP); 16 of 48 ticks differ; largest tick 34 position.x: 0.4969403 vs 0.49694037 (diff 5.96e-08, 2 ULP)
- `fall_onto_edge` acacia vs unastar: first tick 33 velocity.x: -3.6497365e-08 vs -0 (diff 3.65e-08, 857522503 ULP); 16 of 48 ticks differ; largest tick 34 position.x: 0.4969403 vs 0.49694037 (diff 5.96e-08, 2 ULP)
- `flat_odd_yaw_walk` bedsim vs bds: first tick 6 position.z: 0.57774866 vs 0.5777486 (diff 5.96e-08, 1 ULP); 35 of 45 ticks differ; largest tick 35 position.z: 5.4316206 vs 5.431618 (diff 2.38e-06, 5 ULP)
- `flat_odd_yaw_walk` unastar vs bedsim: first tick 6 position.z: 0.5777486 vs 0.57774866 (diff 5.96e-08, 1 ULP); 39 of 45 ticks differ; largest tick 35 position.z: 5.431618 vs 5.4316206 (diff 2.38e-06, 5 ULP)
- `flat_odd_yaw_walk` acacia vs bedsim: first tick 6 position.z: 0.5777486 vs 0.57774866 (diff 5.96e-08, 1 ULP); 39 of 45 ticks differ; largest tick 35 position.z: 5.431618 vs 5.4316206 (diff 2.38e-06, 5 ULP)
- `flat_sneak_diagonal` acacia vs bds: first tick 6 position.x: 0.5294 vs 0.52078897 (diff 0.00861, 144469 ULP); 35 of 40 ticks differ; largest tick 40 position.x: 2.1187599 vs 1.6446357 (diff 0.474, 3479127 ULP)
- `flat_sneak_diagonal` acacia vs bedsim: first tick 6 position.x: 0.5294 vs 0.52078897 (diff 0.00861, 144469 ULP); 35 of 40 ticks differ; largest tick 40 position.x: 2.1187599 vs 1.6446357 (diff 0.474, 3479127 ULP)
- `flat_sneak_diagonal` acacia vs unastar: first tick 6 position.x: 0.5294 vs 0.52078897 (diff 0.00861, 144469 ULP); 35 of 40 ticks differ; largest tick 40 position.x: 2.1187599 vs 1.6446357 (diff 0.474, 3479127 ULP)
- `flat_sprint` acacia vs bds: first tick 36 position.z: 8.832238 vs 8.861638 (diff 0.0294, 30828 ULP); 20 of 55 ticks differ; largest tick 55 position.z: 9.997767 vs 10.321549 (diff 0.324, 339510 ULP)
- `flat_sprint` acacia vs bedsim: first tick 36 position.z: 8.832238 vs 8.861638 (diff 0.0294, 30828 ULP); 20 of 55 ticks differ; largest tick 55 position.z: 9.997767 vs 10.321549 (diff 0.324, 339510 ULP)
- `flat_sprint` acacia vs unastar: first tick 36 position.z: 8.832238 vs 8.861638 (diff 0.0294, 30828 ULP); 20 of 55 ticks differ; largest tick 55 position.z: 9.997767 vs 10.321549 (diff 0.324, 339510 ULP)
- `flat_sprint_strafe` acacia vs bds: first tick 6 position.x: 0.4080761 vs 0.4099146 (diff 0.00184, 61690 ULP); 40 of 45 ticks differ; largest tick 44 position.x: -3.5495083 vs -3.4685178 (diff 0.081, 339699 ULP)
- `flat_sprint_strafe` acacia vs bedsim: first tick 6 position.x: 0.4080761 vs 0.4099146 (diff 0.00184, 61690 ULP); 40 of 45 ticks differ; largest tick 44 position.x: -3.5495083 vs -3.4685178 (diff 0.081, 339699 ULP)
- `flat_sprint_strafe` acacia vs unastar: first tick 6 position.x: 0.4080761 vs 0.4099146 (diff 0.00184, 61690 ULP); 40 of 45 ticks differ; largest tick 44 position.x: -3.5495083 vs -3.4685178 (diff 0.081, 339699 ULP)
- `flat_strafe_diagonal` acacia vs bds: first tick 6 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 45 of 50 ticks differ; largest tick 41 position.z: 3.614997 vs 3.552697 (diff 0.0623, 261305 ULP)
- `flat_strafe_diagonal` acacia vs bedsim: first tick 6 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 45 of 50 ticks differ; largest tick 41 position.z: 3.614997 vs 3.552697 (diff 0.0623, 261305 ULP)
- `flat_strafe_diagonal` acacia vs unastar: first tick 6 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 45 of 50 ticks differ; largest tick 41 position.z: 3.614997 vs 3.552697 (diff 0.0623, 261305 ULP)
- `flat_strafe_right_turning` bedsim vs bds: first tick 36 velocity.x: -0.001220761 vs -0.0012207612 (diff 2.33e-10, 2 ULP); 1 of 55 ticks differ; largest tick 36 velocity.x: -0.001220761 vs -0.0012207612 (diff 2.33e-10, 2 ULP)
- `flat_strafe_right_turning` unastar vs bedsim: first tick 13 velocity.x: -0.109139465 vs -0.10913947 (diff 7.45e-09, 1 ULP); 14 of 55 ticks differ; largest tick 13 velocity.x: -0.109139465 vs -0.10913947 (diff 7.45e-09, 1 ULP)
- `flat_strafe_right_turning` acacia vs bedsim: first tick 13 velocity.x: -0.109139465 vs -0.10913947 (diff 7.45e-09, 1 ULP); 14 of 55 ticks differ; largest tick 13 velocity.x: -0.109139465 vs -0.10913947 (diff 7.45e-09, 1 ULP)
- `flat_turning_walk` bedsim vs bds: first tick 9 position.x: 0.4267397 vs 0.42673963 (diff 5.96e-08, 2 ULP); 37 of 55 ticks differ; largest tick 24 position.z: 3.753367 vs 3.7533665 (diff 4.77e-07, 2 ULP)
- `flat_turning_walk` unastar vs bedsim: first tick 8 velocity.x: -0.012259178 vs -0.012259177 (diff 9.31e-10, 1 ULP); 45 of 55 ticks differ; largest tick 24 position.z: 3.7533665 vs 3.753367 (diff 4.77e-07, 2 ULP)
- `flat_turning_walk` acacia vs bedsim: first tick 8 velocity.x: -0.012259178 vs -0.012259177 (diff 9.31e-10, 1 ULP); 45 of 55 ticks differ; largest tick 24 position.z: 3.7533665 vs 3.753367 (diff 4.77e-07, 2 ULP)
- `glass_pane_post_diagonal` acacia vs bds: first tick 6 position.x: 0.45527864 vs 0.45617306 (diff 0.000894, 30012 ULP); 50 of 55 ticks differ; largest tick 55 position.z: 8.379858 vs 8.222265 (diff 0.158, 165248 ULP)
- `glass_pane_post_diagonal` acacia vs bedsim: first tick 6 position.x: 0.45527864 vs 0.45617306 (diff 0.000894, 30012 ULP); 50 of 55 ticks differ; largest tick 55 position.z: 8.379858 vs 8.222265 (diff 0.158, 165248 ULP)
- `glass_pane_post_diagonal` acacia vs unastar: first tick 6 position.x: 0.45527864 vs 0.45617306 (diff 0.000894, 30012 ULP); 50 of 55 ticks differ; largest tick 55 position.z: 8.379858 vs 8.222265 (diff 0.158, 165248 ULP)
- `glass_pane_walk_into` acacia vs bds: first tick 26 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 30 of 55 ticks differ; largest tick 55 position.x: 3.6145673 vs 3.5522764 (diff 0.0623, 261267 ULP)
- `glass_pane_walk_into` acacia vs bedsim: first tick 26 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 30 of 55 ticks differ; largest tick 55 position.x: 3.6145673 vs 3.5522764 (diff 0.0623, 261267 ULP)
- `glass_pane_walk_into` acacia vs unastar: first tick 26 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 30 of 55 ticks differ; largest tick 55 position.x: 3.6145673 vs 3.5522764 (diff 0.0623, 261267 ULP)
- `honey_walk_jump` acacia vs bds: first tick 48 velocity.z: 1.8068143e-08 vs 0 (diff 1.81e-08, 849032260 ULP); 1 of 51 ticks differ; largest tick 48 velocity.z: 1.8068143e-08 vs 0 (diff 1.81e-08, 849032260 ULP)
- `honey_walk_jump` acacia vs bedsim: first tick 48 velocity.z: 1.8068143e-08 vs 0 (diff 1.81e-08, 849032260 ULP); 2 of 51 ticks differ; largest tick 48 velocity.z: 1.8068143e-08 vs 0 (diff 1.81e-08, 849032260 ULP)
- `honey_walk_jump` acacia vs unastar: first tick 48 velocity.z: 1.8068143e-08 vs 0 (diff 1.81e-08, 849032260 ULP); 2 of 51 ticks differ; largest tick 48 velocity.z: 1.8068143e-08 vs 0 (diff 1.81e-08, 849032260 ULP)
- `honey_wall_slide` bedsim vs bds: first tick 10 position.z: 1.7 vs 1.7610168 (diff 0.061, 511846 ULP); 41 of 50 ticks differ; largest tick 16 position.y: 101.62001 vs 105.55373 (diff 3.93, 515600 ULP)
- `honey_wall_slide` unastar vs bedsim: first tick 10 position.z: 1.7610168 vs 1.7 (diff 0.061, 511846 ULP); 41 of 50 ticks differ; largest tick 16 position.y: 105.55373 vs 101.62001 (diff 3.93, 515600 ULP)
- `honey_wall_slide` acacia vs bedsim: first tick 10 position.z: 1.7610168 vs 1.7 (diff 0.061, 511846 ULP); 41 of 50 ticks differ; largest tick 16 position.y: 105.55373 vs 101.62001 (diff 3.93, 515600 ULP)
- `honey_wall_slide` acacia vs unastar: first tick 10 velocity.z: 0.0047685434 vs 0.012468742 (diff 0.0077, 11536449 ULP); 1 of 50 ticks differ; largest tick 10 velocity.z: 0.0047685434 vs 0.012468742 (diff 0.0077, 11536449 ULP)
- `ice_sneak_turn` bedsim vs bds: first tick 6 velocity.x: -0.00041973486 vs -0.00041973483 (diff 2.91e-11, 1 ULP); 43 of 60 ticks differ; largest tick 25 position.z: 1.113595 vs 1.1135949 (diff 1.19e-07, 1 ULP)
- `ice_sneak_turn` unastar vs bedsim: first tick 6 velocity.x: -0.00041973483 vs -0.00041973486 (diff 2.91e-11, 1 ULP); 49 of 60 ticks differ; largest tick 25 position.z: 1.1135949 vs 1.113595 (diff 1.19e-07, 1 ULP)
- `ice_sneak_turn` acacia vs bedsim: first tick 6 velocity.x: -0.00041973483 vs -0.00041973486 (diff 2.91e-11, 1 ULP); 49 of 60 ticks differ; largest tick 25 position.z: 1.1135949 vs 1.113595 (diff 1.19e-07, 1 ULP)
- `ice_sprint_jump` bedsim vs bds: first tick 6 velocity.z: 0.026074294 vs 0.026074296 (diff 1.86e-09, 1 ULP); 22 of 60 ticks differ; largest tick 34 position.z: 8.179492 vs 8.179493 (diff 9.54e-07, 1 ULP)
- `ice_sprint_jump` acacia vs bds: first tick 6 velocity.z: 0.026074294 vs 0.026074296 (diff 1.86e-09, 1 ULP); 22 of 60 ticks differ; largest tick 34 position.z: 8.179492 vs 8.179493 (diff 9.54e-07, 1 ULP)
- `ice_sprint_jump` unastar vs bedsim: first tick 6 velocity.z: 0.026074296 vs 0.026074294 (diff 1.86e-09, 1 ULP); 28 of 60 ticks differ; largest tick 34 position.z: 8.179493 vs 8.179492 (diff 9.54e-07, 1 ULP)
- `ice_sprint_jump` acacia vs unastar: first tick 6 velocity.z: 0.026074294 vs 0.026074296 (diff 1.86e-09, 1 ULP); 28 of 60 ticks differ; largest tick 34 position.z: 8.179492 vs 8.179493 (diff 9.54e-07, 1 ULP)
- `iron_bars_walk_into` acacia vs bds: first tick 26 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 30 of 55 ticks differ; largest tick 55 position.x: 3.6145673 vs 3.5522764 (diff 0.0623, 261267 ULP)
- `iron_bars_walk_into` acacia vs bedsim: first tick 26 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 30 of 55 ticks differ; largest tick 55 position.x: 3.6145673 vs 3.5522764 (diff 0.0623, 261267 ULP)
- `iron_bars_walk_into` acacia vs unastar: first tick 26 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 30 of 55 ticks differ; largest tick 55 position.x: 3.6145673 vs 3.5522764 (diff 0.0623, 261267 ULP)
- `knockback_air` unastar vs bedsim: first tick 45 velocity.z: -0 vs 0 (diff 0, 0 ULP); 2 of 46 ticks differ; largest tick 45 velocity.z: -0 vs 0 (diff 0, 0 ULP)
- `knockback_air` acacia vs bedsim: first tick 45 velocity.x: 3.9558667e-08 vs 0 (diff 3.96e-08, 858384183 ULP); 2 of 46 ticks differ; largest tick 45 velocity.z: -4.2195907e-08 vs 0 (diff 4.22e-08, 859126500 ULP)
- `knockback_air` acacia vs unastar: first tick 45 velocity.x: 3.9558667e-08 vs 0 (diff 3.96e-08, 858384183 ULP); 2 of 46 ticks differ; largest tick 45 velocity.z: -4.2195907e-08 vs -0 (diff 4.22e-08, 859126500 ULP)
- `knockback_ground` bedsim vs bds: first tick 43 velocity.z: 0 vs -0 (diff 0, 0 ULP); 1 of 45 ticks differ; largest tick 43 velocity.z: 0 vs -0 (diff 0, 0 ULP)
- `knockback_ground` acacia vs bds: first tick 41 position.z: -1.0094237 vs -1.0094236 (diff 1.19e-07, 1 ULP); 5 of 45 ticks differ; largest tick 41 position.z: -1.0094237 vs -1.0094236 (diff 1.19e-07, 1 ULP)
- `knockback_ground` unastar vs bedsim: first tick 39 velocity.z: -0 vs 0 (diff 0, 0 ULP); 7 of 45 ticks differ; largest tick 39 velocity.z: -0 vs 0 (diff 0, 0 ULP)
- `knockback_ground` acacia vs bedsim: first tick 39 velocity.x: 5.249494e-08 vs 0 (diff 5.25e-08, 862025420 ULP); 7 of 45 ticks differ; largest tick 41 position.z: -1.0094237 vs -1.0094236 (diff 1.19e-07, 1 ULP)
- `knockback_ground` acacia vs unastar: first tick 39 velocity.x: 5.249494e-08 vs 0 (diff 5.25e-08, 862025420 ULP); 7 of 45 ticks differ; largest tick 41 position.z: -1.0094237 vs -1.0094236 (diff 1.19e-07, 1 ULP)
- `knockback_sprint_air` unastar vs bds: first tick 16 position.z: 1.7862655 vs 1.7862654 (diff 1.19e-07, 1 ULP); 9 of 50 ticks differ; largest tick 18 position.z: 1.7922325 vs 1.7922323 (diff 2.38e-07, 2 ULP)
- `knockback_sprint_air` bedsim vs bds: first tick 16 position.z: 1.7862655 vs 1.7862654 (diff 1.19e-07, 1 ULP); 9 of 50 ticks differ; largest tick 18 position.z: 1.7922325 vs 1.7922323 (diff 2.38e-07, 2 ULP)
- `knockback_sprint_air` acacia vs bds: first tick 16 position.z: 1.7862655 vs 1.7862654 (diff 1.19e-07, 1 ULP); 9 of 50 ticks differ; largest tick 18 position.z: 1.7922325 vs 1.7922323 (diff 2.38e-07, 2 ULP)
- `knockback_water` unastar vs bedsim: first tick 44 velocity.y: -0.0071423342 vs -0.005 (diff 0.00214, 4600628 ULP); 1 of 50 ticks differ; largest tick 44 velocity.y: -0.0071423342 vs -0.005 (diff 0.00214, 4600628 ULP)
- `knockback_water` acacia vs unastar: first tick 44 velocity.y: -0.005 vs -0.0071423342 (diff 0.00214, 4600628 ULP); 1 of 50 ticks differ; largest tick 44 velocity.y: -0.005 vs -0.0071423342 (diff 0.00214, 4600628 ULP)
- `lava_fall_in` unastar vs bedsim: first tick 13 velocity.y: -0.0977626 vs -0.02 (diff 0.0778, 19161272 ULP); 1 of 40 ticks differ; largest tick 13 velocity.y: -0.0977626 vs -0.02 (diff 0.0778, 19161272 ULP)
- `lava_fall_in` acacia vs unastar: first tick 13 velocity.y: -0.02 vs -0.0977626 (diff 0.0778, 19161272 ULP); 1 of 40 ticks differ; largest tick 13 velocity.y: -0.02 vs -0.0977626 (diff 0.0778, 19161272 ULP)
- `lava_walk` bedsim vs bds: first tick 48 velocity.z: 0 vs 2.3364999e-09 (diff 2.34e-09, 824217636 ULP); 2 of 55 ticks differ; largest tick 48 velocity.z: 0 vs 2.3364999e-09 (diff 2.34e-09, 824217636 ULP)
- `lava_walk` acacia vs bds: first tick 48 velocity.z: 0 vs 2.3364999e-09 (diff 2.34e-09, 824217636 ULP); 2 of 55 ticks differ; largest tick 48 velocity.z: 0 vs 2.3364999e-09 (diff 2.34e-09, 824217636 ULP)
- `lava_walk` unastar vs bedsim: first tick 47 velocity.z: 4.6729998e-09 vs 0 (diff 4.67e-09, 832606244 ULP); 9 of 55 ticks differ; largest tick 47 velocity.z: 4.6729998e-09 vs 0 (diff 4.67e-09, 832606244 ULP)
- `lava_walk` acacia vs unastar: first tick 47 velocity.z: 0 vs 4.6729998e-09 (diff 4.67e-09, 832606244 ULP); 9 of 55 ticks differ; largest tick 47 velocity.z: 0 vs 4.6729998e-09 (diff 4.67e-09, 832606244 ULP)
- `ledge_sneak` acacia vs bds: first tick 46 position.x: 0.5294 vs 0.52078897 (diff 0.00861, 144469 ULP); 30 of 75 ticks differ; largest tick 75 position.y: 98.92545 vs 101.62001 (diff 2.69, 353181 ULP)
- `ledge_sneak` acacia vs bedsim: first tick 46 position.x: 0.5294 vs 0.52078897 (diff 0.00861, 144469 ULP); 30 of 75 ticks differ; largest tick 75 position.y: 98.92545 vs 101.62001 (diff 2.69, 353181 ULP)
- `ledge_sneak` acacia vs unastar: first tick 46 position.x: 0.5294 vs 0.52078897 (diff 0.00861, 144469 ULP); 30 of 75 ticks differ; largest tick 75 position.y: 98.92545 vs 101.62001 (diff 2.69, 353181 ULP)
- `levitation_ceiling` bedsim vs bds: first tick 1 velocity.y: 0.020000001 vs 0.0196 (diff 0.0004, 214749 ULP); 24 of 55 ticks differ; largest tick 16 position.y: 102.73407 vs 102.66033 (diff 0.0737, 9665 ULP)
- `levitation_ceiling` acacia vs bds: first tick 1 velocity.y: 0.019600002 vs 0.0196 (diff 1.86e-09, 1 ULP); 9 of 55 ticks differ; largest tick 13 velocity.y: 0.086904466 vs 0.08690447 (diff 7.45e-09, 1 ULP)
- `levitation_ceiling` unastar vs bedsim: first tick 1 velocity.y: 0.0196 vs 0.020000001 (diff 0.0004, 214749 ULP); 55 of 55 ticks differ; largest tick 16 position.y: 102.66033 vs 102.73407 (diff 0.0737, 9665 ULP)
- `levitation_ceiling` acacia vs bedsim: first tick 1 velocity.y: 0.019600002 vs 0.020000001 (diff 0.0004, 214748 ULP); 55 of 55 ticks differ; largest tick 16 position.y: 102.66033 vs 102.73407 (diff 0.0737, 9665 ULP)
- `levitation_ceiling` acacia vs unastar: first tick 1 velocity.y: 0.019600002 vs 0.0196 (diff 1.86e-09, 1 ULP); 51 of 55 ticks differ; largest tick 11 velocity.y: 0.0844994 vs 0.08449941 (diff 1.49e-08, 2 ULP)
- `levitation_float` bedsim vs bds: first tick 1 on_ground: True vs False; 65 of 65 ticks differ; largest tick 65 position.y: 104.620026 vs 104.35908 (diff 0.261, 34203 ULP)
- `levitation_float` acacia vs bds: first tick 1 on_ground: True vs False; 9 of 65 ticks differ; largest tick 11 velocity.y: 0.0422497 vs 0.042249706 (diff 7.45e-09, 2 ULP)
- `levitation_float` unastar vs bedsim: first tick 1 velocity.y: 0.0098 vs 0.010000001 (diff 0.0002, 214749 ULP); 65 of 65 ticks differ; largest tick 65 position.y: 104.35908 vs 104.620026 (diff 0.261, 34203 ULP)
- `levitation_float` acacia vs bedsim: first tick 1 velocity.y: 0.009800001 vs 0.010000001 (diff 0.0002, 214748 ULP); 65 of 65 ticks differ; largest tick 65 position.y: 104.35908 vs 104.620026 (diff 0.261, 34203 ULP)
- `levitation_float` acacia vs unastar: first tick 1 velocity.y: 0.009800001 vs 0.0098 (diff 9.31e-10, 1 ULP); 40 of 65 ticks differ; largest tick 11 velocity.y: 0.0422497 vs 0.042249706 (diff 7.45e-09, 2 ULP)
- `levitation_jump` bedsim vs bds: first tick 1 on_ground: True vs False; 36 of 36 ticks differ; largest tick 36 position.y: 103.170074 vs 103.04333 (diff 0.127, 16613 ULP)
- `levitation_jump` acacia vs bds: first tick 1 on_ground: True vs False; 4 of 36 ticks differ; largest tick 10 velocity.y: 0.04138992 vs 0.041389924 (diff 3.73e-09, 1 ULP)
- `levitation_jump` unastar vs bedsim: first tick 1 velocity.y: 0.0098 vs 0.010000001 (diff 0.0002, 214749 ULP); 36 of 36 ticks differ; largest tick 36 position.y: 103.04333 vs 103.170074 (diff 0.127, 16613 ULP)
- `levitation_jump` acacia vs bedsim: first tick 1 velocity.y: 0.009800001 vs 0.010000001 (diff 0.0002, 214748 ULP); 36 of 36 ticks differ; largest tick 36 position.y: 103.04333 vs 103.170074 (diff 0.127, 16613 ULP)
- `levitation_jump` acacia vs unastar: first tick 1 velocity.y: 0.009800001 vs 0.0098 (diff 9.31e-10, 1 ULP); 20 of 36 ticks differ; largest tick 11 velocity.y: 0.0422497 vs 0.042249706 (diff 7.45e-09, 2 ULP)
- `packed_ice_sprint_jump` bedsim vs bds: first tick 30 velocity.z: 0.4846741 vs 0.48467404 (diff 5.96e-08, 2 ULP); 23 of 60 ticks differ; largest tick 46 position.z: 16.017803 vs 16.017801 (diff 1.91e-06, 1 ULP)
- `packed_ice_sprint_jump` acacia vs bds: first tick 30 velocity.z: 0.4846741 vs 0.48467404 (diff 5.96e-08, 2 ULP); 23 of 60 ticks differ; largest tick 46 position.z: 16.017803 vs 16.017801 (diff 1.91e-06, 1 ULP)
- `packed_ice_sprint_jump` unastar vs bedsim: first tick 30 velocity.z: 0.48467404 vs 0.4846741 (diff 5.96e-08, 2 ULP); 31 of 60 ticks differ; largest tick 46 position.z: 16.017801 vs 16.017803 (diff 1.91e-06, 1 ULP)
- `packed_ice_sprint_jump` acacia vs unastar: first tick 30 velocity.z: 0.4846741 vs 0.48467404 (diff 5.96e-08, 2 ULP); 31 of 60 ticks differ; largest tick 46 position.z: 16.017803 vs 16.017801 (diff 1.91e-06, 1 ULP)
- `powder_snow_fall` bedsim vs bds: first tick 13 position.y: 103.62001 vs 103.52001 (diff 0.1, 13107 ULP); 38 of 50 ticks differ; largest tick 30 position.y: 103.62001 vs 101.62001 (diff 2, 262144 ULP)
- `powder_snow_fall` acacia vs bds: first tick 13 position.y: 102.931625 vs 103.52001 (diff 0.588, 77121 ULP); 17 of 50 ticks differ; largest tick 14 position.y: 101.62001 vs 103.40241 (diff 1.78, 233623 ULP)
- `powder_snow_fall` unastar vs bedsim: first tick 13 position.y: 103.52001 vs 103.62001 (diff 0.1, 13107 ULP); 38 of 50 ticks differ; largest tick 30 position.y: 101.62001 vs 103.62001 (diff 2, 262144 ULP)
- `powder_snow_fall` acacia vs bedsim: first tick 13 position.y: 102.931625 vs 103.62001 (diff 0.688, 90228 ULP); 38 of 50 ticks differ; largest tick 14 position.y: 101.62001 vs 103.62001 (diff 2, 262144 ULP)
- `powder_snow_fall` acacia vs unastar: first tick 13 position.y: 102.931625 vs 103.52001 (diff 0.588, 77121 ULP); 17 of 50 ticks differ; largest tick 14 position.y: 101.62001 vs 103.40241 (diff 1.78, 233623 ULP)
- `powder_snow_jump` bedsim vs bds: first tick 1 position.x: 1.3 vs 0.5 (diff 0.8, 10905190 ULP); 45 of 45 ticks differ; largest tick 1 position.x: 1.3 vs 0.5 (diff 0.8, 10905190 ULP)
- `powder_snow_jump` unastar vs bedsim: first tick 1 position.x: 0.5 vs 1.3 (diff 0.8, 10905190 ULP); 45 of 45 ticks differ; largest tick 1 position.x: 0.5 vs 1.3 (diff 0.8, 10905190 ULP)
- `powder_snow_jump` acacia vs bedsim: first tick 1 position.x: 0.5 vs 1.3 (diff 0.8, 10905190 ULP); 45 of 45 ticks differ; largest tick 1 position.x: 0.5 vs 1.3 (diff 0.8, 10905190 ULP)
- `powder_snow_walk_into` unastar vs bds: first tick 18 position.z: 3.0466661 vs 3.0247734 (diff 0.0219, 91825 ULP); 38 of 55 ticks differ; largest tick 45 position.z: 5.5341005 vs 5.278284 (diff 0.256, 536486 ULP)
- `powder_snow_walk_into` bedsim vs bds: first tick 17 position.z: 2.7 vs 2.8308897 (diff 0.131, 548991 ULP); 39 of 55 ticks differ; largest tick 45 position.z: 2.7 vs 5.278284 (diff 2.58, 8133351 ULP)
- `powder_snow_walk_into` acacia vs bds: first tick 38 position.z: 4.716324 vs 4.716009 (diff 0.000315, 660 ULP); 18 of 55 ticks differ; largest tick 38 position.z: 4.716324 vs 4.716009 (diff 0.000315, 660 ULP)
- `powder_snow_walk_into` unastar vs bedsim: first tick 17 position.z: 2.8308897 vs 2.7 (diff 0.131, 548991 ULP); 39 of 55 ticks differ; largest tick 45 position.z: 5.5341005 vs 2.7 (diff 2.83, 8669837 ULP)
- `powder_snow_walk_into` acacia vs bedsim: first tick 17 position.z: 2.8308897 vs 2.7 (diff 0.131, 548991 ULP); 39 of 55 ticks differ; largest tick 45 position.z: 5.278599 vs 2.7 (diff 2.58, 8134011 ULP)
- `powder_snow_walk_into` acacia vs unastar: first tick 18 position.z: 3.0247734 vs 3.0466661 (diff 0.0219, 91825 ULP); 38 of 55 ticks differ; largest tick 45 position.z: 5.278599 vs 5.5341005 (diff 0.256, 535826 ULP)
- `random_135` bedsim vs bds: first tick 6 velocity.z: 0.117241286 vs 0.1172413 (diff 1.49e-08, 2 ULP); 25 of 45 ticks differ; largest tick 45 position.x: 1.6119807 vs 1.469682 (diff 0.142, 1193688 ULP)
- `random_135` acacia vs bds: first tick 4 position.x: 0.5438208 vs 0.5429444 (diff 0.000876, 14704 ULP); 42 of 45 ticks differ; largest tick 43 position.z: -1.4736526 vs -1.4970133 (diff 0.0234, 195964 ULP)
- `random_135` unastar vs bedsim: first tick 6 velocity.z: 0.1172413 vs 0.117241286 (diff 1.49e-08, 2 ULP); 39 of 45 ticks differ; largest tick 45 position.x: 1.469682 vs 1.6119807 (diff 0.142, 1193688 ULP)
- `random_135` acacia vs bedsim: first tick 4 position.x: 0.5438208 vs 0.5429444 (diff 0.000876, 14704 ULP); 42 of 45 ticks differ; largest tick 45 position.x: 1.4796314 vs 1.6119807 (diff 0.132, 1110226 ULP)
- `random_135` acacia vs unastar: first tick 4 position.x: 0.5438208 vs 0.5429444 (diff 0.000876, 14704 ULP); 42 of 45 ticks differ; largest tick 43 position.z: -1.4736526 vs -1.4970133 (diff 0.0234, 195964 ULP)
- `random_14` unastar vs bds: first tick 54 velocity.x: -0.061980594 vs -0.06198058 (diff 1.49e-08, 4 ULP); 23 of 87 ticks differ; largest tick 67 position.z: -3.191203 vs -3.1912036 (diff 4.77e-07, 2 ULP)
- `random_14` bedsim vs bds: first tick 2 position.y: 101.62001 vs 101.54161 (diff 0.0784, 10276 ULP); 86 of 87 ticks differ; largest tick 3 position.y: 101.62001 vs 101.49501 (diff 0.125, 16384 ULP)
- `random_14` acacia vs bds: first tick 31 position.x: 0.80518925 vs 0.80529284 (diff 0.000104, 1738 ULP); 57 of 87 ticks differ; largest tick 87 position.x: -2.9781826 vs -4.5773907 (diff 1.6, 5496689 ULP)
- `random_14` unastar vs bedsim: first tick 2 position.y: 101.54161 vs 101.62001 (diff 0.0784, 10276 ULP); 86 of 87 ticks differ; largest tick 3 position.y: 101.49501 vs 101.62001 (diff 0.125, 16384 ULP)
- `random_14` acacia vs bedsim: first tick 2 position.y: 101.54161 vs 101.62001 (diff 0.0784, 10276 ULP); 86 of 87 ticks differ; largest tick 87 position.x: -2.9781826 vs -4.6799517 (diff 1.7, 5711775 ULP)
- `random_14` acacia vs unastar: first tick 31 position.x: 0.80518925 vs 0.80529284 (diff 0.000104, 1738 ULP); 57 of 87 ticks differ; largest tick 87 position.x: -2.9781826 vs -4.5773907 (diff 1.6, 5496689 ULP)
- `random_66` unastar vs bds: first tick 12 position.x: 0.4913932 vs 0.497418 (diff 0.00602, 202158 ULP); 70 of 81 ticks differ; largest tick 81 position.z: 7.0467353 vs 6.9122996 (diff 0.134, 281932 ULP)
- `random_66` bedsim vs bds: first tick 12 position.x: 0.4913932 vs 0.497418 (diff 0.00602, 202158 ULP); 70 of 81 ticks differ; largest tick 81 position.z: 7.0467353 vs 6.9122996 (diff 0.134, 281932 ULP)
- `random_66` acacia vs bds: first tick 60 position.x: -2.4266887 vs -2.4263258 (diff 0.000363, 1522 ULP); 22 of 81 ticks differ; largest tick 81 position.z: 6.6787534 vs 6.9122996 (diff 0.234, 489782 ULP)
- `random_66` unastar vs bedsim: first tick 36 velocity.z: 0.011441614 vs 0.011441613 (diff 9.31e-10, 1 ULP); 33 of 81 ticks differ; largest tick 48 position.z: 0.296633 vs 0.29663286 (diff 1.49e-07, 5 ULP)
- `random_66` acacia vs bedsim: first tick 12 position.x: 0.497418 vs 0.4913932 (diff 0.00602, 202158 ULP); 70 of 81 ticks differ; largest tick 81 position.z: 6.6787534 vs 7.0467353 (diff 0.368, 771714 ULP)
- `random_66` acacia vs unastar: first tick 12 position.x: 0.497418 vs 0.4913932 (diff 0.00602, 202158 ULP); 70 of 81 ticks differ; largest tick 81 position.z: 6.6787534 vs 7.0467353 (diff 0.368, 771714 ULP)
- `scaffolding_climb` bedsim vs bds: first tick 1 position.z: 3.3 vs 2.5 (diff 0.8, 3355443 ULP); 75 of 75 ticks differ; largest tick 40 position.y: 101.741295 vs 105.37005 (diff 3.63, 475628 ULP)
- `scaffolding_climb` acacia vs bds: first tick 36 velocity.y: -0.15 vs -0.14700001 (diff 0.003, 201326 ULP); 9 of 75 ticks differ; largest tick 63 position.y: 101.92001 vs 101.62001 (diff 0.3, 39322 ULP)
- `scaffolding_climb` unastar vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 75 of 75 ticks differ; largest tick 40 position.y: 105.37005 vs 101.741295 (diff 3.63, 475628 ULP)
- `scaffolding_climb` acacia vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 75 of 75 ticks differ; largest tick 40 position.y: 105.37005 vs 101.741295 (diff 3.63, 475628 ULP)
- `scaffolding_climb` acacia vs unastar: first tick 36 velocity.y: -0.15 vs -0.14700001 (diff 0.003, 201326 ULP); 30 of 75 ticks differ; largest tick 63 position.y: 101.92001 vs 101.62001 (diff 0.3, 39322 ULP)
- `scaffolding_sneak_down` bedsim vs bds: first tick 6 position.y: 106.62001 vs 106.47001 (diff 0.15, 19661 ULP); 50 of 55 ticks differ; largest tick 36 position.y: 106.62001 vs 101.62001 (diff 5, 655360 ULP)
- `scaffolding_sneak_down` acacia vs bds: first tick 2 position.y: 106.54161 vs 106.62001 (diff 0.0784, 10276 ULP); 34 of 55 ticks differ; largest tick 33 position.y: 101.65149 vs 102.422966 (diff 0.771, 101119 ULP)
- `scaffolding_sneak_down` unastar vs bedsim: first tick 6 position.y: 106.47001 vs 106.62001 (diff 0.15, 19661 ULP); 50 of 55 ticks differ; largest tick 36 position.y: 101.62001 vs 106.62001 (diff 5, 655360 ULP)
- `scaffolding_sneak_down` acacia vs bedsim: first tick 2 position.y: 106.54161 vs 106.62001 (diff 0.0784, 10276 ULP); 54 of 55 ticks differ; largest tick 34 position.y: 101.62001 vs 106.62001 (diff 5, 655360 ULP)
- `scaffolding_sneak_down` acacia vs unastar: first tick 2 position.y: 106.54161 vs 106.62001 (diff 0.0784, 10276 ULP); 34 of 55 ticks differ; largest tick 33 position.y: 101.65149 vs 102.422966 (diff 0.771, 101119 ULP)
- `scaffolding_walk_into` bedsim vs bds: first tick 12 position.z: 1.7 vs 1.7551675 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 50 position.z: 1.7 vs 7.8764234 (diff 6.18, 19034639 ULP)
- `scaffolding_walk_into` unastar vs bedsim: first tick 12 position.z: 1.7551675 vs 1.7 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 50 position.z: 7.8764234 vs 1.7 (diff 6.18, 19034639 ULP)
- `scaffolding_walk_into` acacia vs bedsim: first tick 12 position.z: 1.7551675 vs 1.7 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 50 position.z: 7.8764234 vs 1.7 (diff 6.18, 19034639 ULP)
- `scaffolding_walk_on_top` bedsim vs bds: first tick 18 position.z: 2.6999998 vs 2.8897486 (diff 0.19, 795864 ULP); 28 of 45 ticks differ; largest tick 45 position.z: 2.6999998 vs 6.8181877 (diff 4.12, 11362764 ULP)
- `scaffolding_walk_on_top` unastar vs bedsim: first tick 18 position.z: 2.8897486 vs 2.6999998 (diff 0.19, 795864 ULP); 28 of 45 ticks differ; largest tick 45 position.z: 6.8181877 vs 2.6999998 (diff 4.12, 11362764 ULP)
- `scaffolding_walk_on_top` acacia vs bedsim: first tick 18 position.z: 2.8897486 vs 2.6999998 (diff 0.19, 795864 ULP); 28 of 45 ticks differ; largest tick 45 position.z: 6.8181877 vs 2.6999998 (diff 4.12, 11362764 ULP)
- `slime_bounce` bedsim vs bds: first tick 14 position.y: 102.368645 vs 102.382965 (diff 0.0143, 1877 ULP); 67 of 80 ticks differ; largest tick 74 position.y: 101.62001 vs 103.16077 (diff 1.54, 201950 ULP)
- `slime_bounce` unastar vs bedsim: first tick 13 velocity.y: 0.7629567 vs 0.74863225 (diff 0.0143, 240324 ULP); 68 of 80 ticks differ; largest tick 74 position.y: 103.16077 vs 101.62001 (diff 1.54, 201950 ULP)
- `slime_bounce` acacia vs bedsim: first tick 13 velocity.y: 0.7629567 vs 0.74863225 (diff 0.0143, 240324 ULP); 68 of 80 ticks differ; largest tick 74 position.y: 103.16077 vs 101.62001 (diff 1.54, 201950 ULP)
- `slow_falling_fall` acacia vs bds: first tick 90 velocity.z: 3.170922e-08 vs 0 (diff 3.17e-08, 856174760 ULP); 1 of 105 ticks differ; largest tick 90 velocity.z: 3.170922e-08 vs 0 (diff 3.17e-08, 856174760 ULP)
- `slow_falling_fall` acacia vs bedsim: first tick 89 velocity.z: 5.807549e-08 vs 0 (diff 5.81e-08, 863596205 ULP); 4 of 105 ticks differ; largest tick 89 velocity.z: 5.807549e-08 vs 0 (diff 5.81e-08, 863596205 ULP)
- `slow_falling_fall` acacia vs unastar: first tick 89 velocity.z: 5.807549e-08 vs 0 (diff 5.81e-08, 863596205 ULP); 4 of 105 ticks differ; largest tick 89 velocity.z: 5.807549e-08 vs 0 (diff 5.81e-08, 863596205 ULP)
- `sneak_off_carpet_diagonal` acacia vs bds: first tick 6 position.x: 0.4706 vs 0.47921103 (diff 0.00861, 288938 ULP); 45 of 50 ticks differ; largest tick 50 position.z: 3.0865314 vs 2.3289533 (diff 0.758, 3177513 ULP)
- `sneak_off_carpet_diagonal` acacia vs bedsim: first tick 6 position.x: 0.4706 vs 0.47921103 (diff 0.00861, 288938 ULP); 45 of 50 ticks differ; largest tick 50 position.z: 3.0865314 vs 2.3289533 (diff 0.758, 3177513 ULP)
- `sneak_off_carpet_diagonal` acacia vs unastar: first tick 6 position.x: 0.4706 vs 0.47921103 (diff 0.00861, 288938 ULP); 45 of 50 ticks differ; largest tick 50 position.z: 3.0865314 vs 2.3289533 (diff 0.758, 3177513 ULP)
- `sneak_off_fence` acacia vs bds: first tick 60 velocity.z: 9.512652e-09 vs 0 (diff 9.51e-09, 841182486 ULP); 1 of 60 ticks differ; largest tick 60 velocity.z: 9.512652e-09 vs 0 (diff 9.51e-09, 841182486 ULP)
- `sneak_off_fence` unastar vs bedsim: first tick 43 velocity.x: 0.00777845 vs 0.035078455 (diff 0.0273, 17878031 ULP); 1 of 60 ticks differ; largest tick 43 velocity.x: 0.00777845 vs 0.035078455 (diff 0.0273, 17878031 ULP)
- `sneak_off_fence` acacia vs bedsim: first tick 57 velocity.z: 5.844179e-08 vs 0 (diff 5.84e-08, 863699309 ULP); 4 of 60 ticks differ; largest tick 57 velocity.z: 5.844179e-08 vs 0 (diff 5.84e-08, 863699309 ULP)
- `sneak_off_fence` acacia vs unastar: first tick 43 velocity.x: 0.035078455 vs 0.00777845 (diff 0.0273, 17878031 ULP); 5 of 60 ticks differ; largest tick 43 velocity.x: 0.035078455 vs 0.00777845 (diff 0.0273, 17878031 ULP)
- `sneak_off_trapdoor` bedsim vs bds: first tick 2 position.y: 101.80751 vs 101.80251 (diff 0.005, 655 ULP); 49 of 50 ticks differ; largest tick 2 position.y: 101.80751 vs 101.80251 (diff 0.005, 655 ULP)
- `sneak_off_trapdoor` unastar vs bedsim: first tick 2 position.y: 101.80251 vs 101.80751 (diff 0.005, 655 ULP); 49 of 50 ticks differ; largest tick 2 position.y: 101.80251 vs 101.80751 (diff 0.005, 655 ULP)
- `sneak_off_trapdoor` acacia vs bedsim: first tick 2 position.y: 101.80251 vs 101.80751 (diff 0.005, 655 ULP); 49 of 50 ticks differ; largest tick 2 position.y: 101.80251 vs 101.80751 (diff 0.005, 655 ULP)
- `snow_layer_1_walk` bedsim vs bds: first tick 17 position.z: 2.7 vs 2.8308897 (diff 0.131, 548991 ULP); 44 of 60 ticks differ; largest tick 60 position.z: 2.7 vs 10.213049 (diff 7.51, 16161753 ULP)
- `snow_layer_1_walk` unastar vs bedsim: first tick 17 position.z: 2.8308897 vs 2.7 (diff 0.131, 548991 ULP); 44 of 60 ticks differ; largest tick 60 position.z: 10.213049 vs 2.7 (diff 7.51, 16161753 ULP)
- `snow_layer_1_walk` acacia vs bedsim: first tick 17 position.z: 2.8308897 vs 2.7 (diff 0.131, 548991 ULP); 44 of 60 ticks differ; largest tick 60 position.z: 10.213049 vs 2.7 (diff 7.51, 16161753 ULP)
- `snow_layer_4_walk` bedsim vs bds: first tick 17 position.y: 101.62001 vs 101.99501 (diff 0.375, 49152 ULP); 44 of 60 ticks differ; largest tick 60 position.z: 2.7 vs 10.093885 (diff 7.39, 16036801 ULP)
- `snow_layer_4_walk` unastar vs bedsim: first tick 17 position.y: 101.99501 vs 101.62001 (diff 0.375, 49152 ULP); 44 of 60 ticks differ; largest tick 60 position.z: 10.093885 vs 2.7 (diff 7.39, 16036801 ULP)
- `snow_layer_4_walk` acacia vs bedsim: first tick 17 position.y: 101.99501 vs 101.62001 (diff 0.375, 49152 ULP); 44 of 60 ticks differ; largest tick 60 position.z: 10.093885 vs 2.7 (diff 7.39, 16036801 ULP)
- `soul_sand_sprint_jump` bedsim vs bds: first tick 2 position.y: 101.62001 vs 101.54161 (diff 0.0784, 10276 ULP); 54 of 55 ticks differ; largest tick 3 position.y: 101.62001 vs 101.49501 (diff 0.125, 16384 ULP)
- `soul_sand_sprint_jump` unastar vs bedsim: first tick 2 position.y: 101.54161 vs 101.62001 (diff 0.0784, 10276 ULP); 54 of 55 ticks differ; largest tick 3 position.y: 101.49501 vs 101.62001 (diff 0.125, 16384 ULP)
- `soul_sand_sprint_jump` acacia vs bedsim: first tick 2 position.y: 101.54161 vs 101.62001 (diff 0.0784, 10276 ULP); 54 of 55 ticks differ; largest tick 3 position.y: 101.49501 vs 101.62001 (diff 0.125, 16384 ULP)
- `soul_sand_walk` bedsim vs bds: first tick 2 position.y: 101.62001 vs 101.54161 (diff 0.0784, 10276 ULP); 44 of 45 ticks differ; largest tick 3 position.y: 101.62001 vs 101.49501 (diff 0.125, 16384 ULP)
- `soul_sand_walk` unastar vs bedsim: first tick 2 position.y: 101.54161 vs 101.62001 (diff 0.0784, 10276 ULP); 44 of 45 ticks differ; largest tick 3 position.y: 101.49501 vs 101.62001 (diff 0.125, 16384 ULP)
- `soul_sand_walk` acacia vs bedsim: first tick 2 position.y: 101.54161 vs 101.62001 (diff 0.0784, 10276 ULP); 44 of 45 ticks differ; largest tick 3 position.y: 101.49501 vs 101.62001 (diff 0.125, 16384 ULP)
- `sprint_jump` acacia vs bds: first tick 46 position.z: 13.402407 vs 13.408287 (diff 0.00588, 6166 ULP); 15 of 60 ticks differ; largest tick 60 position.z: 15.400084 vs 15.545353 (diff 0.145, 152326 ULP)
- `sprint_jump` acacia vs bedsim: first tick 46 position.z: 13.402407 vs 13.408287 (diff 0.00588, 6166 ULP); 15 of 60 ticks differ; largest tick 60 position.z: 15.400084 vs 15.545353 (diff 0.145, 152326 ULP)
- `sprint_jump` acacia vs unastar: first tick 46 position.z: 13.402407 vs 13.408287 (diff 0.00588, 6166 ULP); 15 of 60 ticks differ; largest tick 60 position.z: 15.400084 vs 15.545353 (diff 0.145, 152326 ULP)
- `sprint_sneak_toggle` bedsim vs bds: first tick 31 position.z: 5.4413733 vs 5.3433733 (diff 0.098, 205521 ULP); 25 of 55 ticks differ; largest tick 50 position.z: 9.5497055 vs 9.333849 (diff 0.216, 226342 ULP)
- `sprint_sneak_toggle` unastar vs bedsim: first tick 31 position.z: 5.3433733 vs 5.4413733 (diff 0.098, 205521 ULP); 25 of 55 ticks differ; largest tick 50 position.z: 9.333849 vs 9.5497055 (diff 0.216, 226342 ULP)
- `sprint_sneak_toggle` acacia vs bedsim: first tick 31 position.z: 5.3433733 vs 5.4413733 (diff 0.098, 205521 ULP); 25 of 55 ticks differ; largest tick 50 position.z: 9.333849 vs 9.5497055 (diff 0.216, 226342 ULP)
- `sprint_turn_jump` unastar vs bds: first tick 17 position.x: -0.22081888 vs -0.22081886 (diff 1.49e-08, 1 ULP); 38 of 55 ticks differ; largest tick 32 position.x: -4.525616 vs -4.5256147 (diff 1.43e-06, 3 ULP)
- `sprint_turn_jump` bedsim vs bds: first tick 10 position.x: 0.35615793 vs 0.35615796 (diff 2.98e-08, 1 ULP); 45 of 55 ticks differ; largest tick 32 position.x: -4.5256166 vs -4.5256147 (diff 1.91e-06, 4 ULP)
- `sprint_turn_jump` acacia vs bds: first tick 17 position.x: -0.22081888 vs -0.22081886 (diff 1.49e-08, 1 ULP); 38 of 55 ticks differ; largest tick 55 position.x: -7.0753837 vs -7.0754824 (diff 9.87e-05, 207 ULP)
- `sprint_turn_jump` unastar vs bedsim: first tick 8 velocity.x: -0.022565037 vs -0.022565039 (diff 1.86e-09, 1 ULP); 38 of 55 ticks differ; largest tick 32 position.x: -4.525616 vs -4.5256166 (diff 4.77e-07, 1 ULP)
- `sprint_turn_jump` acacia vs bedsim: first tick 8 velocity.x: -0.022565037 vs -0.022565039 (diff 1.86e-09, 1 ULP); 38 of 55 ticks differ; largest tick 55 position.x: -7.0753837 vs -7.0754843 (diff 0.000101, 211 ULP)
- `sprint_turn_jump` acacia vs unastar: first tick 42 position.x: -6.693557 vs -6.693576 (diff 1.91e-05, 40 ULP); 14 of 55 ticks differ; largest tick 55 position.x: -7.0753837 vs -7.075484 (diff 0.0001, 210 ULP)
- `stairs_corner_diagonal` acacia vs bds: first tick 6 position.x: 0.42928934 vs 0.43070352 (diff 0.00141, 47452 ULP); 50 of 55 ticks differ; largest tick 35 position.y: 101.85153 vs 102.155846 (diff 0.304, 39887 ULP)
- `stairs_corner_diagonal` acacia vs bedsim: first tick 6 position.x: 0.42928934 vs 0.43070352 (diff 0.00141, 47452 ULP); 50 of 55 ticks differ; largest tick 35 position.y: 101.85153 vs 102.155846 (diff 0.304, 39887 ULP)
- `stairs_corner_diagonal` acacia vs unastar: first tick 6 position.x: 0.42928934 vs 0.43070352 (diff 0.00141, 47452 ULP); 50 of 55 ticks differ; largest tick 35 position.y: 101.85153 vs 102.155846 (diff 0.304, 39887 ULP)
- `sweet_berry_jump` bedsim vs bds: first tick 1 position.z: 3.3 vs 2.5 (diff 0.8, 3355443 ULP); 35 of 35 ticks differ; largest tick 1 position.z: 3.3 vs 2.5 (diff 0.8, 3355443 ULP)
- `sweet_berry_jump` unastar vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 35 of 35 ticks differ; largest tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP)
- `sweet_berry_jump` acacia vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 35 of 35 ticks differ; largest tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP)
- `sweet_berry_walk` bedsim vs bds: first tick 17 position.z: 2.7 vs 2.8308897 (diff 0.131, 548991 ULP); 39 of 55 ticks differ; largest tick 45 position.z: 2.7 vs 5.1203117 (diff 2.42, 7802059 ULP)
- `sweet_berry_walk` unastar vs bedsim: first tick 17 position.z: 2.8308897 vs 2.7 (diff 0.131, 548991 ULP); 39 of 55 ticks differ; largest tick 45 position.z: 5.1203117 vs 2.7 (diff 2.42, 7802059 ULP)
- `sweet_berry_walk` acacia vs bedsim: first tick 17 position.z: 2.8308897 vs 2.7 (diff 0.131, 548991 ULP); 39 of 55 ticks differ; largest tick 45 position.z: 5.1203117 vs 2.7 (diff 2.42, 7802059 ULP)
- `trapdoor_bottom_walk` bedsim vs bds: first tick 17 position.y: 101.80751 vs 101.80251 (diff 0.005, 655 ULP); 23 of 55 ticks differ; largest tick 17 position.y: 101.80751 vs 101.80251 (diff 0.005, 655 ULP)
- `trapdoor_bottom_walk` unastar vs bedsim: first tick 17 position.y: 101.80251 vs 101.80751 (diff 0.005, 655 ULP); 23 of 55 ticks differ; largest tick 17 position.y: 101.80251 vs 101.80751 (diff 0.005, 655 ULP)
- `trapdoor_bottom_walk` acacia vs bedsim: first tick 17 position.y: 101.80251 vs 101.80751 (diff 0.005, 655 ULP); 23 of 55 ticks differ; largest tick 17 position.y: 101.80251 vs 101.80751 (diff 0.005, 655 ULP)
- `twisting_vines_climb` bedsim vs bds: first tick 1 position.z: 3.3 vs 2.5 (diff 0.8, 3355443 ULP); 65 of 65 ticks differ; largest tick 29 position.y: 101.62001 vs 106.41994 (diff 4.8, 629136 ULP)
- `twisting_vines_climb` unastar vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 65 of 65 ticks differ; largest tick 29 position.y: 106.41994 vs 101.62001 (diff 4.8, 629136 ULP)
- `twisting_vines_climb` acacia vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 65 of 65 ticks differ; largest tick 29 position.y: 106.41994 vs 101.62001 (diff 4.8, 629136 ULP)
- `twisting_vines_walk_into` bedsim vs bds: first tick 12 position.z: 1.7 vs 1.7551675 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 38 position.y: 101.62001 vs 105.5744 (diff 3.95, 518310 ULP)
- `twisting_vines_walk_into` unastar vs bedsim: first tick 12 position.z: 1.7551675 vs 1.7 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 38 position.y: 105.5744 vs 101.62001 (diff 3.95, 518310 ULP)
- `twisting_vines_walk_into` acacia vs bedsim: first tick 12 position.z: 1.7551675 vs 1.7 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 38 position.y: 105.5744 vs 101.62001 (diff 3.95, 518310 ULP)
- `wall_corner_diagonal` acacia vs bds: first tick 6 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 40 of 45 ticks differ; largest tick 45 position.x: 5.1720705 vs 5.078631 (diff 0.0934, 195957 ULP)
- `wall_corner_diagonal` acacia vs bedsim: first tick 6 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 40 of 45 ticks differ; largest tick 45 position.x: 5.1720705 vs 5.078631 (diff 0.0934, 195957 ULP)
- `wall_corner_diagonal` acacia vs unastar: first tick 6 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 40 of 45 ticks differ; largest tick 45 position.x: 5.1720705 vs 5.078631 (diff 0.0934, 195957 ULP)
- `wall_walk_into` acacia vs bds: first tick 26 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 30 of 55 ticks differ; largest tick 55 position.x: 3.6145673 vs 3.5522764 (diff 0.0623, 261267 ULP)
- `wall_walk_into` acacia vs bedsim: first tick 26 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 30 of 55 ticks differ; largest tick 55 position.x: 3.6145673 vs 3.5522764 (diff 0.0623, 261267 ULP)
- `wall_walk_into` acacia vs unastar: first tick 26 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 30 of 55 ticks differ; largest tick 55 position.x: 3.6145673 vs 3.5522764 (diff 0.0623, 261267 ULP)
- `water_climb_out` unastar vs bds: first tick 21 position.y: 101.64561 vs 101.655014 (diff 0.00941, 1233 ULP); 25 of 45 ticks differ; largest tick 35 position.y: 101.87828 vs 101.62001 (diff 0.258, 33852 ULP)
- `water_climb_out` unastar vs bedsim: first tick 20 velocity.y: -0.014405518 vs -0.005 (diff 0.00941, 13118997 ULP); 26 of 45 ticks differ; largest tick 35 position.y: 101.87828 vs 101.62001 (diff 0.258, 33852 ULP)
- `water_climb_out` acacia vs unastar: first tick 20 velocity.y: -0.005 vs -0.014405518 (diff 0.00941, 13118997 ULP); 26 of 45 ticks differ; largest tick 35 position.y: 101.62001 vs 101.87828 (diff 0.258, 33852 ULP)
- `water_fall_in` unastar vs bedsim: first tick 14 velocity.y: -0.254292 vs -0.005 (diff 0.249, 48126854 ULP); 1 of 40 ticks differ; largest tick 14 velocity.y: -0.254292 vs -0.005 (diff 0.249, 48126854 ULP)
- `water_fall_in` acacia vs unastar: first tick 14 velocity.y: -0.005 vs -0.254292 (diff 0.249, 48126854 ULP); 1 of 40 ticks differ; largest tick 14 velocity.y: -0.005 vs -0.254292 (diff 0.249, 48126854 ULP)
- `water_idle` unastar vs bedsim: first tick 25 velocity.y: -0.024506837 vs -0.005 (diff 0.0195, 19196806 ULP); 1 of 40 ticks differ; largest tick 25 velocity.y: -0.024506837 vs -0.005 (diff 0.0195, 19196806 ULP)
- `water_idle` acacia vs unastar: first tick 25 velocity.y: -0.005 vs -0.024506837 (diff 0.0195, 19196806 ULP); 1 of 40 ticks differ; largest tick 25 velocity.y: -0.005 vs -0.024506837 (diff 0.0195, 19196806 ULP)
- `water_jump` unastar vs bedsim: first tick 52 velocity.y: -0.022773437 vs -0.005 (diff 0.0178, 18266194 ULP); 1 of 55 ticks differ; largest tick 52 velocity.y: -0.022773437 vs -0.005 (diff 0.0178, 18266194 ULP)
- `water_jump` acacia vs unastar: first tick 52 velocity.y: -0.005 vs -0.022773437 (diff 0.0178, 18266194 ULP); 1 of 55 ticks differ; largest tick 52 velocity.y: -0.005 vs -0.022773437 (diff 0.0178, 18266194 ULP)
- `water_sprint_swim` unastar vs bds: first tick 39 position.y: 102.92912 vs 102.95345 (diff 0.0243, 3189 ULP); 22 of 60 ticks differ; largest tick 60 position.y: 102.31259 vs 101.82824 (diff 0.484, 63485 ULP)
- `water_sprint_swim` bedsim vs bds: first tick 11 position.y: 101.95927 vs 101.91866 (diff 0.0406, 5322 ULP); 49 of 60 ticks differ; largest tick 42 position.y: 101.62001 vs 103.31244 (diff 1.69, 221830 ULP)
- `water_sprint_swim` acacia vs bds: first tick 6 position.y: 102.02117 vs 102.06224 (diff 0.0411, 5383 ULP); 11 of 60 ticks differ; largest tick 10 position.y: 101.62001 vs 101.98158 (diff 0.362, 47392 ULP)
- `water_sprint_swim` unastar vs bedsim: first tick 11 position.y: 101.91866 vs 101.95927 (diff 0.0406, 5322 ULP); 50 of 60 ticks differ; largest tick 47 position.y: 103.23833 vs 101.62001 (diff 1.62, 212116 ULP)
- `water_sprint_swim` acacia vs bedsim: first tick 6 position.y: 102.02117 vs 102.06224 (diff 0.0411, 5383 ULP); 55 of 60 ticks differ; largest tick 42 position.y: 103.31244 vs 101.62001 (diff 1.69, 221830 ULP)
- `water_sprint_swim` acacia vs unastar: first tick 6 position.y: 102.02117 vs 102.06224 (diff 0.0411, 5383 ULP); 44 of 60 ticks differ; largest tick 60 position.y: 101.82824 vs 102.31259 (diff 0.484, 63485 ULP)
- `water_surface_sprint` acacia vs bds: first tick 6 velocity.y: 0 vs -0.005 (diff 0.005, 1000593162 ULP); 15 of 45 ticks differ; largest tick 45 position.z: 8.517824 vs 8.444698 (diff 0.0731, 76678 ULP)
- `water_surface_sprint` acacia vs bedsim: first tick 6 velocity.y: 0 vs -0.005 (diff 0.005, 1000593162 ULP); 40 of 45 ticks differ; largest tick 45 position.z: 8.517824 vs 8.444698 (diff 0.0731, 76678 ULP)
- `water_surface_sprint` acacia vs unastar: first tick 6 velocity.y: 0 vs -0.005 (diff 0.005, 1000593162 ULP); 40 of 45 ticks differ; largest tick 45 position.z: 8.517824 vs 8.444698 (diff 0.0731, 76678 ULP)
- `water_swim_deep` unastar vs bds: first tick 46 position.x: 0.11990852 vs 0.11990853 (diff 7.45e-09, 1 ULP); 10 of 55 ticks differ; largest tick 47 position.x: 0.086842574 vs 0.08684259 (diff 1.49e-08, 2 ULP)
- `water_swim_deep` bedsim vs bds: first tick 6 position.y: 102.56224 vs 102.52117 (diff 0.0411, 5383 ULP); 50 of 55 ticks differ; largest tick 53 position.y: 101.62001 vs 104.67999 (diff 3.06, 401078 ULP)
- `water_swim_deep` acacia vs bds: first tick 6 velocity.y: -0.046301577 vs -0.046301555 (diff 2.24e-08, 6 ULP); 19 of 55 ticks differ; largest tick 12 velocity.y: -0.11446585 vs -0.11446577 (diff 7.45e-08, 10 ULP)
- `water_swim_deep` unastar vs bedsim: first tick 6 position.y: 102.52117 vs 102.56224 (diff 0.0411, 5383 ULP); 50 of 55 ticks differ; largest tick 53 position.y: 104.67999 vs 101.62001 (diff 3.06, 401078 ULP)
- `water_swim_deep` acacia vs bedsim: first tick 6 position.y: 102.52117 vs 102.56224 (diff 0.0411, 5383 ULP); 50 of 55 ticks differ; largest tick 53 position.y: 104.67999 vs 101.62001 (diff 3.06, 401078 ULP)
- `water_swim_deep` acacia vs unastar: first tick 6 velocity.y: -0.046301577 vs -0.046301555 (diff 2.24e-08, 6 ULP); 39 of 55 ticks differ; largest tick 14 velocity.y: 0 vs -0.028173828 (diff 0.0282, 1021758669 ULP)
- `weeping_vines_climb` bedsim vs bds: first tick 1 position.z: 3.3 vs 2.5 (diff 0.8, 3355443 ULP); 65 of 65 ticks differ; largest tick 29 position.y: 101.62001 vs 106.41994 (diff 4.8, 629136 ULP)
- `weeping_vines_climb` unastar vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 65 of 65 ticks differ; largest tick 29 position.y: 106.41994 vs 101.62001 (diff 4.8, 629136 ULP)
- `weeping_vines_climb` acacia vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 65 of 65 ticks differ; largest tick 29 position.y: 106.41994 vs 101.62001 (diff 4.8, 629136 ULP)
- `weeping_vines_walk_into` bedsim vs bds: first tick 12 position.z: 1.7 vs 1.7551675 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 38 position.y: 101.62001 vs 105.5744 (diff 3.95, 518310 ULP)
- `weeping_vines_walk_into` unastar vs bedsim: first tick 12 position.z: 1.7551675 vs 1.7 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 38 position.y: 105.5744 vs 101.62001 (diff 3.95, 518310 ULP)
- `weeping_vines_walk_into` acacia vs bedsim: first tick 12 position.z: 1.7551675 vs 1.7 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 38 position.y: 105.5744 vs 101.62001 (diff 3.95, 518310 ULP)

<!-- manual: everything below is hand-written and kept by compare.py -->

## How close are bedsim and acacia-physics to BDS?

On the 147-scenario corpus (committed BDS run md-r5-h, and three random seeds from md-r5-random), unastar matches BDS bit for bit in 135
scenarios, bedsim in 97 and acacia-physics in 103.

**bedsim's runner world.** In 36 scenarios bedsim's runner builds a different world from
BDS's (25 of its divergences are marked "runner world" above):
- Dragonfly computes fence, pane, bar and wall connections from the neighbours;
- it models honey as a full cube, trapdoors 0.1875 thick (BDS: 0.1825) and soul sand as a full
  block;
- it has no shape for scaffolding, the climbing plants, powder snow, sweet berry bushes or snow
  layers.

Those divergences are the runner's, not bedsim's physics. acacia's runner takes BDS's shapes, so
its world equals unastar's and its divergences are its own.

On the round-5 scenarios:

| behaviour | unastar | bedsim | acacia |
|---|---|---|---|
| glide look vector from the `f32` sine table (`elytra_glide`, `elytra_glide_dive`) | equal | differs t6 | differs t12 |
| first swimming tick (`water_swim_deep`) | equal to t45 | differs t6 | differs t6 |
| a levitating player's first ground contact (`levitation_float`) | equal | differs t1 | differs t1 |
| lava (`lava_walk`) | equal | differs t48 | differs t48 |
| scaffolding climb (`scaffolding_climb`) | equal | runner world | differs t36 |
| powder snow landing (`powder_snow_fall`) | equal | runner world | differs t13 |
| honey wall slide, sweet berry bushes | equal | runner world | equal |
| sneak and sprint timing (`sprint_sneak_toggle`) | equal | differs t31 | equal |
| knockback in water, slowness sprints | equal | equal | equal |

The round-4 assessment of acacia still holds on the old scenarios: three undocumented departures
from bedsim (the diagonal move vector, the sprint's end on key release, the tiny-velocity
cut-off). On the new families acacia does better than bedsim: honey's slide, sweet berry bushes,
the sneak timing.

## Velocity on every input

BDS corrects a far-reporting client at most every sixth input, so the committed run reads BDS's
velocity on one input in six. A run of 30 scenarios, each shifted by one to five idle inputs
(md-r5-shift), moved the correction phase over the same movement. In 29 of them the shifted
copies moved exactly as the originals, and together they give BDS's velocity on 1450 of their
1484 inputs (`crates/bds-sweep/testdata/bds-1.26.51-movement-diff-velocities.json`).
`soul_sand_walk` did not: its idle start sinks into the soul sand. unastar's velocity equals
BDS's on every one of those inputs. Velocity cannot be read exactly from consecutive positions:
the wire position is rounded to `f32` after the 1.62001 offset is added, which loses several ULPs
at y 100.

## Randomised testing

`movement-diff random --first 1 --last 150` generated 150 random scenarios: random floors (stone,
ice, blue ice, slime, soul sand), random boxes of 16 kinds of blocks, and random keys, yaw and
turns. They were played on BDS as run md-r5-random.

- The setup check rejected 48 of them. The generator had placed carpets and snow layers in the
  air, and BDS dropped them. Both are now off the generator's list.
- On the 102 that played as written, unastar first matched BDS in 7. Two rules found there are
  fixed and committed as regression scenarios:
  - a sprint needs at least 0.8 of the move vector forward to start (`random_14`);
  - a jump in the tick a sprint stops gets no push (`random_135`).
- unastar now matches 10 of the 102. Most of the rest first differ on the input where the keys
  change, often right after a sneak is let go (`random_66`, kept as a known gap). The rule is
  still open, and fixing it is the next round's main work.
