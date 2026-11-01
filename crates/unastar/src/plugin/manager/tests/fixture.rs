//! Real canonical components generated from the current WIT with controlled callback bodies.

use std::collections::HashMap;
use std::convert::Infallible;
use wasm_encoder::reencode::{self, Reencode};
use wasmparser::{ExternalKind, Parser, Payload, TypeRef};
use wasmtime::{Engine, component::Component};
use wit_parser::{ManglingAndAbi, Resolve, WasmExport, WasmExportKind, WorldItem};

/// Reencode only test function bodies and memory; preserve the canonical generated ABI.
struct Bodies {
    function: u32,
    overrides: HashMap<u32, String>,
}
impl Reencode for Bodies {
    type Error = Infallible;
    fn parse_function_body(
        &mut self,
        code: &mut wasm_encoder::CodeSection,
        original: wasmparser::FunctionBody<'_>,
    ) -> Result<(), reencode::Error<Infallible>> {
        let index = self.function;
        self.function += 1;
        if let Some(body) = self.overrides.get(&index) {
            let module =
                wat::parse_str(format!("(module (func {body}))")).expect("fixture body WAT");
            for payload in Parser::new(0).parse_all(&module) {
                if let Payload::CodeSectionEntry(body) = payload.expect("fixture code") {
                    code.raw(&module[body.range()]);
                }
            }
            Ok(())
        } else {
            reencode::utils::parse_function_body(self, code, original)
        }
    }
    fn parse_memory_section(
        &mut self,
        memories: &mut wasm_encoder::MemorySection,
        section: wasmparser::MemorySectionReader<'_>,
    ) -> Result<(), reencode::Error<Infallible>> {
        for memory in section {
            let mut memory = self.memory_type(memory.expect("fixture memory"))?;
            memory.minimum = 2;
            memories.memory(memory);
        }
        Ok(())
    }
}

/// Compile a redistributable component with current resource types and real host imports.
/// `@method-name@` tokens select generated imported function indices, never host pointers.
pub(super) fn component(engine: &Engine, bodies: &[(&str, String)]) -> Component {
    Component::new(engine, bytes(bodies)).unwrap()
}

/// Encode a complete component for tests that exercise disk discovery and loading.
pub(super) fn bytes(bodies: &[(&str, String)]) -> Vec<u8> {
    bytes_with_start(bodies, false)
}

/// Run the same zero-argument callback once as a core start and once as on-load.
pub(super) fn component_with_start(engine: &Engine, bodies: &[(&str, String)]) -> Component {
    Component::new(engine, bytes_with_start(bodies, true)).unwrap()
}

