use super::super::parser::common::RawJsonAsset;
use super::super::parser::structure_template::StructureTemplateAsset;
use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;

pub struct RegistryAssets<'a> {
    pub biome_source_parameter_lists:
        &'a HashMap<String, super::super::parser::multi_noise_biome_source_parameter_list::MultiNoiseBiomeSourceParameterListJson>,
    pub processor_lists: &'a HashMap<String, super::super::parser::processor_list::ProcessorListJson>,
    pub structures: &'a HashMap<String, super::super::parser::structure::StructureJson>,
    pub structure_sets: &'a HashMap<String, super::super::parser::structure_set::StructureSetJson>,
    pub template_pools: &'a HashMap<String, super::super::parser::template_pool::TemplatePoolJson>,
    pub structure_templates:
        &'a HashMap<String, super::super::parser::structure_template::StructureTemplateAsset>,
}

pub fn emit_worldgen_registries(
    output_dir: &Path,
    assets: RegistryAssets<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut out = String::new();
    writeln!(&mut out, "// Generated worldgen registries.")?;
    writeln!(
        &mut out,
        "// Do not edit manually - regenerated at build time from worldgen assets."
    )?;
    writeln!(&mut out)?;

    emit_json_registry(
        &mut out,
        "MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST_NAMES",
        "multi_noise_biome_source_parameter_list_json",
        "worldgen_data/multi_noise_biome_source_parameter_list",
        assets.biome_source_parameter_lists,
    )?;
    emit_json_registry(
        &mut out,
        "PROCESSOR_LIST_NAMES",
        "processor_list_json",
        "worldgen_data/processor_list",
        assets.processor_lists,
    )?;
    emit_json_registry(
        &mut out,
        "STRUCTURE_NAMES",
        "structure_json",
        "worldgen_data/structure",
        assets.structures,
    )?;
    emit_json_registry(
        &mut out,
        "STRUCTURE_SET_NAMES",
        "structure_set_json",
        "worldgen_data/structure_set",
        assets.structure_sets,
    )?;
    emit_json_registry(
        &mut out,
        "TEMPLATE_POOL_NAMES",
        "template_pool_json",
        "worldgen_data/template_pool",
        assets.template_pools,
    )?;
    emit_binary_registry(
        &mut out,
        "STRUCTURE_TEMPLATE_NAMES",
        "structure_template_nbt",
        "structure_data",
        assets.structure_templates,
    )?;

    std::fs::write(output_dir.join("worldgen_registries.rs"), out)?;
    Ok(())
}

fn emit_json_registry(
    out: &mut String,
    names_const: &str,
    fn_name: &str,
    root: &str,
    assets: &HashMap<String, RawJsonAsset>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut names: Vec<_> = assets.keys().cloned().collect();
    names.sort();

    writeln!(out, "pub const {}: &[&str] = &[", names_const)?;
    for name in &names {
        writeln!(out, "    {},", rust_string_literal(name))?;
    }
    writeln!(out, "];")?;
    writeln!(out)?;

    writeln!(
        out,
        "pub fn {}(name: &str) -> Option<&'static str> {{",
        fn_name
    )?;
    writeln!(out, "    match name {{")?;
    for name in &names {
        let relative_path = &assets[name].relative_path;
        writeln!(
            out,
            "        {} => Some(include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{}/{}\"))),",
            rust_string_literal(name),
            root,
            relative_path
        )?;
    }
    writeln!(out, "        _ => None,")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;

    Ok(())
}

fn emit_binary_registry(
    out: &mut String,
    names_const: &str,
    fn_name: &str,
    root: &str,
    assets: &HashMap<String, StructureTemplateAsset>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut names: Vec<_> = assets.keys().cloned().collect();
    names.sort();

    writeln!(out, "pub const {}: &[&str] = &[", names_const)?;
    for name in &names {
        writeln!(out, "    {},", rust_string_literal(name))?;
    }
    writeln!(out, "];")?;
    writeln!(out)?;

    writeln!(
        out,
        "pub fn {}(name: &str) -> Option<&'static [u8]> {{",
        fn_name
    )?;
    writeln!(out, "    match name {{")?;
    for name in &names {
        let relative_path = &assets[name].relative_path;
        writeln!(
            out,
            "        {} => Some(include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{}/{}\"))),",
            rust_string_literal(name),
            root,
            relative_path
        )?;
    }
    writeln!(out, "        _ => None,")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;

    Ok(())
}

fn rust_string_literal(value: &str) -> String {
    format!("{:?}", value)
}
