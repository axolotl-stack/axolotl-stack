//! Damage and healing sources.

use bevy_ecs::prelude::*;

/// Source of damage dealt to an entity.
#[derive(Debug, Clone)]
pub enum DamageSource {
    /// Damage from another entity's attack.
    Attack { attacker: Entity },
    /// Damage from a projectile.
    Projectile {
        projectile: Entity,
        owner: Option<Entity>,
    },
    /// Damage from falling.
    Fall { distance: f32 },
    /// Damage from drowning.
    Drowning,
    /// Damage from suffocation in blocks.
    Suffocation,
    /// Damage from being in the void.
    Void,
    /// Damage from fire/lava.
    Fire { is_lava: bool },
    /// Damage from explosions.
    Explosion { source: Option<Entity> },
    /// Damage from lightning.
    Lightning,
    /// Damage from starving.
    Starvation,
    /// Damage from magic (potions, etc).
    Magic { source: Option<Entity> },
    /// Damage from thorns enchantment.
    Thorns { attacker: Entity },
    /// Damage from cactus.
    Cactus,
    /// Damage from sweet berry bush.
    SweetBerryBush,
    /// Generic/custom damage.
    Generic,
}

impl DamageSource {
    /// Whether armor reduces this damage type.
    pub fn reduced_by_armor(&self) -> bool {
        matches!(
            self,
            DamageSource::Attack { .. }
                | DamageSource::Projectile { .. }
                | DamageSource::Explosion { .. }
                | DamageSource::Lightning
                | DamageSource::Cactus
                | DamageSource::SweetBerryBush
        )
    }

    /// Whether resistance effect reduces this damage.
    pub fn reduced_by_resistance(&self) -> bool {
        !matches!(self, DamageSource::Void | DamageSource::Starvation)
    }

    /// Whether this is fire-based damage (blocked by fire resistance).
    pub fn is_fire(&self) -> bool {
        matches!(self, DamageSource::Fire { .. })
    }

    /// Whether totems can prevent death from this source.
    pub fn ignores_totem(&self) -> bool {
        matches!(self, DamageSource::Void)
    }

    /// Whether this damage bypasses armor entirely.
    pub fn bypasses_armor(&self) -> bool {
        matches!(
            self,
            DamageSource::Void
                | DamageSource::Starvation
                | DamageSource::Drowning
                | DamageSource::Magic { .. }
        )
    }

    /// Whether this damage is from a player.
    pub fn is_player_damage(&self) -> bool {
        // Would need to check if attacker entity has Player component
        // For now, return false; systems should check this
        false
    }

    /// Get the attacker entity if any.
    pub fn attacker(&self) -> Option<Entity> {
        match self {
            DamageSource::Attack { attacker } => Some(*attacker),
            DamageSource::Projectile { owner, .. } => *owner,
            DamageSource::Thorns { attacker } => Some(*attacker),
            DamageSource::Explosion { source } => *source,
            DamageSource::Magic { source } => *source,
            _ => None,
        }
    }
}

/// Source of healing applied to an entity.
#[derive(Debug, Clone)]
pub enum HealingSource {
    /// Healing from food/saturation.
    Food,
    /// Healing from regeneration effect.
    Regeneration,
    /// Healing from instant health potion.
    InstantHealth,
    /// Healing from a beacon.
    Beacon,
    /// Healing from sleeping.
    Sleep,
    /// Healing from totems.
    Totem,
    /// Generic/custom healing.
    Generic,
}

/// Event for when an entity takes damage.
#[derive(Event, Debug)]
pub struct DamageEvent {
    pub entity: Entity,
    pub source: DamageSource,
    pub amount: f32,
    pub final_amount: f32,
}

/// Event for when an entity heals.
#[derive(Event, Debug)]
pub struct HealEvent {
    pub entity: Entity,
    pub source: HealingSource,
    pub amount: f32,
}

/// Event for when an entity dies.
#[derive(Event, Debug)]
pub struct DeathEvent {
    pub entity: Entity,
    pub source: DamageSource,
}
