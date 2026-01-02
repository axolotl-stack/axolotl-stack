use super::super::parser::density_function::*;
use super::super::parser::noise_settings::NoiseRouter;
use std::collections::HashMap;
use std::path::Path;

/// Counter for generating unique variable names
struct VarGen {
    func_counter: usize,
    spline_counter: usize,
}

impl VarGen {
    fn new() -> Self {
        Self { func_counter: 0, spline_counter: 0 }
    }

    fn next_func(&mut self) -> String {
        let name = format!("f{}", self.func_counter);
        self.func_counter += 1;
        name
    }

    fn next_spline(&mut self) -> String {
        let name = format!("s{}", self.spline_counter);
        self.spline_counter += 1;
        name
    }
}

/// Emit a router file for a dimension (overworld, nether, end)
/// Uses arena-based allocation for better cache locality.
pub fn emit_router_file(
    output_dir: &Path,
    name: &str,
    router: &NoiseRouter,
    refs: &HashMap<String, DensityFunctionArg>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut content = String::new();
    content.push_str(&format!("//! Generated {} noise router.\n", name));
    content.push_str("//!\n");
    content.push_str(
        "//! Do not edit manually - regenerate with `cargo run -p unastar_worldgen_gen`\n\n",
    );

    // Import types from the static types module (parent of generated)
    content.push_str("use super::super::types::*;\n");
    content.push_str("use super::noise_params::NoiseRef;\n\n");

    // Generate the router builder function
    content.push_str(&format!("/// Build the {} noise router.\n", name));
    content.push_str(&format!(
        "pub fn build_{}_router() -> NoiseRouter {{\n",
        name
    ));
    content.push_str("    let mut arena = DensityArena::new();\n\n");

    // We'll collect all emitted statements and the final variable names for each router field
    let mut var_gen = VarGen::new();
    let mut statements = Vec::new();

    // Emit each router field and collect the final variable name
    let barrier = emit_arg_arena(&router.barrier, refs, &mut var_gen, &mut statements);
    let continents = emit_arg_arena(&router.continents, refs, &mut var_gen, &mut statements);
    let depth = emit_arg_arena(&router.depth, refs, &mut var_gen, &mut statements);
    let erosion = emit_arg_arena(&router.erosion, refs, &mut var_gen, &mut statements);
    let final_density = emit_arg_arena(&router.final_density, refs, &mut var_gen, &mut statements);
    let fluid_level_floodedness = emit_arg_arena(&router.fluid_level_floodedness, refs, &mut var_gen, &mut statements);
    let fluid_level_spread = emit_arg_arena(&router.fluid_level_spread, refs, &mut var_gen, &mut statements);
    let lava = emit_arg_arena(&router.lava, refs, &mut var_gen, &mut statements);
    let initial_density_without_jaggedness = emit_arg_arena(&router.initial_density_without_jaggedness, refs, &mut var_gen, &mut statements);
    let ridges = emit_arg_arena(&router.ridges, refs, &mut var_gen, &mut statements);
    let temperature = emit_arg_arena(&router.temperature, refs, &mut var_gen, &mut statements);
    let vegetation = emit_arg_arena(&router.vegetation, refs, &mut var_gen, &mut statements);
    let vein_gap = emit_arg_arena(&router.vein_gap, refs, &mut var_gen, &mut statements);
    let vein_ridged = emit_arg_arena(&router.vein_ridged, refs, &mut var_gen, &mut statements);
    let vein_toggle = emit_arg_arena(&router.vein_toggle, refs, &mut var_gen, &mut statements);

    // Write all statements
    for stmt in &statements {
        content.push_str("    ");
        content.push_str(stmt);
        content.push('\n');
    }

    content.push_str("\n    NoiseRouter {\n");
    content.push_str("        arena,\n");
    content.push_str(&format!("        barrier: {},\n", barrier));
    content.push_str(&format!("        continents: {},\n", continents));
    content.push_str(&format!("        depth: {},\n", depth));
    content.push_str(&format!("        erosion: {},\n", erosion));
    content.push_str(&format!("        final_density: {},\n", final_density));
    content.push_str(&format!("        fluid_level_floodedness: {},\n", fluid_level_floodedness));
    content.push_str(&format!("        fluid_level_spread: {},\n", fluid_level_spread));
    content.push_str(&format!("        lava: {},\n", lava));
    content.push_str(&format!("        initial_density_without_jaggedness: {},\n", initial_density_without_jaggedness));
    content.push_str(&format!("        ridges: {},\n", ridges));
    content.push_str(&format!("        temperature: {},\n", temperature));
    content.push_str(&format!("        vegetation: {},\n", vegetation));
    content.push_str(&format!("        vein_gap: {},\n", vein_gap));
    content.push_str(&format!("        vein_ridged: {},\n", vein_ridged));
    content.push_str(&format!("        vein_toggle: {},\n", vein_toggle));
    content.push_str("    }\n");
    content.push_str("}\n");

    std::fs::write(output_dir.join(format!("{}.rs", name)), content)?;
    Ok(())
}

