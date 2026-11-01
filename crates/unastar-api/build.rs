use anyhow::{Context, Result};
use std::env;
use std::fmt::Write;
use std::fs;
use std::path::PathBuf;
use wit_parser::{Handle, Resolve, Type, TypeDefKind, WorldItem};

fn main() -> Result<()> {
    // Bind generated handlers and rebuild tracking to the owning package's WIT.
    let wit_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("wit");
    println!("cargo:rerun-if-changed={}", wit_dir.display());

    let mut resolve = Resolve::default();
    let (pkg_id, sources) = resolve
        .push_dir(&wit_dir)
        .context("failed to parse WIT directory")?;
    for source in sources.paths() {
        println!("cargo:rerun-if-changed={}", source.display());
    }
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
        let ret = return_str(&resolve, func, false);
        let default = default_body(&resolve, func);
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
    writeln!(out, "    ($t:ty) => {{")?;
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
        let ret = return_str(&resolve, func, true);
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

    generate_handler(&mut out, &resolve, &funcs)?;

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
fn return_str(resolve: &Resolve, func: &wit_parser::Function, in_macro: bool) -> String {
    match &func.result {
        Some(ty) => format!(" -> {}", type_to_rust(resolve, ty, in_macro)),
        None => String::new(),
    }
}

/// Preserve matching inputs for optional decisions; allow boolean decisions.
fn default_body(resolve: &Resolve, func: &wit_parser::Function) -> String {
    match &func.result {
        Some(Type::Bool) => "true".into(),
        Some(Type::Id(id)) => {
            if let TypeDefKind::Option(inner) = &resolve.types[*id].kind
                && let Some(param) = func.params.iter().find(|param| param.ty == *inner)
            {
                return format!("Some({})", param.name.replace('-', "_"));
            }
            panic!("unsupported default return type in `{}`", func.name)
        }
        Some(_) => panic!("unsupported default return type in `{}`", func.name),
        None => String::new(),
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
                        format!("&$crate::raw::{name}")
                    } else {
                        format!("&raw::{name}")
                    }
                }
                TypeDefKind::Handle(Handle::Own(inner_id)) => {
                    let name = resolve_type_name(resolve, *inner_id);
                    if in_macro {
                        format!("$crate::raw::{name}")
                    } else {
                        format!("raw::{name}")
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

/// Whether a callback owns a transaction-scoped player argument.
fn has_player(func: &wit_parser::Function) -> bool {
    func.params.iter().any(|param| param.name == "player")
}

/// Translate canonical callback parameters into lifetime-bound Handler contexts.
fn handler_params(resolve: &Resolve, func: &wit_parser::Function) -> String {
    let player = has_player(func);
    let mut params = Vec::new();
    if player {
        params.push("ctx: &mut PlayerContext<'_>".into());
    } else if func.params.iter().any(|param| param.name == "tx") {
        params.push("tx: &WorldTx<'_>".into());
    }
    for param in &func.params {
        if param.name == "tx" || param.name == "player" {
            continue;
        }
        let name = param.name.replace('-', "_");
        let ty = if name == "attacker" {
            "Option<&Player<'_>>".into()
        } else if func.name == "on-player-damage" && name == "damage" {
            "&mut f32".into()
        } else if func.name == "on-player-respawn" && name == "position" {
            "&mut Vec3".into()
        } else {
            type_to_rust(resolve, &param.ty, false)
        };
        params.push(format!("{name}: {ty}"));
    }
    params.join(", ")
}

/// Generate static Handler methods and a canonical Guest bridge with isolated cancellation.
fn generate_handler(
    out: &mut String,
    resolve: &Resolve,
    funcs: &[&wit_parser::Function],
) -> Result<()> {
    writeln!(
        out,
        "/// Dragonfly-style scoped handlers. Store handles rather than live players or transactions."
    )?;
    writeln!(out, "#[allow(unused_variables)] pub trait Handler {{")?;
    for func in funcs {
        writeln!(
            out,
            "fn {}({}) {{}}",
            func.name.replace('-', "_"),
            handler_params(resolve, func)
        )?;
    }
    writeln!(out, "}}")?;
    writeln!(out, "/// Export one scoped Handler as a component.")?;
    writeln!(
        out,
        "#[macro_export] macro_rules! export_handler {{ ($t:ty) => {{ struct __UnastarHandlerBridge; impl $crate::Guest for __UnastarHandlerBridge {{"
    )?;
    for func in funcs {
        let method = func.name.replace('-', "_");
        writeln!(
            out,
            "fn {method}({}){} {{",
            params_str(resolve, func, true),
            return_str(resolve, func, true)
        )?;
        let has_tx = func.params.iter().any(|param| param.name == "tx");
        let has_player = has_player(func);
        if has_tx {
            writeln!(out, "let tx = $crate::WorldTx::__from_borrowed(tx);")?;
        }
        let cancellable = func.result.is_some();
        if has_player {
            writeln!(
                out,
                "let player = $crate::Player::__from_borrowed(player, &tx); let mut ctx = $crate::PlayerContext::__new(&tx, &player, {cancellable});"
            )?;
        }
        if func.params.iter().any(|param| param.name == "attacker") {
            writeln!(
                out,
                "let attacker = attacker.map(|raw| $crate::Player::__from_borrowed(raw, &tx));"
            )?;
        }
        if func.name == "on-player-damage" {
            writeln!(out, "let mut damage = damage;")?;
        }
        if func.name == "on-player-respawn" {
            writeln!(out, "let mut position = position;")?;
        }
        let mut args = Vec::new();
        if has_player {
            args.push("&mut ctx".to_string());
        } else if has_tx {
            args.push("&tx".to_string());
        }
        for param in &func.params {
            if param.name == "tx" || param.name == "player" {
                continue;
            }
            let name = param.name.replace('-', "_");
            args.push(if name == "attacker" {
                "attacker.as_ref()".into()
            } else if (func.name == "on-player-damage" && name == "damage")
                || (func.name == "on-player-respawn" && name == "position")
            {
                format!("&mut {name}")
            } else {
                name
            });
        }
        writeln!(
            out,
            "<$t as $crate::Handler>::{method}({});",
            args.join(", ")
        )?;
        match &func.result {
            Some(Type::Bool) => writeln!(out, "!ctx.is_cancelled()")?,
            Some(_) if func.name == "on-player-damage" => writeln!(
                out,
                "if ctx.is_cancelled() {{ None }} else {{ Some(damage) }}"
            )?,
            Some(_) if func.name == "on-player-respawn" => writeln!(
                out,
                "if ctx.is_cancelled() {{ None }} else {{ Some(position) }}"
            )?,
            Some(_) => anyhow::bail!("unsupported Handler result for {}", func.name),
            None => {}
        }
        writeln!(out, "}}")?;
    }
    writeln!(out, "}} $crate::export!(__UnastarHandlerBridge); }}; }}")?;
    Ok(())
}
