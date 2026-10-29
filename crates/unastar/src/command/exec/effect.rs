//! `/effect <player> clear [effect]`,
//! `/effect <player> <effect> [seconds] [amplifier] [hideParticles]` and
//! `/effect <player> <effect> infinite [amplifier] [hideParticles]`.
//!
//! Measured on BDS 1.26.51: giving an effect sends `MobEffect` (`Add`, or
//! `Update` when the target already has it; duration `-1` when infinite)
//! and answers `commands.effect.success` (`%potion.<name>`, amplifier,
//! target, seconds) or `.success.infinite`; `clear` sends `MobEffect`
//! `Remove` per effect and answers `commands.effect.success.removed.all`;
//! clearing an effect the target lacks fails with
//! `commands.effect.failure.notActive`.
//!
//! Effects live in the target's [`Effects`] component, which the effect
//! systems tick. Instant effects act once when given (measured: `MobEffect`
//! `Add` for 30 ticks, one second by default): instant health heals
//! `4 << amplifier`, instant damage deals `6 << amplifier` magic damage,
//! reversed for undead mobs.

use crate::command::prelude::*;
use crate::entity::components::{
    ActiveEffect, ActorViewers, EffectType, Effects, Health, Living, Player, PlayerSession,
    RuntimeId,
};
use crate::entity::definition::families::ActorFamilies;
use crate::entity::mob::MobRegistry;
use crate::entity::mob::combat::{Hit, hurt_mob};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::player_life::{hurt_player, send_to_player_and_viewers};
use jolyne::valentine::types::{ActorRuntimeId, EnumsMobEffectPacketPayloadEvent, PlayerInputTick};
use jolyne::valentine::{McpePacket, MobEffectPacket};
use unastar_data::actor_components::body::ActorDamageCause;

/// BDS's limits for `seconds` and `amplifier`.
const MAX_SECONDS: i32 = 1_000_000;
const MAX_AMPLIFIER: i32 = 255;
/// Duration when `seconds` is omitted.
const DEFAULT_SECONDS: i32 = 30;
/// Measured: an instant effect defaults to one second and is sent for 30 ticks.
const INSTANT_DEFAULT_SECONDS: i32 = 1;
const INSTANT_TICKS: i32 = 30;

arg_enum! {
    /// The captured `Effect` enum.
    pub enum EffectName in "Effect" {
        Wither = "wither",
        Speed = "speed",
        Slowness = "slowness",
        Haste = "haste",
        MiningFatigue = "mining_fatigue",
        Strength = "strength",
        InstantHealth = "instant_health",
        InstantDamage = "instant_damage",
        JumpBoost = "jump_boost",
        Nausea = "nausea",
        Regeneration = "regeneration",
        Resistance = "resistance",
        FireResistance = "fire_resistance",
        WaterBreathing = "water_breathing",
        Invisibility = "invisibility",
        Blindness = "blindness",
        NightVision = "night_vision",
        Hunger = "hunger",
        Weakness = "weakness",
        Poison = "poison",
        HealthBoost = "health_boost",
        Absorption = "absorption",
        Saturation = "saturation",
        Levitation = "levitation",
        FatalPoison = "fatal_poison",
        ConduitPower = "conduit_power",
        SlowFalling = "slow_falling",
        BadOmen = "bad_omen",
        VillageHero = "village_hero",
        Darkness = "darkness",
        TrialOmen = "trial_omen",
        WindCharged = "wind_charged",
        Weaving = "weaving",
        Oozing = "oozing",
        Infested = "infested",
        RaidOmen = "raid_omen",
        BreathOfTheNautilus = "breath_of_the_nautilus",
    }
}

overloads! {
    pub enum EffectArgs for "effect" {
        0 => Clear { player: Target, mode: EnumValue = "Mode", effect: Option<EffectName> },
        1 => Give {
            player: Target,
            effect: EffectName,
            seconds: Option<i32>,
            amplifier: Option<i32>,
            hide_particles: Option<bool> = "hideParticles",
        },
        2 => GiveInfinite {
            player: Target,
            effect: EffectName,
            mode: EnumValue = "Mode",
            amplifier: Option<i32>,
            hide_particles: Option<bool> = "hideParticles",
        },
    }
}

