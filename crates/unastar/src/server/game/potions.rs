//! Configurable potion contents shared by drinking, thrown bottles and clouds.
//!
//! A projectile captures an immutable profile when launched; later catalog
//! changes affect subsequent uses. Default profiles cover the classic aux
//! values 0 through 42. Effect identifiers do not imply that every lasting
//! effect's gameplay behavior is implemented by the host.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

use crate::entity::components::EffectType;
use crate::item::ItemStack;
use bevy_ecs::prelude::*;

/// One zero-based effect amplifier and its ordinary drinking duration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PotionEffect {
    pub effect: EffectType,
    pub amplifier: u8,
    /// Instant effects use zero; lasting effects use server ticks.
    pub duration_ticks: u32,
}

/// Frozen contents and presentation of a potion aux value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PotionProfile {
    pub effects: Vec<PotionEffect>,
    /// RGB color used for cloud and splash presentation.
    pub color: u32,
}

/// Invalid configurable contents are rejected before entering the catalog.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PotionProfileError {
    #[error("potion color must fit an RGB value")]
    Color,
    #[error("potion has more than 64 effects")]
    EffectCount,
    #[error("potion effect duration is outside the finite protocol range")]
    Duration,
}

/// Per-world potion definitions. Absence uses the built-in default catalog.
#[derive(Resource, Clone, Debug)]
pub struct PotionCatalog {
    profiles: BTreeMap<i16, Arc<PotionProfile>>,
}
impl PotionCatalog {
    /// Start with no recognized aux values; callers may install their own set.
    pub fn empty() -> Self {
        Self {
            profiles: BTreeMap::new(),
        }
    }
    /// Read an immutable profile without a mutable catalog borrow.
    pub fn get(&self, aux: i16) -> Option<Arc<PotionProfile>> {
        self.profiles.get(&aux).cloned()
    }
    /// Validate and replace one profile for subsequent drinks and launches.
    pub fn insert(
        &mut self,
        aux: i16,
        profile: PotionProfile,
    ) -> Result<Option<Arc<PotionProfile>>, PotionProfileError> {
        if profile.color > 0xffffff {
            return Err(PotionProfileError::Color);
        }
        if profile.effects.len() > 64 {
            return Err(PotionProfileError::EffectCount);
        }
        if profile
            .effects
            .iter()
            .any(|effect| effect.duration_ticks > i32::MAX as u32)
        {
            return Err(PotionProfileError::Duration);
        }
        Ok(self.profiles.insert(aux, Arc::new(profile)))
    }
}

impl Default for PotionCatalog {
    /// Built-in classic aux profiles; applications may replace any profile.
    fn default() -> Self {
        // The default profile table and splash/cloud rules are adapted from
        // Dragonfly v0.10.13; see licenses/dragonfly-MIT.txt.
        use EffectType as E;
        let mut catalog = Self::empty();
        for aux in 0..=42 {
            let values: &[(EffectType, u8, u32)] = match aux {
                5 => &[(E::NIGHT_VISION, 0, 3600)],
                6 => &[(E::NIGHT_VISION, 0, 9600)],
                7 => &[(E::INVISIBILITY, 0, 3600)],
                8 => &[(E::INVISIBILITY, 0, 9600)],
                9 => &[(E::JUMP_BOOST, 0, 3600)],
                10 => &[(E::JUMP_BOOST, 0, 9600)],
                11 => &[(E::JUMP_BOOST, 1, 1800)],
                12 => &[(E::FIRE_RESISTANCE, 0, 3600)],
                13 => &[(E::FIRE_RESISTANCE, 0, 9600)],
                14 => &[(E::SPEED, 0, 3600)],
                15 => &[(E::SPEED, 0, 9600)],
                16 => &[(E::SPEED, 1, 1800)],
                17 => &[(E::SLOWNESS, 0, 1800)],
                18 => &[(E::SLOWNESS, 0, 4800)],
                19 => &[(E::WATER_BREATHING, 0, 3600)],
                20 => &[(E::WATER_BREATHING, 0, 9600)],
                21 => &[(E::INSTANT_HEALTH, 0, 0)],
                22 => &[(E::INSTANT_HEALTH, 1, 0)],
                23 => &[(E::INSTANT_DAMAGE, 0, 0)],
                24 => &[(E::INSTANT_DAMAGE, 1, 0)],
                25 => &[(E::POISON, 0, 900)],
                26 => &[(E::POISON, 0, 2400)],
                27 => &[(E::POISON, 1, 450)],
                28 => &[(E::REGENERATION, 0, 900)],
                29 => &[(E::REGENERATION, 0, 2400)],
                30 => &[(E::REGENERATION, 1, 450)],
                31 => &[(E::STRENGTH, 0, 3600)],
                32 => &[(E::STRENGTH, 0, 9600)],
                33 => &[(E::STRENGTH, 1, 1800)],
                34 => &[(E::WEAKNESS, 0, 1800)],
                35 => &[(E::WEAKNESS, 0, 4800)],
                36 => &[(E::WITHER, 0, 800)],
                37 => &[(E::RESISTANCE, 2, 400), (E::SLOWNESS, 3, 400)],
                38 => &[(E::RESISTANCE, 2, 800), (E::SLOWNESS, 3, 800)],
                39 => &[(E::RESISTANCE, 4, 400), (E::SLOWNESS, 5, 400)],
                40 => &[(E::SLOW_FALLING, 0, 1800)],
                41 => &[(E::SLOW_FALLING, 0, 4800)],
                42 => &[(E::SLOWNESS, 3, 400)],
                _ => &[],
            };
            let effects = values
                .iter()
                .map(|&(effect, amplifier, duration_ticks)| PotionEffect {
                    effect,
                    amplifier,
                    duration_ticks,
                })
                .collect::<Vec<_>>();
            let color = resulting_color(&effects);
            catalog
                .insert(aux, PotionProfile { effects, color })
                .expect("valid default potion profile");
        }
        catalog
    }
}

