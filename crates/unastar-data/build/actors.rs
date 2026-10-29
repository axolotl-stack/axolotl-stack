use std::path::Path;
use unastar_packs::actor;
use unastar_packs::packs::{Family, PackStack};
use unastar_rules::filter::FilterRegistry;

pub fn write(pack: &Path, out: &Path) {
    let schema_dir = pack
        .parent()
        .expect("behavior_pack parent")
        .join("metadata/json_schemas/server/entity");
    println!("cargo:rerun-if-changed={}", schema_dir.display());
    let schema_files = actor::component_schema_files(&schema_dir).expect("actor component schemas");
    for input in &schema_files {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    crate::component_defaults::write(&schema_files, out);
    let mut parser = actor::ActorParser::from_schemas(
        FilterRegistry::vanilla(),
        schema_files.iter().map(|input| {
            let bytes = std::fs::read(input).unwrap_or_else(|e| panic!("{}: {e}", input.display()));
            unastar_packs::json::parse(&input.display().to_string(), &bytes)
        }),
    )
    .expect("actor parser registry");
    let version = parser.schema_version();
    let document_schema = schema_dir
        .join(format!("{}.{}.{}", version[0], version[1], version[2]))
        .join("ActorDocument.json");
    println!("cargo:rerun-if-changed={}", document_schema.display());
    let bytes = std::fs::read(&document_schema).expect("reading actor document schema");
    parser
        .register_document_schema(
            unastar_packs::json::parse(&document_schema.display().to_string(), &bytes)
                .expect("actor document schema JSON"),
        )
        .expect("actor document fields");
    let legacy_form = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("data/upstream/mojang/entity_forms/entity_component_definitions.form.json");
    println!("cargo:rerun-if-changed={}", legacy_form.display());
    let legacy_bytes = std::fs::read(&legacy_form).expect("reading legacy component form");
    parser
        .register_form(
            unastar_packs::json::parse(&legacy_form.display().to_string(), &legacy_bytes)
                .expect("legacy component form JSON"),
        )
        .expect("legacy component registrations");
    let registry = parser.to_schema();
    let names = registry["properties"]
        .as_object()
        .expect("actor component registry properties")
        .keys()
        .cloned()
        .collect();
    let forms = legacy_form.parent().expect("legacy form directory");
    crate::actor_components::write(&schema_files, forms, &names, out);
    std::fs::write(
        out.join("actor_component_registry.json"),
        serde_json::to_vec(&registry).expect("actor component registry serialization"),
    )
    .expect("writing actor component registry");
    let stack = PackStack::single_pack("bedrock-samples", pack);
    // Watch the directory as well as files: adding or removing a definition
    // must invalidate the build, not only changing a file already present.
    println!("cargo:rerun-if-changed={}", pack.join("entities").display());
    for input in stack.inputs(Family::Entities) {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    let resolved =
        actor::resolve_stack(&stack, &parser).unwrap_or_else(|e| panic!("actor definitions: {e}"));
    assert!(
        resolved.skipped.is_empty(),
        "vanilla actor files without identifiers: {:?}",
        resolved.skipped
    );
    assert!(
        !resolved.definitions.is_empty(),
        "vanilla actor definitions are empty"
    );
    let docs: Vec<_> = resolved
        .definitions
        .values()
        .map(|d| d.definition.to_json())
        .collect();
    let data = serde_json::to_vec(&docs).expect("normalised actor definitions serialize");
    std::fs::write(out.join("actor_definitions.json"), data).expect("writing actor definitions");
}
