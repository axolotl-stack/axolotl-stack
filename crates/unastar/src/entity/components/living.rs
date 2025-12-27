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
