//! `/difficulty <name|0-3>`: sets the level difficulty and broadcasts
//! `SetDifficulty`; answers `commands.difficulty.success ["HARD"]` (measured).

use crate::command::prelude::*;
use crate::world::rules::Difficulty;
use jolyne::valentine::SetDifficultyPacket;

arg_enum! {
    /// Captured `Difficulty`.
    pub enum DifficultyName in "Difficulty" {
        Peaceful = "peaceful" | "p",
        Easy = "easy" | "e",
        Normal = "normal" | "n",
        Hard = "hard" | "h",
    }
}

overloads! {
    pub enum DifficultyArgs for "difficulty" {
        0 => Named { difficulty: DifficultyName },
        1 => Numbered { difficulty: i32 },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: DifficultyArgs) -> CommandResult {
    let difficulty = match args {
        DifficultyArgs::Named { difficulty } => match difficulty {
            DifficultyName::Peaceful => Difficulty::Peaceful,
            DifficultyName::Easy => Difficulty::Easy,
            DifficultyName::Normal => Difficulty::Normal,
            DifficultyName::Hard => Difficulty::Hard,
        },
        DifficultyArgs::Numbered { difficulty } => match difficulty {
            0 => Difficulty::Peaceful,
            1 => Difficulty::Easy,
            2 => Difficulty::Normal,
            3 => Difficulty::Hard,
            _ => return failure!("commands.difficulty.usage"),
        },
    };
    *ctx.world.resource_mut::<Difficulty>() = difficulty;
    ctx.broadcast(SetDifficultyPacket {
        difficulty: difficulty.wire(),
    });
    success!(
        "commands.difficulty.success",
        difficulty.name().to_ascii_uppercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;

    #[test]
    fn enum_covers_the_captured_values() {
        crate::command::args::assert_covers::<DifficultyName>();
    }

    #[test]
    fn names_and_numbers_set_the_difficulty() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        assert_eq!(
            h.run(op, "/difficulty hard"),
            CommandOutput::success("commands.difficulty.success", vec!["HARD".into()])
        );
        assert_eq!(*h.world().resource::<Difficulty>(), Difficulty::Hard);
        assert_eq!(
            h.run(op, "/difficulty 0"),
            CommandOutput::success("commands.difficulty.success", vec!["PEACEFUL".into()])
        );
        assert_eq!(
            h.run(op, "/difficulty 7"),
            CommandOutput::failure("commands.difficulty.usage", vec![])
        );
        assert_eq!(*h.world().resource::<Difficulty>(), Difficulty::Peaceful);
    }
}
