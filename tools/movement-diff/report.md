# Movement differential: BDS 1.26.51, unastar, bedsim, acacia-physics

- **bds**: BDS 1.26.51, run md-r6-a; dt_gap_5 from run md-r6-probe2
- **unastar**: unastar_rules::movement
- **bedsim**: oomph-ac/bedsim v0.1.7-0.20261007160526-9c12b344afde
- **acacia**: acacia-rs/acacia acacia-physics @ 97ec4c7ce23e5abb7cde1e66026363d3c2e6fbf0

| scenario | ticks | unastar = BDS | bedsim = BDS | acacia = BDS | unastar = bedsim | acacia = bedsim | acacia = unastar |
|---|---|---|---|---|---|---|---|
| analog_half_walk | 45 | equal | equal | equal | equal | equal | equal |
| bed_bounce | 50 | differs t12 velocity.y | differs t12 velocity.y | differs t12 velocity.y | differs t12 velocity.y | differs t42 velocity.y | differs t12 velocity.y |
| bed_drop_1 | 45 | differs t6 velocity.y | differs t6 velocity.y | differs t6 velocity.y | differs t6 velocity.y | equal | differs t6 velocity.y |
| bed_drop_2 | 45 | differs t9 position.y | differs t9 position.y | differs t9 position.y | differs t8 velocity.y | differs t29 velocity.y | differs t8 velocity.y |
| bed_drop_3 | 45 | differs t11 position.y | differs t11 position.y | differs t11 position.y | differs t10 velocity.y | differs t34 velocity.y | differs t10 velocity.y |
| bed_drop_4 | 45 | differs t12 position.y | differs t12 position.y | differs t12 position.y | differs t11 velocity.y | differs t36 velocity.y | differs t11 velocity.y |
| bed_drop_7 | 45 | differs t16 position.y | differs t16 position.y | differs t16 position.y | differs t15 velocity.y | equal | differs t15 velocity.y |
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
| dive_90 | 40 | equal | differs t6 position.y | equal | differs t6 position.y | differs t6 position.y | differs t33 velocity.y |
| door_closed_walk_into | 45 | equal | equal | equal | equal | equal | equal |
| door_open_walk_through | 55 | equal | equal | equal | equal | equal | equal |
| dt_air | 26 | equal | equal | equal | equal | equal | equal |
| dt_air_second | 25 | equal | equal | equal | equal | equal | equal |
| dt_back | 27 | equal | differs t11 position.z | differs t11 position.z | differs t11 position.z | equal | differs t11 position.z |
| dt_gap_1 | 27 | equal | differs t8 position.z | differs t8 position.z | differs t8 position.z | equal | differs t8 position.z |
| dt_gap_5 | 31 | equal | differs t12 position.z | differs t12 position.z | differs t12 position.z | equal | differs t12 position.z |
| dt_gap_6 | 32 | equal | equal | equal | equal | equal | equal |
| dt_gap_7 | 33 | equal | equal | equal | equal | equal | equal |
| dt_gap_8 | 34 | equal | equal | equal | equal | equal | equal |
| dt_half | 28 | equal | equal | equal | equal | equal | equal |
| dt_half_then_full | 31 | equal | differs t12 position.z | differs t12 position.z | differs t12 position.z | equal | differs t12 position.z |
| dt_sneak_hold | 25 | equal | equal | equal | equal | equal | equal |
| dt_sprint_key_first | 25 | equal | equal | equal | equal | equal | equal |
| dt_strafe | 25 | equal | differs t9 position.x | differs t6 position.x | differs t9 position.x | differs t6 position.x | differs t6 position.x |
| dt_three | 26 | equal | differs t8 position.z | differs t8 position.z | differs t8 position.z | equal | differs t8 position.z |
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
| knockback_ground | 45 | equal | differs t42 velocity.z | differs t41 position.z | differs t39 velocity.z | differs t39 velocity.x | differs t39 velocity.x |
| knockback_sprint_air | 50 | equal | differs t14 position.z | differs t14 position.z | differs t14 position.z | equal | differs t14 position.z |
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
| levitation_ceiling | 55 | equal | differs t1 on_ground | differs t1 on_ground | differs t1 velocity.y | differs t1 velocity.y | differs t1 velocity.y |
| levitation_float | 65 | equal | differs t1 on_ground | differs t1 on_ground | differs t1 velocity.y | differs t1 velocity.y | differs t1 velocity.y |
| levitation_jump | 36 | equal | differs t1 on_ground | differs t1 on_ground | differs t1 velocity.y | differs t1 velocity.y | differs t1 velocity.y |
| packed_ice_sprint_jump | 60 | equal | differs t30 velocity.z | differs t30 velocity.z | differs t30 velocity.z | equal | differs t30 velocity.z |
| powder_snow_fall | 50 | equal | differs t13 position.y (runner world) | differs t13 position.y | differs t13 position.y | differs t13 position.y | differs t13 position.y |
| powder_snow_jump | 45 | equal | differs t1 position.x (runner world) | equal | differs t1 position.x | differs t1 position.x | equal |
| powder_snow_walk_into | 55 | differs t18 position.z | differs t17 position.z (runner world) | differs t33 position.z | differs t17 position.z | differs t17 position.z | differs t18 position.z |
| random_135 | 45 | equal | differs t6 velocity.z | differs t4 position.x | differs t6 velocity.z | differs t4 position.x | differs t4 position.x |
| random_14 | 87 | differs t54 velocity.x | differs t2 position.y (runner world) | differs t31 position.x | differs t2 position.y | differs t2 position.y | differs t31 position.x |
| random_172 | 58 | differs t6 velocity.z | differs t6 position.x (runner world) | differs t6 position.x | differs t5 velocity.x | differs t5 velocity.z | differs t5 velocity.x |
| random_186 | 114 | differs t32 position.y | differs t30 position.z (runner world) | differs t5 position.x | differs t26 velocity.z | differs t5 position.x | differs t5 position.x |
| random_248 | 80 | equal | differs t16 position.y | differs t4 position.x | differs t14 velocity.z | differs t4 position.x | differs t4 position.x |
| random_291 | 95 | differs t72 velocity.y | differs t13 position.x (runner world) | differs t25 position.x | differs t13 position.x | differs t13 position.x | differs t13 velocity.x |
| random_297 | 104 | equal | differs t2 position.y (runner world) | differs t5 position.x | differs t2 position.y | differs t2 position.y | differs t5 position.x |
| random_66 | 81 | differs t60 position.x | differs t12 position.x (runner world) | differs t60 position.x | differs t12 position.x | differs t12 position.x | differs t60 position.x |
| random_a123 | 82 | equal | differs t6 velocity.x (runner world) | differs t4 position.x | differs t6 velocity.x | differs t4 position.x | differs t4 position.x |
| random_a141 | 105 | differs t58 position.y | differs t6 position.x (runner world) | differs t6 position.x | differs t6 position.x | differs t6 position.x | differs t6 position.x |
| random_a146 | 96 | equal | differs t40 position.z | differs t36 velocity.z | differs t38 velocity.x | differs t34 velocity.z | differs t34 velocity.z |
| random_a27 | 63 | equal | differs t40 position.x (runner world) | equal | differs t40 position.x | differs t40 position.x | equal |
| random_a30 | 57 | equal | differs t19 position.z | differs t31 position.x | differs t19 position.z | differs t19 position.z | differs t31 position.x |
| random_a67 | 91 | equal | differs t2 position.y (runner world) | differs t19 position.x | differs t2 position.y | differs t2 position.y | differs t19 position.x |
| random_a7 | 81 | equal | differs t17 position.y (runner world) | differs t11 position.x | differs t16 velocity.y | differs t11 position.x | differs t11 position.x |
| random_a90 | 81 | equal | differs t43 position.x (runner world) | differs t4 position.x | differs t43 position.x | differs t4 position.x | differs t4 position.x |
| scaffolding_climb | 75 | equal | differs t1 position.z (runner world) | differs t36 velocity.y | differs t1 position.z | differs t1 position.z | differs t36 velocity.y |
| scaffolding_sneak_down | 55 | equal | differs t6 position.y (runner world) | differs t2 position.y | differs t6 position.y | differs t2 position.y | differs t2 position.y |
| scaffolding_walk_into | 50 | equal | differs t12 position.z (runner world) | equal | differs t12 position.z | differs t12 position.z | equal |
| scaffolding_walk_on_top | 45 | equal | differs t18 position.z (runner world) | equal | differs t18 position.z | differs t18 position.z | equal |
| slab_double_walk_into | 45 | equal | equal | equal | equal | equal | equal |
| slab_top_jump_onto | 45 | equal | equal | equal | equal | equal | equal |
| slime_bounce | 80 | equal | differs t14 position.y | equal | differs t13 velocity.y | differs t13 velocity.y | equal |
| slime_sneak_land | 45 | equal | equal | equal | equal | equal | equal |
| slime_walk | 45 | equal | equal | equal | equal | equal | equal |
| slime_web_jump | 36 | equal | equal | equal | equal | equal | equal |
| slow_fall_tower | 83 | equal | equal | equal | equal | equal | equal |
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
| sprint_jump_yaw_37_3 | 27 | equal | differs t9 position.x | differs t6 velocity.x | differs t9 position.x | differs t6 velocity.x | differs t6 velocity.x |
| sprint_jump_yaw_m71_9 | 27 | equal | differs t6 velocity.z | differs t18 velocity.x | differs t6 velocity.z | differs t6 velocity.z | differs t11 velocity.x |
| sprint_sneak_toggle | 55 | equal | differs t31 position.z | equal | differs t31 position.z | differs t31 position.z | equal |
| sprint_turn_jump | 55 | equal | differs t10 position.x | differs t17 position.x | differs t8 velocity.x | differs t8 velocity.x | differs t14 velocity.x |
| sr_air | 30 | equal | differs t11 position.z | equal | differs t11 position.z | differs t11 position.z | equal |
| sr_back | 35 | equal | differs t16 position.z | equal | differs t16 position.z | differs t16 position.z | equal |
| sr_edge | 65 | equal | differs t46 position.z | equal | differs t46 position.z | differs t46 position.z | equal |
| sr_ice_still_forward | 31 | equal | differs t12 position.x | equal | differs t12 position.x | differs t12 position.x | equal |
| sr_jump | 35 | equal | differs t16 position.z | equal | differs t16 position.z | differs t16 position.z | equal |
| sr_low_ceiling | 45 | equal | differs t26 position.z | equal | differs t26 position.z | differs t26 position.z | equal |
| sr_sprint_jump | 35 | equal | differs t16 position.z | differs t24 position.z | differs t16 position.z | differs t16 position.z | differs t19 velocity.z |
| sr_still_forward_4 | 32 | equal | differs t13 position.z | equal | differs t13 position.z | differs t13 position.z | equal |
| sr_still_forward_7 | 35 | equal | differs t16 position.z | equal | differs t16 position.z | differs t16 position.z | equal |
| sr_strafe | 35 | equal | differs t16 position.x | equal | differs t16 position.x | differs t16 position.x | equal |
| sr_tap | 33 | equal | differs t17 position.z | equal | differs t17 position.z | differs t17 position.z | equal |
| sr_toggle | 30 | equal | differs t8 position.z | differs t13 position.z | differs t8 position.z | differs t8 position.z | differs t13 position.z |
| sr_walk_5 | 35 | equal | differs t16 position.z | equal | differs t16 position.z | differs t16 position.z | equal |
| ss_air_stop | 29 | equal | equal | equal | equal | equal | equal |
| ss_back | 30 | equal | equal | equal | equal | equal | equal |
| ss_diag_air | 40 | equal | differs t6 velocity.x | differs t6 position.x | differs t6 velocity.x | differs t6 position.x | differs t6 position.x |
| ss_half_forward | 38 | equal | differs t16 position.z | equal | differs t16 position.z | differs t16 position.z | equal |
| ss_half_forward_nokey | 38 | equal | differs t16 position.z | equal | differs t16 position.z | differs t16 position.z | equal |
| ss_strafe_jump | 40 | equal | differs t10 position.z | differs t6 position.x | differs t8 velocity.x | differs t6 position.x | differs t6 position.x |
| ss_strafe_stop | 30 | equal | equal | equal | equal | equal | equal |
| stairs_corner_diagonal | 55 | equal | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x |
| stairs_down | 55 | equal | equal | equal | equal | equal | equal |
| stairs_up | 55 | equal | equal | equal | equal | equal | equal |
| stairs_upside_down_walk_under | 55 | equal | equal | equal | equal | equal | equal |
| step_lectern_slab | 35 | equal | equal | n/a | equal | n/a | n/a |
| step_slab | 45 | equal | equal | equal | equal | equal | equal |
| step_sprint_into_slab | 45 | equal | equal | equal | equal | equal | equal |
| step_top_slab | 40 | equal | equal | equal | equal | equal | equal |
| sw_corner | 35 | equal | differs t6 velocity.x | equal | differs t6 velocity.x | differs t6 velocity.x | equal |
| sw_ground_20 | 40 | equal | differs t11 position.z | equal | differs t7 velocity.z | differs t7 velocity.z | equal |
| sw_ground_22 | 40 | equal | differs t6 velocity.x | differs t6 velocity.x | differs t6 velocity.x | differs t17 position.x | differs t6 velocity.x |
| sw_ground_24 | 40 | equal | differs t6 velocity.z | equal | differs t6 velocity.z | differs t6 velocity.z | equal |
| sw_ground_25 | 40 | equal | differs t6 velocity.x | equal | differs t6 velocity.x | differs t6 velocity.x | equal |
| sw_ground_30 | 40 | equal | equal | equal | equal | equal | equal |
| sw_ground_5 | 40 | equal | differs t17 position.x | equal | differs t17 position.x | differs t17 position.x | equal |
| sw_ground_75 | 40 | equal | equal | equal | equal | equal | equal |
| sw_head_on_strafe | 40 | equal | differs t21 position.x | differs t21 position.x | differs t21 position.x | differs t21 position.x | differs t21 position.x |
| sw_jump_10 | 40 | equal | differs t6 velocity.x | differs t9 position.x | differs t6 velocity.x | differs t6 velocity.x | differs t7 velocity.x |
| sw_jump_30 | 40 | equal | differs t6 velocity.x | differs t18 position.x | differs t6 velocity.x | differs t6 velocity.x | differs t18 position.x |
| sw_near_partial | 35 | equal | differs t8 position.x | equal | differs t8 position.x | differs t8 position.x | equal |
| sw_nokey_10 | 38 | equal | differs t17 position.x | differs t9 position.x | differs t17 position.x | differs t9 position.x | differs t9 position.x |
| sw_nokey_30 | 38 | equal | equal | differs t9 position.x | equal | differs t9 position.x | differs t9 position.x |
| sw_post | 40 | equal | differs t20 position.x | equal | differs t20 position.x | differs t20 position.x | equal |
| sw_strafe_wall | 40 | equal | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x |
| sw_strafe_wall_03 | 40 | equal | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x |
| sw_xwall_65 | 40 | equal | differs t6 position.z | equal | differs t6 position.z | differs t6 position.z | equal |
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
| wall_slide_east | 60 | equal | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x |
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
| web_cross_0 | 22 | equal | differs t6 position.x | equal | differs t6 position.x | differs t6 position.x | equal |
| web_walk_0 | 35 | equal | differs t6 velocity.x | equal | differs t6 velocity.x | differs t6 velocity.x | equal |
| weeping_vines_climb | 65 | equal | differs t1 position.z (runner world) | equal | differs t1 position.z | differs t1 position.z | equal |
| weeping_vines_walk_into | 50 | equal | differs t12 position.z (runner world) | equal | differs t12 position.z | differs t12 position.z | equal |
| **equal** | 226 | 207 | 117 | 143 | 109 | 102 | 133 |

## Runner world fidelity

