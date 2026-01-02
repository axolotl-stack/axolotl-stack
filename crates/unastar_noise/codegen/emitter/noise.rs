use super::super::parser::noise::NoiseParams;
use std::collections::HashMap;
use std::path::Path;

pub fn emit_noise_params(
    output_dir: &Path,
    noises: &HashMap<String, NoiseParams>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut content = String::new();
    // Use regular comments instead of doc comments for include!() compatibility
    content.push_str("// Generated noise parameters.\n");
    content.push_str("// Do not edit manually - regenerated at build time from worldgen JSON.\n\n");

    // Generate NoiseRef enum
    content.push_str("/// Reference to a noise parameter set (resolved at runtime with seed).\n");
    content.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    content.push_str("pub enum NoiseRef {\n");

    let mut noise_names: Vec<_> = noises.keys().collect();
    noise_names.sort();

    for name in &noise_names {
        let variant = noise_name_to_variant(name);
        content.push_str(&format!("    {},\n", variant));
    }
    content.push_str("}\n\n");

    // Generate noise params data
    content.push_str("/// Noise parameters for a given NoiseRef.\n");
    content.push_str("#[derive(Debug, Clone)]\n");
    content.push_str("pub struct NoiseParamsData {\n");
    content.push_str("    pub first_octave: i32,\n");
    content.push_str("    pub amplitudes: &'static [f64],\n");
    content.push_str("}\n\n");

    content.push_str("impl NoiseRef {\n");
    content.push_str(&format!("    /// Total number of noise variants.\n"));
    content.push_str(&format!("    pub const COUNT: usize = {};\n\n", noise_names.len()));
    content.push_str("    pub fn params(&self) -> NoiseParamsData {\n");
    content.push_str("        match self {\n");

    for name in &noise_names {
        let variant = noise_name_to_variant(name);
        let params = noises.get(*name).unwrap();
        let amps: Vec<String> = params.amplitudes.iter().map(|a| format!("{:.16}", a)).collect();
        content.push_str(&format!(
            "            NoiseRef::{} => NoiseParamsData {{ first_octave: {}, amplitudes: &[{}] }},\n",
            variant,
            params.first_octave,
            amps.join(", ")
        ));
    }

    content.push_str("        }\n");
    content.push_str("    }\n");
    content.push_str("}\n\n");

    // Generate NOISE_PARAMS static array for NoiseRegistry iteration
    content.push_str("/// All noise parameters for registry initialization.\n");
    content.push_str(&format!("pub static NOISE_PARAMS: [(NoiseRef, NoiseParamsData); {}] = [\n", noise_names.len()));

    for name in &noise_names {
        let variant = noise_name_to_variant(name);
        let params = noises.get(*name).unwrap();
        let amps: Vec<String> = params.amplitudes.iter().map(|a| format!("{:.16}", a)).collect();
        content.push_str(&format!(
            "    (NoiseRef::{}, NoiseParamsData {{ first_octave: {}, amplitudes: &[{}] }}),\n",
            variant,
            params.first_octave,
            amps.join(", ")
        ));
    }

    content.push_str("];\n");

    std::fs::write(output_dir.join("noise_params.rs"), content)?;
    Ok(())
}

fn noise_name_to_variant(name: &str) -> String {
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