/// Emit code for a density function argument using arena allocation.
/// Returns the variable name holding the DensityIdx.
fn emit_arg_arena(
    arg: &DensityFunctionArg,
    refs: &HashMap<String, DensityFunctionArg>,
    var_gen: &mut VarGen,
    statements: &mut Vec<String>,
) -> String {
    match arg {
        DensityFunctionArg::Constant(v) => {
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::Constant({:?}));", var, v));
            var
        }

        DensityFunctionArg::Reference(name) => {
            // Resolve the reference
            if let Some(def) = refs.get(name) {
                emit_arg_arena(def, refs, var_gen, statements)
            } else {
                // Could be a built-in reference like minecraft:y
                let var = var_gen.next_func();
                match name.as_str() {
                    "minecraft:y" => {
                        statements.push(format!("let {} = arena.alloc(DensityFunction::YClampedGradient {{ from_y: -64, to_y: 320, from_value: -64.0, to_value: 320.0 }});", var));
                    }
                    "minecraft:zero" => {
                        statements.push(format!("let {} = arena.alloc(DensityFunction::Constant(0.0));", var));
                    }
                    "minecraft:shift_x" => {
                        // FlatCache(Cache2D(ShiftA(Offset)))
                        let shift_a = var_gen.next_func();
                        let cache_2d = var_gen.next_func();
                        statements.push(format!("let {} = arena.alloc(DensityFunction::ShiftA(NoiseRef::Offset));", shift_a));
                        statements.push(format!("let {} = arena.alloc(DensityFunction::Cache2D({}));", cache_2d, shift_a));
                        statements.push(format!("let {} = arena.alloc(DensityFunction::FlatCache({}));", var, cache_2d));
                    }
                    "minecraft:shift_z" => {
                        // FlatCache(Cache2D(ShiftB(Offset)))
                        let shift_b = var_gen.next_func();
                        let cache_2d = var_gen.next_func();
                        statements.push(format!("let {} = arena.alloc(DensityFunction::ShiftB(NoiseRef::Offset));", shift_b));
                        statements.push(format!("let {} = arena.alloc(DensityFunction::Cache2D({}));", cache_2d, shift_b));
                        statements.push(format!("let {} = arena.alloc(DensityFunction::FlatCache({}));", var, cache_2d));
                    }
                    _ => {
                        eprintln!("Warning: Unknown reference: {}", name);
                        statements.push(format!("let {} = arena.alloc(DensityFunction::Constant(0.0)); // TODO: Unknown ref {}", var, name));
                    }
                }
                var
            }
        }

        DensityFunctionArg::Inline(def) => emit_def_arena(def, refs, var_gen, statements),
    }
}