impl EffectName {
    /// The Bedrock mob effect id and the translation key BDS names it with.
    ///
    /// Ids 1–23 are unastar's [`EffectType`] constants and speed (1) is
    /// measured; the rest follow the captured enum's order, which lists the
    /// effects by id (wither, 20, first). [INFERENCE] Keys: `potion.*`
    /// where the language has one (measured for speed), else the
    /// `effect.*` key of the effect's name.
    fn info(self) -> (EffectType, &'static str) {
        let (id, key) = match self {
            Self::Speed => (1, "potion.moveSpeed"),
            Self::Slowness => (2, "potion.moveSlowdown"),
            Self::Haste => (3, "potion.digSpeed"),
            Self::MiningFatigue => (4, "potion.digSlowDown"),
            Self::Strength => (5, "potion.damageBoost"),
            Self::InstantHealth => (6, "potion.heal"),
            Self::InstantDamage => (7, "potion.harm"),
            Self::JumpBoost => (8, "potion.jump"),
            Self::Nausea => (9, "potion.confusion"),
            Self::Regeneration => (10, "potion.regeneration"),
            Self::Resistance => (11, "potion.resistance"),
            Self::FireResistance => (12, "potion.fireResistance"),
            Self::WaterBreathing => (13, "potion.waterBreathing"),
            Self::Invisibility => (14, "potion.invisibility"),
            Self::Blindness => (15, "potion.blindness"),
            Self::NightVision => (16, "potion.nightVision"),
            Self::Hunger => (17, "potion.hunger"),
            Self::Weakness => (18, "potion.weakness"),
            Self::Poison => (19, "potion.poison"),
            Self::Wither => (20, "potion.wither"),
            Self::HealthBoost => (21, "potion.healthBoost"),
            Self::Absorption => (22, "potion.absorption"),
            Self::Saturation => (23, "potion.saturation"),
            Self::Levitation => (24, "potion.levitation"),
            Self::FatalPoison => (25, "potion.poison"),
            Self::ConduitPower => (26, "potion.conduitPower"),
            Self::SlowFalling => (27, "potion.slowFalling"),
            Self::BadOmen => (28, "effect.badOmen"),
            Self::VillageHero => (29, "effect.villageHero"),
            Self::Darkness => (30, "effect.darkness"),
            Self::TrialOmen => (31, "effect.trial_omen"),
            Self::WindCharged => (32, "potion.windCharged"),
            Self::Weaving => (33, "potion.weaving"),
            Self::Oozing => (34, "potion.oozing"),
            Self::Infested => (35, "potion.infested"),
            Self::RaidOmen => (36, "effect.raid_omen"),
            Self::BreathOfTheNautilus => (37, "effect.customNautilus"),
        };
        (EffectType(id), key)
    }

    /// The `%key` parameter BDS puts in its messages.
    fn param(self) -> String {
        format!("%{}", self.info().1)
    }

