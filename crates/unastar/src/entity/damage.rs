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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::world::World;

    fn dummy_entity() -> Entity {
        let mut world = World::new();
        world.spawn_empty().id()
    }

    // ========== DamageSource::reduced_by_armor Tests ==========

    #[test]
    fn attack_reduced_by_armor() {
        let source = DamageSource::Attack {
            attacker: dummy_entity(),
        };
        assert!(source.reduced_by_armor());
    }

    #[test]
    fn projectile_reduced_by_armor() {
        let source = DamageSource::Projectile {
            projectile: dummy_entity(),
            owner: Some(dummy_entity()),
        };
        assert!(source.reduced_by_armor());
    }

    #[test]
    fn explosion_reduced_by_armor() {
        let source = DamageSource::Explosion {
            source: Some(dummy_entity()),
        };
        assert!(source.reduced_by_armor());
    }

    #[test]
    fn lightning_reduced_by_armor() {
        assert!(DamageSource::Lightning.reduced_by_armor());
    }

    #[test]
    fn cactus_reduced_by_armor() {
        assert!(DamageSource::Cactus.reduced_by_armor());
    }

    #[test]
    fn sweet_berry_bush_reduced_by_armor() {
        assert!(DamageSource::SweetBerryBush.reduced_by_armor());
    }

    #[test]
    fn fall_not_reduced_by_armor() {
        assert!(!DamageSource::Fall { distance: 10.0 }.reduced_by_armor());
    }

    #[test]
    fn drowning_not_reduced_by_armor() {
        assert!(!DamageSource::Drowning.reduced_by_armor());
    }

    #[test]
    fn suffocation_not_reduced_by_armor() {
        assert!(!DamageSource::Suffocation.reduced_by_armor());
    }

    #[test]
    fn void_not_reduced_by_armor() {
        assert!(!DamageSource::Void.reduced_by_armor());
    }

    #[test]
    fn fire_not_reduced_by_armor() {
        assert!(!DamageSource::Fire { is_lava: false }.reduced_by_armor());
        assert!(!DamageSource::Fire { is_lava: true }.reduced_by_armor());
    }

    #[test]
    fn starvation_not_reduced_by_armor() {
        assert!(!DamageSource::Starvation.reduced_by_armor());
    }

    #[test]
    fn magic_not_reduced_by_armor() {
        assert!(!DamageSource::Magic { source: None }.reduced_by_armor());
    }

    #[test]
    fn generic_not_reduced_by_armor() {
        assert!(!DamageSource::Generic.reduced_by_armor());
    }

    // ========== DamageSource::reduced_by_resistance Tests ==========

    #[test]
    fn most_damage_reduced_by_resistance() {
        let sources = [
            DamageSource::Attack {
                attacker: dummy_entity(),
            },
            DamageSource::Projectile {
                projectile: dummy_entity(),
                owner: None,
            },
            DamageSource::Fall { distance: 5.0 },
            DamageSource::Drowning,
            DamageSource::Suffocation,
            DamageSource::Fire { is_lava: false },
            DamageSource::Explosion { source: None },
            DamageSource::Lightning,
            DamageSource::Magic { source: None },
            DamageSource::Thorns {
                attacker: dummy_entity(),
            },
            DamageSource::Cactus,
            DamageSource::SweetBerryBush,
            DamageSource::Generic,
        ];

        for source in sources {
            assert!(
                source.reduced_by_resistance(),
                "{:?} should be reduced by resistance",
                source
            );
        }
    }

    #[test]
    fn void_not_reduced_by_resistance() {
        assert!(!DamageSource::Void.reduced_by_resistance());
    }

    #[test]
    fn starvation_not_reduced_by_resistance() {
        assert!(!DamageSource::Starvation.reduced_by_resistance());
    }

    // ========== DamageSource::is_fire Tests ==========

    #[test]
    fn fire_is_fire() {
        assert!(DamageSource::Fire { is_lava: false }.is_fire());
        assert!(DamageSource::Fire { is_lava: true }.is_fire());
    }

    #[test]
    fn non_fire_is_not_fire() {
        assert!(!DamageSource::Attack {
            attacker: dummy_entity()
        }
        .is_fire());
        assert!(!DamageSource::Lightning.is_fire());
        assert!(!DamageSource::Explosion { source: None }.is_fire());
    }

    // ========== DamageSource::ignores_totem Tests ==========

    #[test]
    fn void_ignores_totem() {
        assert!(DamageSource::Void.ignores_totem());
    }

    #[test]
    fn other_damage_respects_totem() {
        let sources = [
            DamageSource::Attack {
                attacker: dummy_entity(),
            },
            DamageSource::Fall { distance: 100.0 },
            DamageSource::Drowning,
            DamageSource::Fire { is_lava: true },
            DamageSource::Explosion { source: None },
            DamageSource::Starvation,
            DamageSource::Generic,
        ];

        for source in sources {
            assert!(
                !source.ignores_totem(),
                "{:?} should respect totem",
                source
            );
        }
    }

    // ========== DamageSource::bypasses_armor Tests ==========

    #[test]
    fn void_bypasses_armor() {
        assert!(DamageSource::Void.bypasses_armor());
    }

    #[test]
    fn starvation_bypasses_armor() {
        assert!(DamageSource::Starvation.bypasses_armor());
    }

    #[test]
    fn drowning_bypasses_armor() {
        assert!(DamageSource::Drowning.bypasses_armor());
    }

    #[test]
    fn magic_bypasses_armor() {
        assert!(DamageSource::Magic { source: None }.bypasses_armor());
        assert!(DamageSource::Magic {
            source: Some(dummy_entity())
        }
        .bypasses_armor());
    }

    #[test]
    fn physical_damage_does_not_bypass_armor() {
        let sources = [
            DamageSource::Attack {
                attacker: dummy_entity(),
            },
            DamageSource::Projectile {
                projectile: dummy_entity(),
                owner: None,
            },
            DamageSource::Fall { distance: 5.0 },
            DamageSource::Fire { is_lava: false },
            DamageSource::Explosion { source: None },
            DamageSource::Lightning,
            DamageSource::Cactus,
            DamageSource::SweetBerryBush,
            DamageSource::Generic,
        ];

        for source in sources {
            assert!(
                !source.bypasses_armor(),
                "{:?} should not bypass armor",
                source
            );
        }
    }

    // ========== DamageSource::attacker Tests ==========

    #[test]
    fn attack_has_attacker() {
        let entity = dummy_entity();
        let source = DamageSource::Attack { attacker: entity };
        assert_eq!(source.attacker(), Some(entity));
    }

    #[test]
    fn projectile_attacker_from_owner() {
        let projectile = dummy_entity();
        let owner = dummy_entity();

        let source = DamageSource::Projectile {
            projectile,
            owner: Some(owner),
        };
        assert_eq!(source.attacker(), Some(owner));
    }

    #[test]
    fn projectile_no_owner_no_attacker() {
        let source = DamageSource::Projectile {
            projectile: dummy_entity(),
            owner: None,
        };
        assert_eq!(source.attacker(), None);
    }

    #[test]
    fn thorns_has_attacker() {
        let entity = dummy_entity();
        let source = DamageSource::Thorns { attacker: entity };
        assert_eq!(source.attacker(), Some(entity));
    }

    #[test]
    fn explosion_attacker_from_source() {
        let entity = dummy_entity();
        let source = DamageSource::Explosion {
            source: Some(entity),
        };
        assert_eq!(source.attacker(), Some(entity));
    }

    #[test]
    fn explosion_no_source_no_attacker() {
        let source = DamageSource::Explosion { source: None };
        assert_eq!(source.attacker(), None);
    }

    #[test]
    fn magic_attacker_from_source() {
        let entity = dummy_entity();
        let source = DamageSource::Magic {
            source: Some(entity),
        };
        assert_eq!(source.attacker(), Some(entity));
    }

    #[test]
    fn magic_no_source_no_attacker() {
        let source = DamageSource::Magic { source: None };
        assert_eq!(source.attacker(), None);
    }

    #[test]
    fn environmental_damage_no_attacker() {
        let sources = [
            DamageSource::Fall { distance: 10.0 },
            DamageSource::Drowning,
            DamageSource::Suffocation,
            DamageSource::Void,
            DamageSource::Fire { is_lava: false },
            DamageSource::Lightning,
            DamageSource::Starvation,
            DamageSource::Cactus,
            DamageSource::SweetBerryBush,
            DamageSource::Generic,
        ];

        for source in sources {
            assert_eq!(
                source.attacker(),
                None,
                "{:?} should have no attacker",
                source
            );
        }
    }

    // ========== is_player_damage Test ==========

    #[test]
    fn is_player_damage_returns_false() {
        // Current implementation always returns false
        let source = DamageSource::Attack {
            attacker: dummy_entity(),
        };
        assert!(!source.is_player_damage());
    }

    // ========== HealingSource Variant Tests ==========

    #[test]
    fn healing_source_variants_exist() {
        // Just ensure variants compile and can be constructed
        let _ = HealingSource::Food;
        let _ = HealingSource::Regeneration;
        let _ = HealingSource::InstantHealth;
        let _ = HealingSource::Beacon;
        let _ = HealingSource::Sleep;
        let _ = HealingSource::Totem;
        let _ = HealingSource::Generic;
    }
}