fn emit_def_arena(
    def: &DensityFunctionDef,
    refs: &HashMap<String, DensityFunctionArg>,
    var_gen: &mut VarGen,
    statements: &mut Vec<String>,
) -> String {
    match def {
        DensityFunctionDef::Constant { argument } => {
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::Constant({:?}));", var, argument));
            var
        }

        DensityFunctionDef::Add { argument1, argument2 } => {
            let a = emit_arg_arena(argument1, refs, var_gen, statements);
            let b = emit_arg_arena(argument2, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::Add({}, {}));", var, a, b));
            var
        }

        DensityFunctionDef::Mul { argument1, argument2 } => {
            let a = emit_arg_arena(argument1, refs, var_gen, statements);
            let b = emit_arg_arena(argument2, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::Mul({}, {}));", var, a, b));
            var
        }

        DensityFunctionDef::Min { argument1, argument2 } => {
            let a = emit_arg_arena(argument1, refs, var_gen, statements);
            let b = emit_arg_arena(argument2, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::Min({}, {}));", var, a, b));
            var
        }

        DensityFunctionDef::Max { argument1, argument2 } => {
            let a = emit_arg_arena(argument1, refs, var_gen, statements);
            let b = emit_arg_arena(argument2, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::Max({}, {}));", var, a, b));
            var
        }

        DensityFunctionDef::Clamp { input, min, max } => {
            let inp = emit_arg_arena(input, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!(
                "let {} = arena.alloc(DensityFunction::Clamp {{ input: {}, min: {:?}, max: {:?} }});",
                var, inp, min, max
            ));
            var
        }

        DensityFunctionDef::Abs { argument } => {
            let a = emit_arg_arena(argument, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::Abs({}));", var, a));
            var
        }

        DensityFunctionDef::Square { argument } => {
            let a = emit_arg_arena(argument, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::Square({}));", var, a));
            var
        }

        DensityFunctionDef::Cube { argument } => {
            let a = emit_arg_arena(argument, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::Cube({}));", var, a));
            var
        }

        DensityFunctionDef::Squeeze { argument } => {
            let a = emit_arg_arena(argument, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::Squeeze({}));", var, a));
            var
        }

        DensityFunctionDef::HalfNegative { argument } => {
            let a = emit_arg_arena(argument, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::HalfNegative({}));", var, a));
            var
        }

        DensityFunctionDef::QuarterNegative { argument } => {
            let a = emit_arg_arena(argument, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::QuarterNegative({}));", var, a));
            var
        }

        DensityFunctionDef::YClampedGradient { from_y, to_y, from_value, to_value } => {
            let var = var_gen.next_func();
            statements.push(format!(
                "let {} = arena.alloc(DensityFunction::YClampedGradient {{ from_y: {}, to_y: {}, from_value: {:?}, to_value: {:?} }});",
                var, from_y, to_y, from_value, to_value
            ));
            var
        }

        DensityFunctionDef::Noise { noise, xz_scale, y_scale } => {
            let noise_ref = noise_name_to_ref(noise);
            let var = var_gen.next_func();
            statements.push(format!(
                "let {} = arena.alloc(DensityFunction::Noise {{ noise_ref: NoiseRef::{}, xz_scale: {:?}, y_scale: {:?} }});",
                var, noise_ref, xz_scale, y_scale
            ));
            var
        }

        DensityFunctionDef::ShiftedNoise { noise, shift_x, shift_y, shift_z, xz_scale, y_scale } => {
            let noise_ref = noise_name_to_ref(noise);
            let sx = emit_arg_arena(shift_x, refs, var_gen, statements);
            let sy = emit_arg_arena(shift_y, refs, var_gen, statements);
            let sz = emit_arg_arena(shift_z, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!(
                "let {} = arena.alloc(DensityFunction::ShiftedNoise {{ noise_ref: NoiseRef::{}, shift_x: {}, shift_y: {}, shift_z: {}, xz_scale: {:?}, y_scale: {:?} }});",
                var, noise_ref, sx, sy, sz, xz_scale, y_scale
            ));
            var
        }

        DensityFunctionDef::FlatCache { argument } => {
            let a = emit_arg_arena(argument, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::FlatCache({}));", var, a));
            var
        }

        DensityFunctionDef::Cache2D { argument } => {
            let a = emit_arg_arena(argument, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::Cache2D({}));", var, a));
            var
        }

        DensityFunctionDef::CacheOnce { argument } => {
            let a = emit_arg_arena(argument, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::CacheOnce({}));", var, a));
            var
        }

        DensityFunctionDef::Interpolated { argument } => {
            let a = emit_arg_arena(argument, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::Interpolated({}));", var, a));
            var
        }

        DensityFunctionDef::BlendAlpha {} => {
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::BlendAlpha);", var));
            var
        }

        DensityFunctionDef::BlendOffset {} => {
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::BlendOffset);", var));
            var
        }

        DensityFunctionDef::BlendDensity { argument } => {
            let a = emit_arg_arena(argument, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::BlendDensity({}));", var, a));
            var
        }

        DensityFunctionDef::RangeChoice { input, min_inclusive, max_exclusive, when_in_range, when_out_of_range } => {
            let inp = emit_arg_arena(input, refs, var_gen, statements);
            let wir = emit_arg_arena(when_in_range, refs, var_gen, statements);
            let wor = emit_arg_arena(when_out_of_range, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!(
                "let {} = arena.alloc(DensityFunction::RangeChoice {{ input: {}, min_inclusive: {:?}, max_exclusive: {:?}, when_in_range: {}, when_out_of_range: {} }});",
                var, inp, min_inclusive, max_exclusive, wir, wor
            ));
            var
        }

        DensityFunctionDef::Spline { spline } => {
            let spline_var = emit_spline_arena(spline, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::Spline({}));", var, spline_var));
            var
        }

        DensityFunctionDef::WeirdScaledSampler { input, noise, rarity_value_mapper } => {
            let noise_ref = noise_name_to_ref(noise);
            let rarity_type = match rarity_value_mapper.as_str() {
                "type_1" => "RarityType::Type1",
                "type_2" => "RarityType::Type2",
                _ => "RarityType::Type1",
            };
            let inp = emit_arg_arena(input, refs, var_gen, statements);
            let var = var_gen.next_func();
            statements.push(format!(
                "let {} = arena.alloc(DensityFunction::WeirdScaledSampler {{ input: {}, noise_ref: NoiseRef::{}, rarity_type: {} }});",
                var, inp, noise_ref, rarity_type
            ));
            var
        }

        DensityFunctionDef::ShiftA { argument } => {
            let noise_ref = noise_name_to_ref(argument);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::ShiftA(NoiseRef::{}));", var, noise_ref));
            var
        }

        DensityFunctionDef::ShiftB { argument } => {
            let noise_ref = noise_name_to_ref(argument);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::ShiftB(NoiseRef::{}));", var, noise_ref));
            var
        }

        DensityFunctionDef::Shift { argument } => {
            let noise_ref = noise_name_to_ref(argument);
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::ShiftA(NoiseRef::{}));", var, noise_ref));
            var
        }

        DensityFunctionDef::OldBlendedNoise { xz_scale, y_scale, xz_factor, y_factor, smear_scale_multiplier } => {
            let var = var_gen.next_func();
            statements.push(format!(
                "let {} = arena.alloc(DensityFunction::OldBlendedNoise {{ xz_scale: {:?}, y_scale: {:?}, xz_factor: {:?}, y_factor: {:?}, smear_scale_multiplier: {:?} }});",
                var, xz_scale, y_scale, xz_factor, y_factor, smear_scale_multiplier
            ));
            var
        }

        DensityFunctionDef::EndIslands {} => {
            let var = var_gen.next_func();
            statements.push(format!("let {} = arena.alloc(DensityFunction::EndIslands);", var));
            var
        }
    }
}

