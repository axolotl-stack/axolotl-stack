//! Components for living entities (players, mobs).

use bevy_ecs::prelude::*;
use std::collections::HashMap;

/// Marker component for living entities.
#[derive(Component, Debug, Default)]
pub struct Living;

/// Health component.
#[derive(Component, Debug, Clone)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            current: 20.0,
            max: 20.0,
        }
    }
}

impl Health {
    pub fn new(max: f32) -> Self {
        Self { current: max, max }
    }

    pub fn with_current(current: f32, max: f32) -> Self {
        Self {
            current: current.min(max),
            max,
        }
    }

    pub fn damage(&mut self, amount: f32) {
        self.current = (self.current - amount).max(0.0);
    }

    pub fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }

    pub fn is_dead(&self) -> bool {
        self.current <= 0.0
    }

    pub fn set_max(&mut self, max: f32) {
        self.max = max.max(1.0);
        self.current = self.current.min(self.max);
    }
}

/// Status effect type ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectType(pub u8);

impl EffectType {
    pub const SPEED: Self = Self(1);
    pub const SLOWNESS: Self = Self(2);
    pub const HASTE: Self = Self(3);
    pub const MINING_FATIGUE: Self = Self(4);
    pub const STRENGTH: Self = Self(5);
    pub const INSTANT_HEALTH: Self = Self(6);
    pub const INSTANT_DAMAGE: Self = Self(7);
    pub const JUMP_BOOST: Self = Self(8);
    pub const NAUSEA: Self = Self(9);
    pub const REGENERATION: Self = Self(10);
    pub const RESISTANCE: Self = Self(11);
    pub const FIRE_RESISTANCE: Self = Self(12);
    pub const WATER_BREATHING: Self = Self(13);
    pub const INVISIBILITY: Self = Self(14);
    pub const BLINDNESS: Self = Self(15);
    pub const NIGHT_VISION: Self = Self(16);
    pub const HUNGER: Self = Self(17);
    pub const WEAKNESS: Self = Self(18);
    pub const POISON: Self = Self(19);
    pub const WITHER: Self = Self(20);
    pub const HEALTH_BOOST: Self = Self(21);
    pub const ABSORPTION: Self = Self(22);
    pub const SATURATION: Self = Self(23);
}

/// A single active effect.
#[derive(Debug, Clone)]
pub struct ActiveEffect {
    pub level: u8,
    pub duration_ticks: u32,
    pub ambient: bool,
    pub show_particles: bool,
}

impl ActiveEffect {
    pub fn new(level: u8, duration_ticks: u32) -> Self {
        Self {
            level,
            duration_ticks,
            ambient: false,
            show_particles: true,
        }
    }

    pub fn tick(&mut self) {
        self.duration_ticks = self.duration_ticks.saturating_sub(1);
    }

    pub fn is_expired(&self) -> bool {
        self.duration_ticks == 0
    }
}

/// Component storing all active effects.
#[derive(Component, Debug, Default)]
pub struct Effects {
    pub active: HashMap<EffectType, ActiveEffect>,
}

impl Effects {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update an effect. Higher level or longer duration overwrites.
    pub fn add(&mut self, effect_type: EffectType, level: u8, duration_ticks: u32) {
        if let Some(existing) = self.active.get(&effect_type) {
            if existing.level > level {
                return;
            }
            if existing.level == level && existing.duration_ticks > duration_ticks {
                return;
            }
        }
        self.active
            .insert(effect_type, ActiveEffect::new(level, duration_ticks));
    }

    pub fn remove(&mut self, effect_type: EffectType) {
        self.active.remove(&effect_type);
    }

    pub fn get(&self, effect_type: EffectType) -> Option<&ActiveEffect> {
        self.active.get(&effect_type)
    }

    pub fn has(&self, effect_type: EffectType) -> bool {
        self.active.contains_key(&effect_type)
    }

    /// Tick all effects, removing expired ones. Returns expired effect types.
    pub fn tick(&mut self) -> Vec<EffectType> {
        let mut expired = Vec::new();
        for (effect_type, effect) in &mut self.active {
            effect.tick();
            if effect.is_expired() {
                expired.push(*effect_type);
            }
        }
        for effect_type in &expired {
            self.active.remove(effect_type);
        }
        expired
    }
}

