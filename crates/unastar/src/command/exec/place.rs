//! `/place` (admin level: the console) and `/locate` (game directors).
//!
//! Measured on BDS 1.26.51 (`tools/bds-sweep/command_probe.py`,
//! `testdata/bds-command-probe.json`), a new flat world:
//!
//! - `place structure` with a name that is not one of the server's jigsaw
//!   structures (the captured `JigsawStructure` soft enum) answers
//!   `commands.place.invalidstructure.error`;
//! - `locate structure village` (also with `useNewChunksOnly` true) answers
//!   `commands.locate.structure.fail.nostructurefound` in a world whose
//!   generator places no structures; the name needs no namespace.
//!
//! unastar does not place jigsaw structures, features or feature rules on
//! command, does not search for structures in generated terrain, and does
//! not search biomes (BDS's search order is not measured: in the flat world
//! the first plains it named was 32 blocks west and north of the origin);
//! those answer the not-implemented failure.

use crate::command::prelude::*;
use crate::server::game::ServerConfigResource;
use crate::world::WorldGenerator;

overloads! {
    pub enum PlaceArgs for "place" {
        0 => Structure {
            action: EnumValue,
            structure: SoftEnumValue,
            pos: Option<BlockPosition>,
            ignore_start_height: Option<bool> = "ignoreStartHeight",
            keep_jigsaws: Option<bool> = "keepJigsaws",
            include_entities: Option<bool> = "includeEntities",
            liquid_settings: Option<EnumValue> = "liquidSettings",
        },
        1 => Jigsaw {
            action: EnumValue,
            pool: FilePath,
            jigsaw_target: Text = "jigsawTarget",
            max_depth: i32 = "maxDepth",
            pos: Option<BlockPosition>,
            keep_jigsaws: Option<bool> = "keepJigsaws",
            include_entities: Option<bool> = "includeEntities",
            liquid_settings: Option<EnumValue> = "liquidSettings",
        },
        2 => Feature { action: EnumValue, feature: EnumValue, position: Option<BlockPosition> },
        3 => FeatureRule {
            action: EnumValue,
            featurerule: EnumValue,
            position: Option<BlockPosition>,
        },
    }
}

/// Whether `name` is one of the captured soft enum's values, with or
/// without the `minecraft:` namespace.
fn soft_enum_has(soft_enum: &str, name: &str) -> bool {
    let full = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    unastar_data::commands::vanilla()
        .soft_enums
        .iter()
        .find(|e| e.name == soft_enum)
        .is_some_and(|e| e.values.contains(&full))
}

pub fn run_place(_ctx: &mut CommandContext<'_>, args: PlaceArgs) -> CommandResult {
    match args {
        PlaceArgs::Structure { structure, .. } if !soft_enum_has("JigsawStructure", &structure) => {
            failure!("commands.place.invalidstructure.error")
        }
        _ => Err(CommandOutput::not_implemented("place")),
    }
}

overloads! {
    pub enum LocateArgs for "locate" {
        0 => Structure {
            subcommand: EnumValue,
            structure: SoftEnumValue,
            use_new_chunks_only: Option<bool> = "useNewChunksOnly",
        },
        1 => Biome { subcommand: EnumValue, biome: EnumValue },
    }
}

pub fn run_locate(ctx: &mut CommandContext<'_>, args: LocateArgs) -> CommandResult {
    let structureless = ctx
        .world
        .get_resource::<ServerConfigResource>()
        .is_some_and(|config| {
            matches!(
                config.0.world.generator,
                WorldGenerator::SuperFlat | WorldGenerator::VoidSpawnPlatform { .. }
            )
        });
    match args {
        LocateArgs::Structure { structure, .. }
            if structureless && soft_enum_has("StructureFeature", &structure) =>
        {
            failure!("commands.locate.structure.fail.nostructurefound")
        }
        _ => Err(CommandOutput::not_implemented("locate")),
    }
}

#[cfg(test)]
mod tests {
    use crate::command::exec::evidence::console_records;
    use crate::command::exec::probe::{Probe, captured_console, captured_player};
    use crate::command::{CommandSource, execute};

    #[test]
    fn place_and_locate_answer_like_bds() {
        let mut probe = Probe::new();
        for command in [
            "place structure minecraft:no_such_structure 0 -60 0",
            "locate structure village",
            "place feature minecraft:no_such_feature",
        ] {
            let output = execute(probe.h.world(), CommandSource::server(), command);
            assert_eq!(
                console_records(&output),
                captured_console(command),
                "{command}"
            );
        }
        let player = probe.player();
        for command in [
            "/locate structure village",
            "/locate structure village true",
            "/place feature minecraft:no_such_feature",
            "/wsserver ws://127.0.0.1:1",
            "/reloadconfig",
            "/locate biome plains",
        ] {
            let output = probe.h.run(player, command);
            assert_eq!(output.messages, captured_player(command), "{command}");
        }
    }
}
