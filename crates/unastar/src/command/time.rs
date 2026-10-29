//! `/time`: set, add to and query the world clock.

use super::{Command, CommandArgs, CommandContext, CommandOutput, WorldRequest};
use crate::world::rules::WorldTime;

/// What a `/time` command does to the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeRequest {
    /// `/time set <day|night|noon|midnight|sunrise|sunset>`: the next time
    /// the clock reaches that time of day (now if it is there already).
    SetTimeOfDay(u64),
    /// `/time set <ticks>`: the clock itself.
    Set(u64),
    /// `/time add <ticks>`.
    Add(i64),
    Query(TimeQuery),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeQuery {
    /// Ticks into the current day.
    Daytime,
    /// Ticks the level has run.
    Gametime,
    /// Days the clock has counted.
    Day,
}

/// The named times of day `/time set` accepts (BDS values).
const TIME_SPECS: [(&str, u64); 6] = [
    ("day", 1_000),
    ("noon", 6_000),
    ("sunset", 12_000),
    ("night", 13_000),
    ("midnight", 18_000),
    ("sunrise", 23_000),
];

impl TimeRequest {
    /// The clock after a set or add, or `None` when it would leave the
    /// clock's range (`0..=i32::MAX`).
    pub fn resolve(self, now: WorldTime) -> Option<u64> {
        let ticks = match self {
            Self::SetTimeOfDay(target) => {
                let day_start = now.ticks - now.time_of_day();
                if now.time_of_day() <= target {
                    day_start + target
                } else {
                    day_start + target + WorldTime::DAY_TICKS
                }
            }
            Self::Set(ticks) => ticks,
            Self::Add(delta) => now.ticks.checked_add_signed(delta)?,
            Self::Query(_) => return None,
        };
        (ticks <= WorldTime::MAX).then_some(ticks)
    }
}

pub(super) struct TimeCommand;

impl Command for TimeCommand {
    fn name(&self) -> &'static str {
        "time"
    }

    fn description(&self) -> &'static str {
        "Changes or queries the world's game time"
    }

    fn requires_operator(&self) -> bool {
        true
    }

    fn usage(&self) -> &'static str {
        "/time <set <day|night|noon|midnight|sunrise|sunset|ticks> | add <ticks> | query <daytime|gametime|day>>"
    }

    fn execute(&self, _ctx: &CommandContext, args: &mut CommandArgs) -> CommandOutput {
        let mut output = CommandOutput::default();
        let request = match args.rest() {
            [action, value] => parse(action, value),
            _ => None,
        };
        match request {
            Some(request) => output.world = Some(WorldRequest::Time(request)),
            None => output.error(format!("Usage: {}", self.usage())),
        }
        output
    }
}

fn parse(action: &str, value: &str) -> Option<TimeRequest> {
    let value = value.to_ascii_lowercase();
    match action.to_ascii_lowercase().as_str() {
        "set" => TIME_SPECS
            .iter()
            .find(|(name, _)| *name == value)
            .map(|&(_, ticks)| TimeRequest::SetTimeOfDay(ticks))
            .or_else(|| ticks(&value).map(TimeRequest::Set)),
        "add" => value
            .parse::<i32>()
            .ok()
            .map(|delta| TimeRequest::Add(delta.into())),
        "query" => Some(TimeRequest::Query(match value.as_str() {
            "daytime" => TimeQuery::Daytime,
            "gametime" => TimeQuery::Gametime,
            "day" => TimeQuery::Day,
            _ => return None,
        })),
        _ => None,
    }
}

/// A clock value: a non-negative `i32`.
fn ticks(value: &str) -> Option<u64> {
    value
        .parse::<i32>()
        .ok()
        .and_then(|ticks| u64::try_from(ticks).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ticks: u64) -> WorldTime {
        WorldTime { ticks }
    }

    #[test]
    fn named_times_advance_to_their_next_occurrence() {
        assert_eq!(
            parse("set", "night"),
            Some(TimeRequest::SetTimeOfDay(13_000))
        );
        let night = TimeRequest::SetTimeOfDay(13_000);
        assert_eq!(night.resolve(at(1_000)), Some(13_000));
        assert_eq!(night.resolve(at(13_000)), Some(13_000));
        assert_eq!(night.resolve(at(24_000 + 14_000)), Some(48_000 + 13_000));
    }

    #[test]
    fn numbers_set_and_add_the_clock_within_its_range() {
        assert_eq!(parse("set", "500").unwrap().resolve(at(30_000)), Some(500));
        assert_eq!(parse("set", "-1"), None);
        assert_eq!(parse("add", "-100").unwrap().resolve(at(50)), None);
        assert_eq!(parse("add", "250").unwrap().resolve(at(50)), Some(300));
        assert_eq!(
            TimeRequest::Add(1).resolve(at(WorldTime::MAX)),
            None,
            "the clock is an i32"
        );
        assert_eq!(
            parse("query", "daytime"),
            Some(TimeRequest::Query(TimeQuery::Daytime))
        );
        assert_eq!(parse("query", "week"), None);
    }
}
