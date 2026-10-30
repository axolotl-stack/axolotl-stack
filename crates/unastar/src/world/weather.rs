//! The world's weather: whether it rains or thunders, as `/weather` sets it.
//!
//! Weather is not simulated: there is no natural weather cycle (the
//! `doWeatherCycle` rule changes nothing), a set duration is recorded but
//! never counts down, and rain does not darken the sky, put out fires or
//! fill cauldrons. Clients render it from the `LevelEvent`s BDS sends.

use bevy_ecs::prelude::*;
use jolyne::valentine::LevelEventPacket;
use jolyne::valentine::types::Vec3;

/// `LevelEvent` ids BDS sends when the weather changes.
pub const LEVEL_EVENT_START_RAIN: i32 = 3001;
pub const LEVEL_EVENT_START_THUNDER: i32 = 3002;
pub const LEVEL_EVENT_STOP_RAIN: i32 = 3003;
pub const LEVEL_EVENT_STOP_THUNDER: i32 = 3004;

/// Rain/lightning level BDS sends with a start event (full intensity).
const FULL_LEVEL: i32 = 65_535;

/// The three weather states `/weather` sets and queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WeatherState {
    #[default]
    Clear,
    Rain,
    Thunder,
}

/// Current world weather.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Weather {
    pub state: WeatherState,
    /// Duration in ticks given to `/weather`, if any. Recorded only: no
    /// weather cycle runs it down.
    pub duration: Option<i32>,
}

impl Weather {
    pub fn is_raining(self) -> bool {
        self.state != WeatherState::Clear
    }

    /// The events that bring a client to this weather, as BDS sends them on
    /// `/weather`: rain starts with 3001, thunder adds 3002 (both at full
    /// level); clear stops both (3003, 3004, data 0). All at the origin.
    pub fn level_events(self) -> Vec<LevelEventPacket> {
        let event = |event_id, data| LevelEventPacket {
            event_id,
            position: Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            data,
        };
        match self.state {
            WeatherState::Clear => vec![
                event(LEVEL_EVENT_STOP_RAIN, 0),
                event(LEVEL_EVENT_STOP_THUNDER, 0),
            ],
            WeatherState::Rain => vec![event(LEVEL_EVENT_START_RAIN, FULL_LEVEL)],
            WeatherState::Thunder => vec![
                event(LEVEL_EVENT_START_RAIN, FULL_LEVEL),
                event(LEVEL_EVENT_START_THUNDER, FULL_LEVEL),
            ],
        }
    }
}

/// Weather events for a joining player: none in clear weather (the client
/// starts clear), otherwise the events `/weather` sends.
pub fn join_packets(world: &World) -> Vec<LevelEventPacket> {
    match world.get_resource::<Weather>() {
        Some(weather) if weather.is_raining() => weather.level_events(),
        _ => Vec::new(),
    }
}
