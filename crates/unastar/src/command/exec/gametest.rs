//! `/gametest` and `/dialogue`: commands of systems unastar does not run
//! (the GameTest framework of behaviour-pack scripts; NPC dialogue), which
//! answer as BDS does for a world that has none of their content.
//!
//! Measured on BDS 1.26.51 (command capture, `gametest` and `dialogue`
//! segments, a world without test scripts or NPCs): `runthis` finds no
//! structure block within 15 blocks (`gameTest.error.missingStructure
//! [15]`), `run` no test (`gameTest.error.testMissing [name]`), `runset` and
//! `runsetuntilfail` no test of the tag (`gameTest.error.noTestsFound
//! [tag]`, `suite:default` without one), `pos` no looked-at block
//! (`gameTest.error.cannotFindLookedAtBlock`), `stopall` and `clearall`
//! answer nothing; `/dialogue` on an actor without the NPC component
//! answers `commands.dialogue.noNpcComponent`, and no target
//! `noTargetMatch` then `commands.dialogue.npcNotFound`.

use crate::command::prelude::*;

/// How far `runthis` looks for a test's structure block.
const STRUCTURE_SEARCH_RADIUS: i32 = 15;
/// The tag `runset` runs without one.
const DEFAULT_TAG: &str = "suite:default";

overloads! {
    pub enum GameTestArgs for "gametest" {
        0 => RunThis { mode: EnumValue },
        1 => Run { mode: EnumValue, test_name: SoftEnumValue = "testName", rotation: Option<i32> = "rotationSteps" },
        2 => RunRepeat {
            mode: EnumValue,
            test_name: SoftEnumValue = "testName",
            stop_on_failure: bool = "stopOnFailure",
            repeat: i32 = "repeatCount",
            rotation: Option<i32> = "rotationSteps",
        },
        3 => RunSet { mode: EnumValue, tag: Option<SoftEnumValue>, rotation: Option<i32> = "rotationSteps" },
        4 => RunSetUntilFail { mode: EnumValue, tag: Option<SoftEnumValue>, rotation: Option<i32> = "rotationSteps" },
        5 => ClearAll { mode: EnumValue },
        6 => ShowPosition { mode: EnumValue },
        7 => Create {
            mode: EnumValue,
            test_name: Text = "testName",
            width: Option<i32>,
            height: Option<i32>,
            depth: Option<i32>,
        },
        8 => RunThese { mode: EnumValue },
        9 => StopAll { mode: EnumValue },
    }
}

/// unastar runs no GameTest scripts, so no test or tag is ever registered.
pub fn run_gametest(_ctx: &mut CommandContext<'_>, args: GameTestArgs) -> CommandResult {
    match args {
        GameTestArgs::RunThis { .. } => {
            failure!("gameTest.error.missingStructure", STRUCTURE_SEARCH_RADIUS)
        }
        GameTestArgs::Run { test_name, .. } | GameTestArgs::RunRepeat { test_name, .. } => {
            failure!("gameTest.error.testMissing", &*test_name)
        }
        GameTestArgs::RunSet { tag, .. } | GameTestArgs::RunSetUntilFail { tag, .. } => failure!(
            "gameTest.error.noTestsFound",
            tag.as_deref().unwrap_or(DEFAULT_TAG)
        ),
        GameTestArgs::ShowPosition { .. } => failure!("gameTest.error.cannotFindLookedAtBlock"),
        // [INFERENCE] With no tests nearby there is nothing to run.
        GameTestArgs::RunThese { .. } => {
            failure!("gameTest.error.noTestsFound", DEFAULT_TAG)
        }
        // Measured: no answer.
        GameTestArgs::ClearAll { .. } | GameTestArgs::StopAll { .. } => {
            Ok(CommandOutput::default())
        }
        // BDS places a test structure, which needs the framework.
        GameTestArgs::Create { .. } => Err(CommandOutput::not_implemented("gametest")),
    }
}

overloads! {
    pub enum DialogueArgs for "dialogue" {
        0 => Open { open: EnumValue, npc: Target, player: Target, scene: Option<Text> = "sceneName" },
        1 => Change { change: EnumValue, npc: Target, scene: Text = "sceneName", players: Option<Target> },
    }
}

/// No actor in unastar has the NPC component.
pub fn run_dialogue(ctx: &mut CommandContext<'_>, args: DialogueArgs) -> CommandResult {
    let npc = match &args {
        DialogueArgs::Open { npc, .. } | DialogueArgs::Change { npc, .. } => npc,
    };
    match ctx.entities(Some(npc)) {
        Ok(_) => failure!("commands.dialogue.noNpcComponent"),
        Err(mut output) => {
            output.add_failure("commands.dialogue.npcNotFound", vec![]);
            Err(output)
        }
    }
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::Replay;

    #[test]
    fn gametest_matches_the_bds_capture() {
        Replay::new().assert_segment("gametest", &[]);
    }

    #[test]
    fn dialogue_matches_the_bds_capture() {
        Replay::new().assert_segment("dialogue", &[]);
    }
}
