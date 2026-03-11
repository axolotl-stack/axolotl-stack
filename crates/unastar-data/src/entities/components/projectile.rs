use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitArrowEffect {
    ///If true, the effect will be applied to blocking targets.
    pub apply_effect_to_blocking_targets: Option<bool>,
}
impl Default for ProjectileOnHitArrowEffect {
    fn default() -> Self {
        Self {
            apply_effect_to_blocking_targets: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitDefinitionEvent {
    ///The projectile that will be affected by this event.
    pub affect_projectile: Option<bool>,
    ///The shooter that will be affected by this event.
    pub affect_shooter: Option<bool>,
    ///All entities in the splash area will be affected by this event.
    pub affect_splash_area: Option<bool>,
    ///The target will be affected by this event.
    pub affect_target: Option<bool>,
    ///The event triggered. Also has an option filters parameter to limit affected targets.
    pub event_trigger: Option<crate::types::BedrockValue>,
    ///The splash area that will be affected.
    pub splash_area: Option<f32>,
}
impl Default for ProjectileOnHitDefinitionEvent {
    fn default() -> Self {
        Self {
            affect_projectile: None,
            affect_shooter: None,
            affect_splash_area: None,
            affect_target: None,
            event_trigger: None,
            splash_area: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitFreezeOnHit {
    ///The shape of the area that is frozen.
    pub shape: Option<String>,
    ///The size of the area that is frozen.
    pub size: Option<f32>,
    ///If true, the area will snap to the nearest block.
    pub snap_to_block: Option<bool>,
}
impl Default for ProjectileOnHitFreezeOnHit {
    fn default() -> Self {
        Self {
            shape: None,
            size: None,
            snap_to_block: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitGrantXp {
    ///The maximum XP granted.
    pub max_xp: Option<f32>,
    ///The minimum XP granted.
    pub min_xp: Option<f32>,
}
impl Default for ProjectileOnHitGrantXp {
    fn default() -> Self {
        Self {
            max_xp: None,
            min_xp: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitHurtOwner {
    ///If true, the owner will be set on fire.
    pub ignite: Option<bool>,
    ///If true, the owner will be knocked back.
    pub knockback: Option<bool>,
    ///The amount of damage the owner will take.
    pub owner_damage: Option<f32>,
}
impl Default for ProjectileOnHitHurtOwner {
    fn default() -> Self {
        Self {
            ignite: None,
            knockback: None,
            owner_damage: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitImpactDamage {
    ///UNDOCUMENTED
    pub apply_knockback_to_blocking_targets: Option<bool>,
    ///Determines if the struck object is set on fire.
    pub catch_fire: Option<bool>,
    ///Whether lightning can be channeled through hte weapon.
    pub channeling: Option<bool>,
    ///The damage dealt on impact.
    pub damage: Option<crate::types::RangeOrVal<f32>>,
    ///Projectile is removed on hit.
    pub destroy_on_hit: Option<bool>,
    ///If true, then the hit must cause damage to destroy the projectile.
    pub destroy_on_hit_requires_damage: Option<bool>,
    ///The identifier of an entity that can be hit.
    pub filter: Option<String>,
    ///If true, the projectile will knock back the entity it hits.
    pub knockback: Option<bool>,
    ///Maximum critical damage.
    pub max_critical_damage: Option<i32>,
    ///Minimum critical damage.
    pub min_critical_damage: Option<i32>,
    ///How much the base damage is multiplied.
    pub power_multiplier: Option<f32>,
    ///If true, damage will be randomized based on damage and speed.
    pub semi_random_diff_damage: Option<bool>,
    ///If true, then the hit must cause damage to update the last hurt property.
    pub set_last_hurt_requires_damage: Option<bool>,
    ///If true, the projectile will bounce
    pub should_bounce: Option<bool>,
}
impl Default for ProjectileOnHitImpactDamage {
    fn default() -> Self {
        Self {
            apply_knockback_to_blocking_targets: None,
            catch_fire: None,
            channeling: None,
            damage: None,
            destroy_on_hit: None,
            destroy_on_hit_requires_damage: None,
            filter: None,
            knockback: None,
            max_critical_damage: None,
            min_critical_damage: None,
            power_multiplier: None,
            semi_random_diff_damage: None,
            set_last_hurt_requires_damage: None,
            should_bounce: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitMobEffect {
    ///If true, a mob will spawn that is not hostile, like the bat entity in Minecraft.
    pub ambient: Option<bool>,
    ///The multiplier of the amplification of this effect.
    pub amplifier: Option<i32>,
    ///The effect's duration.
    pub duration: Option<crate::types::MolangOr<i32>>,
    ///The effect's duration on easy mode.
    pub durationeasy: Option<crate::types::MolangOr<i32>>,
    ///The effect's duration on hard mode.
    pub durationhard: Option<crate::types::MolangOr<i32>>,
    ///The effect's duration on normal mode.
    pub durationnormal: Option<crate::types::MolangOr<i32>>,
    ///The identifier of the mob entity to affect.
    pub effect: Option<String>,
    ///Does the entity's look change.
    pub visible: Option<bool>,
}
impl Default for ProjectileOnHitMobEffect {
    fn default() -> Self {
        Self {
            ambient: None,
            amplifier: None,
            duration: None,
            durationeasy: None,
            durationhard: None,
            durationnormal: None,
            effect: None,
            visible: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitParticleOnHitParticleItemName {
    /// Additional dynamic entries not captured by the upstream schema.
    pub additional: std::collections::HashMap<String, crate::types::BedrockValue>,
}
impl Default for ProjectileOnHitParticleOnHitParticleItemName {
    fn default() -> Self {
        Self {
            additional: std::collections::HashMap::new(),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitParticleOnHit {
    ///The number of particles to spawn.
    pub num_particles: Option<f32>,
    ///If true, spawns particles on an entity hit.
    pub on_entity_hit: Option<bool>,
    ///If true, spawns particles on any other hit.
    pub on_other_hit: Option<bool>,
    ///Maps an item name to an actor filter to determine what the name of the item used in the particle should be.
    pub particle_item_name: Option<ProjectileOnHitParticleOnHitParticleItemName>,
    ///The id of the particle to spawn on hit.
    pub particle_type: Option<String>,
}
impl Default for ProjectileOnHitParticleOnHit {
    fn default() -> Self {
        Self {
            num_particles: None,
            on_entity_hit: None,
            on_other_hit: None,
            particle_item_name: None,
            particle_type: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitRemoveOnHit {
    /// Additional dynamic entries not captured by the upstream schema.
    pub additional: std::collections::HashMap<String, crate::types::BedrockValue>,
}
impl Default for ProjectileOnHitRemoveOnHit {
    fn default() -> Self {
        Self {
            additional: std::collections::HashMap::new(),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitSpawnAoeCloud {
    ///Determines if the projectile shooter is affected.
    pub affect_owner: Option<bool>,
    ///Particle color defined by three rgb values.
    pub color: Option<Vec<f32>>,
    ///How long the particle emits.
    pub duration: Option<i32>,
    ///The particle emitter.
    pub particle: Option<String>,
    ///The id of the potion.
    pub potion: Option<i32>,
    ///Defines the affected area.
    pub radius: Option<f32>,
    ///Defines the affected area when potion is used.
    pub radius_on_use: Option<f32>,
    ///Delay before the potion can affect the area again.
    pub reapplication_delay: Option<i32>,
}
impl Default for ProjectileOnHitSpawnAoeCloud {
    fn default() -> Self {
        Self {
            affect_owner: None,
            color: None,
            duration: None,
            particle: None,
            potion: None,
            radius: None,
            radius_on_use: None,
            reapplication_delay: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitSpawnChance {
    ///The chance that a first spawn occurs when a projectile hits the entity.
    pub first_spawn_chance: Option<f32>,
    ///The amount of new entities spawned.
    pub first_spawn_count: Option<i32>,
    ///The chance that a spawn occurs when a projectile hits the entity.
    pub first_spawn_percent_chance: Option<f32>,
    ///Triggered on the newly spawned entity with other set to the owning entity.
    pub on_spawn: Option<Vec<crate::types::BedrockValue>>,
    ///The chance that a second spawn occurs when a projectile hits the entity.
    pub second_spawn_chance: Option<f32>,
    ///The amount of new entities spawned in teh second spawn.
    pub second_spawn_count: Option<i32>,
    ///Determines if a baby spawns.
    pub spawn_baby: Option<bool>,
    ///The entity that will spawn.
    pub spawn_definition: Option<String>,
}
impl Default for ProjectileOnHitSpawnChance {
    fn default() -> Self {
        Self {
            first_spawn_chance: None,
            first_spawn_count: None,
            first_spawn_percent_chance: None,
            on_spawn: None,
            second_spawn_chance: None,
            second_spawn_count: None,
            spawn_baby: None,
            spawn_definition: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitStickInGround {}
impl Default for ProjectileOnHitStickInGround {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHitThrownPotionEffect {}
impl Default for ProjectileOnHitThrownPotionEffect {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileOnHit {
    ///The target receives a mob effect. See the table below for all arrow_effect parameters.
    pub arrow_effect: Option<ProjectileOnHitArrowEffect>,
    ///Determines if the struck object is set on fire.
    pub catch_fire: Option<bool>,
    ///The event that is triggered on a hit. See the table below for all definition event parameters.
    pub definition_event: Option<ProjectileOnHitDefinitionEvent>,
    ///If the target is on fire, then douse the fire.
    pub douse_fire: Option<bool>,
    ///An area of entities that is frozen to block on hits. Has shape of either sphere or cube, snap_to_block boolean ,and size decimal properties.
    pub freeze_on_hit: Option<ProjectileOnHitFreezeOnHit>,
    ///Grants XP on hit. Has minXP for minimum XP granted, maxXp for maximum, or simply flat xp properties.
    pub grant_xp: Option<ProjectileOnHitGrantXp>,
    ///Determines if the owner of the entity is hurt on hit. Contains decimal owner_damage, knockback boolean, and ignite boolean.
    pub hurt_owner: Option<ProjectileOnHitHurtOwner>,
    ///Determines if a fire may be started on a flammable target.
    pub ignite: Option<bool>,
    ///Defines the damage that an entity may receive on being hit by this projectile. See the table below for all impact_damage parameters.
    pub impact_damage: Option<ProjectileOnHitImpactDamage>,
    ///The target receives a mob effect. See the table below for all mob_effect parameters.
    pub mob_effect: Option<ProjectileOnHitMobEffect>,
    ///The amount of time a target will remain on fire.
    pub on_fire_time: Option<f32>,
    ///The particles that spawn on hit. See the table below for all particle_on_hit parameters.
    pub particle_on_hit: Option<ProjectileOnHitParticleOnHit>,
    ///Defines the effect the arrow will apply to the entity it hits.
    pub potion_effect: Option<i32>,
    ///Removes the projectile.
    pub remove_on_hit: Option<ProjectileOnHitRemoveOnHit>,
    ///Potion spawns an area of effect cloud. See the table below for all spawn_aoe_cloud parameters.
    pub spawn_aoe_cloud: Option<ProjectileOnHitSpawnAoeCloud>,
    ///Contains information on the chance of spawning an entity on hit. See parameters below.
    pub spawn_chance: Option<ProjectileOnHitSpawnChance>,
    ///Decides if the object sticks in ground and contains shake_time integer parameter to determine how long it will shake.
    pub stick_in_ground: Option<ProjectileOnHitStickInGround>,
    ///Determines if the owner is transported on hit.
    pub teleport_owner: Option<bool>,
    ///Creates a splash area for effects caused by a thrown potion.
    pub thrown_potion_effect: Option<ProjectileOnHitThrownPotionEffect>,
}
impl Default for ProjectileOnHit {
    fn default() -> Self {
        Self {
            arrow_effect: None,
            catch_fire: None,
            definition_event: None,
            douse_fire: None,
            freeze_on_hit: None,
            grant_xp: None,
            hurt_owner: None,
            ignite: None,
            impact_damage: None,
            mob_effect: None,
            on_fire_time: None,
            particle_on_hit: None,
            potion_effect: None,
            remove_on_hit: None,
            spawn_aoe_cloud: None,
            spawn_chance: None,
            stick_in_ground: None,
            teleport_owner: None,
            thrown_potion_effect: None,
        }
    }
}
/// Bedrock component `minecraft:projectile`. Allows the entity to be a thrown entity.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Projectile {
    ///Allows you to choose an anchor point for where the projectile is fired from. 0 = Original point, 1 = EyeHeight, and 2 = Middle or body height.
    pub anchor: Option<i32>,
    ///Alters the angle at which a projectile is vertically shot. Many splash potions in the game use this to offset their angles by -20 degrees.
    pub angle_offset: Option<f32>,
    ///If true, the entity hit will be set on fire.
    pub catch_fire: Option<bool>,
    ///If true, when a projectile deals damage, whether or not to spawn in the critical damage particles.
    pub crit_particle_on_hurt: Option<bool>,
    ///When this projectile deals damage, whether or not to immediately destroy this projectile.
    pub destroy_on_hurt: Option<bool>,
    ///Entity Definitions defined here can't be hurt by the projectile.
    pub filter: Option<String>,
    ///If true, whether the projectile causes fire is affected by the mob griefing game rule.
    pub fire_affected_by_griefing: Option<bool>,
    ///The gravity applied to this entity when thrown. When this actor is not on the ground, subtracts this amount from the actors change in vertical position every tick. The higher the value, the faster the entity falls.
    pub gravity: Option<f32>,
    ///The sound that plays when the projectile hits the ground.
    pub hit_ground_sound: Option<String>,
    ///If true, when hitting a vehicle, and there's at least one passenger in the vehicle, the damage will be dealt to the passenger closest to the projectile impact point. If there are no passengers, this setting does nothing.
    pub hit_nearest_passenger: Option<bool>,
    ///The sound that plays when the projectile hits something.
    pub hit_sound: Option<String>,
    ///If true, the projectile homes in to the nearest entity.
    pub homing: Option<bool>,
    ///[EXPERIMENTAL] An array of strings defining the types of entities that this entity does not collide with.
    pub ignored_entities: Option<Vec<String>>,
    ///The fraction of the projectile's speed maintained every frame while traveling in air.
    pub inertia: Option<f32>,
    ///If true, the projectile will be treated as dangerous to the players.
    pub is_dangerous: Option<bool>,
    ///If true, the projectile will knock back the entity it hits.
    pub knockback: Option<bool>,
    ///If true, the entity hit will be struck by lightning.
    pub lightning: Option<bool>,
    ///The fraction of the projectile's speed maintained every frame while traveling in water.
    pub liquid_inertia: Option<f32>,
    ///SEE on_hit/mob_effect.
    pub mob_effect: Option<crate::types::BedrockValue>,
    ///If true, the projectile can hit multiple entities per flight.
    pub multiple_targets: Option<bool>,
    ///The offset from the entity's anchor where the projectile will spawn.
    pub offset: Option<Vec<f32>>,
    ///Time in seconds that the entity hit will be on fire for.
    pub on_fire_time: Option<f32>,
    ///Defines the behaviors that may execute on a projectile's hit, including impact damage, impact effect, and stuck in ground. See more on these parameters below.
    pub on_hit: Option<ProjectileOnHit>,
    ///Particle to use upon collision.
    pub particle: Option<String>,
    ///Defines the effect the arrow will apply to the entity it hits.
    pub potion_effect: Option<i32>,
    ///Determines the velocity of the projectile.
    pub power: Option<f32>,
    ///During the specified time, in seconds, the projectile cannot be reflected by hitting it
    pub reflect_immunity: Option<f32>,
    ///If true, this entity will be reflected back when hit.
    pub reflect_on_hurt: Option<bool>,
    ///If true, damage will be randomized based on damage and speed.
    pub semi_random_diff_damage: Option<bool>,
    ///The sound that plays when the projectile is shot.
    pub shoot_sound: Option<String>,
    ///If true, the projectile will be shot towards the target of the entity firing it.
    pub shoot_target: Option<bool>,
    ///If true, the projectile will bounce upon hit.
    pub should_bounce: Option<bool>,
    ///If true, the projectile will be treated like a splash potion.
    pub splash_potion: Option<bool>,
    ///Radius in blocks of the 'splash' effect.
    pub splash_range: Option<f32>,
    ///Determines if the projectile stops when the target is hurt.
    pub stop_on_hurt: Option<bool>,
    ///The base accuracy. Accuracy is determined by the formula uncertaintyBase - difficultyLevel * uncertaintyMultiplier.
    pub uncertainty_base: Option<f32>,
    ///Determines how much difficulty affects accuracy. Accuracy is determined by the formula uncertaintyBase - difficultyLevel * uncertaintyMultiplier.
    pub uncertainty_multiplier: Option<f32>,
}
impl Default for Projectile {
    fn default() -> Self {
        Self {
            anchor: None,
            angle_offset: Some(0f32),
            catch_fire: Some(false),
            crit_particle_on_hurt: Some(false),
            destroy_on_hurt: Some(false),
            filter: None,
            fire_affected_by_griefing: Some(false),
            gravity: Some(0.05f32),
            hit_ground_sound: None,
            hit_nearest_passenger: Some(false),
            hit_sound: None,
            homing: Some(false),
            ignored_entities: None,
            inertia: Some(0.99f32),
            is_dangerous: Some(false),
            knockback: Some(true),
            lightning: Some(false),
            liquid_inertia: Some(0.6f32),
            mob_effect: None,
            multiple_targets: Some(true),
            offset: Some(vec![0f32, 0f32, 0f32]),
            on_fire_time: Some(5f32),
            on_hit: None,
            particle: Some("iconcrack".to_string()),
            potion_effect: Some(-1i32),
            power: Some(1.3f32),
            reflect_immunity: Some(0f32),
            reflect_on_hurt: Some(false),
            semi_random_diff_damage: Some(false),
            shoot_sound: None,
            shoot_target: Some(true),
            should_bounce: Some(false),
            splash_potion: Some(false),
            splash_range: Some(4f32),
            stop_on_hurt: None,
            uncertainty_base: Some(0f32),
            uncertainty_multiplier: Some(0f32),
        }
    }
}
