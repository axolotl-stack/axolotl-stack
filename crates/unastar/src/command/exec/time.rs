//! `/time add|set|query`, on the world clock (`world::clock`, replicated
//! with `SyncWorldClocks` like BDS).
//!
//! Measured (BDS 1.26.51): `set day` answers `commands.time.set [1000]` (the
//! value asked for) and moves the clock to the next time it reads 1000;
//! `add 100` answers `commands.time.added [100]`; queries answer
//! `commands.time.query.daytime|gametime|day`.

use crate::command::prelude::*;
use crate::ecs::resources::TickCounter;
use crate::world::clock;
use crate::world::rules::WorldTime;

arg_enum! {
    /// `/time set` presets (captured `TimeSpec`).
    pub enum TimeSpec in "TimeSpec" {
        Day = "day",
        Sunrise = "sunrise",
        Noon = "noon",
        Sunset = "sunset",
        Night = "night",
        Midnight = "midnight",
    }
}

impl TimeSpec {
    /// The time of day BDS sets for the preset.
    pub const fn ticks(self) -> i64 {
        match self {
            Self::Day => 1_000,
            Self::Noon => 6_000,
            Self::Sunset => 12_000,
            Self::Night => 13_000,
            Self::Midnight => 18_000,
            Self::Sunrise => 23_000,
        }
    }
}

arg_enum! {
    /// Captured `TimeQuery`.
    pub enum TimeQuery in "TimeQuery" {
        Daytime = "daytime",
        Gametime = "gametime",
        Day = "day",
    }
}

overloads! {
    pub enum TimeArgs for "time" {
        0 => Add { mode: EnumValue, amount: i32 },
        1 => Set { mode: EnumValue, amount: i32 },
        2 => SetPreset { mode: EnumValue, time: TimeSpec },
        3 => Query { mode: EnumValue, time: TimeQuery },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: TimeArgs) -> CommandResult {
    let now = *ctx.world.resource::<WorldTime>();
    match args {
        TimeArgs::Query { time, .. } => {
            let (key, value) = match time {
                TimeQuery::Daytime => ("commands.time.query.daytime", now.time_of_day()),
                TimeQuery::Gametime => (
                    "commands.time.query.gametime",
                    ctx.world.resource::<TickCounter>().current as i64,
                ),
                // [INFERENCE] whole days, rounded toward zero (not captured
                // for a negative clock).
                TimeQuery::Day => ("commands.time.query.day", now.ticks / WorldTime::DAY_TICKS),
            };
            success!(key, value)
        }
        TimeArgs::Add { amount, .. } => {
            set_clock(ctx, now.ticks + i64::from(amount), i64::from(amount))?;
            success!("commands.time.added", amount)
        }
        // Measured: any `i32`, negative included (`/time set -5` keeps a
        // clock of -5, whose daytime reads 23995).
        TimeArgs::Set { amount, .. } => {
            set_clock(ctx, i64::from(amount), i64::from(amount))?;
            success!("commands.time.set", amount)
        }
        TimeArgs::SetPreset { time, .. } => {
            // The next time the clock reads the preset (now if it does).
            let target = time.ticks();
            let day_start = now.ticks - now.time_of_day();
            let ticks = if now.time_of_day() <= target {
                day_start + target
            } else {
                day_start + target + WorldTime::DAY_TICKS
            };
            set_clock(ctx, ticks, target)?;
            success!("commands.time.set", target)
        }
    }
}

fn set_clock(ctx: &mut CommandContext<'_>, ticks: i64, shown: i64) -> Result<(), CommandOutput> {
    if !(WorldTime::MIN..=WorldTime::MAX).contains(&ticks) {
        return Err(out_of_range(shown));
    }
    clock::set_time(ctx.world, ticks);
    Ok(())
}

/// The clock is an `i32`, as in BDS.
fn out_of_range(value: i64) -> CommandOutput {
    if value < 0 {
        CommandOutput::failure(
            "commands.generic.num.tooSmall",
            vec![value.to_string(), WorldTime::MIN.to_string()],
        )
    } else {
        CommandOutput::failure(
            "commands.generic.num.tooBig",
            vec![value.to_string(), WorldTime::MAX.to_string()],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::args::assert_covers;
    use crate::command::exec::test_support::Harness;

    #[test]
    fn enums_cover_the_captured_values() {
        assert_covers::<TimeSpec>();
        assert_covers::<TimeQuery>();
    }

    #[test]
    fn set_add_and_query_answer_like_bds() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        h.world().resource_mut::<WorldTime>().ticks = 263;
        assert_eq!(
            h.run(op, "/time set day"),
            CommandOutput::success("commands.time.set", vec!["1000".into()])
        );
        assert_eq!(h.world().resource::<WorldTime>().ticks, 1000);
        assert_eq!(
            h.run(op, "/time set 6000"),
            CommandOutput::success("commands.time.set", vec!["6000".into()])
        );
        assert_eq!(
            h.run(op, "/time add 100"),
            CommandOutput::success("commands.time.added", vec!["100".into()])
        );
        assert_eq!(
            h.run(op, "/time query daytime"),
            CommandOutput::success("commands.time.query.daytime", vec!["6100".into()])
        );
        assert_eq!(
            h.run(op, "/time query day"),
            CommandOutput::success("commands.time.query.day", vec!["0".into()])
        );
        // A preset already passed today moves to tomorrow.
        h.run(op, "/time set day");
        assert_eq!(h.world().resource::<WorldTime>().ticks, 25_000);
        // A negative clock is kept; its day starts 24000 ticks earlier.
        assert_eq!(
            h.run(op, "/time set -5"),
            CommandOutput::success("commands.time.set", vec!["-5".into()])
        );
        assert_eq!(h.world().resource::<WorldTime>().ticks, -5);
        assert_eq!(
            h.run(op, "/time query daytime"),
            CommandOutput::success("commands.time.query.daytime", vec!["23995".into()])
        );
        h.run(op, "/time set night");
        assert_eq!(h.world().resource::<WorldTime>().ticks, 13_000);
    }
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::Replay;

    #[test]
    fn time_matches_the_bds_capture() {
        Replay::new().assert_segment("time", &[]);
    }
}
