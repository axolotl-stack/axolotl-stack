//! `/weather <clear|rain|thunder> [duration]` and `/weather query`, on the
//! world's [`Weather`] resource (no natural weather cycle runs: see
//! `world::weather`).
//!
//! Measured (BDS 1.26.51): `rain` broadcasts `LevelEvent 3001` (data 65535)
//! and answers `commands.weather.rain`; `thunder 100` broadcasts 3001 and
//! 3002 (data 65535) and answers `commands.weather.thunder`; `clear`
//! broadcasts 3003 and 3004 (data 0) and answers `commands.weather.clear`;
//! all events at (0, 0, 0). `query` answers
//! `commands.weather.query [%commands.weather.query.<state>]`.

use crate::command::prelude::*;
use crate::world::weather::{Weather, WeatherState};

arg_enum! {
    /// Captured `WeatherType`.
    pub enum WeatherKind in "WeatherType" {
        Clear = "clear",
        Rain = "rain",
        Thunder = "thunder",
    }
}

overloads! {
    pub enum WeatherArgs for "weather" {
        0 => Set { kind: WeatherKind = "type", duration: Option<i32> },
        1 => Query { query: EnumValue },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: WeatherArgs) -> CommandResult {
    match args {
        WeatherArgs::Query { .. } => {
            let state = ctx
                .world
                .get_resource::<Weather>()
                .copied()
                .unwrap_or_default()
                .state;
            let name = match state {
                WeatherState::Clear => "clear",
                WeatherState::Rain => "rain",
                WeatherState::Thunder => "thunder",
            };
            success!(
                "commands.weather.query",
                format!("%commands.weather.query.{name}")
            )
        }
        WeatherArgs::Set { kind, duration } => {
            let (state, key) = match kind {
                WeatherKind::Clear => (WeatherState::Clear, "commands.weather.clear"),
                WeatherKind::Rain => (WeatherState::Rain, "commands.weather.rain"),
                WeatherKind::Thunder => (WeatherState::Thunder, "commands.weather.thunder"),
            };
            let weather = Weather { state, duration };
            ctx.world.insert_resource(weather);
            for event in weather.level_events() {
                ctx.broadcast(event);
            }
            success!(key)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::args::assert_covers;
    use crate::command::exec::test_support::Harness;
    use jolyne::valentine::McpePacketData;

    #[test]
    fn enum_covers_the_captured_values() {
        assert_covers::<WeatherKind>();
    }

    /// `(event_id, data)` of the LevelEvents sent to `player`; all at the origin.
    fn level_events(h: &mut Harness, player: Entity) -> Vec<(i32, i32)> {
        h.sent(player)
            .into_iter()
            .filter_map(|packet| match packet {
                McpePacketData::LevelEventPacket(event) => {
                    assert_eq!(
                        (event.position.x, event.position.y, event.position.z),
                        (0.0, 0.0, 0.0)
                    );
                    Some((event.event_id, event.data))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn set_and_query_answer_like_bds() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        let other = h.player("Other", false);
        assert_eq!(
            h.run(op, "/weather query"),
            CommandOutput::success(
                "commands.weather.query",
                vec!["%commands.weather.query.clear".into()]
            )
        );

        assert_eq!(
            h.run(op, "/weather rain"),
            CommandOutput::success("commands.weather.rain", vec![])
        );
        assert_eq!(level_events(&mut h, other), [(3001, 65535)]);
        assert_eq!(
            *h.world().resource::<Weather>(),
            Weather {
                state: WeatherState::Rain,
                duration: None
            }
        );

        assert_eq!(
            h.run(op, "/weather thunder 100"),
            CommandOutput::success("commands.weather.thunder", vec![])
        );
        assert_eq!(
            level_events(&mut h, op),
            [(3001, 65535), (3001, 65535), (3002, 65535)]
        );
        assert_eq!(h.world().resource::<Weather>().duration, Some(100));
        assert_eq!(
            h.run(op, "/weather query"),
            CommandOutput::success(
                "commands.weather.query",
                vec!["%commands.weather.query.thunder".into()]
            )
        );

        assert_eq!(
            h.run(op, "/weather clear"),
            CommandOutput::success("commands.weather.clear", vec![])
        );
        assert_eq!(
            level_events(&mut h, other),
            [(3001, 65535), (3002, 65535), (3003, 0), (3004, 0)]
        );
        assert_eq!(h.world().resource::<Weather>().state, WeatherState::Clear);
    }

    #[test]
    fn joining_players_get_the_ongoing_weather() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        assert!(crate::world::weather::join_packets(h.world()).is_empty());
        h.run(op, "/weather thunder");
        let events: Vec<_> = crate::world::weather::join_packets(h.world())
            .into_iter()
            .map(|event| (event.event_id, event.data))
            .collect();
        assert_eq!(events, [(3001, 65535), (3002, 65535)]);
    }
}