Scenarios whose world a runner builds differently from unastar's (BDS 1.26.51's shapes and friction). Their divergences above are marked "runner world": they come from the runner, not from the library's physics.

- **bedsim**: 49 scenarios
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
  - `random_172`: 4 cells, e.g. 1,101,7 minecraft:honey_block: boxes [[1, 101, 7, 2, 102, 8]] instead of [[1.0625, 101.0, 7.0625, 1.9375, 102.0, 7.9375]]
  - `random_186`: 2 cells, e.g. 4,102,3 minecraft:honey_block: boxes [[4, 102, 3, 5, 103, 4]] instead of [[4.0625, 102.0, 3.0625, 4.9375, 103.0, 3.9375]]
  - `random_291`: 2 cells, e.g. 0,100,5 minecraft:honey_block: boxes [[0, 100, 5, 1, 101, 6]] instead of [[0.0625, 100.0, 5.0625, 0.9375, 101.0, 5.9375]]
  - `random_297`: 833 cells, e.g. -1,99,-1 minecraft:soul_sand: a plain full cube of friction 0.6 to bedsim only
  - `random_66`: 10 cells, e.g. -1,100,4 minecraft:soul_sand: a plain full cube of friction 0.6 to bedsim only
  - `random_a123`: 6 cells, e.g. 2,101,10 minecraft:honey_block: boxes [[2, 101, 10, 3, 102, 11]] instead of [[2.0625, 101.0, 10.0625, 2.9375, 102.0, 10.9375]]
  - `random_a141`: 3 cells, e.g. 3,101,5 minecraft:trapdoor: boxes [[3, 101, 5, 4, 101.1875, 6]] instead of [[3.0, 101.0, 5.0, 4.0, 101.18250274658203, 6.0]]
  - `random_a27`: 3 cells, e.g. -1,100,1 minecraft:honey_block: boxes [[-1, 100, 1, 0, 101, 2]] instead of [[-0.9375, 100.0, 1.0625, -0.0625, 101.0, 1.9375]]
  - `random_a67`: 833 cells, e.g. -1,99,-1 minecraft:soul_sand: a plain full cube of friction 0.6 to bedsim only
  - `random_a7`: 5 cells, e.g. 2,102,6 minecraft:honey_block: boxes [[2, 102, 6, 3, 103, 7]] instead of [[2.0625, 102.0, 6.0625, 2.9375, 103.0, 6.9375]]
  - `random_a90`: 2 cells, e.g. -2,101,1 minecraft:honey_block: boxes [[-2, 101, 1, -1, 102, 2]] instead of [[-1.9375, 101.0, 1.0625, -1.0625, 102.0, 1.9375]]
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
  - `step_lectern_slab`: 9 cells, e.g. -1,100,-1 minecraft:lectern: boxes [[-1, 100, -1, 0, 100.9, 0]] instead of [[-1.0, 100.0, -1.0, 0.0, 100.9000015258789, 0.0]]
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