/// Air supply for underwater breathing.
#[derive(Component, Debug, Clone)]
pub struct AirSupply {
    pub current_ticks: u32,
    pub max_ticks: u32,
}

impl Default for AirSupply {
    fn default() -> Self {
        Self {
            current_ticks: 300, // 15 seconds
            max_ticks: 300,
        }
    }
}

impl AirSupply {
    /// Tick air supply. Returns true if drowning.
    pub fn tick(&mut self, is_underwater: bool) -> bool {
        if is_underwater {
            if self.current_ticks > 0 {
                self.current_ticks -= 1;
                false
            } else {
                true // Drowning
            }
        } else {
            // Recover air when not underwater
            self.current_ticks = (self.current_ticks + 4).min(self.max_ticks);
            false
        }
    }
}

/// Fire duration (ticks remaining on fire).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct OnFire {
    pub ticks_remaining: u32,
}

impl OnFire {
    pub fn new(ticks: u32) -> Self {
        Self {
            ticks_remaining: ticks,
        }
    }

    pub fn tick(&mut self) {
        self.ticks_remaining = self.ticks_remaining.saturating_sub(1);
    }

    pub fn is_on_fire(&self) -> bool {
        self.ticks_remaining > 0
    }

    pub fn extinguish(&mut self) {
        self.ticks_remaining = 0;
    }
}

/// Movement speed multiplier.
#[derive(Component, Debug, Clone, Copy)]
pub struct Speed(pub f32);

impl Default for Speed {
    fn default() -> Self {
        Self(0.1) // Default player speed
    }
}

/// Damage immunity timer (ticks until can be hurt again).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct DamageImmunity {
    pub ticks_remaining: u32,
}

impl DamageImmunity {
    pub fn new(ticks: u32) -> Self {
        Self {
            ticks_remaining: ticks,
        }
    }

    pub fn tick(&mut self) {
        self.ticks_remaining = self.ticks_remaining.saturating_sub(1);
    }