/// Optionally include a real instantiation start function in the canonical component.
fn bytes_with_start(bodies: &[(&str, String)], start: bool) -> Vec<u8> {
    let mut resolve = Resolve::default();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../unastar-api/wit");
    let (package, _) = resolve.push_dir(path).unwrap();
    let world = resolve
        .select_world(&[package], Some("unastar-plugin"))
        .unwrap();
    let mangling = ManglingAndAbi::Standard32;
    let module = wit_component::dummy_module(&resolve, world, mangling);
    let mut imports = HashMap::new();
    let mut exports = HashMap::new();
    let mut imported_functions = 0;
    for payload in Parser::new(0).parse_all(&module) {
        match payload.unwrap() {
            Payload::ImportSection(section) => {
                for import in section.into_imports() {
                    let import = import.unwrap();
                    if matches!(import.ty, TypeRef::Func(_)) {
                        imports.insert(import.name.to_owned(), imported_functions);
                        imported_functions += 1;
                    }
                }
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    if export.kind == ExternalKind::Func {
                        exports.insert(export.name.to_owned(), export.index);
                    }
                }
            }
            _ => {}
        }
    }
    let mut overrides = HashMap::new();
    let mut start_index = None;
    for (_, item) in &resolve.worlds[world].exports {
        let WorldItem::Function(function) = item else {
            continue;
        };
        let export = resolve.wasm_export_name(
            mangling,
            WasmExport::Func {
                interface: None,
                func: function,
                kind: WasmExportKind::Normal,
            },
        );
        let body = bodies.iter().find(|(name, _)| *name == function.name).map(|(_, body)| body.clone()).unwrap_or_else(|| match function.name.as_str() {
            "on-player-command" | "on-player-chat" | "on-block-break" | "on-block-place" | "on-player-interact-block" => "i32.const 1".into(),
            "on-player-admission" => "i32.const 32 i32.const 0 i32.store i32.const 32".into(),
            "on-player-quit" => "i32.const 0".into(),
            "on-player-move" => "i32.const 32 i32.const 1 i32.store i32.const 40 local.get 5 f64.store i32.const 48 local.get 6 f64.store i32.const 56 local.get 7 f64.store i32.const 32".into(),
            "on-player-damage" => legacy_damage_body("i32.const 32 i32.const 1 i32.store i32.const 36 local.get 4 f32.store i32.const 32"),
            "on-player-respawn" => "i32.const 64 i32.const 1 i32.store i32.const 72 local.get 2 f64.store i32.const 80 local.get 3 f64.store i32.const 88 local.get 4 f64.store i32.const 64".into(),
            _ => String::new(),
        });
        if function.name == "on-load" {
            start_index = Some(exports[&export]);
        }
        let mut body = body;
        // Large owned records indirect all parameters; resource prefixes follow
        // the canonical memory layout rather than pretending the pointer is a handle.
        let indirect = resolve
            .wasm_signature(wit_parser::abi::AbiVariant::GuestExport, function)
            .indirect_params;
        let mut sizes = wit_parser::SizeAlign::default();
        sizes.fill(&resolve);
        let mut offset = 0usize;
        let mut offsets = HashMap::new();
        for param in &function.params {
            offset = offset.next_multiple_of(sizes.align(&param.ty).align_wasm32());
            offsets.insert(param.name.as_str(), offset);
            offset += sizes.size(&param.ty).size_wasm32();
        }
        for (name, index, drop) in [("tx", 0, "world-tx_drop"), ("player", 1, "player_drop")] {
            if function.params.iter().any(|param| param.name == name) {
                if indirect {
                    body.push_str(&format!(
                        " local.get 0 i32.load offset={} call @{drop}@",
                        offsets[name]
                    ));
                } else {
                    body.push_str(&format!(" local.get {index} call @{drop}@"));
                }
            }
        }
        if function.params.iter().any(|param| param.name == "attacker") {
            if indirect {
                let at = offsets["attacker"];
                body.push_str(&format!(" local.get 0 i32.load8_u offset={at} if local.get 0 i32.load offset={} call @player_drop@ end",at+4));
            } else {
                body.push_str(" local.get 2 if local.get 3 call @player_drop@ end");
            }
        }
        for (name, index) in &imports {
            body = body.replace(&format!("@{name}@"), &index.to_string());
        }
        overrides.insert(exports[&export], body);
    }
    let realloc = resolve.wasm_export_name(mangling, WasmExport::Realloc);
    overrides.insert(
        exports[&realloc],
        r#"
        (local i32)
        local.get 3 i32.eqz
        if (result i32) i32.const 0 else
            i32.const 0 i32.load i32.eqz
            if i32.const 0 i32.const 4096 i32.store end
            i32.const 0 i32.load local.get 2 i32.const 1 i32.sub i32.add
            i32.const 0 local.get 2 i32.sub i32.and local.tee 4
            i32.const 0 local.get 4 local.get 3 i32.add i32.store
        end
    "#
        .into(),
    );
    let mut encoder = Bodies {
        function: imported_functions,
        overrides,
    };
    let mut encoded = wasm_encoder::Module::new();
    encoder
        .parse_core_module(&mut encoded, Parser::new(0), &module)
        .unwrap();
    let mut module = encoded.finish();
    if start {
        let mut output = wasm_encoder::Module::new();
        let mut inserted = false;
        for payload in Parser::new(0).parse_all(&module) {
            if let Some((id, range)) = payload.unwrap().as_section() {
                if !inserted && id > 7 {
                    output.section(&wasm_encoder::StartSection {
                        function_index: start_index.unwrap(),
                    });
                    inserted = true;
                }
                output.section(&wasm_encoder::RawSection {
                    id,
                    data: &module[range],
                });
            }
        }
        assert!(inserted);
        module = output.finish();
    }
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
    )
    .unwrap();
    wit_component::ComponentEncoder::default()
        .module(&module)
        .unwrap()
        .validate(true)
        .encode()
        .unwrap()
}

/// Resolve aliases before inspecting a canonical record layout.
fn unalias(resolve: &Resolve, mut ty: wit_parser::Type) -> wit_parser::Type {
    loop {
        if let wit_parser::Type::Id(id) = ty
            && let wit_parser::TypeDefKind::Type(inner) = resolve.types[id].kind
        {
            ty = inner;
            continue;
        }
        return ty;
    }
}

