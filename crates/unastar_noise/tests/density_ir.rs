//! Invalid density inputs must fail at generation, never become zero-valued terrain.

#[allow(dead_code)]
#[path = "../codegen"]
mod compiler {
    #[path = "analyzer/mod.rs"]
    pub mod analyzer;
    #[path = "parser"]
    pub mod parser {
        pub mod density_function;
    }
    #[path = "emitter"]
    pub mod emitter {
        pub mod emitter_quote;
    }
}

use compiler::analyzer::DependencyGraph;
use compiler::emitter::emitter_quote::AotEmitter;
use compiler::parser::density_function::{
    DensityFunctionArg, DensityFunctionDef, SplineDef, SplinePoint, SplineValue,
};
use std::collections::HashMap;

#[test]
#[should_panic(expected = "Unknown density builtin reference: minecraft:missing_density")]
fn missing_density_reference_is_not_fabricated_as_zero() {
    let argument = DensityFunctionArg::Reference("minecraft:missing_density".into());
    DependencyGraph::build(&[("final_density", &argument)], &HashMap::new());
}

fn emit_cached_blended(flat: bool, spline_coordinate: bool) {
    let argument = DensityFunctionArg::Inline(Box::new(DensityFunctionDef::OldBlendedNoise {
        xz_scale: 1.0,
        y_scale: 1.0,
        xz_factor: 80.0,
        y_factor: 160.0,
        smear_scale_multiplier: 8.0,
    }));
    let argument = if spline_coordinate {
        DensityFunctionArg::Inline(Box::new(DensityFunctionDef::Spline {
            spline: SplineDef {
                coordinate: argument,
                points: vec![
                    SplinePoint {
                        location: -1.0,
                        value: SplineValue::Constant(-1.0),
                        derivative: 1.0,
                    },
                    SplinePoint {
                        location: 1.0,
                        value: SplineValue::Constant(1.0),
                        derivative: 1.0,
                    },
                ],
            },
        }))
    } else {
        argument
    };
    let argument = DensityFunctionArg::Inline(Box::new(if flat {
        DensityFunctionDef::FlatCache { argument }
    } else {
        DensityFunctionDef::Cache2D { argument }
    }));
    let roots = [
        "final_density",
        "temperature",
        "vegetation",
        "continents",
        "erosion",
        "depth",
        "ridges",
        "preliminary_surface_level",
    ]
    .map(|name| (name, &argument));
    let graph = DependencyGraph::build(&roots, &HashMap::new());
    let _ = AotEmitter::new(&graph).emit_module();
}

#[test]
#[should_panic(expected = "OldBlendedNoise cannot be lowered into flat cache")]
fn blended_flat_cache_rejects_lost_vertical_coordinates() {
    emit_cached_blended(true, false);
}

#[test]
#[should_panic(expected = "OldBlendedNoise cannot be lowered into column cache")]
fn blended_column_cache_rejects_lost_vertical_coordinates() {
    emit_cached_blended(false, false);
}

#[test]
#[should_panic(expected = "OldBlendedNoise cannot be lowered into inline flat cache")]
fn inline_blended_flat_cache_rejects_lost_vertical_coordinates() {
    emit_cached_blended(true, true);
}

#[test]
#[should_panic(expected = "OldBlendedNoise cannot be lowered into inline column cache")]
fn inline_blended_column_cache_rejects_lost_vertical_coordinates() {
    emit_cached_blended(false, true);
}
