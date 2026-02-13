use anyhow::{Context, Result};
use std::env;
use std::fmt::Write;
use std::fs;
use std::path::PathBuf;
use wit_parser::{Handle, Resolve, Type, TypeDefKind, WorldItem};

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=wit");

    let mut resolve = Resolve::default();
    let (pkg_id, _) = resolve
        .push_dir("wit")
        .context("failed to parse WIT directory")?;
    let world_id = resolve
        .select_world(&[pkg_id], Some("unastar-plugin"))
        .context("failed to select world")?;
    let world = &resolve.worlds[world_id];

    // Collect exported functions.
    let funcs: Vec<_> = world
        .exports
        .iter()
        .filter_map(|(_key, item)| match item {
            WorldItem::Function(f) => Some(f),
            _ => None,
        })
        .collect();

    let mut out = String::new();

    // ── Plugin trait ──────────────────────────────────────────────────
    writeln!(
        out,
        "/// Trait for writing plugins with minimal boilerplate."
    )?;
    writeln!(out, "///")?;
    writeln!(
        out,
        "/// Unlike [`Guest`] (which requires all methods), every method here has a"
    )?;
    writeln!(
        out,
        "/// sensible default so you only implement the events you care about."
    )?;
    writeln!(out, "///")?;
    writeln!(
        out,
        "/// Use [`export_plugin!`] instead of [`export!`] to register your plugin."
    )?;
    writeln!(out, "#[allow(unused_variables)]")?;
    writeln!(out, "pub trait Plugin {{")?;

    for func in &funcs {
        let name = func.name.replace('-', "_");
        let params = params_str(&resolve, func, false);
        let ret = return_str(&resolve, func);
        let default = default_body(func);
        writeln!(out, "    fn {name}({params}){ret} {{ {default} }}")?;
    }

    writeln!(out, "}}")?;
    writeln!(out)?;

    // ── export_plugin! macro ──────────────────────────────────────────
    writeln!(
        out,
        "/// Register a [`Plugin`] implementation as a WASM component export."
    )?;
    writeln!(out, "#[macro_export]")?;
    writeln!(out, "macro_rules! export_plugin {{")?;
    writeln!(out, "    ($t:ident) => {{")?;
    writeln!(out, "        struct __UnastarPluginBridge;")?;
    writeln!(out)?;
    writeln!(
        out,
        "        impl $crate::Guest for __UnastarPluginBridge {{"
    )?;

    for func in &funcs {
        let name = func.name.replace('-', "_");
        let params = params_str(&resolve, func, true);
        let args = arg_names(func);
        let ret = return_str(&resolve, func);
        writeln!(out, "            fn {name}({params}){ret} {{")?;
        writeln!(
            out,
            "                <$t as $crate::Plugin>::{name}({args})"
        )?;
        writeln!(out, "            }}")?;
    }

    writeln!(out, "        }}")?;
    writeln!(out)?;
    writeln!(out, "        $crate::export!(__UnastarPluginBridge);")?;
    writeln!(out, "    }};")?;
    writeln!(out, "}}")?;

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    fs::write(out_dir.join("plugin_api.rs"), out)?;

    Ok(())
}

/// Build a comma-separated parameter list.
/// When `in_macro` is true, custom types are `$crate::`-qualified.
fn params_str(resolve: &Resolve, func: &wit_parser::Function, in_macro: bool) -> String {
    func.params
        .iter()
        .map(|p| {
            let name = p.name.replace('-', "_");
            let ty = type_to_rust(resolve, &p.ty, in_macro);
            format!("{name}: {ty}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build a comma-separated list of just the argument names (for delegation).
fn arg_names(func: &wit_parser::Function) -> String {
    func.params
        .iter()
        .map(|p| p.name.replace('-', "_"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build the `-> Type` suffix, or empty string for void functions.
fn return_str(resolve: &Resolve, func: &wit_parser::Function) -> String {
    match &func.result {
        Some(ty) => format!(" -> {}", type_to_rust(resolve, ty, false)),
        None => String::new(),
    }
}

/// Default body for trait methods: `true` for bool returns, empty otherwise.
fn default_body(func: &wit_parser::Function) -> &'static str {
    match &func.result {
        Some(Type::Bool) => "true",
        Some(_) => panic!(
            "unsupported default for non-bool return type in `{}`",
            func.name
        ),
        None => "",
    }
}

/// Convert a WIT type to its Rust string representation.
/// When `in_macro`, custom types get a `$crate::` prefix.
fn type_to_rust(resolve: &Resolve, ty: &Type, in_macro: bool) -> String {
    match ty {
        Type::Bool => "bool".into(),
        Type::U8 => "u8".into(),
        Type::U16 => "u16".into(),
        Type::U32 => "u32".into(),
        Type::U64 => "u64".into(),
        Type::S8 => "i8".into(),
        Type::S16 => "i16".into(),
        Type::S32 => "i32".into(),
        Type::S64 => "i64".into(),
        Type::F32 => "f32".into(),
        Type::F64 => "f64".into(),
        Type::Char => "char".into(),
        Type::String => "String".into(),
        Type::Id(id) => {
            let typedef = &resolve.types[*id];
            match &typedef.kind {
                TypeDefKind::Handle(Handle::Borrow(inner_id)) => {
                    let name = resolve_type_name(resolve, *inner_id);
                    if in_macro {
                        format!("&$crate::{name}")
                    } else {
                        format!("&{name}")
                    }
                }
                TypeDefKind::Handle(Handle::Own(inner_id)) => {
                    let name = resolve_type_name(resolve, *inner_id);
                    if in_macro {
                        format!("$crate::{name}")
                    } else {
                        name
                    }
                }
                TypeDefKind::List(inner) => {
                    format!("Vec<{}>", type_to_rust(resolve, inner, in_macro))
                }
                TypeDefKind::Option(inner) => {
                    format!("Option<{}>", type_to_rust(resolve, inner, in_macro))
                }
                _ => {
                    let name = to_pascal_case(
                        typedef
                            .name
                            .as_deref()
                            .unwrap_or_else(|| panic!("unnamed type: {:?}", typedef.kind)),
                    );
                    if in_macro {
                        format!("$crate::{name}")
                    } else {
                        name
                    }
                }
            }
        }
        other => panic!("unsupported WIT type: {other:?}"),
    }
}

/// Resolve a TypeId to its PascalCase Rust name.
fn resolve_type_name(resolve: &Resolve, id: wit_parser::TypeId) -> String {
    let typedef = &resolve.types[id];
    to_pascal_case(
        typedef
            .name
            .as_deref()
            .unwrap_or_else(|| panic!("unnamed type: {:?}", typedef.kind)),
    )
}

fn to_pascal_case(s: &str) -> String {
    s.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let mut out = String::new();
                    for uc in c.to_uppercase() {
                        out.push(uc);
                    }
                    out.extend(chars);
                    out
                }
            }
        })
        .collect()
}