    pub fn is_immune(&self) -> bool {
        self.ticks_remaining > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Health Tests ==========

    #[test]
    fn health_default() {
        let health = Health::default();
        assert_eq!(health.current, 20.0);
        assert_eq!(health.max, 20.0);
    }

    #[test]
    fn health_new_sets_current_to_max() {
        let health = Health::new(30.0);
        assert_eq!(health.current, 30.0);
        assert_eq!(health.max, 30.0);
    }

    #[test]
    fn health_with_current_clamps_to_max() {
        let health = Health::with_current(100.0, 20.0);
        assert_eq!(health.current, 20.0);
        assert_eq!(health.max, 20.0);
    }

    #[test]
    fn health_with_current_allows_lower() {
        let health = Health::with_current(10.0, 20.0);
        assert_eq!(health.current, 10.0);
        assert_eq!(health.max, 20.0);
    }

    #[test]
    fn health_damage_reduces_current() {
        let mut health = Health::new(20.0);
        health.damage(5.0);
        assert_eq!(health.current, 15.0);
    }

    #[test]
    fn health_damage_clamps_to_zero() {
        let mut health = Health::new(20.0);
        health.damage(100.0);
        assert_eq!(health.current, 0.0);
    }

    #[test]
    fn health_damage_exactly_to_zero() {
        let mut health = Health::new(20.0);
        health.damage(20.0);
        assert_eq!(health.current, 0.0);
    }

    #[test]
    fn health_heal_increases_current() {
        let mut health = Health::with_current(10.0, 20.0);
        health.heal(5.0);
        assert_eq!(health.current, 15.0);
    }

    #[test]
    fn health_heal_clamps_to_max() {
        let mut health = Health::with_current(10.0, 20.0);
        health.heal(100.0);
        assert_eq!(health.current, 20.0);
    }

    #[test]
    fn health_heal_at_max_unchanged() {
        let mut health = Health::new(20.0);
        health.heal(10.0);
        assert_eq!(health.current, 20.0);
    }

    #[test]
    fn health_is_dead_at_zero() {
        let health = Health::with_current(0.0, 20.0);
        assert!(health.is_dead());
    }

    #[test]
    fn health_is_dead_below_zero() {
        // This shouldn't happen normally, but test the boundary
        let mut health = Health::new(5.0);
        health.damage(10.0);
        assert!(health.is_dead());
        assert_eq!(health.current, 0.0);
    }

    #[test]
    fn health_is_alive_above_zero() {
        let health = Health::with_current(0.1, 20.0);
        assert!(!health.is_dead());
    }

    #[test]
    fn health_set_max_clamps_current() {
        let mut health = Health::new(20.0);
        health.set_max(10.0);
        assert_eq!(health.max, 10.0);
        assert_eq!(health.current, 10.0);
    }

    #[test]
    fn health_set_max_preserves_lower_current() {
        let mut health = Health::with_current(5.0, 20.0);
        health.set_max(30.0);
        assert_eq!(health.max, 30.0);
        assert_eq!(health.current, 5.0);
    }

    #[test]
    fn health_set_max_minimum_one() {
        let mut health = Health::new(20.0);
        health.set_max(0.0);
        assert_eq!(health.max, 1.0);
    }

    #[test]
    fn health_set_max_negative_becomes_one() {
        let mut health = Health::new(20.0);
        health.set_max(-10.0);
        assert_eq!(health.max, 1.0);
    }

    // ========== ActiveEffect Tests ==========

    #[test]
    fn active_effect_new_defaults() {
        let effect = ActiveEffect::new(2, 100);
        assert_eq!(effect.level, 2);
        assert_eq!(effect.duration_ticks, 100);
        assert!(!effect.ambient);
        assert!(effect.show_particles);
    }

    #[test]
    fn active_effect_tick_decreases_duration() {
        let mut effect = ActiveEffect::new(1, 10);
        effect.tick();
        assert_eq!(effect.duration_ticks, 9);
    }

    #[test]
    fn active_effect_tick_saturates_at_zero() {
        let mut effect = ActiveEffect::new(1, 0);
        effect.tick();
        assert_eq!(effect.duration_ticks, 0);
    }

    #[test]
    fn active_effect_is_expired_at_zero() {
        let effect = ActiveEffect::new(1, 0);
        assert!(effect.is_expired());
    }

    #[test]
    fn active_effect_not_expired_above_zero() {
        let effect = ActiveEffect::new(1, 1);
        assert!(!effect.is_expired());
    }

    #[test]
    fn active_effect_expires_after_tick() {
        let mut effect = ActiveEffect::new(1, 1);
        assert!(!effect.is_expired());
        effect.tick();
        assert!(effect.is_expired());
    }

    // ========== Effects Collection Tests ==========

    #[test]
    fn effects_new_is_empty() {
        let effects = Effects::new();
        assert!(effects.active.is_empty());
    }

    #[test]
    fn effects_add_new_effect() {
        let mut effects = Effects::new();
        effects.add(EffectType::SPEED, 1, 100);
        assert!(effects.has(EffectType::SPEED));
        assert_eq!(effects.get(EffectType::SPEED).unwrap().level, 1);
        assert_eq!(effects.get(EffectType::SPEED).unwrap().duration_ticks, 100);
    }

    #[test]
    fn effects_add_higher_level_overwrites() {
        let mut effects = Effects::new();
        effects.add(EffectType::SPEED, 1, 100);
        effects.add(EffectType::SPEED, 2, 50);

        let effect = effects.get(EffectType::SPEED).unwrap();
        assert_eq!(effect.level, 2);
        assert_eq!(effect.duration_ticks, 50);
    }

    #[test]
    fn effects_add_lower_level_ignored() {
        let mut effects = Effects::new();
        effects.add(EffectType::SPEED, 2, 100);
        effects.add(EffectType::SPEED, 1, 200);

        let effect = effects.get(EffectType::SPEED).unwrap();
        assert_eq!(effect.level, 2);
        assert_eq!(effect.duration_ticks, 100);
    }

    #[test]
    fn effects_add_same_level_longer_duration_overwrites() {
        let mut effects = Effects::new();
        effects.add(EffectType::SPEED, 1, 100);
        effects.add(EffectType::SPEED, 1, 200);

        let effect = effects.get(EffectType::SPEED).unwrap();
        assert_eq!(effect.level, 1);
        assert_eq!(effect.duration_ticks, 200);
    }

    #[test]
    fn effects_add_same_level_shorter_duration_ignored() {
        let mut effects = Effects::new();
        effects.add(EffectType::SPEED, 1, 200);
        effects.add(EffectType::SPEED, 1, 100);

        let effect = effects.get(EffectType::SPEED).unwrap();
        assert_eq!(effect.level, 1);
        assert_eq!(effect.duration_ticks, 200);
    }

    #[test]
    fn effects_remove() {
        let mut effects = Effects::new();
        effects.add(EffectType::SPEED, 1, 100);
        effects.remove(EffectType::SPEED);
        assert!(!effects.has(EffectType::SPEED));
    }

    #[test]
    fn effects_remove_nonexistent_ok() {
        let mut effects = Effects::new();
        effects.remove(EffectType::SPEED);
        assert!(!effects.has(EffectType::SPEED));
    }

    #[test]
    fn effects_tick_decreases_all_durations() {
        let mut effects = Effects::new();
        effects.add(EffectType::SPEED, 1, 10);
        effects.add(EffectType::STRENGTH, 2, 20);

        effects.tick();

        assert_eq!(effects.get(EffectType::SPEED).unwrap().duration_ticks, 9);
        assert_eq!(effects.get(EffectType::STRENGTH).unwrap().duration_ticks, 19);
    }

    #[test]
    fn effects_tick_removes_expired() {
        let mut effects = Effects::new();
        effects.add(EffectType::SPEED, 1, 1);
        effects.add(EffectType::STRENGTH, 2, 10);

        let expired = effects.tick();

        assert!(!effects.has(EffectType::SPEED));
        assert!(effects.has(EffectType::STRENGTH));
        assert_eq!(expired, vec![EffectType::SPEED]);
    }

    #[test]
    fn effects_tick_returns_multiple_expired() {
        let mut effects = Effects::new();
        effects.add(EffectType::SPEED, 1, 1);
        effects.add(EffectType::STRENGTH, 2, 1);
        effects.add(EffectType::HASTE, 1, 10);

        let expired = effects.tick();

        assert!(!effects.has(EffectType::SPEED));
        assert!(!effects.has(EffectType::STRENGTH));
        assert!(effects.has(EffectType::HASTE));
        assert_eq!(expired.len(), 2);
    }

    #[test]
    fn effects_multiple_different_types() {
        let mut effects = Effects::new();
        effects.add(EffectType::SPEED, 1, 100);
        effects.add(EffectType::STRENGTH, 2, 200);
        effects.add(EffectType::RESISTANCE, 3, 300);

        assert!(effects.has(EffectType::SPEED));
        assert!(effects.has(EffectType::STRENGTH));
        assert!(effects.has(EffectType::RESISTANCE));
        assert!(!effects.has(EffectType::POISON));
    }

    // ========== AirSupply Tests ==========

    #[test]
    fn air_supply_default() {
        let air = AirSupply::default();
        assert_eq!(air.current_ticks, 300);
        assert_eq!(air.max_ticks, 300);
    }

    #[test]
    fn air_supply_tick_underwater_decreases() {
        let mut air = AirSupply::default();
        let drowning = air.tick(true);
        assert_eq!(air.current_ticks, 299);
        assert!(!drowning);
    }

    #[test]
    fn air_supply_tick_at_zero_drowning() {
        let mut air = AirSupply {
            current_ticks: 0,
            max_ticks: 300,
        };
        let drowning = air.tick(true);
        assert!(drowning);
    }

    #[test]
    fn air_supply_tick_not_underwater_recovers() {
        let mut air = AirSupply {
            current_ticks: 100,
            max_ticks: 300,
        };
        let drowning = air.tick(false);
        assert_eq!(air.current_ticks, 104);
        assert!(!drowning);
    }

    #[test]
    fn air_supply_recovery_clamps_to_max() {
        let mut air = AirSupply {
            current_ticks: 298,
            max_ticks: 300,
        };
        air.tick(false);
        assert_eq!(air.current_ticks, 300);
    }

    #[test]
    fn air_supply_at_max_no_change() {
        let mut air = AirSupply::default();
        air.tick(false);
        assert_eq!(air.current_ticks, 300);
    }

    // ========== OnFire Tests ==========

    #[test]
    fn on_fire_default_not_on_fire() {
        let fire = OnFire::default();
        assert!(!fire.is_on_fire());
        assert_eq!(fire.ticks_remaining, 0);
    }

    #[test]
    fn on_fire_new() {
        let fire = OnFire::new(100);
        assert!(fire.is_on_fire());
        assert_eq!(fire.ticks_remaining, 100);
    }

    #[test]
    fn on_fire_tick_decreases() {
        let mut fire = OnFire::new(10);
        fire.tick();
        assert_eq!(fire.ticks_remaining, 9);
        assert!(fire.is_on_fire());
    }

    #[test]
    fn on_fire_tick_saturates_at_zero() {
        let mut fire = OnFire::new(0);
        fire.tick();
        assert_eq!(fire.ticks_remaining, 0);
    }

    #[test]
    fn on_fire_extinguish() {
        let mut fire = OnFire::new(100);
        fire.extinguish();
        assert_eq!(fire.ticks_remaining, 0);
        assert!(!fire.is_on_fire());
    }

    #[test]
    fn on_fire_expires_after_ticks() {
        let mut fire = OnFire::new(1);
        assert!(fire.is_on_fire());
        fire.tick();
        assert!(!fire.is_on_fire());
    }

    // ========== Speed Tests ==========

    #[test]
    fn speed_default() {
        let speed = Speed::default();
        assert_eq!(speed.0, 0.1);
    }

    // ========== DamageImmunity Tests ==========

    #[test]
    fn damage_immunity_default_not_immune() {
        let immunity = DamageImmunity::default();
        assert!(!immunity.is_immune());
        assert_eq!(immunity.ticks_remaining, 0);
    }

    #[test]
    fn damage_immunity_new() {
        let immunity = DamageImmunity::new(20);
        assert!(immunity.is_immune());
        assert_eq!(immunity.ticks_remaining, 20);
    }

    #[test]
    fn damage_immunity_tick_decreases() {
        let mut immunity = DamageImmunity::new(10);
        immunity.tick();
        assert_eq!(immunity.ticks_remaining, 9);
    }

    #[test]
    fn damage_immunity_tick_saturates_at_zero() {
        let mut immunity = DamageImmunity::new(0);
        immunity.tick();
        assert_eq!(immunity.ticks_remaining, 0);
    }

    #[test]
    fn damage_immunity_expires_after_ticks() {
        let mut immunity = DamageImmunity::new(1);
        assert!(immunity.is_immune());
        immunity.tick();
        assert!(!immunity.is_immune());
    }

    // ========== EffectType Constants Tests ==========

    #[test]
    fn effect_type_constants_unique() {
        let types = [
            EffectType::SPEED,
            EffectType::SLOWNESS,
            EffectType::HASTE,
            EffectType::MINING_FATIGUE,
            EffectType::STRENGTH,
            EffectType::INSTANT_HEALTH,
            EffectType::INSTANT_DAMAGE,
            EffectType::JUMP_BOOST,
            EffectType::NAUSEA,
            EffectType::REGENERATION,
            EffectType::RESISTANCE,
            EffectType::FIRE_RESISTANCE,
            EffectType::WATER_BREATHING,
            EffectType::INVISIBILITY,
            EffectType::BLINDNESS,
            EffectType::NIGHT_VISION,
            EffectType::HUNGER,
            EffectType::WEAKNESS,
            EffectType::POISON,
            EffectType::WITHER,
            EffectType::HEALTH_BOOST,
            EffectType::ABSORPTION,
            EffectType::SATURATION,
        ];

        // Check all unique
        let mut seen = std::collections::HashSet::new();
        for t in &types {
            assert!(seen.insert(t.0), "Duplicate effect ID: {}", t.0);
        }
    }

    #[test]
    fn effect_type_ids_in_expected_range() {
        assert_eq!(EffectType::SPEED.0, 1);
        assert_eq!(EffectType::SATURATION.0, 23);
    }
}