/// Emit a spline into the arena. Returns the SplineIdx variable name.
fn emit_spline_arena(
    spline: &SplineDef,
    refs: &HashMap<String, DensityFunctionArg>,
    var_gen: &mut VarGen,
    statements: &mut Vec<String>,
) -> String {
    // First, emit the coordinate function
    let coord = emit_arg_arena(&spline.coordinate, refs, var_gen, statements);

    // Emit all nested splines first (depth-first)
    let points_code: Vec<String> = spline.points.iter().map(|p| {
        let value_code = match &p.value {
            SplineValue::Constant(v) => format!("SplineValue::Constant({:?})", v),
            SplineValue::Nested(nested) => {
                let nested_var = emit_spline_arena(nested, refs, var_gen, statements);
                format!("SplineValue::Nested({})", nested_var)
            }
        };
        format!(
            "SplinePoint {{ location: {:?}, value: {}, derivative: {:?} }}",
            p.location, value_code, p.derivative
        )
    }).collect();

    let spline_var = var_gen.next_spline();
    statements.push(format!(
        "let {} = arena.alloc_spline(Spline {{ coordinate: {}, points: vec![{}] }});",
        spline_var, coord, points_code.join(", ")
    ));
    spline_var
}

fn noise_name_to_ref(name: &str) -> String {
    let clean = name.strip_prefix("minecraft:").unwrap_or(name);
    // Convert snake_case to PascalCase
    clean
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}