/// Built-in profiles are shared by worlds that install no custom catalog.
static DEFAULTS: LazyLock<PotionCatalog> = LazyLock::new(PotionCatalog::default);

/// Contents resolved from the actual item's aux value, captured at launch.
#[derive(Component, Clone, Debug)]
pub struct ProjectilePotion {
    pub aux: i16,
    pub profile: Arc<PotionProfile>,
}
impl ProjectilePotion {
    /// Resolve one recognized potion bottle without trusting packet item data.
    pub fn from_stack(world: &World, stack: &ItemStack) -> Option<Self> {
        if !matches!(
            stack.item_id(),
            "minecraft:potion" | "minecraft:splash_potion" | "minecraft:lingering_potion"
        ) {
            return None;
        }
        let catalog = world.get_resource::<PotionCatalog>().unwrap_or(&DEFAULTS);
        Some(Self {
            aux: stack.damage,
            profile: catalog.get(stack.damage)?,
        })
    }
    /// Resolve an authored potion override, independently of a held bottle.
    pub(crate) fn from_aux(world: &World, aux: i16) -> Option<Self> {
        let catalog = world.get_resource::<PotionCatalog>().unwrap_or(&DEFAULTS);
        Some(Self {
            aux,
            profile: catalog.get(aux)?,
        })
    }
}

/// Whether this effect applies once instead of having a lasting duration.
pub fn is_instant(effect: EffectType) -> bool {
    matches!(
        effect,
        EffectType::INSTANT_HEALTH | EffectType::INSTANT_DAMAGE | EffectType::SATURATION
    )
}

/// Blend effect colors with their amplifier weights, keeping water blue.
fn resulting_color(effects: &[PotionEffect]) -> u32 {
    let mut channels = [0u32; 3];
    let mut weight = 0;
    for effect in effects {
        let color = match effect.effect {
            EffectType::SPEED => 0x33ebff,
            EffectType::SLOWNESS => 0x8bafe0,
            EffectType::STRENGTH => 0xffc700,
            EffectType::INSTANT_HEALTH => 0xf82423,
            EffectType::INSTANT_DAMAGE => 0xa9656a,
            EffectType::JUMP_BOOST => 0xfdff84,
            EffectType::REGENERATION => 0xcd5cab,
            EffectType::RESISTANCE => 0x9146f0,
            EffectType::FIRE_RESISTANCE => 0xff9900,
            EffectType::WATER_BREATHING => 0x98dac0,
            EffectType::INVISIBILITY => 0xf6f6f6,
            EffectType::NIGHT_VISION => 0xc2ff66,
            EffectType::WEAKNESS => 0x484d48,
            EffectType::POISON => 0x87a363,
            EffectType::WITHER => 0x736156,
            EffectType::SLOW_FALLING => 0xf3cfb9,
            _ => 0x385dc6,
        };
        let w = u32::from(effect.amplifier) + 1;
        weight += w;
        for (i, channel) in channels.iter_mut().enumerate() {
            *channel += ((color >> (16 - i * 8)) & 255) * w;
        }
    }
    if weight == 0 {
        return 0x385dc6;
    }
    ((channels[0] / weight) << 16) | ((channels[1] / weight) << 8) | (channels[2] / weight)
}

#[cfg(test)]
mod tests;
