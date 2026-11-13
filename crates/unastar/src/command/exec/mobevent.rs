//! `/mobevent <event> [value]`: turns the world's mob events on or off.
//!
//! Measured on BDS 1.26.51: every event starts on; a query answers
//! `commands.mobevent.eventIsTrue|eventIsFalse [%entity.<mob>.name, id]`
//! (or `eventsEnabledIsTrue|IsFalse` for `events_enabled`), a change
//! `commands.mobevent.eventSetToTrue|eventSetToFalse` (`…ButEventsDisabled`
//! when events are off), always as a success.
//!
//! unastar runs none of these events (no pillager patrols, wandering trader
//! visits or dragon fight): the switches are kept but change nothing, and
//! they are not saved with the world.

use crate::command::prelude::*;
use bevy_ecs::prelude::Resource;

arg_enum! {
    /// Captured `MobEvent`.
    pub enum MobEventName in "MobEvent" {
        PillagerPatrols = "minecraft:pillager_patrols_event",
        WanderingTrader = "minecraft:wandering_trader_event",
        EnderDragon = "minecraft:ender_dragon_event",
        EventsEnabled = "events_enabled",
    }
}

overloads! {
    pub enum MobEventArgs for "mobevent" {
        0 => MobEvent { event: MobEventName, value: Option<bool> },
    }
}

/// The world's mob event switches.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobEvents {
    pub enabled: bool,
    pub pillager_patrols: bool,
    pub wandering_trader: bool,
    pub ender_dragon: bool,
}

impl Default for MobEvents {
    fn default() -> Self {
        Self {
            enabled: true,
            pillager_patrols: true,
            wandering_trader: true,
            ender_dragon: true,
        }
    }
}

impl MobEventName {
    /// The mob named in the answer, as a translation.
    fn mob(self) -> &'static str {
        match self {
            Self::PillagerPatrols => "%entity.pillager.name",
            Self::WanderingTrader => "%entity.wandering_trader.name",
            Self::EnderDragon => "%entity.ender_dragon.name",
            Self::EventsEnabled => "",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::PillagerPatrols => "minecraft:pillager_patrols_event",
            Self::WanderingTrader => "minecraft:wandering_trader_event",
            Self::EnderDragon => "minecraft:ender_dragon_event",
            Self::EventsEnabled => "events_enabled",
        }
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: MobEventArgs) -> CommandResult {
    let MobEventArgs::MobEvent { event, value } = args;
    let mut events = *ctx.world.get_resource_or_insert_with(MobEvents::default);
    if event == MobEventName::EventsEnabled {
        let key = match value {
            None if events.enabled => "commands.mobevent.eventsEnabledIsTrue",
            None => "commands.mobevent.eventsEnabledIsFalse",
            Some(true) => "commands.mobevent.eventsEnabledSetToTrue",
            Some(false) => "commands.mobevent.eventsEnabledSetToFalse",
        };
        if let Some(value) = value {
            events.enabled = value;
            ctx.world.insert_resource(events);
        }
        return success!(key);
    }
    let slot = match event {
        MobEventName::PillagerPatrols => &mut events.pillager_patrols,
        MobEventName::WanderingTrader => &mut events.wandering_trader,
        MobEventName::EnderDragon => &mut events.ender_dragon,
        MobEventName::EventsEnabled => unreachable!("handled above"),
    };
    let key = match value {
        None if !*slot => "commands.mobevent.eventIsFalse",
        None if events.enabled => "commands.mobevent.eventIsTrue",
        None => "commands.mobevent.eventIsTrueButEventsDisabled",
        Some(false) => "commands.mobevent.eventSetToFalse",
        Some(true) if events.enabled => "commands.mobevent.eventSetToTrue",
        Some(true) => "commands.mobevent.eventSetToTrueButEventsDisabled",
    };
    if let Some(value) = value {
        *slot = value;
        ctx.world.insert_resource(events);
    }
    success!(key, event.mob(), event.id())
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::Replay;

    #[test]
    fn mobevent_matches_the_bds_capture() {
        Replay::new().assert_segment("mobevent", &[]);
    }
}