    fn is_instant(self) -> bool {
        matches!(self, Self::InstantHealth | Self::InstantDamage)
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: EffectArgs) -> CommandResult {
    match args {
        EffectArgs::Clear { player, effect, .. } => clear(ctx, &player, effect),
        EffectArgs::Give {
            player,
            effect,
            seconds,
            amplifier,
            hide_particles,
        } => {
            let default = if effect.is_instant() {
                INSTANT_DEFAULT_SECONDS
            } else {
                DEFAULT_SECONDS
            };
            let seconds = seconds.unwrap_or(default);
            in_range(seconds, MAX_SECONDS)?;
            let amplifier = amplifier.unwrap_or(0);
            in_range(amplifier, MAX_AMPLIFIER)?;
            if seconds == 0 {
                // [INFERENCE] As the older command system did, zero seconds
                // takes the effect away.
                return clear(ctx, &player, Some(effect));
            }
            give(
                ctx,
                &player,
                effect,
                Some(seconds),
                amplifier,
                hide_particles.unwrap_or(false),
            )
        }
        EffectArgs::GiveInfinite {
            player,
            effect,
            amplifier,
            hide_particles,
            ..
        } => {
            let amplifier = amplifier.unwrap_or(0);
            in_range(amplifier, MAX_AMPLIFIER)?;
            give(
                ctx,
                &player,
                effect,
                None,
                amplifier,
                hide_particles.unwrap_or(false),
            )
        }
    }
}

fn in_range(value: i32, max: i32) -> Result<(), CommandOutput> {
    if value < 0 {
        return failure!("commands.generic.num.tooSmall", value, 0);
    }
    if value > max {
        return failure!("commands.generic.num.tooBig", value, max);
    }
    Ok(())
}

/// Entities that can have effects: players and mobs.
fn targets(ctx: &mut CommandContext<'_>, player: &Target) -> Result<Vec<Entity>, CommandOutput> {
    let targets = ctx.entities(Some(player))?;
    if let Some(&other) = targets
        .iter()
        .find(|&&e| ctx.world.get::<Player>(e).is_none() && ctx.world.get::<Living>(e).is_none())
    {
        return failure!("commands.effect.failure.notAMob", ctx.name(other));
    }
    Ok(targets)
}

/// `seconds` is `None` for an infinite effect.
fn give(
    ctx: &mut CommandContext<'_>,
    player: &Target,
    effect: EffectName,
    seconds: Option<i32>,
    amplifier: i32,
    hide_particles: bool,
) -> CommandResult {
    let targets = targets(ctx, player)?;
    let (effect_type, _) = effect.info();
    let active = ActiveEffect {
        level: amplifier as u8,
        duration_ticks: seconds.map_or(0, |s| s as u32 * 20),
        ambient: false,
        show_particles: !hide_particles,
        infinite: seconds.is_none(),
    };
    let mut output = CommandOutput::default();
    for target in targets {
        let name = ctx.name(target);
        if effect.is_instant() {
            // [INFERENCE] An infinite instant effect acts and shows like any
            // other instant effect.
            let shown = ActiveEffect {
                duration_ticks: INSTANT_TICKS as u32,
                infinite: false,
                ..active.clone()
            };
            send_effect(
                ctx.world,
                target,
                EnumsMobEffectPacketPayloadEvent::Add,
                effect_type,
                &shown,
            );
            apply_instant(ctx.world, target, effect, amplifier);
        } else {
            give_lasting(ctx.world, target, effect_type, &active);
        }
        match seconds {
            Some(seconds) => output.add_success(
                "commands.effect.success",
                vec![
                    effect.param(),
                    amplifier.to_string(),
                    name,
                    seconds.to_string(),
                ],
            ),
            None => output.add_success(
                "commands.effect.success.infinite",
                vec![effect.param(), amplifier.to_string(), name],
            ),
        }
    }
    Ok(output)
}

/// Adds a lasting effect under the effect add rule and shows it.
fn give_lasting(world: &mut World, target: Entity, effect_type: EffectType, active: &ActiveEffect) {
    // [INFERENCE] A weaker effect leaves the active one in place (the effect
    // add rule) and is still reported as given.
    let applied = {
        let mut entity = world.entity_mut(target);
        let mut effects = entity.entry::<Effects>().or_default();
        let existed = effects.get().has(effect_type);
        effects
            .get_mut()
            .apply(effect_type, active.clone())
            .then_some(existed)
    };
    if let Some(existed) = applied {
        let event = if existed {
            EnumsMobEffectPacketPayloadEvent::Update
        } else {
            EnumsMobEffectPacketPayloadEvent::Add
        };
        send_effect(world, target, event, effect_type, active);
    }
}

/// Instant health heals `4 << amplifier`; instant damage deals
/// `6 << amplifier` magic damage. Undead mobs swap the two.
fn apply_instant(world: &mut World, target: Entity, effect: EffectName, amplifier: i32) {
    let scale = 2f32.powi(amplifier.min(30));
    let heals = (effect == EffectName::InstantHealth) != is_undead(world, target);
    if heals {
        if let Some(mut health) = world.get_mut::<Health>(target)
            && !health.is_dead()
        {
            health.heal(4.0 * scale);
        }
        return;
    }
    let hit = Hit {
        damage: 6.0 * scale,
        cause: ActorDamageCause::Magic,
        attacker: None,
        by_player: false,
        knockback_from: None,
        knockback_power: KnockbackPower::DEFAULT,
    };
    if world.get::<Player>(target).is_some() {
        hurt_player(world, target, &hit);
    } else {
        hurt_mob(world, target, hit);
    }
}

/// Whether a mob is in the `undead` family.
fn is_undead(world: &World, entity: Entity) -> bool {
    let (Some(families), Some(registry)) = (
        world.get::<ActorFamilies>(entity),
        world.get_resource::<MobRegistry>(),
    ) else {
        return false;
    };
    registry
        .families()
        .resolve("undead")
        .is_some_and(|id| families.contains(&id).unwrap_or(false))
}

/// Clears one effect, or every effect when `effect` is `None`.
fn clear(
    ctx: &mut CommandContext<'_>,
    player: &Target,
    effect: Option<EffectName>,
) -> CommandResult {
    let targets = targets(ctx, player)?;
    let mut output = CommandOutput::default();
    for target in targets {
        let name = ctx.name(target);
        let removed: Vec<(EffectType, ActiveEffect)> = match ctx.world.get_mut::<Effects>(target) {
            Some(mut effects) => match effect {
                Some(effect) => {
                    let effect_type = effect.info().0;
                    effects
                        .active
                        .remove(&effect_type)
                        .map(|active| (effect_type, active))
                        .into_iter()
                        .collect()
                }
                None => effects.active.drain().collect(),
            },
            None => Vec::new(),
        };
        for (effect_type, active) in &removed {
            send_effect(
                ctx.world,
                target,
                EnumsMobEffectPacketPayloadEvent::Remove,
                *effect_type,
                active,
            );
        }
        match (effect, removed.is_empty()) {
            (Some(effect), true) => output.add_failure(
                "commands.effect.failure.notActive",
                vec![effect.param(), name],
            ),
            (None, true) => {
                output.add_failure("commands.effect.failure.notActive.all", vec![name]);
            }
            (Some(effect), false) => {
                output.add_success(
                    "commands.effect.success.removed",
                    vec![effect.param(), name],
                );
            }
            (None, false) => {
                output.add_success("commands.effect.success.removed.all", vec![name]);
            }
        }
    }
    if output.is_success() {
        Ok(output)
    } else {
        Err(output)
    }
}

/// `MobEffect` for `target`, to itself (players) and the players that see it.
fn send_effect(
    world: &World,
    target: Entity,
    event: EnumsMobEffectPacketPayloadEvent,
    effect: EffectType,
    active: &ActiveEffect,
) {
    let player = world.get::<Player>(target).is_some();
    let runtime_id = if player {
        world
            .get::<crate::entity::components::RuntimeEntityId>(target)
            .map(|id| id.0)
    } else {
        world.get::<RuntimeId>(target).map(|id| id.0)
    };
    let Some(runtime_id) = runtime_id else {
        return;
    };
    let packet = McpePacket::from(MobEffectPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        event_id: event,
        effect_id: i32::from(effect.0),
        effect_amplifier: i32::from(active.level),
        show_particles: active.show_particles,
        effect_duration_ticks: if active.infinite {
            -1
        } else {
            active.duration_ticks as i32
        },
        // The player's input tick only for effects that change movement, as
        // BDS 1.26.51 sends them; 0 for every other effect and for mobs.
        tick: if effect.affects_movement() {
            crate::entity::components::input_tick_of(world, target)
        } else {
            PlayerInputTick { inputtick: 0 }
        },
        ambient: active.ambient,
    });
    if player {
        send_to_player_and_viewers(world, target, &packet);
    } else if let Some(viewers) = world.get::<ActorViewers>(target) {
        for viewer in &viewers.viewers {
            if let Some(session) = world.get::<PlayerSession>(*viewer) {
                let _ = session.send(packet.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;
    use jolyne::valentine::McpePacketData;

    #[test]
    fn effect_enum_covers_the_captured_values() {
        crate::command::args::assert_covers::<EffectName>();
    }

    #[test]
    fn every_effect_has_a_distinct_id() {
        let mut ids: Vec<u8> = <EffectName as ArgEnum>::SPELLINGS
            .iter()
            .map(|s| EffectName::from_value(s).unwrap().info().0.0)
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, (1..=37).collect::<Vec<u8>>());
    }

    fn effect_packets(h: &mut Harness, player: Entity) -> Vec<MobEffectPacket> {
        h.sent(player)
            .into_iter()
            .filter_map(|p| match p {
                McpePacketData::MobEffectPacket(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn give_update_clear_match_the_bds_capture() {
        let mut h = Harness::new();
        let alice = h.player("Alice", true);

        let output = h.run(alice, "/effect @s speed 10 1");
        assert_eq!(
            output,
            CommandOutput::success(
                "commands.effect.success",
                vec![
                    "%potion.moveSpeed".into(),
                    "1".into(),
                    "Alice".into(),
                    "10".into()
                ]
            )
        );
        let packets = effect_packets(&mut h, alice);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].event_id, EnumsMobEffectPacketPayloadEvent::Add);
        assert_eq!(
            (
                packets[0].effect_id,
                packets[0].effect_amplifier,
                packets[0].effect_duration_ticks,
                packets[0].show_particles,
                packets[0].ambient,
            ),
            (1, 1, 200, true, false)
        );
        let active = h
            .world()
            .get::<Effects>(alice)
            .unwrap()
            .get(EffectType::SPEED)
            .cloned();
        assert_eq!(active.map(|a| (a.level, a.duration_ticks)), Some((1, 200)));

        let output = h.run(alice, "/effect @s speed infinite 2 true");
        assert_eq!(
            output,
            CommandOutput::success(
                "commands.effect.success.infinite",
                vec!["%potion.moveSpeed".into(), "2".into(), "Alice".into()]
            )
        );
        let packets = effect_packets(&mut h, alice);
        assert_eq!(
            packets[0].event_id,
            EnumsMobEffectPacketPayloadEvent::Update
        );
        assert_eq!(
            (
                packets[0].effect_amplifier,
                packets[0].effect_duration_ticks,
                packets[0].show_particles
            ),
            (2, -1, false)
        );
        assert!(
            h.world()
                .get::<Effects>(alice)
                .unwrap()
                .get(EffectType::SPEED)
                .unwrap()
                .infinite
        );

        let output = h.run(alice, "/effect @s clear");
        assert_eq!(
            output,
            CommandOutput::success("commands.effect.success.removed.all", vec!["Alice".into()])
        );
        let packets = effect_packets(&mut h, alice);
        assert_eq!(
            packets[0].event_id,
            EnumsMobEffectPacketPayloadEvent::Remove
        );
        assert_eq!(
            (
                packets[0].effect_id,
                packets[0].effect_amplifier,
                packets[0].effect_duration_ticks
            ),
            (1, 2, -1)
        );
        assert!(h.world().get::<Effects>(alice).unwrap().active.is_empty());

        let output = h.run(alice, "/effect @s clear speed");
        assert_eq!(
            output,
            CommandOutput::failure(
                "commands.effect.failure.notActive",
                vec!["%potion.moveSpeed".into(), "Alice".into()]
            )
        );
        assert!(effect_packets(&mut h, alice).is_empty());
    }

    #[test]
    fn defaults_ranges_single_clear_and_viewers() {
        let mut h = Harness::new();
        let alice = h.player("Alice", true);
        let bob = h.player("Bob", false);

        let output = h.run(alice, "/effect Bob night_vision");
        assert_eq!(
            output.messages[0].params,
            ["%potion.nightVision", "0", "Bob", "30"]
        );
        let active = h
            .world()
            .get::<Effects>(bob)
            .unwrap()
            .get(EffectType::NIGHT_VISION)
            .cloned();
        assert_eq!(active.map(|a| a.duration_ticks), Some(600));
        assert_eq!(effect_packets(&mut h, bob).len(), 1);

        assert_eq!(
            h.run(alice, "/effect Bob speed 1000001"),
            CommandOutput::failure(
                "commands.generic.num.tooBig",
                vec!["1000001".into(), "1000000".into()]
            )
        );
        assert_eq!(
            h.run(alice, "/effect Bob speed 5 256"),
            CommandOutput::failure(
                "commands.generic.num.tooBig",
                vec!["256".into(), "255".into()]
            )
        );

        let output = h.run(alice, "/effect Bob clear night_vision");
        assert_eq!(
            output,
            CommandOutput::success(
                "commands.effect.success.removed",
                vec!["%potion.nightVision".into(), "Bob".into()]
            )
        );
        assert_eq!(
            h.run(alice, "/effect Bob clear"),
            CommandOutput::failure("commands.effect.failure.notActive.all", vec!["Bob".into()])
        );
    }

    #[test]
    fn hidden_particles_match_the_bds_capture() {
        let mut h = Harness::new();
        let alice = h.player("Alice", true);
        let output = h.run(alice, "/effect @s regeneration 5 0 true");
        assert_eq!(
            output,
            CommandOutput::success(
                "commands.effect.success",
                vec![
                    "%potion.regeneration".into(),
                    "0".into(),
                    "Alice".into(),
                    "5".into()
                ]
            )
        );
        let packets = effect_packets(&mut h, alice);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].event_id, EnumsMobEffectPacketPayloadEvent::Add);
        assert_eq!(
            (
                packets[0].effect_id,
                packets[0].effect_amplifier,
                packets[0].effect_duration_ticks,
                packets[0].show_particles,
            ),
            (10, 0, 100, false)
        );
    }

    fn health(h: &mut Harness, entity: Entity) -> f32 {
        h.world().get::<Health>(entity).unwrap().current
    }

    #[test]
    fn instant_effects_heal_and_hurt_players_like_bds() {
        let mut h = Harness::new();
        let alice = h.player("Alice", true);
        h.world()
            .entity_mut(alice)
            .insert(Health::with_current(10.0, 20.0));

        let output = h.run(alice, "/effect @s instant_health");
        assert_eq!(
            output,
            CommandOutput::success(
                "commands.effect.success",
                vec![
                    "%potion.heal".into(),
                    "0".into(),
                    "Alice".into(),
                    "1".into()
                ]
            )
        );
        let packets = effect_packets(&mut h, alice);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].event_id, EnumsMobEffectPacketPayloadEvent::Add);
        assert_eq!(
            (
                packets[0].effect_id,
                packets[0].effect_amplifier,
                packets[0].effect_duration_ticks,
                packets[0].show_particles,
            ),
            (6, 0, 30, true)
        );
        assert_eq!(health(&mut h, alice), 14.0);
        assert!(
            h.world()
                .get::<Effects>(alice)
                .is_none_or(|e| !e.has(EffectType::INSTANT_HEALTH))
        );

        h.run(alice, "/effect @s instant_health 1 1");
        assert_eq!(health(&mut h, alice), 20.0);
        assert_eq!(effect_packets(&mut h, alice)[0].effect_amplifier, 1);

        let output = h.run(alice, "/effect @s instant_damage");
        assert_eq!(
            output.messages[0].params,
            ["%potion.harm", "0", "Alice", "1"]
        );
        assert_eq!(effect_packets(&mut h, alice)[0].effect_id, 7);
        assert_eq!(health(&mut h, alice), 14.0);
    }

    #[test]
    fn instant_effects_are_reversed_for_undead_mobs() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        h.run(op, "/summon zombie ~ ~ ~3");
        h.run(op, "/summon cow ~ ~ ~3");
        let mob = |h: &mut Harness, kind: &str| {
            crate::command::context::candidates(h.world())
                .into_iter()
                .find(|c| c.type_id == kind)
                .map(|c| c.entity)
                .unwrap()
        };
        let zombie = mob(&mut h, "minecraft:zombie");
        let cow = mob(&mut h, "minecraft:cow");
        let (zombie_before, cow_before) = (health(&mut h, zombie), health(&mut h, cow));

        assert!(
            h.run(op, "/effect @e[type=zombie] instant_health")
                .is_success()
        );
        assert_eq!(health(&mut h, zombie), zombie_before - 6.0);
        assert!(
            h.run(op, "/effect @e[type=cow] instant_damage")
                .is_success()
        );
        assert_eq!(health(&mut h, cow), cow_before - 6.0);
    }

    #[test]
    fn infinite_effects_do_not_expire() {
        let mut effect = ActiveEffect {
            infinite: true,
            ..ActiveEffect::new(0, 0)
        };
        for _ in 0..5 {
            effect.tick();
        }
        assert!(!effect.is_expired());
        let mut effects = Effects::default();
        assert!(effects.apply(EffectType::SPEED, effect));
        assert!(!effects.apply(EffectType::SPEED, ActiveEffect::new(0, 100)));
        assert!(effects.apply(EffectType::SPEED, ActiveEffect::new(1, 100)));
    }
}