- **Setup, inputs, knockbacks and relays** (the committed run's `validation.json`): 226 of 226 scenarios played exactly as the corpus says (world, effects, chest slot, start teleport; every input's move vector, rotation and keys; every velocity BDS set, after the corpus's input); 11238 relays for 11238 inputs.
- **repeat** (`BDS 1.26.51, run md-r6-repeat`, offset [3.0, 3.0, 3.0], flags from simulation): 224 of 225 scenarios bit-identical to the committed run.
  - `powder_snow_walk_into` differs: powder snow's freezing slowdown follows BDS's game ticks, not the inputs, so the walk into the snow is slowed by a different amount
- **offset** (`BDS 1.26.51, run md-r6-offset`, offset [-5.0, 5.0, -4.0], flags from simulation): 223 of 225 scenarios bit-identical to the committed run.
  - `powder_snow_fall` differs: positions equal; powder snow's freezing updates follow BDS's game ticks, not the inputs, so a different number of them falls within the scenario
  - `powder_snow_walk_into` differs: powder snow's freezing slowdown follows BDS's game ticks, not the inputs, so the walk into the snow is slowed by a different amount
- **flags** (`BDS 1.26.51, run md-r6-keys`, offset [3.0, 3.0, 3.0], flags from keys): 187 of 225 scenarios bit-identical to the committed run.
  - `dt_back` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `dt_gap_1` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `dt_half_then_full` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `dt_strafe` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `dt_three` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `flat_sprint` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `knockback_ground` differs: the knockback command ran an input later than in the committed run (BDS set the velocity an input later than the corpus says), so the push lands a tick later; the extraction's knockback check fails the scenario in this run
  - `knockback_water` differs: the knockback command ran an input later than in the committed run (BDS set the velocity an input later than the corpus says), so the push lands a tick later; the extraction's knockback check fails the scenario in this run
  - `powder_snow_fall` differs: positions equal; powder snow's freezing updates follow BDS's game ticks, not the inputs, so a different number of them falls within the scenario
  - `powder_snow_walk_into` differs: powder snow's freezing slowdown follows BDS's game ticks, not the inputs, so the walk into the snow is slowed by a different amount
  - `random_135` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `random_14` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `random_186` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `random_291` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `random_66` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `random_a123` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `random_a141` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `random_a27` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `random_a7` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sprint_jump` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sprint_jump_onto_block` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sprint_sneak_toggle` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sr_sprint_jump` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sr_toggle` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `ss_half_forward` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sw_corner` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sw_ground_20` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sw_ground_22` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sw_ground_24` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sw_ground_5` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sw_head_on_strafe` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sw_jump_10` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sw_jump_30` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sw_near_partial` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sw_nokey_10` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sw_nokey_30` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `sw_post` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
  - `wall_sprint_jump` differs: positions equal; BDS sends the movement attribute again for each sprint request it refuses, and the key-only flags request the sprint on other inputs
- **Mutations** (`mutate.py`): 38 of 41 caught by at least one scenario that matches BDS without them.
  - gravity 0.08 one ULP up: caught by 198 scenarios (e.g. `analog_half_walk`)
  - vertical drag 0.98 one ULP down: caught by 199 scenarios (e.g. `analog_half_walk`)
  - air friction 0.91 one ULP up: caught by 74 scenarios (e.g. `bed_walk`)
  - the block friction one ULP up: caught by 16 scenarios (e.g. `blue_ice_sprint_jump`)
  - air acceleration 0.02 one ULP up: caught by 23 scenarios (e.g. `chunk_border_step`)
  - sprinting air acceleration 0.025999999 one ULP up (to 0.026): caught by 12 scenarios (e.g. `knockback_sprint_air`)
  - jump 0.42 one ULP up: caught by 47 scenarios (e.g. `blue_ice_sprint_jump`)
  - jump boost step 0.1 one ULP up: caught by 1 scenarios (e.g. `jump_boost_4_sprint_jump`)
  - sprint jump push 0.2 one ULP up: caught by 18 scenarios (e.g. `blue_ice_sprint_jump`)
  - move vector scale 0.98 one ULP down: caught by 141 scenarios (e.g. `analog_half_walk`)
  - sneak factor 0.3 one ULP up: caught by 30 scenarios (e.g. `dt_sneak_hold`)
  - ground acceleration rounded as ((speed r) r) r: caught by 3 scenarios (e.g. `dt_three`)
  - gravity and drag as v drag - g drag: caught by 81 scenarios (e.g. `bed_walk`)
  - the walk turned by the sine table: caught by 33 scenarios (e.g. `flat_odd_yaw_walk`)
  - the sprint jump turned by the exact sine, not the table: caught by 9 scenarios (e.g. `random_297`)
  - step height 0.5625 up to 0.6: caught by 1 scenarios (e.g. `step_lectern_slab`)
  - a slide stops at twice f32::EPSILON: caught by 3 scenarios (e.g. `fall_onto_edge`)
  - touching tolerance 1e-6 down to 1e-7: caught by 1 scenarios (e.g. `random_248`)
  - sneak edge probe step 0.05 down to 0.04: caught by 1 scenarios (e.g. `sneak_off_fence`)
  - water drag 0.8 one ULP up: caught by 8 scenarios (e.g. `dive_90`)
  - water sink 0.005 one ULP up: caught by 9 scenarios (e.g. `dive_90`)
  - swim steering 0.085 four ULPs up: caught by 1 scenarios (e.g. `dive_90`)
  - lava drag 0.5 one ULP up: caught by 4 scenarios (e.g. `lava_climb_out`)
  - glide horizontal drag 0.99 one ULP down: caught by 2 scenarios (e.g. `elytra_glide`)
  - levitation lift 0.01 one ULP up: caught by 3 scenarios (e.g. `levitation_ceiling`)
  - slow falling gravity 0.01 eight ULPs up: caught by 4 scenarios (e.g. `slow_fall_tower`)
  - climb speed 0.2 one ULP up: caught by 10 scenarios (e.g. `cave_vines_climb`)
  - soul sand factor 1.225 one ULP up: caught by 4 scenarios (e.g. `random_297`)
  - honey jump 0.6 one ULP up: caught by 1 scenarios (e.g. `honey_walk_jump`)
  - slowest bouncing fall 0.08000012 down to 0.0784: caught by 1 scenarios (e.g. `slime_web_jump`)
  - cobweb horizontal factor 0.25 one ULP up: caught by 1 scenarios (e.g. `web_walk_0`)
  - bed bounce 0.66 one ULP up: **missed**
  - powder snow factor 0.9 one ULP up: **missed**
  - a released sneak no longer slows its input: caught by 21 scenarios (e.g. `random_248`)
  - double-tap window 7 inputs down to 6: caught by 1 scenarios (e.g. `dt_gap_5`)
  - double tap also in the air: caught by 3 scenarios (e.g. `dt_air`)
  - a wall ends a sprint from 40 degrees, not 45: caught by 7 scenarios (e.g. `random_a123`)
  - a sprint lasts while the move vector is forward at all: caught by 7 scenarios (e.g. `dt_half`)
  - the horizontal slowdown beside honey 0.4 one ULP up: caught by 2 scenarios (e.g. `random_a27`)
  - a web the box only touches holds it: caught by 1 scenarios (e.g. `random_297`)
  - wire offset 1.62001 one ULP up: **missed**

## Divergences

- `bed_bounce` unastar vs bds: first tick 12 velocity.y: 0.42683694 vs 0.5324077 (diff 0.106, 2998656 ULP); 38 of 50 ticks differ; largest tick 23 position.y: 102.37192 vs 103.423775 (diff 1.05, 137869 ULP)
- `bed_bounce` bedsim vs bds: first tick 12 velocity.y: 0.49573284 vs 0.5324077 (diff 0.0367, 686893 ULP); 37 of 50 ticks differ; largest tick 40 position.y: 102.18251 vs 102.62937 (diff 0.447, 58571 ULP)
- `bed_bounce` acacia vs bds: first tick 12 velocity.y: 0.49573284 vs 0.5324077 (diff 0.0367, 686893 ULP); 37 of 50 ticks differ; largest tick 40 position.y: 102.18251 vs 102.62937 (diff 0.447, 58571 ULP)
- `bed_bounce` unastar vs bedsim: first tick 12 velocity.y: 0.42683694 vs 0.49573284 (diff 0.0689, 2311763 ULP); 30 of 50 ticks differ; largest tick 31 position.y: 102.18251 vs 102.88831 (diff 0.706, 92511 ULP)
- `bed_bounce` acacia vs bedsim: first tick 42 velocity.y: -0.04744069 vs -0.0784 (diff 0.031, 6176515 ULP); 1 of 50 ticks differ; largest tick 42 velocity.y: -0.04744069 vs -0.0784 (diff 0.031, 6176515 ULP)
- `bed_bounce` acacia vs unastar: first tick 12 velocity.y: 0.49573284 vs 0.42683694 (diff 0.0689, 2311763 ULP); 31 of 50 ticks differ; largest tick 31 position.y: 102.88831 vs 102.18251 (diff 0.706, 92511 ULP)
- `bed_drop_1` unastar vs bds: first tick 6 velocity.y: 0.16520461 vs 0.24382493 (diff 0.0786, 5276120 ULP); 19 of 45 ticks differ; largest tick 11 position.y: 102.20768 vs 102.58536 (diff 0.378, 49503 ULP)
- `bed_drop_1` bedsim vs bds: first tick 6 velocity.y: 0.1984234 vs 0.24382493 (diff 0.0454, 3046845 ULP); 19 of 45 ticks differ; largest tick 12 velocity.y: 0.066594504 vs -0.23150742 (diff 0.298, 2079683323 ULP)
- `bed_drop_1` acacia vs bds: first tick 6 velocity.y: 0.1984234 vs 0.24382493 (diff 0.0454, 3046845 ULP); 19 of 45 ticks differ; largest tick 12 velocity.y: 0.066594504 vs -0.23150742 (diff 0.298, 2079683323 ULP)
- `bed_drop_1` unastar vs bedsim: first tick 6 velocity.y: 0.16520461 vs 0.1984234 (diff 0.0332, 2229275 ULP); 10 of 45 ticks differ; largest tick 11 position.y: 102.20768 vs 102.367256 (diff 0.16, 20916 ULP)
- `bed_drop_1` acacia vs unastar: first tick 6 velocity.y: 0.1984234 vs 0.16520461 (diff 0.0332, 2229275 ULP); 10 of 45 ticks differ; largest tick 11 position.y: 102.367256 vs 102.20768 (diff 0.16, 20916 ULP)
- `bed_drop_2` unastar vs bds: first tick 9 position.y: 102.43847 vs 102.54248 (diff 0.104, 13633 ULP); 27 of 45 ticks differ; largest tick 15 position.y: 102.27764 vs 102.96348 (diff 0.686, 89894 ULP)
- `bed_drop_2` bedsim vs bds: first tick 9 position.y: 102.48407 vs 102.54248 (diff 0.0584, 7656 ULP); 28 of 45 ticks differ; largest tick 22 position.y: 102.19715 vs 102.65386 (diff 0.457, 59862 ULP)
- `bed_drop_2` acacia vs bds: first tick 9 position.y: 102.48407 vs 102.54248 (diff 0.0584, 7656 ULP); 28 of 45 ticks differ; largest tick 22 position.y: 102.19715 vs 102.65386 (diff 0.457, 59862 ULP)
- `bed_drop_2` unastar vs bedsim: first tick 8 velocity.y: 0.25596192 vs 0.30155674 (diff 0.0456, 1529908 ULP); 21 of 45 ticks differ; largest tick 16 velocity.y: 0.11223838 vs -0.3284556 (diff 0.441, 2089683089 ULP)
- `bed_drop_2` acacia vs bedsim: first tick 29 velocity.y: -0.04144518 vs -0.0784 (diff 0.037, 7785922 ULP); 1 of 45 ticks differ; largest tick 29 velocity.y: -0.04144518 vs -0.0784 (diff 0.037, 7785922 ULP)
- `bed_drop_2` acacia vs unastar: first tick 8 velocity.y: 0.30155674 vs 0.25596192 (diff 0.0456, 1529908 ULP); 22 of 45 ticks differ; largest tick 16 velocity.y: -0.3284556 vs 0.11223838 (diff 0.441, 2089683089 ULP)
- `bed_drop_3` unastar vs bds: first tick 11 position.y: 102.525635 vs 102.61886 (diff 0.0932, 12219 ULP); 31 of 45 ticks differ; largest tick 19 position.y: 102.340225 vs 103.115166 (diff 0.775, 101573 ULP)
- `bed_drop_3` bedsim vs bds: first tick 11 position.y: 102.583115 vs 102.61886 (diff 0.0357, 4685 ULP); 32 of 45 ticks differ; largest tick 27 position.y: 102.22502 vs 102.796234 (diff 0.571, 74870 ULP)
- `bed_drop_3` acacia vs bds: first tick 11 position.y: 102.583115 vs 102.61886 (diff 0.0357, 4685 ULP); 32 of 45 ticks differ; largest tick 27 position.y: 102.22502 vs 102.796234 (diff 0.571, 74870 ULP)
- `bed_drop_3` unastar vs bedsim: first tick 10 velocity.y: 0.34312525 vs 0.40060598 (diff 0.0575, 1928733 ULP); 24 of 45 ticks differ; largest tick 20 velocity.y: 0.15808873 vs -0.38975033 (diff 0.548, 2095673182 ULP)
- `bed_drop_3` acacia vs bedsim: first tick 34 velocity.y: -0.029027497 vs -0.0784 (diff 0.0494, 11715867 ULP); 1 of 45 ticks differ; largest tick 34 velocity.y: -0.029027497 vs -0.0784 (diff 0.0494, 11715867 ULP)
- `bed_drop_3` acacia vs unastar: first tick 10 velocity.y: 0.40060598 vs 0.34312525 (diff 0.0575, 1928733 ULP); 25 of 45 ticks differ; largest tick 20 velocity.y: -0.38975033 vs 0.15808873 (diff 0.548, 2095673182 ULP)
- `bed_drop_4` unastar vs bds: first tick 12 position.y: 102.56792 vs 102.699326 (diff 0.131, 17224 ULP); 33 of 45 ticks differ; largest tick 21 position.y: 102.36129 vs 103.56325 (diff 1.2, 157543 ULP)
- `bed_drop_4` bedsim vs bds: first tick 12 position.y: 102.63116 vs 102.699326 (diff 0.0682, 8935 ULP); 34 of 45 ticks differ; largest tick 30 position.y: 102.18251 vs 103.03055 (diff 0.848, 111154 ULP)
- `bed_drop_4` acacia vs bds: first tick 12 position.y: 102.63116 vs 102.699326 (diff 0.0682, 8935 ULP); 34 of 45 ticks differ; largest tick 30 position.y: 102.18251 vs 103.03055 (diff 0.848, 111154 ULP)
- `bed_drop_4` unastar vs bedsim: first tick 11 velocity.y: 0.38540393 vs 0.44864988 (diff 0.0632, 2122182 ULP); 25 of 45 ticks differ; largest tick 22 velocity.y: 0.18172456 vs -0.42188513 (diff 0.604, 2098337621 ULP)
- `bed_drop_4` acacia vs bedsim: first tick 36 velocity.y: -0.036690276 vs -0.0784 (diff 0.0417, 9062307 ULP); 1 of 45 ticks differ; largest tick 36 velocity.y: -0.036690276 vs -0.0784 (diff 0.0417, 9062307 ULP)
- `bed_drop_4` acacia vs unastar: first tick 11 velocity.y: 0.44864988 vs 0.38540393 (diff 0.0632, 2122182 ULP); 26 of 45 ticks differ; largest tick 22 velocity.y: -0.42188513 vs 0.18172456 (diff 0.604, 2098337621 ULP)
- `bed_drop_7` unastar vs bds: first tick 16 position.y: 102.72874 vs 102.84634 (diff 0.118, 15414 ULP); 29 of 45 ticks differ; largest tick 29 position.y: 102.31709 vs 103.76564 (diff 1.45, 189864 ULP)
- `bed_drop_7` bedsim vs bds: first tick 16 position.y: 102.81392 vs 102.84634 (diff 0.0324, 4249 ULP); 29 of 45 ticks differ; largest tick 42 position.y: 102.3255 vs 102.82241 (diff 0.497, 65131 ULP)
- `bed_drop_7` acacia vs bds: first tick 16 position.y: 102.81392 vs 102.84634 (diff 0.0324, 4249 ULP); 29 of 45 ticks differ; largest tick 42 position.y: 102.3255 vs 102.82241 (diff 0.497, 65131 ULP)
- `bed_drop_7` unastar vs bedsim: first tick 15 velocity.y: 0.54623 vs 0.63140684 (diff 0.0852, 1429030 ULP); 31 of 45 ticks differ; largest tick 29 position.y: 102.31709 vs 103.36631 (diff 1.05, 137523 ULP)
- `bed_drop_7` acacia vs unastar: first tick 15 velocity.y: 0.63140684 vs 0.54623 (diff 0.0852, 1429030 ULP); 31 of 45 ticks differ; largest tick 29 position.y: 103.36631 vs 102.31709 (diff 1.05, 137523 ULP)
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
- `dive_90` bedsim vs bds: first tick 6 position.y: 109.56224 vs 109.47867 (diff 0.0836, 10954 ULP); 35 of 40 ticks differ; largest tick 33 position.y: 108.91991 vs 101.62001 (diff 7.3, 956812 ULP)
- `dive_90` unastar vs bedsim: first tick 6 position.y: 109.47867 vs 109.56224 (diff 0.0836, 10954 ULP); 35 of 40 ticks differ; largest tick 33 position.y: 101.62001 vs 108.91991 (diff 7.3, 956812 ULP)
- `dive_90` acacia vs bedsim: first tick 6 position.y: 109.47867 vs 109.56224 (diff 0.0836, 10954 ULP); 35 of 40 ticks differ; largest tick 33 position.y: 101.62001 vs 108.91991 (diff 7.3, 956812 ULP)
- `dive_90` acacia vs unastar: first tick 33 velocity.y: 0 vs -0.16344605 (diff 0.163, 1042767463 ULP); 1 of 40 ticks differ; largest tick 33 velocity.y: 0 vs -0.16344605 (diff 0.163, 1042767463 ULP)
- `dt_back` bedsim vs bds: first tick 11 position.z: 0.046919033 vs 0.076319024 (diff 0.0294, 6037242 ULP); 17 of 27 ticks differ; largest tick 27 position.z: 2.430149 vs 3.203465 (diff 0.773, 3243522 ULP)
- `dt_back` acacia vs bds: first tick 11 position.z: 0.046919033 vs 0.076319024 (diff 0.0294, 6037242 ULP); 17 of 27 ticks differ; largest tick 27 position.z: 2.430149 vs 3.203465 (diff 0.773, 3243522 ULP)
- `dt_back` unastar vs bedsim: first tick 11 position.z: 0.076319024 vs 0.046919033 (diff 0.0294, 6037242 ULP); 17 of 27 ticks differ; largest tick 27 position.z: 3.203465 vs 2.430149 (diff 0.773, 3243522 ULP)
- `dt_back` acacia vs unastar: first tick 11 position.z: 0.046919033 vs 0.076319024 (diff 0.0294, 6037242 ULP); 17 of 27 ticks differ; largest tick 27 position.z: 2.430149 vs 3.203465 (diff 0.773, 3243522 ULP)
- `dt_gap_1` bedsim vs bds: first tick 8 position.z: 0.7787234 vs 0.80812335 (diff 0.0294, 493249 ULP); 20 of 27 ticks differ; largest tick 27 position.z: 3.9411492 vs 4.9087353 (diff 0.968, 2152594 ULP)
- `dt_gap_1` acacia vs bds: first tick 8 position.z: 0.7787234 vs 0.80812335 (diff 0.0294, 493249 ULP); 20 of 27 ticks differ; largest tick 27 position.z: 3.9411492 vs 4.9087353 (diff 0.968, 2152594 ULP)
- `dt_gap_1` unastar vs bedsim: first tick 8 position.z: 0.80812335 vs 0.7787234 (diff 0.0294, 493249 ULP); 20 of 27 ticks differ; largest tick 27 position.z: 4.9087353 vs 3.9411492 (diff 0.968, 2152594 ULP)
- `dt_gap_1` acacia vs unastar: first tick 8 position.z: 0.7787234 vs 0.80812335 (diff 0.0294, 493249 ULP); 20 of 27 ticks differ; largest tick 27 position.z: 3.9411492 vs 4.9087353 (diff 0.968, 2152594 ULP)
- `dt_gap_5` bedsim vs bds: first tick 12 position.z: 0.8107364 vs 0.8401364 (diff 0.0294, 493250 ULP); 20 of 31 ticks differ; largest tick 31 position.z: 3.941149 vs 4.908736 (diff 0.968, 2152597 ULP)
- `dt_gap_5` acacia vs bds: first tick 12 position.z: 0.8107364 vs 0.8401364 (diff 0.0294, 493250 ULP); 20 of 31 ticks differ; largest tick 31 position.z: 3.941149 vs 4.908736 (diff 0.968, 2152597 ULP)
- `dt_gap_5` unastar vs bedsim: first tick 12 position.z: 0.8401364 vs 0.8107364 (diff 0.0294, 493250 ULP); 20 of 31 ticks differ; largest tick 31 position.z: 4.908736 vs 3.941149 (diff 0.968, 2152597 ULP)
- `dt_gap_5` acacia vs unastar: first tick 12 position.z: 0.8107364 vs 0.8401364 (diff 0.0294, 493250 ULP); 20 of 31 ticks differ; largest tick 31 position.z: 3.941149 vs 4.908736 (diff 0.968, 2152597 ULP)
- `dt_half_then_full` bedsim vs bds: first tick 12 position.z: 1.3776135 vs 1.4070134 (diff 0.0294, 246624 ULP); 20 of 31 ticks differ; largest tick 31 position.z: 4.588726 vs 5.556312 (diff 0.968, 2029175 ULP)
- `dt_half_then_full` acacia vs bds: first tick 12 position.z: 1.3776135 vs 1.4070134 (diff 0.0294, 246624 ULP); 20 of 31 ticks differ; largest tick 31 position.z: 4.588726 vs 5.556312 (diff 0.968, 2029175 ULP)
- `dt_half_then_full` unastar vs bedsim: first tick 12 position.z: 1.4070134 vs 1.3776135 (diff 0.0294, 246624 ULP); 20 of 31 ticks differ; largest tick 31 position.z: 5.556312 vs 4.588726 (diff 0.968, 2029175 ULP)
- `dt_half_then_full` acacia vs unastar: first tick 12 position.z: 1.3776135 vs 1.4070134 (diff 0.0294, 246624 ULP); 20 of 31 ticks differ; largest tick 31 position.z: 4.588726 vs 5.556312 (diff 0.968, 2029175 ULP)
- `dt_strafe` bedsim vs bds: first tick 9 position.x: 1.0405982 vs 1.0613871 (diff 0.0208, 174390 ULP); 17 of 25 ticks differ; largest tick 25 position.z: 2.4753585 vs 3.0221756 (diff 0.547, 2293517 ULP)
- `dt_strafe` acacia vs bds: first tick 6 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 20 of 25 ticks differ; largest tick 25 position.z: 2.5156713 vs 3.0221756 (diff 0.507, 2124433 ULP)
- `dt_strafe` unastar vs bedsim: first tick 9 position.x: 1.0613871 vs 1.0405982 (diff 0.0208, 174390 ULP); 17 of 25 ticks differ; largest tick 25 position.z: 3.0221756 vs 2.4753585 (diff 0.547, 2293517 ULP)
- `dt_strafe` acacia vs bedsim: first tick 6 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 20 of 25 ticks differ; largest tick 25 position.x: 2.9473834 vs 2.9070704 (diff 0.0403, 169085 ULP)
- `dt_strafe` acacia vs unastar: first tick 6 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 20 of 25 ticks differ; largest tick 25 position.z: 2.5156713 vs 3.0221756 (diff 0.507, 2124433 ULP)
- `dt_three` bedsim vs bds: first tick 8 position.z: 0.5829254 vs 0.58967257 (diff 0.00675, 113199 ULP); 19 of 26 ticks differ; largest tick 26 position.z: 2.6458578 vs 3.2328863 (diff 0.587, 2462176 ULP)
- `dt_three` acacia vs bds: first tick 8 position.z: 0.5829254 vs 0.58967257 (diff 0.00675, 113199 ULP); 19 of 26 ticks differ; largest tick 26 position.z: 2.6458578 vs 3.2328863 (diff 0.587, 2462176 ULP)
- `dt_three` unastar vs bedsim: first tick 8 position.z: 0.58967257 vs 0.5829254 (diff 0.00675, 113199 ULP); 19 of 26 ticks differ; largest tick 26 position.z: 3.2328863 vs 2.6458578 (diff 0.587, 2462176 ULP)
- `dt_three` acacia vs unastar: first tick 8 position.z: 0.5829254 vs 0.58967257 (diff 0.00675, 113199 ULP); 19 of 26 ticks differ; largest tick 26 position.z: 2.6458578 vs 3.2328863 (diff 0.587, 2462176 ULP)
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
- `honey_wall_slide` acacia vs unastar: first tick 10 velocity.z: 0.0047685434 vs 0.029803395 (diff 0.025, 22537423 ULP); 1 of 50 ticks differ; largest tick 10 velocity.z: 0.0047685434 vs 0.029803395 (diff 0.025, 22537423 ULP)
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
- `knockback_ground` bedsim vs bds: first tick 42 velocity.z: 0 vs -0 (diff 0, 0 ULP); 1 of 45 ticks differ; largest tick 42 velocity.z: 0 vs -0 (diff 0, 0 ULP)
- `knockback_ground` acacia vs bds: first tick 41 position.z: -1.0094237 vs -1.0094236 (diff 1.19e-07, 1 ULP); 5 of 45 ticks differ; largest tick 41 position.z: -1.0094237 vs -1.0094236 (diff 1.19e-07, 1 ULP)
- `knockback_ground` unastar vs bedsim: first tick 39 velocity.z: -0 vs 0 (diff 0, 0 ULP); 7 of 45 ticks differ; largest tick 39 velocity.z: -0 vs 0 (diff 0, 0 ULP)
- `knockback_ground` acacia vs bedsim: first tick 39 velocity.x: 5.249494e-08 vs 0 (diff 5.25e-08, 862025420 ULP); 7 of 45 ticks differ; largest tick 41 position.z: -1.0094237 vs -1.0094236 (diff 1.19e-07, 1 ULP)
- `knockback_ground` acacia vs unastar: first tick 39 velocity.x: 5.249494e-08 vs 0 (diff 5.25e-08, 862025420 ULP); 7 of 45 ticks differ; largest tick 41 position.z: -1.0094237 vs -1.0094236 (diff 1.19e-07, 1 ULP)
- `knockback_sprint_air` bedsim vs bds: first tick 14 position.z: 1.5791676 vs 1.5791675 (diff 1.19e-07, 1 ULP); 14 of 50 ticks differ; largest tick 31 position.z: 4.2364926 vs 4.236492 (diff 4.77e-07, 1 ULP)
- `knockback_sprint_air` acacia vs bds: first tick 14 position.z: 1.5791676 vs 1.5791675 (diff 1.19e-07, 1 ULP); 14 of 50 ticks differ; largest tick 31 position.z: 4.2364926 vs 4.236492 (diff 4.77e-07, 1 ULP)
- `knockback_sprint_air` unastar vs bedsim: first tick 14 position.z: 1.5791675 vs 1.5791676 (diff 1.19e-07, 1 ULP); 16 of 50 ticks differ; largest tick 31 position.z: 4.236492 vs 4.2364926 (diff 4.77e-07, 1 ULP)
- `knockback_sprint_air` acacia vs unastar: first tick 14 position.z: 1.5791676 vs 1.5791675 (diff 1.19e-07, 1 ULP); 16 of 50 ticks differ; largest tick 31 position.z: 4.2364926 vs 4.236492 (diff 4.77e-07, 1 ULP)
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
- `levitation_ceiling` bedsim vs bds: first tick 1 on_ground: True vs False; 23 of 55 ticks differ; largest tick 16 position.y: 102.73407 vs 102.66033 (diff 0.0737, 9665 ULP)
- `levitation_ceiling` acacia vs bds: first tick 1 on_ground: True vs False; 10 of 55 ticks differ; largest tick 8 velocity.y: 0.07778893 vs 0.077788934 (diff 7.45e-09, 1 ULP)
- `levitation_ceiling` unastar vs bedsim: first tick 1 velocity.y: 0.0196 vs 0.020000001 (diff 0.0004, 214749 ULP); 55 of 55 ticks differ; largest tick 16 position.y: 102.66033 vs 102.73407 (diff 0.0737, 9665 ULP)
- `levitation_ceiling` acacia vs bedsim: first tick 1 velocity.y: 0.019600002 vs 0.020000001 (diff 0.0004, 214748 ULP); 55 of 55 ticks differ; largest tick 16 position.y: 102.66033 vs 102.73407 (diff 0.0737, 9665 ULP)
- `levitation_ceiling` acacia vs unastar: first tick 1 velocity.y: 0.019600002 vs 0.0196 (diff 1.86e-09, 1 ULP); 51 of 55 ticks differ; largest tick 11 velocity.y: 0.0844994 vs 0.08449941 (diff 1.49e-08, 2 ULP)
- `levitation_float` bedsim vs bds: first tick 1 on_ground: True vs False; 65 of 65 ticks differ; largest tick 65 position.y: 104.620026 vs 104.35908 (diff 0.261, 34203 ULP)
- `levitation_float` acacia vs bds: first tick 1 on_ground: True vs False; 7 of 65 ticks differ; largest tick 57 velocity.y: 0.04537033 vs 0.045370337 (diff 7.45e-09, 2 ULP)
- `levitation_float` unastar vs bedsim: first tick 1 velocity.y: 0.0098 vs 0.010000001 (diff 0.0002, 214749 ULP); 65 of 65 ticks differ; largest tick 65 position.y: 104.35908 vs 104.620026 (diff 0.261, 34203 ULP)
- `levitation_float` acacia vs bedsim: first tick 1 velocity.y: 0.009800001 vs 0.010000001 (diff 0.0002, 214748 ULP); 65 of 65 ticks differ; largest tick 65 position.y: 104.35908 vs 104.620026 (diff 0.261, 34203 ULP)
- `levitation_float` acacia vs unastar: first tick 1 velocity.y: 0.009800001 vs 0.0098 (diff 9.31e-10, 1 ULP); 40 of 65 ticks differ; largest tick 11 velocity.y: 0.0422497 vs 0.042249706 (diff 7.45e-09, 2 ULP)
- `levitation_jump` bedsim vs bds: first tick 1 on_ground: True vs False; 36 of 36 ticks differ; largest tick 36 position.y: 103.170074 vs 103.04333 (diff 0.127, 16613 ULP)
- `levitation_jump` acacia vs bds: first tick 1 on_ground: True vs False; 4 of 36 ticks differ; largest tick 8 velocity.y: 0.038894463 vs 0.038894467 (diff 3.73e-09, 1 ULP)
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
- `powder_snow_walk_into` unastar vs bds: first tick 18 position.z: 3.0466661 vs 3.0247734 (diff 0.0219, 91825 ULP); 38 of 55 ticks differ; largest tick 45 position.z: 5.5341005 vs 5.2789145 (diff 0.255, 535164 ULP)
- `powder_snow_walk_into` bedsim vs bds: first tick 17 position.z: 2.7 vs 2.8308897 (diff 0.131, 548991 ULP); 39 of 55 ticks differ; largest tick 45 position.z: 2.7 vs 5.2789145 (diff 2.58, 8134673 ULP)
- `powder_snow_walk_into` acacia vs bds: first tick 33 position.z: 4.3052483 vs 4.305564 (diff 0.000316, 662 ULP); 23 of 55 ticks differ; largest tick 33 position.z: 4.3052483 vs 4.305564 (diff 0.000316, 662 ULP)
- `powder_snow_walk_into` unastar vs bedsim: first tick 17 position.z: 2.8308897 vs 2.7 (diff 0.131, 548991 ULP); 39 of 55 ticks differ; largest tick 45 position.z: 5.5341005 vs 2.7 (diff 2.83, 8669837 ULP)
- `powder_snow_walk_into` acacia vs bedsim: first tick 17 position.z: 2.8308897 vs 2.7 (diff 0.131, 548991 ULP); 39 of 55 ticks differ; largest tick 45 position.z: 5.278599 vs 2.7 (diff 2.58, 8134011 ULP)
- `powder_snow_walk_into` acacia vs unastar: first tick 18 position.z: 3.0247734 vs 3.0466661 (diff 0.0219, 91825 ULP); 38 of 55 ticks differ; largest tick 45 position.z: 5.278599 vs 5.5341005 (diff 0.256, 535826 ULP)
- `random_135` bedsim vs bds: first tick 6 velocity.z: 0.117241286 vs 0.1172413 (diff 1.49e-08, 2 ULP); 25 of 45 ticks differ; largest tick 45 position.x: 1.6119807 vs 1.469682 (diff 0.142, 1193688 ULP)
- `random_135` acacia vs bds: first tick 4 position.x: 0.5438208 vs 0.5429444 (diff 0.000876, 14704 ULP); 42 of 45 ticks differ; largest tick 43 position.z: -1.4736526 vs -1.4970133 (diff 0.0234, 195964 ULP)
- `random_135` unastar vs bedsim: first tick 6 velocity.z: 0.1172413 vs 0.117241286 (diff 1.49e-08, 2 ULP); 39 of 45 ticks differ; largest tick 45 position.x: 1.469682 vs 1.6119807 (diff 0.142, 1193688 ULP)
- `random_135` acacia vs bedsim: first tick 4 position.x: 0.5438208 vs 0.5429444 (diff 0.000876, 14704 ULP); 42 of 45 ticks differ; largest tick 45 position.x: 1.4796314 vs 1.6119807 (diff 0.132, 1110226 ULP)
- `random_135` acacia vs unastar: first tick 4 position.x: 0.5438208 vs 0.5429444 (diff 0.000876, 14704 ULP); 42 of 45 ticks differ; largest tick 43 position.z: -1.4736526 vs -1.4970133 (diff 0.0234, 195964 ULP)
- `random_14` unastar vs bds: first tick 54 velocity.x: -0.061980594 vs -0.06198058 (diff 1.49e-08, 4 ULP); 34 of 87 ticks differ; largest tick 66 position.x: -2.1466575 vs -2.146657 (diff 4.77e-07, 2 ULP)
- `random_14` bedsim vs bds: first tick 2 position.y: 101.62001 vs 101.54161 (diff 0.0784, 10276 ULP); 86 of 87 ticks differ; largest tick 3 position.y: 101.62001 vs 101.49501 (diff 0.125, 16384 ULP)
- `random_14` acacia vs bds: first tick 31 position.x: 0.80518925 vs 0.80529284 (diff 0.000104, 1738 ULP); 57 of 87 ticks differ; largest tick 87 position.x: -2.9781826 vs -4.5773907 (diff 1.6, 5496689 ULP)
- `random_14` unastar vs bedsim: first tick 2 position.y: 101.54161 vs 101.62001 (diff 0.0784, 10276 ULP); 86 of 87 ticks differ; largest tick 3 position.y: 101.49501 vs 101.62001 (diff 0.125, 16384 ULP)
- `random_14` acacia vs bedsim: first tick 2 position.y: 101.54161 vs 101.62001 (diff 0.0784, 10276 ULP); 86 of 87 ticks differ; largest tick 87 position.x: -2.9781826 vs -4.6799517 (diff 1.7, 5711775 ULP)
- `random_14` acacia vs unastar: first tick 31 position.x: 0.80518925 vs 0.80529284 (diff 0.000104, 1738 ULP); 57 of 87 ticks differ; largest tick 87 position.x: -2.9781826 vs -4.577391 (diff 1.6, 5496690 ULP)
- `random_172` unastar vs bds: first tick 6 velocity.z: 0.104619995 vs 0.10461997 (diff 2.24e-08, 3 ULP); 51 of 58 ticks differ; largest tick 23 position.z: -0.84018433 vs -0.8401848 (diff 4.77e-07, 8 ULP)
- `random_172` bedsim vs bds: first tick 6 position.x: 0.10237469 vs 0.10237467 (diff 1.49e-08, 2 ULP); 53 of 58 ticks differ; largest tick 58 position.x: -5.39314 vs -5.255267 (diff 0.138, 289140 ULP)
- `random_172` acacia vs bds: first tick 6 position.x: 0.10237469 vs 0.10237467 (diff 1.49e-08, 2 ULP); 53 of 58 ticks differ; largest tick 58 position.x: -5.364021 vs -5.255267 (diff 0.109, 228073 ULP)
- `random_172` unastar vs bedsim: first tick 5 velocity.x: -0.17703961 vs -0.1770396 (diff 1.49e-08, 1 ULP); 54 of 58 ticks differ; largest tick 58 position.x: -5.255267 vs -5.39314 (diff 0.138, 289140 ULP)
- `random_172` acacia vs bedsim: first tick 5 velocity.z: 0.102226995 vs 0.102227025 (diff 2.98e-08, 4 ULP); 54 of 58 ticks differ; largest tick 58 position.z: -1.1880676 vs -1.2828014 (diff 0.0947, 794685 ULP)
- `random_172` acacia vs unastar: first tick 5 velocity.x: -0.1770396 vs -0.17703961 (diff 1.49e-08, 1 ULP); 54 of 58 ticks differ; largest tick 58 position.x: -5.364021 vs -5.255267 (diff 0.109, 228073 ULP)
- `random_186` unastar vs bds: first tick 32 position.y: 102.04001 vs 101.63001 (diff 0.41, 53739 ULP); 83 of 114 ticks differ; largest tick 114 position.x: 2.8774436 vs 0.97541773 (diff 1.9, 12481295 ULP)
- `random_186` bedsim vs bds: first tick 30 position.z: 1.9917675 vs 1.9917674 (diff 1.19e-07, 1 ULP); 68 of 114 ticks differ; largest tick 36 position.z: 2.184067 vs 2.1840668 (diff 2.38e-07, 1 ULP)
- `random_186` acacia vs bds: first tick 5 position.x: 0.46635354 vs 0.46702647 (diff 0.000673, 22580 ULP); 110 of 114 ticks differ; largest tick 114 position.z: 0.84103715 vs 2.3745148 (diff 1.53, 12626391 ULP)
- `random_186` unastar vs bedsim: first tick 26 velocity.z: 0.022473928 vs 0.02247393 (diff 1.86e-09, 1 ULP); 88 of 114 ticks differ; largest tick 114 position.x: 2.8774436 vs 0.9754175 (diff 1.9, 12481299 ULP)
- `random_186` acacia vs bedsim: first tick 5 position.x: 0.46635354 vs 0.46702647 (diff 0.000673, 22580 ULP); 110 of 114 ticks differ; largest tick 114 position.z: 0.84103715 vs 2.3745148 (diff 1.53, 12626391 ULP)
- `random_186` acacia vs unastar: first tick 5 position.x: 0.46635354 vs 0.46702647 (diff 0.000673, 22580 ULP); 110 of 114 ticks differ; largest tick 114 position.x: 0.6799164 vs 2.8774436 (diff 2.2, 17438985 ULP)
- `random_248` bedsim vs bds: first tick 16 position.y: 101.97754 vs 101.99841 (diff 0.0209, 2736 ULP); 65 of 80 ticks differ; largest tick 39 position.y: 101.62001 vs 102.30375 (diff 0.684, 89619 ULP)
- `random_248` acacia vs bds: first tick 4 position.x: 0.458669 vs 0.4594956 (diff 0.000827, 27736 ULP); 77 of 80 ticks differ; largest tick 41 position.z: 2.6628141 vs 2.560051 (diff 0.103, 431020 ULP)
- `random_248` unastar vs bedsim: first tick 14 velocity.z: 0.046209577 vs 0.04620958 (diff 3.73e-09, 1 ULP); 67 of 80 ticks differ; largest tick 39 position.y: 102.30375 vs 101.62001 (diff 0.684, 89619 ULP)
- `random_248` acacia vs bedsim: first tick 4 position.x: 0.458669 vs 0.4594956 (diff 0.000827, 27736 ULP); 77 of 80 ticks differ; largest tick 39 position.y: 102.30375 vs 101.62001 (diff 0.684, 89619 ULP)
- `random_248` acacia vs unastar: first tick 4 position.x: 0.458669 vs 0.4594956 (diff 0.000827, 27736 ULP); 77 of 80 ticks differ; largest tick 41 position.z: 2.6628141 vs 2.560051 (diff 0.103, 431020 ULP)
- `random_291` unastar vs bds: first tick 72 velocity.y: -0.04195231 vs -0.04195228 (diff 2.98e-08, 8 ULP); 1 of 95 ticks differ; largest tick 72 velocity.y: -0.04195231 vs -0.04195228 (diff 2.98e-08, 8 ULP)
- `random_291` bedsim vs bds: first tick 13 position.x: 0.4819829 vs 0.48198295 (diff 5.96e-08, 2 ULP); 83 of 95 ticks differ; largest tick 95 position.y: 101.62001 vs 94.78734 (diff 6.83, 895572 ULP)
- `random_291` acacia vs bds: first tick 25 position.x: -0.7045797 vs -0.7043682 (diff 0.000212, 3549 ULP); 71 of 95 ticks differ; largest tick 95 position.x: -10.755991 vs -10.5920105 (diff 0.164, 171946 ULP)
- `random_291` unastar vs bedsim: first tick 13 position.x: 0.48198295 vs 0.4819829 (diff 5.96e-08, 2 ULP); 83 of 95 ticks differ; largest tick 95 position.y: 94.78734 vs 101.62001 (diff 6.83, 895572 ULP)
- `random_291` acacia vs bedsim: first tick 13 position.x: 0.48198295 vs 0.4819829 (diff 5.96e-08, 2 ULP); 83 of 95 ticks differ; largest tick 95 position.y: 94.78734 vs 101.62001 (diff 6.83, 895572 ULP)
- `random_291` acacia vs unastar: first tick 13 velocity.x: -0.016395545 vs -0.016395543 (diff 1.86e-09, 1 ULP); 75 of 95 ticks differ; largest tick 95 position.x: -10.755991 vs -10.5920105 (diff 0.164, 171946 ULP)
- `random_297` bedsim vs bds: first tick 2 position.y: 101.62001 vs 101.54161 (diff 0.0784, 10276 ULP); 103 of 104 ticks differ; largest tick 66 position.x: 3.0487528 vs 2.8497214 (diff 0.199, 834798 ULP)
- `random_297` acacia vs bds: first tick 5 position.x: 0.55146664 vs 0.5504373 (diff 0.00103, 17270 ULP); 100 of 104 ticks differ; largest tick 66 position.x: 2.9972749 vs 2.8497214 (diff 0.148, 618884 ULP)
- `random_297` unastar vs bedsim: first tick 2 position.y: 101.54161 vs 101.62001 (diff 0.0784, 10276 ULP); 103 of 104 ticks differ; largest tick 66 position.x: 2.8497214 vs 3.0487528 (diff 0.199, 834798 ULP)
- `random_297` acacia vs bedsim: first tick 2 position.y: 101.54161 vs 101.62001 (diff 0.0784, 10276 ULP); 103 of 104 ticks differ; largest tick 3 position.y: 101.49501 vs 101.62001 (diff 0.125, 16384 ULP)
- `random_297` acacia vs unastar: first tick 5 position.x: 0.55146664 vs 0.5504373 (diff 0.00103, 17270 ULP); 100 of 104 ticks differ; largest tick 66 position.x: 2.9972749 vs 2.8497214 (diff 0.148, 618884 ULP)
- `random_66` unastar vs bds: first tick 60 position.x: -2.4263253 vs -2.4263258 (diff 4.77e-07, 2 ULP); 15 of 81 ticks differ; largest tick 60 position.x: -2.4263253 vs -2.4263258 (diff 4.77e-07, 2 ULP)
- `random_66` bedsim vs bds: first tick 12 position.x: 0.4913932 vs 0.497418 (diff 0.00602, 202158 ULP); 70 of 81 ticks differ; largest tick 81 position.z: 7.0467353 vs 6.9122996 (diff 0.134, 281932 ULP)
- `random_66` acacia vs bds: first tick 60 position.x: -2.4266887 vs -2.4263258 (diff 0.000363, 1522 ULP); 22 of 81 ticks differ; largest tick 81 position.z: 6.6787534 vs 6.9122996 (diff 0.234, 489782 ULP)
- `random_66` unastar vs bedsim: first tick 12 position.x: 0.497418 vs 0.4913932 (diff 0.00602, 202158 ULP); 70 of 81 ticks differ; largest tick 81 position.z: 6.9122996 vs 7.0467353 (diff 0.134, 281932 ULP)
- `random_66` acacia vs bedsim: first tick 12 position.x: 0.497418 vs 0.4913932 (diff 0.00602, 202158 ULP); 70 of 81 ticks differ; largest tick 81 position.z: 6.6787534 vs 7.0467353 (diff 0.368, 771714 ULP)
- `random_66` acacia vs unastar: first tick 60 position.x: -2.4266887 vs -2.4263253 (diff 0.000363, 1524 ULP); 22 of 81 ticks differ; largest tick 81 position.z: 6.6787534 vs 6.9122996 (diff 0.234, 489782 ULP)
- `random_a123` bedsim vs bds: first tick 6 velocity.x: 0.02927781 vs 0.029277807 (diff 3.73e-09, 2 ULP); 67 of 82 ticks differ; largest tick 80 position.x: -2.2322042 vs -2.413937 (diff 0.182, 762243 ULP)
- `random_a123` acacia vs bds: first tick 4 position.x: 0.53735524 vs 0.5366081 (diff 0.000747, 12535 ULP); 79 of 82 ticks differ; largest tick 55 position.x: 0.61816967 vs -0.918962 (diff 1.54, 2122940790 ULP)
- `random_a123` unastar vs bedsim: first tick 6 velocity.x: 0.029277807 vs 0.02927781 (diff 3.73e-09, 2 ULP); 71 of 82 ticks differ; largest tick 80 position.x: -2.413937 vs -2.2322042 (diff 0.182, 762243 ULP)
- `random_a123` acacia vs bedsim: first tick 4 position.x: 0.53735524 vs 0.5366081 (diff 0.000747, 12535 ULP); 79 of 82 ticks differ; largest tick 34 position.x: -1.8426594 vs -3.3217726 (diff 1.48, 6863785 ULP)
- `random_a123` acacia vs unastar: first tick 4 position.x: 0.53735524 vs 0.5366081 (diff 0.000747, 12535 ULP); 79 of 82 ticks differ; largest tick 55 position.x: 0.61816967 vs -0.918962 (diff 1.54, 2122940790 ULP)
- `random_a141` unastar vs bds: first tick 58 position.y: 102.04001 vs 101.63001 (diff 0.41, 53739 ULP); 48 of 105 ticks differ; largest tick 105 position.x: -6.069132 vs 0.26047918 (diff 6.33, 2135397339 ULP)
- `random_a141` bedsim vs bds: first tick 6 position.x: 0.52032024 vs 0.5203202 (diff 5.96e-08, 1 ULP); 100 of 105 ticks differ; largest tick 105 position.x: 1.9313303 vs 0.26047918 (diff 1.67, 24238158 ULP)
- `random_a141` acacia vs bds: first tick 6 position.x: 0.52052134 vs 0.5203202 (diff 0.000201, 3375 ULP); 100 of 105 ticks differ; largest tick 102 position.z: 2.3022223 vs 1.8214977 (diff 0.481, 2764998 ULP)
- `random_a141` unastar vs bedsim: first tick 6 position.x: 0.5203202 vs 0.52032024 (diff 5.96e-08, 1 ULP); 100 of 105 ticks differ; largest tick 105 position.x: -6.069132 vs 1.9313303 (diff 8, 2159635497 ULP)
- `random_a141` acacia vs bedsim: first tick 6 position.x: 0.52052134 vs 0.52032024 (diff 0.000201, 3374 ULP); 100 of 105 ticks differ; largest tick 105 position.x: 0.0029557794 vs 1.9313303 (diff 1.93, 79003669 ULP)
- `random_a141` acacia vs unastar: first tick 6 position.x: 0.52052134 vs 0.5203202 (diff 0.000201, 3375 ULP); 100 of 105 ticks differ; largest tick 105 position.x: 0.0029557794 vs -6.069132 (diff 6.07, 2080631828 ULP)
- `random_a146` bedsim vs bds: first tick 40 position.z: -1.3709474 vs -1.3709475 (diff 1.19e-07, 1 ULP); 57 of 96 ticks differ; largest tick 69 position.y: 102.869194 vs 101.62001 (diff 1.25, 163733 ULP)
- `random_a146` acacia vs bds: first tick 36 velocity.z: 9.112231e-09 vs 0 (diff 9.11e-09, 840731652 ULP); 60 of 96 ticks differ; largest tick 96 position.z: 2.3786182 vs 2.3000226 (diff 0.0786, 329654 ULP)
- `random_a146` unastar vs bedsim: first tick 38 velocity.x: 0.05063335 vs 0.050633345 (diff 3.73e-09, 1 ULP); 59 of 96 ticks differ; largest tick 69 position.y: 101.62001 vs 102.869194 (diff 1.25, 163733 ULP)
- `random_a146` acacia vs bedsim: first tick 34 velocity.z: 1.3754725e-08 vs 0 (diff 1.38e-08, 845958636 ULP); 62 of 96 ticks differ; largest tick 69 position.y: 101.62001 vs 102.869194 (diff 1.25, 163733 ULP)
- `random_a146` acacia vs unastar: first tick 34 velocity.z: 1.3754725e-08 vs 0 (diff 1.38e-08, 845958636 ULP); 62 of 96 ticks differ; largest tick 96 position.z: 2.3786182 vs 2.3000226 (diff 0.0786, 329654 ULP)
- `random_a27` bedsim vs bds: first tick 40 position.x: 0.5018565 vs 0.5014281 (diff 0.000428, 7187 ULP); 24 of 63 ticks differ; largest tick 54 position.z: 0.7 vs 0.46344274 (diff 0.237, 4582101 ULP)
- `random_a27` unastar vs bedsim: first tick 40 position.x: 0.5014281 vs 0.5018565 (diff 0.000428, 7187 ULP); 24 of 63 ticks differ; largest tick 54 position.z: 0.46344274 vs 0.7 (diff 0.237, 4582101 ULP)
- `random_a27` acacia vs bedsim: first tick 40 position.x: 0.5014281 vs 0.5018565 (diff 0.000428, 7187 ULP); 24 of 63 ticks differ; largest tick 54 position.z: 0.46344274 vs 0.7 (diff 0.237, 4582101 ULP)
- `random_a30` bedsim vs bds: first tick 19 position.z: -0.89294153 vs -0.90764153 (diff 0.0147, 246625 ULP); 39 of 57 ticks differ; largest tick 57 position.z: 4.9063315 vs 3.5956874 (diff 1.31, 3596525 ULP)
- `random_a30` acacia vs bds: first tick 31 position.x: 0.5287348 vs 0.5281601 (diff 0.000575, 9642 ULP); 27 of 57 ticks differ; largest tick 57 position.z: 3.6584146 vs 3.5956874 (diff 0.0627, 263097 ULP)
- `random_a30` unastar vs bedsim: first tick 19 position.z: -0.90764153 vs -0.89294153 (diff 0.0147, 246625 ULP); 39 of 57 ticks differ; largest tick 57 position.z: 3.5956874 vs 4.9063315 (diff 1.31, 3596525 ULP)
- `random_a30` acacia vs bedsim: first tick 19 position.z: -0.90764153 vs -0.89294153 (diff 0.0147, 246625 ULP); 39 of 57 ticks differ; largest tick 57 position.z: 3.6584146 vs 4.9063315 (diff 1.25, 3333428 ULP)
- `random_a30` acacia vs unastar: first tick 31 position.x: 0.5287348 vs 0.5281601 (diff 0.000575, 9642 ULP); 27 of 57 ticks differ; largest tick 57 position.z: 3.6584146 vs 3.5956874 (diff 0.0627, 263097 ULP)
- `random_a67` bedsim vs bds: first tick 2 position.y: 101.62001 vs 101.54161 (diff 0.0784, 10276 ULP); 90 of 91 ticks differ; largest tick 3 position.y: 101.62001 vs 101.49501 (diff 0.125, 16384 ULP)
- `random_a67` acacia vs bds: first tick 19 position.x: 0.18940803 vs 0.18979439 (diff 0.000386, 25928 ULP); 73 of 91 ticks differ; largest tick 91 position.z: -2.1885216 vs -2.1484187 (diff 0.0401, 168204 ULP)
- `random_a67` unastar vs bedsim: first tick 2 position.y: 101.54161 vs 101.62001 (diff 0.0784, 10276 ULP); 90 of 91 ticks differ; largest tick 3 position.y: 101.49501 vs 101.62001 (diff 0.125, 16384 ULP)
- `random_a67` acacia vs bedsim: first tick 2 position.y: 101.54161 vs 101.62001 (diff 0.0784, 10276 ULP); 90 of 91 ticks differ; largest tick 3 position.y: 101.49501 vs 101.62001 (diff 0.125, 16384 ULP)
- `random_a67` acacia vs unastar: first tick 19 position.x: 0.18940803 vs 0.18979439 (diff 0.000386, 25928 ULP); 73 of 91 ticks differ; largest tick 91 position.z: -2.1885216 vs -2.1484187 (diff 0.0401, 168204 ULP)
- `random_a7` bedsim vs bds: first tick 17 position.y: 101.97754 vs 101.99841 (diff 0.0209, 2736 ULP); 65 of 81 ticks differ; largest tick 25 position.y: 101.89746 vs 102.070984 (diff 0.174, 22744 ULP)
- `random_a7` acacia vs bds: first tick 11 position.x: 0.51414216 vs 0.5138593 (diff 0.000283, 4746 ULP); 71 of 81 ticks differ; largest tick 81 position.x: -0.27785832 vs -0.48168528 (diff 0.204, 6839298 ULP)
- `random_a7` unastar vs bedsim: first tick 16 velocity.y: 0.3784058 vs 0.35752946 (diff 0.0209, 700494 ULP); 66 of 81 ticks differ; largest tick 26 velocity.y: -0.4078895 vs -0.0784 (diff 0.329, 19941046 ULP)
- `random_a7` acacia vs bedsim: first tick 11 position.x: 0.51414216 vs 0.5138593 (diff 0.000283, 4746 ULP); 71 of 81 ticks differ; largest tick 26 velocity.y: -0.4078895 vs -0.0784 (diff 0.329, 19941046 ULP)
- `random_a7` acacia vs unastar: first tick 11 position.x: 0.51414216 vs 0.5138593 (diff 0.000283, 4746 ULP); 71 of 81 ticks differ; largest tick 81 position.x: -0.27785832 vs -0.48168528 (diff 0.204, 6839298 ULP)
- `random_a90` bedsim vs bds: first tick 43 position.x: -2.783612 vs -2.7944431 (diff 0.0108, 45429 ULP); 39 of 81 ticks differ; largest tick 73 position.y: 101.62001 vs 102.16441 (diff 0.544, 71356 ULP)
- `random_a90` acacia vs bds: first tick 4 position.x: 0.48421115 vs 0.48452693 (diff 0.000316, 10596 ULP); 78 of 81 ticks differ; largest tick 63 position.z: 0.6866747 vs 0.2491945 (diff 0.437, 11574546 ULP)
- `random_a90` unastar vs bedsim: first tick 43 position.x: -2.7944431 vs -2.783612 (diff 0.0108, 45429 ULP); 39 of 81 ticks differ; largest tick 73 position.y: 102.16441 vs 101.62001 (diff 0.544, 71356 ULP)
- `random_a90` acacia vs bedsim: first tick 4 position.x: 0.48421115 vs 0.48452693 (diff 0.000316, 10596 ULP); 78 of 81 ticks differ; largest tick 73 position.y: 102.19675 vs 101.62001 (diff 0.577, 75594 ULP)
- `random_a90` acacia vs unastar: first tick 4 position.x: 0.48421115 vs 0.48452693 (diff 0.000316, 10596 ULP); 78 of 81 ticks differ; largest tick 63 position.z: 0.6866747 vs 0.2491945 (diff 0.437, 11574546 ULP)
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
- `sprint_jump_yaw_37_3` bedsim vs bds: first tick 9 position.x: -0.11506109 vs -0.11506112 (diff 2.98e-08, 4 ULP); 19 of 27 ticks differ; largest tick 22 position.z: 4.470188 vs 4.470187 (diff 9.54e-07, 2 ULP)
- `sprint_jump_yaw_37_3` acacia vs bds: first tick 6 velocity.x: -0.0421528 vs -0.042152796 (diff 3.73e-09, 1 ULP); 17 of 27 ticks differ; largest tick 21 position.z: 4.19718 vs 4.1971807 (diff 9.54e-07, 2 ULP)
- `sprint_jump_yaw_37_3` unastar vs bedsim: first tick 9 position.x: -0.11506112 vs -0.11506109 (diff 2.98e-08, 4 ULP); 19 of 27 ticks differ; largest tick 22 position.z: 4.470187 vs 4.470188 (diff 9.54e-07, 2 ULP)
- `sprint_jump_yaw_37_3` acacia vs bedsim: first tick 6 velocity.x: -0.0421528 vs -0.042152796 (diff 3.73e-09, 1 ULP); 22 of 27 ticks differ; largest tick 23 position.z: 4.718623 vs 4.7186246 (diff 1.43e-06, 3 ULP)
- `sprint_jump_yaw_37_3` acacia vs unastar: first tick 6 velocity.x: -0.0421528 vs -0.042152796 (diff 3.73e-09, 1 ULP); 19 of 27 ticks differ; largest tick 21 position.z: 4.19718 vs 4.1971807 (diff 9.54e-07, 2 ULP)
- `sprint_jump_yaw_m71_9` bedsim vs bds: first tick 6 velocity.z: 0.021610778 vs 0.021610776 (diff 1.86e-09, 1 ULP); 13 of 27 ticks differ; largest tick 19 position.x: 4.098793 vs 4.0987926 (diff 4.77e-07, 1 ULP)
- `sprint_jump_yaw_m71_9` acacia vs bds: first tick 18 velocity.x: 0.24128899 vs 0.24128896 (diff 2.98e-08, 2 ULP); 2 of 27 ticks differ; largest tick 18 velocity.x: 0.24128899 vs 0.24128896 (diff 2.98e-08, 2 ULP)
- `sprint_jump_yaw_m71_9` unastar vs bedsim: first tick 6 velocity.z: 0.021610776 vs 0.021610778 (diff 1.86e-09, 1 ULP); 20 of 27 ticks differ; largest tick 19 position.x: 4.0987926 vs 4.098793 (diff 4.77e-07, 1 ULP)
- `sprint_jump_yaw_m71_9` acacia vs bedsim: first tick 6 velocity.z: 0.021610776 vs 0.021610778 (diff 1.86e-09, 1 ULP); 19 of 27 ticks differ; largest tick 19 position.x: 4.0987926 vs 4.098793 (diff 4.77e-07, 1 ULP)
- `sprint_jump_yaw_m71_9` acacia vs unastar: first tick 11 velocity.x: 0.23792852 vs 0.23792851 (diff 1.49e-08, 1 ULP); 14 of 27 ticks differ; largest tick 15 velocity.x: 0.24011384 vs 0.2401138 (diff 4.47e-08, 3 ULP)
- `sprint_sneak_toggle` bedsim vs bds: first tick 31 position.z: 5.4413733 vs 5.3433733 (diff 0.098, 205521 ULP); 25 of 55 ticks differ; largest tick 50 position.z: 9.5497055 vs 9.333849 (diff 0.216, 226342 ULP)
- `sprint_sneak_toggle` unastar vs bedsim: first tick 31 position.z: 5.3433733 vs 5.4413733 (diff 0.098, 205521 ULP); 25 of 55 ticks differ; largest tick 50 position.z: 9.333849 vs 9.5497055 (diff 0.216, 226342 ULP)
- `sprint_sneak_toggle` acacia vs bedsim: first tick 31 position.z: 5.3433733 vs 5.4413733 (diff 0.098, 205521 ULP); 25 of 55 ticks differ; largest tick 50 position.z: 9.333849 vs 9.5497055 (diff 0.216, 226342 ULP)
- `sprint_turn_jump` bedsim vs bds: first tick 10 position.x: 0.35615793 vs 0.35615796 (diff 2.98e-08, 1 ULP); 45 of 55 ticks differ; largest tick 32 position.x: -4.5256166 vs -4.5256147 (diff 1.91e-06, 4 ULP)
- `sprint_turn_jump` acacia vs bds: first tick 17 position.x: -0.22081888 vs -0.22081886 (diff 1.49e-08, 1 ULP); 38 of 55 ticks differ; largest tick 55 position.x: -7.0753837 vs -7.0754824 (diff 9.87e-05, 207 ULP)
- `sprint_turn_jump` unastar vs bedsim: first tick 8 velocity.x: -0.022565037 vs -0.022565039 (diff 1.86e-09, 1 ULP); 48 of 55 ticks differ; largest tick 32 position.x: -4.5256147 vs -4.5256166 (diff 1.91e-06, 4 ULP)
- `sprint_turn_jump` acacia vs bedsim: first tick 8 velocity.x: -0.022565037 vs -0.022565039 (diff 1.86e-09, 1 ULP); 38 of 55 ticks differ; largest tick 55 position.x: -7.0753837 vs -7.0754843 (diff 0.000101, 211 ULP)
- `sprint_turn_jump` acacia vs unastar: first tick 14 velocity.x: -0.0741906 vs -0.074190594 (diff 7.45e-09, 1 ULP); 41 of 55 ticks differ; largest tick 55 position.x: -7.0753837 vs -7.0754824 (diff 9.87e-05, 207 ULP)
- `sr_air` bedsim vs bds: first tick 11 position.z: 0.91355056 vs 0.8998306 (diff 0.0137, 230183 ULP); 20 of 30 ticks differ; largest tick 30 position.z: 3.5845144 vs 3.4952369 (diff 0.0893, 374457 ULP)
- `sr_air` unastar vs bedsim: first tick 11 position.z: 0.8998306 vs 0.91355056 (diff 0.0137, 230183 ULP); 20 of 30 ticks differ; largest tick 30 position.z: 3.4952369 vs 3.5845144 (diff 0.0893, 374457 ULP)
- `sr_air` acacia vs bedsim: first tick 11 position.z: 0.8998306 vs 0.91355056 (diff 0.0137, 230183 ULP); 20 of 30 ticks differ; largest tick 30 position.z: 3.4952369 vs 3.5845144 (diff 0.0893, 374457 ULP)
- `sr_back` bedsim vs bds: first tick 16 position.z: -0.20315468 vs -0.13455468 (diff 0.0686, 4603668 ULP); 20 of 35 ticks differ; largest tick 35 position.z: -3.372867 vs -3.2217665 (diff 0.151, 633762 ULP)
- `sr_back` unastar vs bedsim: first tick 16 position.z: -0.13455468 vs -0.20315468 (diff 0.0686, 4603668 ULP); 20 of 35 ticks differ; largest tick 35 position.z: -3.2217665 vs -3.372867 (diff 0.151, 633762 ULP)
- `sr_back` acacia vs bedsim: first tick 16 position.z: -0.13455468 vs -0.20315468 (diff 0.0686, 4603668 ULP); 20 of 35 ticks differ; largest tick 35 position.z: -3.2217665 vs -3.372867 (diff 0.151, 633762 ULP)
- `sr_edge` bedsim vs bds: first tick 46 position.z: 3.145788 vs 3.077188 (diff 0.0686, 287729 ULP); 20 of 65 ticks differ; largest tick 60 position.y: 98.92545 vs 99.51046 (diff 0.585, 76678 ULP)
- `sr_edge` unastar vs bedsim: first tick 46 position.z: 3.077188 vs 3.145788 (diff 0.0686, 287729 ULP); 20 of 65 ticks differ; largest tick 60 position.y: 99.51046 vs 98.92545 (diff 0.585, 76678 ULP)
- `sr_edge` acacia vs bedsim: first tick 46 position.z: 3.077188 vs 3.145788 (diff 0.0686, 287729 ULP); 20 of 65 ticks differ; largest tick 60 position.y: 99.51046 vs 98.92545 (diff 0.585, 76678 ULP)
- `sr_ice_still_forward` bedsim vs bds: first tick 12 position.x: 0.4913932 vs 0.497418 (diff 0.00602, 202158 ULP); 20 of 31 ticks differ; largest tick 31 position.z: 2.648004 vs 2.527186 (diff 0.121, 506748 ULP)
- `sr_ice_still_forward` unastar vs bedsim: first tick 12 position.x: 0.497418 vs 0.4913932 (diff 0.00602, 202158 ULP); 20 of 31 ticks differ; largest tick 31 position.z: 2.527186 vs 2.648004 (diff 0.121, 506748 ULP)
- `sr_ice_still_forward` acacia vs bedsim: first tick 12 position.x: 0.497418 vs 0.4913932 (diff 0.00602, 202158 ULP); 20 of 31 ticks differ; largest tick 31 position.z: 2.527186 vs 2.648004 (diff 0.121, 506748 ULP)
- `sr_jump` bedsim vs bds: first tick 16 position.z: 1.2031547 vs 1.1345547 (diff 0.0686, 575458 ULP); 20 of 35 ticks differ; largest tick 35 position.z: 3.8881469 vs 3.4986672 (diff 0.389, 1633596 ULP)
- `sr_jump` unastar vs bedsim: first tick 16 position.z: 1.1345547 vs 1.2031547 (diff 0.0686, 575458 ULP); 20 of 35 ticks differ; largest tick 35 position.z: 3.4986672 vs 3.8881469 (diff 0.389, 1633596 ULP)
- `sr_jump` acacia vs bedsim: first tick 16 position.z: 1.1345547 vs 1.2031547 (diff 0.0686, 575458 ULP); 20 of 35 ticks differ; largest tick 35 position.z: 3.4986672 vs 3.8881469 (diff 0.389, 1633596 ULP)
- `sr_low_ceiling` bedsim vs bds: first tick 26 position.z: 1.8506322 vs 1.7820323 (diff 0.0686, 575458 ULP); 6 of 45 ticks differ; largest tick 30 position.z: 2.6236668 vs 2.4798975 (diff 0.144, 603012 ULP)
- `sr_low_ceiling` unastar vs bedsim: first tick 26 position.z: 1.7820323 vs 1.8506322 (diff 0.0686, 575458 ULP); 6 of 45 ticks differ; largest tick 30 position.z: 2.4798975 vs 2.6236668 (diff 0.144, 603012 ULP)
- `sr_low_ceiling` acacia vs bedsim: first tick 26 position.z: 1.7820323 vs 1.8506322 (diff 0.0686, 575458 ULP); 6 of 45 ticks differ; largest tick 30 position.z: 2.4798975 vs 2.6236668 (diff 0.144, 603012 ULP)
- `sr_sprint_jump` bedsim vs bds: first tick 16 position.z: 1.4325547 vs 1.1345547 (diff 0.298, 2499805 ULP); 20 of 35 ticks differ; largest tick 35 position.z: 6.595384 vs 4.903475 (diff 1.69, 3548191 ULP)
- `sr_sprint_jump` acacia vs bds: first tick 24 position.z: 2.0908566 vs 2.0908563 (diff 2.38e-07, 1 ULP); 9 of 35 ticks differ; largest tick 26 position.z: 2.457396 vs 2.4573956 (diff 4.77e-07, 2 ULP)
- `sr_sprint_jump` unastar vs bedsim: first tick 16 position.z: 1.1345547 vs 1.4325547 (diff 0.298, 2499805 ULP); 20 of 35 ticks differ; largest tick 35 position.z: 4.903475 vs 6.595384 (diff 1.69, 3548191 ULP)
- `sr_sprint_jump` acacia vs bedsim: first tick 16 position.z: 1.1345547 vs 1.4325547 (diff 0.298, 2499805 ULP); 20 of 35 ticks differ; largest tick 35 position.z: 4.903475 vs 6.595384 (diff 1.69, 3548191 ULP)
- `sr_sprint_jump` acacia vs unastar: first tick 19 velocity.z: 0.09009808 vs 0.090098076 (diff 7.45e-09, 1 ULP); 14 of 35 ticks differ; largest tick 26 position.z: 2.457396 vs 2.4573956 (diff 4.77e-07, 2 ULP)
- `sr_still_forward_4` bedsim vs bds: first tick 13 position.z: 0.598 vs 0.5294 (diff 0.0686, 1150917 ULP); 20 of 32 ticks differ; largest tick 32 position.z: 3.7252905 vs 3.5741897 (diff 0.151, 633763 ULP)
- `sr_still_forward_4` unastar vs bedsim: first tick 13 position.z: 0.5294 vs 0.598 (diff 0.0686, 1150917 ULP); 20 of 32 ticks differ; largest tick 32 position.z: 3.5741897 vs 3.7252905 (diff 0.151, 633763 ULP)
- `sr_still_forward_4` acacia vs bedsim: first tick 13 position.z: 0.5294 vs 0.598 (diff 0.0686, 1150917 ULP); 20 of 32 ticks differ; largest tick 32 position.z: 3.5741897 vs 3.7252905 (diff 0.151, 633763 ULP)
- `sr_still_forward_7` bedsim vs bds: first tick 16 position.z: 0.598 vs 0.5294 (diff 0.0686, 1150917 ULP); 20 of 35 ticks differ; largest tick 35 position.z: 3.7252905 vs 3.5741897 (diff 0.151, 633763 ULP)
- `sr_still_forward_7` unastar vs bedsim: first tick 16 position.z: 0.5294 vs 0.598 (diff 0.0686, 1150917 ULP); 20 of 35 ticks differ; largest tick 35 position.z: 3.5741897 vs 3.7252905 (diff 0.151, 633763 ULP)
- `sr_still_forward_7` acacia vs bedsim: first tick 16 position.z: 0.5294 vs 0.598 (diff 0.0686, 1150917 ULP); 20 of 35 ticks differ; largest tick 35 position.z: 3.5741897 vs 3.7252905 (diff 0.151, 633763 ULP)
- `sr_strafe` bedsim vs bds: first tick 16 position.x: 1.2031547 vs 1.1345547 (diff 0.0686, 575458 ULP); 20 of 35 ticks differ; largest tick 35 position.x: 4.3728676 vs 4.2217665 (diff 0.151, 316882 ULP)
- `sr_strafe` unastar vs bedsim: first tick 16 position.x: 1.1345547 vs 1.2031547 (diff 0.0686, 575458 ULP); 20 of 35 ticks differ; largest tick 35 position.x: 4.2217665 vs 4.3728676 (diff 0.151, 316882 ULP)
- `sr_strafe` acacia vs bedsim: first tick 16 position.x: 1.1345547 vs 1.2031547 (diff 0.0686, 575458 ULP); 20 of 35 ticks differ; largest tick 35 position.x: 4.2217665 vs 4.3728676 (diff 0.151, 316882 ULP)
- `sr_tap` bedsim vs bds: first tick 17 position.z: 2.724834 vs 2.656234 (diff 0.0686, 287729 ULP); 17 of 33 ticks differ; largest tick 33 position.z: 5.3010635 vs 5.149967 (diff 0.151, 316872 ULP)
- `sr_tap` unastar vs bedsim: first tick 17 position.z: 2.656234 vs 2.724834 (diff 0.0686, 287729 ULP); 17 of 33 ticks differ; largest tick 33 position.z: 5.149967 vs 5.3010635 (diff 0.151, 316872 ULP)
- `sr_tap` acacia vs bedsim: first tick 17 position.z: 2.656234 vs 2.724834 (diff 0.0686, 287729 ULP); 17 of 33 ticks differ; largest tick 33 position.z: 5.149967 vs 5.3010635 (diff 0.151, 316872 ULP)
- `sr_toggle` bedsim vs bds: first tick 8 position.z: 0.69766945 vs 0.62906945 (diff 0.0686, 1150917 ULP); 23 of 30 ticks differ; largest tick 30 position.z: 3.2955954 vs 2.67185 (diff 0.624, 2616178 ULP)
- `sr_toggle` acacia vs bds: first tick 13 position.z: 1.1531656 vs 1.1825657 (diff 0.0294, 246626 ULP); 18 of 30 ticks differ; largest tick 30 position.z: 2.5424883 vs 2.67185 (diff 0.129, 542582 ULP)
- `sr_toggle` unastar vs bedsim: first tick 8 position.z: 0.62906945 vs 0.69766945 (diff 0.0686, 1150917 ULP); 23 of 30 ticks differ; largest tick 30 position.z: 2.67185 vs 3.2955954 (diff 0.624, 2616178 ULP)
- `sr_toggle` acacia vs bedsim: first tick 8 position.z: 0.62906945 vs 0.69766945 (diff 0.0686, 1150917 ULP); 23 of 30 ticks differ; largest tick 30 position.z: 2.5424883 vs 3.2955954 (diff 0.753, 3158760 ULP)
- `sr_toggle` acacia vs unastar: first tick 13 position.z: 1.1531656 vs 1.1825657 (diff 0.0294, 246626 ULP); 18 of 30 ticks differ; largest tick 30 position.z: 2.5424883 vs 2.67185 (diff 0.129, 542582 ULP)
- `sr_walk_5` bedsim vs bds: first tick 16 position.z: 1.2031547 vs 1.1345547 (diff 0.0686, 575458 ULP); 20 of 35 ticks differ; largest tick 35 position.z: 4.3728676 vs 4.2217665 (diff 0.151, 316882 ULP)
- `sr_walk_5` unastar vs bedsim: first tick 16 position.z: 1.1345547 vs 1.2031547 (diff 0.0686, 575458 ULP); 20 of 35 ticks differ; largest tick 35 position.z: 4.2217665 vs 4.3728676 (diff 0.151, 316882 ULP)
- `sr_walk_5` acacia vs bedsim: first tick 16 position.z: 1.1345547 vs 1.2031547 (diff 0.0686, 575458 ULP); 20 of 35 ticks differ; largest tick 35 position.z: 4.2217665 vs 4.3728676 (diff 0.151, 316882 ULP)
- `ss_diag_air` bedsim vs bds: first tick 6 velocity.x: -0.03659343 vs -0.036593433 (diff 3.73e-09, 1 ULP); 32 of 40 ticks differ; largest tick 28 position.z: 6.645153 vs 6.645152 (diff 9.54e-07, 2 ULP)
- `ss_diag_air` acacia vs bds: first tick 6 position.x: 0.43365198 vs 0.43297905 (diff 0.000673, 22580 ULP); 35 of 40 ticks differ; largest tick 40 position.z: 10.40798 vs 10.267244 (diff 0.141, 147572 ULP)
- `ss_diag_air` unastar vs bedsim: first tick 6 velocity.x: -0.036593433 vs -0.03659343 (diff 3.73e-09, 1 ULP); 35 of 40 ticks differ; largest tick 28 position.z: 6.645152 vs 6.645153 (diff 9.54e-07, 2 ULP)
- `ss_diag_air` acacia vs bedsim: first tick 6 position.x: 0.43365198 vs 0.43297905 (diff 0.000673, 22580 ULP); 35 of 40 ticks differ; largest tick 40 position.z: 10.40798 vs 10.267245 (diff 0.141, 147571 ULP)
- `ss_diag_air` acacia vs unastar: first tick 6 position.x: 0.43365198 vs 0.43297905 (diff 0.000673, 22580 ULP); 35 of 40 ticks differ; largest tick 40 position.z: 10.40798 vs 10.267244 (diff 0.141, 147572 ULP)
- `ss_half_forward` bedsim vs bds: first tick 16 position.z: 3.1860366 vs 3.1713367 (diff 0.0147, 61656 ULP); 23 of 38 ticks differ; largest tick 38 position.z: 7.2184453 vs 6.959419 (diff 0.259, 543218 ULP)
- `ss_half_forward` unastar vs bedsim: first tick 16 position.z: 3.1713367 vs 3.1860366 (diff 0.0147, 61656 ULP); 23 of 38 ticks differ; largest tick 38 position.z: 6.959419 vs 7.2184453 (diff 0.259, 543218 ULP)
- `ss_half_forward` acacia vs bedsim: first tick 16 position.z: 3.1713367 vs 3.1860366 (diff 0.0147, 61656 ULP); 23 of 38 ticks differ; largest tick 38 position.z: 6.959419 vs 7.2184453 (diff 0.259, 543218 ULP)
- `ss_half_forward_nokey` bedsim vs bds: first tick 16 position.z: 3.1860366 vs 3.1713367 (diff 0.0147, 61656 ULP); 23 of 38 ticks differ; largest tick 38 position.z: 7.2184453 vs 6.315612 (diff 0.903, 1893379 ULP)
- `ss_half_forward_nokey` unastar vs bedsim: first tick 16 position.z: 3.1713367 vs 3.1860366 (diff 0.0147, 61656 ULP); 23 of 38 ticks differ; largest tick 38 position.z: 6.315612 vs 7.2184453 (diff 0.903, 1893379 ULP)
- `ss_half_forward_nokey` acacia vs bedsim: first tick 16 position.z: 3.1713367 vs 3.1860366 (diff 0.0147, 61656 ULP); 23 of 38 ticks differ; largest tick 38 position.z: 6.315612 vs 7.2184453 (diff 0.903, 1893379 ULP)
- `ss_strafe_jump` bedsim vs bds: first tick 10 position.z: 1.6590164 vs 1.6590163 (diff 1.19e-07, 1 ULP); 28 of 40 ticks differ; largest tick 30 position.z: 7.768276 vs 7.7682753 (diff 9.54e-07, 2 ULP)
- `ss_strafe_jump` acacia vs bds: first tick 6 position.x: 0.53735524 vs 0.5366081 (diff 0.000747, 12535 ULP); 35 of 40 ticks differ; largest tick 40 position.z: 10.793062 vs 10.653505 (diff 0.14, 146336 ULP)
- `ss_strafe_jump` unastar vs bedsim: first tick 8 velocity.x: 0.029277807 vs 0.02927781 (diff 3.73e-09, 2 ULP); 33 of 40 ticks differ; largest tick 30 position.z: 7.7682753 vs 7.768276 (diff 9.54e-07, 2 ULP)
- `ss_strafe_jump` acacia vs bedsim: first tick 6 position.x: 0.53735524 vs 0.5366081 (diff 0.000747, 12535 ULP); 35 of 40 ticks differ; largest tick 40 position.z: 10.793062 vs 10.653505 (diff 0.14, 146336 ULP)
- `ss_strafe_jump` acacia vs unastar: first tick 6 position.x: 0.53735524 vs 0.5366081 (diff 0.000747, 12535 ULP); 35 of 40 ticks differ; largest tick 40 position.z: 10.793062 vs 10.653505 (diff 0.14, 146336 ULP)
- `stairs_corner_diagonal` acacia vs bds: first tick 6 position.x: 0.42928934 vs 0.43070352 (diff 0.00141, 47452 ULP); 50 of 55 ticks differ; largest tick 35 position.y: 101.85153 vs 102.155846 (diff 0.304, 39887 ULP)
- `stairs_corner_diagonal` acacia vs bedsim: first tick 6 position.x: 0.42928934 vs 0.43070352 (diff 0.00141, 47452 ULP); 50 of 55 ticks differ; largest tick 35 position.y: 101.85153 vs 102.155846 (diff 0.304, 39887 ULP)
- `stairs_corner_diagonal` acacia vs unastar: first tick 6 position.x: 0.42928934 vs 0.43070352 (diff 0.00141, 47452 ULP); 50 of 55 ticks differ; largest tick 35 position.y: 101.85153 vs 102.155846 (diff 0.304, 39887 ULP)
- `sw_corner` bedsim vs bds: first tick 6 velocity.x: 0.04918664 vs 0.049186636 (diff 3.73e-09, 1 ULP); 29 of 35 ticks differ; largest tick 13 position.x: 1.8506575 vs 1.8506572 (diff 2.38e-07, 2 ULP)
- `sw_corner` unastar vs bedsim: first tick 6 velocity.x: 0.049186636 vs 0.04918664 (diff 3.73e-09, 1 ULP); 30 of 35 ticks differ; largest tick 13 position.x: 1.8506572 vs 1.8506575 (diff 2.38e-07, 2 ULP)
- `sw_corner` acacia vs bedsim: first tick 6 velocity.x: 0.049186636 vs 0.04918664 (diff 3.73e-09, 1 ULP); 30 of 35 ticks differ; largest tick 13 position.x: 1.8506572 vs 1.8506575 (diff 2.38e-07, 2 ULP)
- `sw_ground_20` bedsim vs bds: first tick 11 position.z: 1.7734342 vs 1.7734339 (diff 2.38e-07, 2 ULP); 28 of 40 ticks differ; largest tick 40 position.x: 3.3736963 vs 2.9541686 (diff 0.42, 1759627 ULP)
- `sw_ground_20` unastar vs bedsim: first tick 7 velocity.z: 0.101054914 vs 0.10105492 (diff 7.45e-09, 1 ULP); 29 of 40 ticks differ; largest tick 40 position.x: 2.9541686 vs 3.3736963 (diff 0.42, 1759627 ULP)
- `sw_ground_20` acacia vs bedsim: first tick 7 velocity.z: 0.101054914 vs 0.10105492 (diff 7.45e-09, 1 ULP); 29 of 40 ticks differ; largest tick 40 position.x: 2.9541686 vs 3.3736963 (diff 0.42, 1759627 ULP)
- `sw_ground_22` bedsim vs bds: first tick 6 velocity.x: 0.026057787 vs 0.026057785 (diff 1.86e-09, 1 ULP); 28 of 40 ticks differ; largest tick 40 position.x: 3.647493 vs 3.2122521 (diff 0.435, 1825532 ULP)
- `sw_ground_22` acacia vs bds: first tick 6 velocity.x: 0.026057787 vs 0.026057785 (diff 1.86e-09, 1 ULP); 12 of 40 ticks differ; largest tick 33 position.x: 2.957996 vs 2.9579957 (diff 2.38e-07, 1 ULP)
- `sw_ground_22` unastar vs bedsim: first tick 6 velocity.x: 0.026057785 vs 0.026057787 (diff 1.86e-09, 1 ULP); 35 of 40 ticks differ; largest tick 40 position.x: 3.2122521 vs 3.647493 (diff 0.435, 1825532 ULP)
- `sw_ground_22` acacia vs bedsim: first tick 17 position.x: 1.6241032 vs 1.6351166 (diff 0.011, 92387 ULP); 24 of 40 ticks differ; largest tick 40 position.x: 3.2122521 vs 3.647493 (diff 0.435, 1825532 ULP)
- `sw_ground_22` acacia vs unastar: first tick 6 velocity.x: 0.026057787 vs 0.026057785 (diff 1.86e-09, 1 ULP); 35 of 40 ticks differ; largest tick 33 position.x: 2.957996 vs 2.9579957 (diff 2.38e-07, 1 ULP)
- `sw_ground_24` bedsim vs bds: first tick 6 velocity.z: 0.0635466 vs 0.06354659 (diff 7.45e-09, 1 ULP); 26 of 40 ticks differ; largest tick 40 position.x: 3.917453 vs 3.6550539 (diff 0.262, 1100582 ULP)
- `sw_ground_24` unastar vs bedsim: first tick 6 velocity.z: 0.06354659 vs 0.0635466 (diff 7.45e-09, 1 ULP); 31 of 40 ticks differ; largest tick 40 position.x: 3.6550539 vs 3.917453 (diff 0.262, 1100582 ULP)
- `sw_ground_24` acacia vs bedsim: first tick 6 velocity.z: 0.06354659 vs 0.0635466 (diff 7.45e-09, 1 ULP); 31 of 40 ticks differ; largest tick 40 position.x: 3.6550539 vs 3.917453 (diff 0.262, 1100582 ULP)
- `sw_ground_25` bedsim vs bds: first tick 6 velocity.x: 0.0293975 vs 0.029397497 (diff 3.73e-09, 2 ULP); 25 of 40 ticks differ; largest tick 37 position.x: 4.0152936 vs 4.015293 (diff 4.77e-07, 1 ULP)
- `sw_ground_25` unastar vs bedsim: first tick 6 velocity.x: 0.029397497 vs 0.0293975 (diff 3.73e-09, 2 ULP); 29 of 40 ticks differ; largest tick 37 position.x: 4.015293 vs 4.0152936 (diff 4.77e-07, 1 ULP)
- `sw_ground_25` acacia vs bedsim: first tick 6 velocity.x: 0.029397497 vs 0.0293975 (diff 3.73e-09, 2 ULP); 29 of 40 ticks differ; largest tick 37 position.x: 4.015293 vs 4.0152936 (diff 4.77e-07, 1 ULP)
- `sw_ground_5` bedsim vs bds: first tick 17 position.x: 0.7640955 vs 0.76153314 (diff 0.00256, 42989 ULP); 24 of 40 ticks differ; largest tick 40 position.x: 1.2322934 vs 1.1253867 (diff 0.107, 896798 ULP)
- `sw_ground_5` unastar vs bedsim: first tick 17 position.x: 0.76153314 vs 0.7640955 (diff 0.00256, 42989 ULP); 24 of 40 ticks differ; largest tick 40 position.x: 1.1253867 vs 1.2322934 (diff 0.107, 896798 ULP)
- `sw_ground_5` acacia vs bedsim: first tick 17 position.x: 0.76153314 vs 0.7640955 (diff 0.00256, 42989 ULP); 24 of 40 ticks differ; largest tick 40 position.x: 1.1253867 vs 1.2322934 (diff 0.107, 896798 ULP)
- `sw_head_on_strafe` bedsim vs bds: first tick 21 position.x: 0.5900854 vs 0.5692965 (diff 0.0208, 348780 ULP); 20 of 40 ticks differ; largest tick 39 position.x: 3.4551845 vs 3.4093947 (diff 0.0458, 192056 ULP)
- `sw_head_on_strafe` acacia vs bds: first tick 21 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 20 of 40 ticks differ; largest tick 40 position.x: 3.4785933 vs 3.419022 (diff 0.0596, 249860 ULP)
- `sw_head_on_strafe` unastar vs bedsim: first tick 21 position.x: 0.5692965 vs 0.5900854 (diff 0.0208, 348780 ULP); 20 of 40 ticks differ; largest tick 39 position.x: 3.4093947 vs 3.4551845 (diff 0.0458, 192056 ULP)
- `sw_head_on_strafe` acacia vs bedsim: first tick 21 position.x: 0.57071066 vs 0.5900854 (diff 0.0194, 325054 ULP); 20 of 40 ticks differ; largest tick 23 position.x: 0.8644369 vs 0.89548534 (diff 0.031, 520906 ULP)
- `sw_head_on_strafe` acacia vs unastar: first tick 21 position.x: 0.57071066 vs 0.5692965 (diff 0.00141, 23726 ULP); 20 of 40 ticks differ; largest tick 40 position.x: 3.4785933 vs 3.419022 (diff 0.0596, 249860 ULP)
- `sw_jump_10` bedsim vs bds: first tick 6 velocity.x: 0.031036839 vs 0.031036835 (diff 3.73e-09, 2 ULP); 33 of 40 ticks differ; largest tick 40 position.x: 2.3152838 vs 2.299329 (diff 0.016, 66919 ULP)
- `sw_jump_10` acacia vs bds: first tick 9 position.x: 0.6668163 vs 0.66681623 (diff 5.96e-08, 1 ULP); 32 of 40 ticks differ; largest tick 31 position.x: 1.8517649 vs 1.8517642 (diff 7.15e-07, 6 ULP)
- `sw_jump_10` unastar vs bedsim: first tick 6 velocity.x: 0.031036835 vs 0.031036839 (diff 3.73e-09, 2 ULP); 35 of 40 ticks differ; largest tick 40 position.x: 2.299329 vs 2.3152838 (diff 0.016, 66919 ULP)
- `sw_jump_10` acacia vs bedsim: first tick 6 velocity.x: 0.031036835 vs 0.031036839 (diff 3.73e-09, 2 ULP); 26 of 40 ticks differ; largest tick 40 position.x: 2.2993298 vs 2.3152838 (diff 0.016, 66916 ULP)
- `sw_jump_10` acacia vs unastar: first tick 7 velocity.x: 0.03226987 vs 0.032269865 (diff 3.73e-09, 1 ULP); 34 of 40 ticks differ; largest tick 31 position.x: 1.8517649 vs 1.8517642 (diff 7.15e-07, 6 ULP)
- `sw_jump_30` bedsim vs bds: first tick 6 velocity.x: 0.089377195 vs 0.08937716 (diff 3.73e-08, 5 ULP); 35 of 40 ticks differ; largest tick 40 position.x: 5.7272096 vs 5.701217 (diff 0.026, 54510 ULP)
- `sw_jump_30` acacia vs bds: first tick 18 position.x: 2.2164214 vs 2.2164211 (diff 2.38e-07, 1 ULP); 1 of 40 ticks differ; largest tick 18 position.x: 2.2164214 vs 2.2164211 (diff 2.38e-07, 1 ULP)
- `sw_jump_30` unastar vs bedsim: first tick 6 velocity.x: 0.08937716 vs 0.089377195 (diff 3.73e-08, 5 ULP); 35 of 40 ticks differ; largest tick 40 position.x: 5.701217 vs 5.7272096 (diff 0.026, 54510 ULP)
- `sw_jump_30` acacia vs bedsim: first tick 6 velocity.x: 0.08937716 vs 0.089377195 (diff 3.73e-08, 5 ULP); 35 of 40 ticks differ; largest tick 40 position.x: 5.701217 vs 5.7272096 (diff 0.026, 54510 ULP)
- `sw_jump_30` acacia vs unastar: first tick 18 position.x: 2.2164214 vs 2.2164211 (diff 2.38e-07, 1 ULP); 1 of 40 ticks differ; largest tick 18 position.x: 2.2164214 vs 2.2164211 (diff 2.38e-07, 1 ULP)
- `sw_near_partial` bedsim vs bds: first tick 8 position.x: 0.5971216 vs 0.59201634 (diff 0.00511, 85652 ULP); 28 of 35 ticks differ; largest tick 35 position.x: 1.7153711 vs 1.457391 (diff 0.258, 2164094 ULP)
- `sw_near_partial` unastar vs bedsim: first tick 8 position.x: 0.59201634 vs 0.5971216 (diff 0.00511, 85652 ULP); 28 of 35 ticks differ; largest tick 35 position.x: 1.457391 vs 1.7153711 (diff 0.258, 2164094 ULP)
- `sw_near_partial` acacia vs bedsim: first tick 8 position.x: 0.59201634 vs 0.5971216 (diff 0.00511, 85652 ULP); 28 of 35 ticks differ; largest tick 35 position.x: 1.457391 vs 1.7153711 (diff 0.258, 2164094 ULP)
- `sw_nokey_10` bedsim vs bds: first tick 17 position.x: 1.0261812 vs 1.021076 (diff 0.00511, 42826 ULP); 22 of 38 ticks differ; largest tick 38 position.x: 1.861557 vs 1.6710472 (diff 0.191, 1598112 ULP)
- `sw_nokey_10` acacia vs bds: first tick 9 position.x: 0.6364143 vs 0.64151955 (diff 0.00511, 85652 ULP); 30 of 38 ticks differ; largest tick 14 position.z: 2.3482826 vs 2.6562605 (diff 0.308, 1291753 ULP)
- `sw_nokey_10` unastar vs bedsim: first tick 17 position.x: 1.021076 vs 1.0261812 (diff 0.00511, 42826 ULP); 22 of 38 ticks differ; largest tick 38 position.x: 1.6710472 vs 1.861557 (diff 0.191, 1598112 ULP)
- `sw_nokey_10` acacia vs bedsim: first tick 9 position.x: 0.6364143 vs 0.64151955 (diff 0.00511, 85652 ULP); 30 of 38 ticks differ; largest tick 14 position.z: 2.3482826 vs 2.6562605 (diff 0.308, 1291753 ULP)
- `sw_nokey_10` acacia vs unastar: first tick 9 position.x: 0.6364143 vs 0.64151955 (diff 0.00511, 85652 ULP); 30 of 38 ticks differ; largest tick 14 position.z: 2.3482826 vs 2.6562605 (diff 0.308, 1291753 ULP)
- `sw_nokey_30` acacia vs bds: first tick 9 position.x: 0.8927891 vs 0.9074891 (diff 0.0147, 246625 ULP); 30 of 38 ticks differ; largest tick 38 position.x: 3.612865 vs 4.4204464 (diff 0.808, 2505502 ULP)
- `sw_nokey_30` acacia vs bedsim: first tick 9 position.x: 0.8927891 vs 0.9074891 (diff 0.0147, 246625 ULP); 30 of 38 ticks differ; largest tick 38 position.x: 3.612865 vs 4.4204464 (diff 0.808, 2505502 ULP)
- `sw_nokey_30` acacia vs unastar: first tick 9 position.x: 0.8927891 vs 0.9074891 (diff 0.0147, 246625 ULP); 30 of 38 ticks differ; largest tick 38 position.x: 3.612865 vs 4.4204464 (diff 0.808, 2505502 ULP)
- `sw_post` bedsim vs bds: first tick 20 position.x: 0.8374503 vs 0.8348879 (diff 0.00256, 42990 ULP); 21 of 40 ticks differ; largest tick 40 position.x: 1.2322934 vs 1.1423188 (diff 0.09, 754761 ULP)
- `sw_post` unastar vs bedsim: first tick 20 position.x: 0.8348879 vs 0.8374503 (diff 0.00256, 42990 ULP); 21 of 40 ticks differ; largest tick 40 position.x: 1.1423188 vs 1.2322934 (diff 0.09, 754761 ULP)
- `sw_post` acacia vs bedsim: first tick 20 position.x: 0.8348879 vs 0.8374503 (diff 0.00256, 42990 ULP); 21 of 40 ticks differ; largest tick 40 position.x: 1.1423188 vs 1.2322934 (diff 0.09, 754761 ULP)
- `sw_strafe_wall` acacia vs bds: first tick 6 position.x: 0.5919239 vs 0.5900854 (diff 0.00184, 30845 ULP); 35 of 40 ticks differ; largest tick 40 position.x: 6.5624514 vs 6.4412017 (diff 0.121, 254279 ULP)
- `sw_strafe_wall` acacia vs bedsim: first tick 6 position.x: 0.5919239 vs 0.5900854 (diff 0.00184, 30845 ULP); 35 of 40 ticks differ; largest tick 40 position.x: 6.5624514 vs 6.4412017 (diff 0.121, 254279 ULP)
- `sw_strafe_wall` acacia vs unastar: first tick 6 position.x: 0.5919239 vs 0.5900854 (diff 0.00184, 30845 ULP); 35 of 40 ticks differ; largest tick 40 position.x: 6.5624514 vs 6.4412017 (diff 0.121, 254279 ULP)
- `sw_strafe_wall_03` acacia vs bds: first tick 6 position.x: 0.53735524 vs 0.5366081 (diff 0.000747, 12535 ULP); 35 of 40 ticks differ; largest tick 40 position.z: 8.712016 vs 8.547777 (diff 0.164, 172217 ULP)
- `sw_strafe_wall_03` acacia vs bedsim: first tick 6 position.x: 0.53735524 vs 0.5366081 (diff 0.000747, 12535 ULP); 35 of 40 ticks differ; largest tick 40 position.z: 8.712016 vs 8.547777 (diff 0.164, 172217 ULP)
- `sw_strafe_wall_03` acacia vs unastar: first tick 6 position.x: 0.53735524 vs 0.5366081 (diff 0.000747, 12535 ULP); 35 of 40 ticks differ; largest tick 40 position.z: 8.712016 vs 8.547777 (diff 0.164, 172217 ULP)
- `sw_xwall_65` bedsim vs bds: first tick 6 position.z: 0.5538416 vs 0.55384153 (diff 5.96e-08, 1 ULP); 21 of 40 ticks differ; largest tick 13 position.x: 2.2311547 vs 2.231155 (diff 2.38e-07, 1 ULP)
- `sw_xwall_65` unastar vs bedsim: first tick 6 position.z: 0.55384153 vs 0.5538416 (diff 5.96e-08, 1 ULP); 28 of 40 ticks differ; largest tick 13 position.x: 2.231155 vs 2.2311547 (diff 2.38e-07, 1 ULP)
- `sw_xwall_65` acacia vs bedsim: first tick 6 position.z: 0.55384153 vs 0.5538416 (diff 5.96e-08, 1 ULP); 28 of 40 ticks differ; largest tick 13 position.x: 2.231155 vs 2.2311547 (diff 2.38e-07, 1 ULP)
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
- `wall_slide_east` acacia vs bds: first tick 6 position.x: 6.2292895 vs 6.230704 (diff 0.00141, 2966 ULP); 55 of 60 ticks differ; largest tick 60 position.z: 1.2784309 vs 1.1228633 (diff 0.156, 1304996 ULP)
- `wall_slide_east` acacia vs bedsim: first tick 6 position.x: 6.2292895 vs 6.230704 (diff 0.00141, 2966 ULP); 55 of 60 ticks differ; largest tick 60 position.z: 1.2784309 vs 1.1228633 (diff 0.156, 1304996 ULP)
- `wall_slide_east` acacia vs unastar: first tick 6 position.x: 6.2292895 vs 6.230704 (diff 0.00141, 2966 ULP); 55 of 60 ticks differ; largest tick 60 position.z: 1.2784309 vs 1.1228633 (diff 0.156, 1304996 ULP)
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
- `web_cross_0` bedsim vs bds: first tick 6 position.x: -1.2684932 vs -1.2684933 (diff 1.19e-07, 1 ULP); 17 of 22 ticks differ; largest tick 6 position.x: -1.2684932 vs -1.2684933 (diff 1.19e-07, 1 ULP)
- `web_cross_0` unastar vs bedsim: first tick 6 position.x: -1.2684933 vs -1.2684932 (diff 1.19e-07, 1 ULP); 17 of 22 ticks differ; largest tick 6 position.x: -1.2684933 vs -1.2684932 (diff 1.19e-07, 1 ULP)
- `web_cross_0` acacia vs bedsim: first tick 6 position.x: -1.2684933 vs -1.2684932 (diff 1.19e-07, 1 ULP); 17 of 22 ticks differ; largest tick 6 position.x: -1.2684933 vs -1.2684932 (diff 1.19e-07, 1 ULP)
- `web_walk_0` bedsim vs bds: first tick 6 velocity.x: 0.037835877 vs 0.037835874 (diff 3.73e-09, 1 ULP); 28 of 35 ticks differ; largest tick 14 position.x: -0.12303849 vs -0.12303856 (diff 6.71e-08, 9 ULP)
- `web_walk_0` unastar vs bedsim: first tick 6 velocity.x: 0.037835874 vs 0.037835877 (diff 3.73e-09, 1 ULP); 30 of 35 ticks differ; largest tick 14 position.x: -0.12303856 vs -0.12303849 (diff 6.71e-08, 9 ULP)
- `web_walk_0` acacia vs bedsim: first tick 6 velocity.x: 0.037835874 vs 0.037835877 (diff 3.73e-09, 1 ULP); 30 of 35 ticks differ; largest tick 14 position.x: -0.12303856 vs -0.12303849 (diff 6.71e-08, 9 ULP)
- `weeping_vines_climb` bedsim vs bds: first tick 1 position.z: 3.3 vs 2.5 (diff 0.8, 3355443 ULP); 65 of 65 ticks differ; largest tick 29 position.y: 101.62001 vs 106.41994 (diff 4.8, 629136 ULP)
- `weeping_vines_climb` unastar vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 65 of 65 ticks differ; largest tick 29 position.y: 106.41994 vs 101.62001 (diff 4.8, 629136 ULP)
- `weeping_vines_climb` acacia vs bedsim: first tick 1 position.z: 2.5 vs 3.3 (diff 0.8, 3355443 ULP); 65 of 65 ticks differ; largest tick 29 position.y: 106.41994 vs 101.62001 (diff 4.8, 629136 ULP)
- `weeping_vines_walk_into` bedsim vs bds: first tick 12 position.z: 1.7 vs 1.7551675 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 38 position.y: 101.62001 vs 105.5744 (diff 3.95, 518310 ULP)
- `weeping_vines_walk_into` unastar vs bedsim: first tick 12 position.z: 1.7551675 vs 1.7 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 38 position.y: 105.5744 vs 101.62001 (diff 3.95, 518310 ULP)
- `weeping_vines_walk_into` acacia vs bedsim: first tick 12 position.z: 1.7551675 vs 1.7 (diff 0.0552, 462778 ULP); 39 of 50 ticks differ; largest tick 38 position.y: 105.5744 vs 101.62001 (diff 3.95, 518310 ULP)

<!-- manual: everything below is hand-written and kept by compare.py -->

## How close are bedsim and acacia-physics to BDS?

On the 226-scenario corpus (committed BDS run md-r6-a, `dt_gap_5` from md-r6-probe2), unastar
matches BDS bit for bit in 207 scenarios, bedsim in 117 and acacia-physics in 143. On the 147
scenarios of round 5 they match in 137, 97 and 103: unastar gained `sprint_turn_jump` (the sprinter's 0.025999999 air
acceleration) and `knockback_sprint_air` (round 5's run had applied its knockback an input
late).

**bedsim's runner world.** In about 40 scenarios bedsim's runner builds a different world from
BDS's (the divergences it causes are marked "runner world" above):
- Dragonfly computes fence, pane, bar and wall connections from the neighbours;
- it models honey as a full cube, trapdoors 0.1875 thick (BDS: 0.1825) and soul sand as a full
  block;
- it has no shape for scaffolding, the climbing plants, powder snow, sweet berry bushes or snow
  layers.

Those divergences are the runner's, not bedsim's physics. acacia's runner takes BDS's shapes, so
its world equals unastar's and its divergences are its own. acacia has no lectern physics, so it
skips `step_lectern_slab`.

On round 6's probes:

| rule | unastar | bedsim | acacia |
|---|---|---|---|
| a released sneak still slows its input (`sr_*`, 13 scenarios) | 13 | 0 | 11 |
| sprint stops at half forward and before the move (`ss_*`, 7) | 7 | 3 | 5 |
| double-tap sprint (`dt_*`, 14) | 14 | 8 | 8 |
| sprinting into walls (`sw_*`, 18) | 18 | 5 | 10 |

## Velocity on every input

BDS corrects a far-reporting client at most every sixth input, so the committed run reads BDS's
velocity on one input in six. A run of 50 scenarios, each shifted by one to five idle inputs
(md-r6-shift), moved the correction phase over the same movement: round 5's 30 and 20 of round
6's probes and catchers. In 49 of them the shifted copies moved exactly as the originals, and
together they give BDS's velocity on nearly every input
(`crates/bds-sweep/testdata/bds-1.26.51-movement-diff-velocities.json`). `soul_sand_walk` did
not: its idle start sinks into the soul sand. unastar's velocity equals BDS's on every one of
those inputs. Velocity cannot be read exactly from consecutive positions: the wire position is
rounded to `f32` after the 1.62001 offset is added, which loses several ULPs at y 100.

## Randomised testing

Each batch was played on BDS with the final simulation. The subject derives its flags from the
simulation, so a run made before a rule change no longer scores that rule.

| batch | seeds | valid | round 5 | mid-round run | final run |
|---|---|---|---|---|---|
| round 5's seeds, old generator (md-r6-final-r1) | 1-150 | 80 | 10 of 102 | 90 of 101 (md-r6-random1) | 71 |
| fresh (md-r6-final-r2) | 151-300 | 150 | - | 135 of 150 (md-r6-random2) | 136 |
| **total** | | 230 | | | **207 (90.0 %)** |

The setup check rejects 49 of the first batch: the old generator placed carpets and snow layers
in the air. In the final run it also rejected 21 more, whose first input went out idle because
the start teleport arrived late. The input check caught every one, and none became a trace.
The fresh batch was made after the generator dropped carpets and snow; all 150 seeds are valid.

The first differences of round 5's failing seeds pointed at the rules found this round:

- 49 of 92 first differed on the input that let go of a sneak: a released sneak stands the body
  up, but its input is still slowed (random seeds from 10 to 39);
- a sprint's forward input dropping to a half, or let go for a strafe: a sprint starts and lasts
  only at 0.8 forward, and starts and stops before the move (to 49);
- strafing sprint jumps an ULP or two off: a sprinter accelerates 0.025999999 in the air (to
  81);
- probing the sneak release found the double tap (`sr_toggle` sprinted without the key); the
  remaining seeds found walls ending sprints, honey's side and touching blocks.

Seeds kept as regression scenarios: `random_a7`, `random_a67`, `random_a30`, `random_a146`,
`random_a123`, `random_a27` and `random_a90` from the first batch, and
`random_248` and `random_297` from the second. Seeds kept as known gaps:
- `random_172` (a sprint jump at yaw 60, 3 ULP);
- `random_186` and `random_a141` (a sneak let go under a ceiling);
- `random_291` (a slime bounce, 1 ULP).

Of the 23 seeds still failing in the final runs, 22 first differ by 1 to 9 ULP:
- sprint jumps at yaw 60 and other yaws where the sine table matters;
- slime bounces;
- drifts late in long strafing sprints.

The other one is the crouch under a ceiling (`random_186`).

## Fitting data for the next round

- **Bed bounces** (`bed_drop_1` to `bed_drop_7`, `bed_bounce`): BDS's velocity after the landing
  tick against the velocity the landing move started with (the bounce is not a constant
  fraction of either it or the speed at impact):

  | scenario | velocity in | moved | velocity after |
  |---|---|---|---|
  | `bed_drop_1` | -0.3766305 | -0.23152 | 0.24382493 (corrected) |
  | `bed_drop_2` | -0.5169479 | -0.40740 | 0.35997 |
  | `bed_drop_3` | -0.6517088 | -0.30544 | 0.43635 |
  | `bed_drop_4` | -0.7170746 | -0.65373 | 0.51682 |
  | `bed_drop_7` | -0.9657236 | -0.40618 | 0.66383 |
  | `bed_bounce` | -0.7811331 | -0.37416 | 0.5324077 (corrected) |

- **BDS's sine table** is not the correctly rounded one. The glide at yaw 37.5 needs a look
  vector with x 4 ULP smaller and z 3 to 4 ULP larger. A sprint jump at yaw 60 needs cos 60 at
  0.5000552 (the correctly rounded entry is 0.5000554). No `f32` angle near the entry's gives
  both. Sprint jumps from rest at many yaws would read the table entry by entry.
