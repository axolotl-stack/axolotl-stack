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
//! - `locate biome` in the flat world, all plains, names the column 32
//!   blocks west and north of the source's block (`-32, -32` from `0.5,
//!   0.5`; `68, 5` from `100, 37`; `8, 8` from `/execute positioned 40 -60
//!   40`), at the flat world's surface whatever the source's height, `45`
//!   blocks away (the horizontal distance, 45.25, cut); any other biome
//!   answers `commands.locate.biome.fail [biome]`.
//!
//! unastar does not place jigsaw structures, features or feature rules on
//! command, does not search for structures in generated terrain, and does
//! not search biomes in generated terrain (BDS's search order past its first
//! candidate is not measured); those answer the not-implemented failure.

use crate::command::prelude::*;
use crate::server::game::ServerConfigResource;
use crate::world::WorldGenerator;
use crate::world::generator::FlatGenerator;

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

/// Measured: where `locate biome` looks first, from the source's block.
const FIRST_BIOME_CANDIDATE: IVec3 = IVec3::new(-32, 0, -32);

pub fn run_locate(ctx: &mut CommandContext<'_>, args: LocateArgs) -> CommandResult {
    let generator = ctx
        .world
        .get_resource::<ServerConfigResource>()
        .map(|config| config.0.world.generator);
    let structureless = matches!(
        generator,
        Some(WorldGenerator::SuperFlat | WorldGenerator::VoidSpawnPlatform { .. })
    );
    match args {
        LocateArgs::Biome { biome, .. } if generator == Some(WorldGenerator::SuperFlat) => {
            locate_flat_biome(ctx, &biome)
        }
        LocateArgs::Structure { structure, .. }
            if structureless && soft_enum_has("StructureFeature", &structure) =>
        {
            failure!("commands.locate.structure.fail.nostructurefound")
        }
        _ => Err(CommandOutput::not_implemented("locate")),
    }
}

/// `locate biome` in the flat world, where every column is
/// [`FlatGenerator::BIOME`]: its first candidate holds it.
fn locate_flat_biome(ctx: &mut CommandContext<'_>, biome: &str) -> CommandResult {
    let wanted = unastar_data::biomes::vanilla()
        .iter()
        .find(|b| b.identifier == biome)
        .map(|b| b.id);
    if wanted != Some(FlatGenerator::BIOME) {
        return failure!("commands.locate.biome.fail", biome);
    }
    let source = ctx.source.position.floor().as_ivec3();
    let found = IVec3::new(
        source.x + FIRST_BIOME_CANDIDATE.x,
        FlatGenerator::SURFACE_Y,
        source.z + FIRST_BIOME_CANDIDATE.z,
    );
    let distance = f64::from(FIRST_BIOME_CANDIDATE.x).hypot(f64::from(FIRST_BIOME_CANDIDATE.z));
    success!(
        "commands.locate.biome.success",
        biome,
        found.x,
        found.y,
        found.z,
        distance as i32
    )
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

    /// Every `locate biome minecraft:plains` of the probe, from where its
    /// `/tp` (or `/execute positioned`) put the source: the same column and
    /// distance, at unastar's flat surface (BDS's flat world is lower).
    #[test]
    fn locate_biome_finds_the_flat_worlds_plains_where_bds_does() {
        use crate::command::exec::probe::player_steps;
        use crate::world::generator::FlatGenerator;
        let mut probe = Probe::new();
        probe
            .h
            .world()
            .resource_mut::<crate::server::game::ServerConfigResource>()
            .0
            .world
            .generator = crate::world::WorldGenerator::SuperFlat;
        let player = probe.player();
        let desert = "/locate biome minecraft:desert";
        assert_eq!(
            probe.h.run(player, desert).messages,
            captured_player(desert)
        );
        let mut compared = 0;
        for (line, mut bds) in player_steps() {
            if line.starts_with("/tp @s ") {
                assert!(probe.h.run(player, &line).is_success(), "{line}");
            } else if line.contains("locate biome minecraft:plains") {
                bds[0].params[2] = FlatGenerator::SURFACE_Y.to_string();
                assert_eq!(probe.h.run(player, &line).messages, bds, "{line}");
                compared += 1;
            }
        }
        assert!(compared >= 10, "{compared}");
    }
}
