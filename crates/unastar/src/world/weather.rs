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

/// How far rain and lightning have faded in or out.
///
/// Measured on BDS 1.26.51: `/weather query` reports these levels, not the
/// weather last set: 12 ticks after `/weather clear` ends a thunderstorm it
/// still answers thunder, and 12 ticks after `/toggledownfall` stops rain it
/// still answers rain, while a thunderstorm started and stopped 12 ticks
/// apart has faded out 12 ticks later. Both levels move a fixed step per
/// tick towards 1 while it rains (thunders) and 0 otherwise. [INFERENCE] The
/// step is 0.01, as in Java Edition; the captures only show that fading in
/// and out take equally long.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct WeatherLevels {
    rain: f32,
    lightning: f32,
    since_tick: u64,
    rain_target: f32,
    lightning_target: f32,
}

impl WeatherLevels {
    const STEP_PER_TICK: f32 = 0.01;

    /// `(rain, lightning)` at `tick`.
    pub fn at(&self, tick: u64) -> (f32, f32) {
        let moved = tick.saturating_sub(self.since_tick) as f32 * Self::STEP_PER_TICK;
        let toward = |from: f32, to: f32| {
            if from < to {
                (from + moved).min(to)
            } else {
                (from - moved).max(to)
            }
        };
        (
            toward(self.rain, self.rain_target),
            toward(self.lightning, self.lightning_target),
        )
    }

    /// Starts fading towards `state` at `tick`.
    pub fn retarget(&mut self, tick: u64, state: WeatherState) {
        let (rain, lightning) = self.at(tick);
        *self = Self {
            rain,
            lightning,
            since_tick: tick,
            rain_target: if state == WeatherState::Clear {
                0.0
            } else {
                1.0
            },
            lightning_target: if state == WeatherState::Thunder {
                1.0
            } else {
                0.0
            },
        };
    }

    /// What `/weather query` answers at `tick`.
    pub fn reported(&self, tick: u64) -> WeatherState {
        match self.at(tick) {
            (_, lightning) if lightning > 0.0 => WeatherState::Thunder,
            (rain, _) if rain > 0.0 => WeatherState::Rain,
            _ => WeatherState::Clear,
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