/// Locate a field in an indirect callback using current WIT canonical alignment and size.
pub(super) fn field_layout(export: &str, parameter: &str, fields: &[&str]) -> (usize, usize) {
    let mut resolve = Resolve::default();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../unastar-api/wit");
    let (package, _) = resolve.push_dir(path).unwrap();
    let world = resolve
        .select_world(&[package], Some("unastar-plugin"))
        .unwrap();
    let function = resolve.worlds[world]
        .exports
        .values()
        .find_map(|item| match item {
            WorldItem::Function(function) if function.name == export => Some(function),
            _ => None,
        })
        .unwrap();
    assert!(
        resolve
            .wasm_signature(wit_parser::abi::AbiVariant::GuestExport, function)
            .indirect_params
    );
    let mut sizes = wit_parser::SizeAlign::default();
    sizes.fill(&resolve);
    let mut offset = 0usize;
    for param in &function.params {
        offset = offset.next_multiple_of(sizes.align(&param.ty).align_wasm32());
        if param.name == parameter {
            let mut ty = param.ty;
            for name in fields {
                if *name == "ok" {
                    let wit_parser::Type::Id(id) = unalias(&resolve, ty) else {
                        panic!("not a result")
                    };
                    let wit_parser::TypeDefKind::Result(result) = &resolve.types[id].kind else {
                        panic!("not a result")
                    };
                    offset = (offset + 1).next_multiple_of(sizes.align(&ty).align_wasm32());
                    ty = result.ok.expect("result value");
                    continue;
                }
                if *name == "some" {
                    let wit_parser::Type::Id(id) = unalias(&resolve, ty) else {
                        panic!("not an option")
                    };
                    let wit_parser::TypeDefKind::Option(inner) = resolve.types[id].kind else {
                        panic!("not an option")
                    };
                    offset = (offset + 1).next_multiple_of(sizes.align(&inner).align_wasm32());
                    ty = inner;
                    continue;
                }
                let wit_parser::Type::Id(id) = unalias(&resolve, ty) else {
                    panic!("not a record")
                };
                let wit_parser::TypeDefKind::Record(record) = &resolve.types[id].kind else {
                    panic!("not a record")
                };
                let mut found = None;
                for field in &record.fields {
                    offset = offset.next_multiple_of(sizes.align(&field.ty).align_wasm32());
                    if field.name == *name {
                        found = Some(field.ty);
                        break;
                    }
                    offset += sizes.size(&field.ty).size_wasm32();
                }
                ty = found.expect("canonical field");
            }
            return (offset, sizes.size(&ty).size_wasm32());
        }
        offset += sizes.size(&param.ty).size_wasm32();
    }
    panic!("canonical parameter")
}

/// Preserve the native tuning while adapting older controlled scalar fixture bodies.
/// Scratch return memory stays below the fixture allocator's heap at 4096.
pub(super) fn legacy_damage_body(body: &str) -> String {
    let (tuning, size) = field_layout("on-player-damage", "proposal", &["tuning"]);
    let (player, _) = field_layout("on-player-damage", "player", &[]);
    let (attacker, _) = field_layout("on-player-damage", "attacker", &[]);
    let (cause, _) = field_layout("on-player-damage", "proposal", &["cause"]);
    let loads = [
        format!("local.get 0 i32.load offset={player}"),
        format!("local.get 0 i32.load8_u offset={attacker}"),
        format!("local.get 0 i32.load offset={}", attacker + 4),
        format!("local.get 0 i32.load8_u offset={cause}"),
        format!("local.get 0 f32.load offset={tuning}"),
    ];
    let mut body = body.replace("global.get $damage-calls", "i32.const 400 i32.load");
    for index in 0..loads.len() {
        body = body.replace(&format!("local.get {index}"), &format!("@legacy-{index}@"));
    }
    for (index, load) in loads.iter().enumerate() {
        body = body.replace(&format!("@legacy-{index}@"), load);
    }
    format!(
        "i32.const 520 local.get 0 i32.const {tuning} i32.add i32.const {size} memory.copy {body} drop i32.const 512 i32.const 32 i32.load8_u i32.store i32.const 520 i32.const 36 f32.load f32.store i32.const 512"
    )
}
