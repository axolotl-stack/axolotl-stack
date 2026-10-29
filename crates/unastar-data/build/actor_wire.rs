//! Compile retained native observations, not authored actor defaults.

use bytes::Buf;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use unastar_packs::nbt::{Encoding, Reader, Tag};
use valentine::bedrock::codec::{BedrockCodec, BedrockSized, VarUInt};
use valentine::bedrock::version::v1_26_51::{McpePacketArgs, McpePacketData};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read(path: &Path) -> Vec<u8> {
    println!("cargo:rerun-if-changed={}", path.display());
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("missing string {key}: {value}"))
}

struct Capture {
    manifest: Value,
    manifest_sha256: String,
    files: BTreeMap<String, Value>,
}

impl Capture {
    fn open(root: &Path) -> Self {
        let bytes = read(&root.join("manifest.json"));
        let manifest: Value = serde_json::from_slice(&bytes).expect("capture manifest JSON");
        assert_eq!(
            manifest["format"].as_u64(),
            Some(1),
            "capture manifest format"
        );
        assert_eq!(
            manifest["server_version"].as_str(),
            Some("1.26.51"),
            "capture server version"
        );
        assert_eq!(
            manifest["protocol_version"].as_u64(),
            Some(2193),
            "capture protocol version"
        );
        let mut files = BTreeMap::new();
        for file in manifest["files"].as_array().expect("capture files") {
            let path = string(file, "path");
            assert!(
                Path::new(path)
                    .components()
                    .all(|p| matches!(p, Component::Normal(_)))
                    && !path.contains('\\'),
                "unsafe capture path: {path}"
            );
            assert!(
                files.insert(path.to_owned(), file.clone()).is_none(),
                "duplicate capture path: {path}"
            );
        }
        Self {
            manifest,
            manifest_sha256: hash(&bytes),
            files,
        }
    }

    fn consume(
        &self,
        root: &Path,
        relative: &str,
        sources: &mut BTreeMap<String, Value>,
    ) -> Vec<u8> {
        let record = self.files.get(relative).unwrap_or_else(|| {
            panic!(
                "{}: source not in capture manifest: {relative}",
                root.display()
            )
        });
        let bytes = read(&root.join(relative));
        assert_eq!(
            record["bytes"].as_u64(),
            Some(bytes.len() as u64),
            "{relative}: capture byte count"
        );
        let digest = hash(&bytes);
        assert_eq!(
            string(record, "sha256"),
            digest,
            "{relative}: capture SHA256"
        );
        let path = format!(
            "{}/{relative}",
            root.file_name()
                .expect("capture role")
                .to_str()
                .expect("capture role UTF-8")
        );
        assert!(
            sources
                .insert(
                    path.clone(),
                    json!({"path": path, "bytes": bytes.len(), "sha256": digest})
                )
                .is_none(),
            "source consumed twice: {relative}"
        );
        bytes
    }
}

/// Captures omit the batch length. Add it with the shared scalar codec and
/// chain the original bytes, so the generated header/payload decoder sees its
/// documented [Length][Header][Body] layout without copying the packet.
fn decode(bytes: &[u8]) -> Result<McpePacketData, String> {
    let length = VarUInt(u32::try_from(bytes.len()).map_err(|e| e.to_string())?);
    let mut prefix = [0; 5];
    length
        .encode(&mut &mut prefix[..])
        .map_err(|e| e.to_string())?;
    let mut framed = (&prefix[..length.encoded_size()]).chain(bytes);
    let (_, packet) =
        McpePacketData::decode_inner(&mut framed, McpePacketArgs).map_err(|e| format!("{e:?}"))?;
    if framed.has_remaining() {
        return Err("trailing captured packet bytes".into());
    }
    Ok(packet)
}

fn observe_type(
    types: &mut BTreeMap<String, i32>,
    declared: &str,
    observed: i32,
) -> Result<(), String> {
    if let Some(previous) = types.get(declared)
        && *previous != observed
    {
        return Err(format!(
            "inconsistent native property type {declared}: {previous} versus {observed}"
        ));
    }
    if let Some((other, _)) = types
        .iter()
        .find(|(name, code)| name.as_str() != declared && **code == observed)
    {
        return Err(format!(
            "native property type {observed} used for both {other} and {declared}"
        ));
    }
    types.insert(declared.to_owned(), observed);
    Ok(())
}

pub fn write(captures: &Path, out: &Path) {
    let operator_root = captures.join("operator");
    let member_root = captures.join("member");
    let operator = Capture::open(&operator_root);
    let member = Capture::open(&member_root);
    let mut sources = BTreeMap::new();
    let mut observations = Vec::new();
    let mut consumed_summons = BTreeSet::new();
    for summon in operator.manifest["summons"]
        .as_array()
        .expect("operator summons")
    {
        let identifier = string(summon, "identifier");
        let local = identifier
            .strip_prefix("minecraft:")
            .expect("vanilla summon namespace");
        assert!(
            !local.is_empty()
                && local
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'),
            "unsafe summon identifier: {identifier}"
        );
        let relative = format!("summons/{local}.bin");
        assert!(
            consumed_summons.insert(relative.clone()),
            "duplicate summon: {identifier}"
        );
        let record = &operator.files[&relative];
        assert_eq!(
            record["packet"].as_str(),
            Some("AddActorPacket"),
            "{relative}: packet manifest name"
        );
        assert_eq!(record["seq"], summon["seq"], "{relative}: summon sequence");
        let bytes = operator.consume(&operator_root, &relative, &mut sources);
        let McpePacketData::AddActorPacket(actor) =
            decode(&bytes).unwrap_or_else(|e| panic!("{relative}: {e}"))
        else {
            panic!("{relative}: expected AddActorPacket");
        };
        assert_eq!(
            &*actor.actor_type,
            string(summon, "actor_type"),
            "{relative}: actual actor alias"
        );
        let mut names = BTreeSet::new();
        let attributes: Vec<_> = actor.attributes_list.iter().map(|a| {
            assert!(names.insert(&*a.attribute_name), "{identifier}: duplicate native attribute");
            assert!(a.min_value.is_finite() && a.current_value.is_finite() && a.max_value.is_finite(), "{identifier}: nonfinite native attribute");
            assert!(a.min_value <= a.max_value, "{identifier}: reversed native attribute bounds");
            json!({"name": &*a.attribute_name, "min": a.min_value, "observed_current": a.current_value, "max": a.max_value})
        }).collect();
        observations.push(json!({"identifier": identifier, "actor_type": &*actor.actor_type, "attributes": attributes, "packet_sha256": hash(&bytes)}));
    }
    let retained_summons: BTreeSet<_> = operator
        .files
        .keys()
        .filter(|p| p.starts_with("summons/") && p.ends_with(".bin"))
        .cloned()
        .collect();
    assert_eq!(
        consumed_summons, retained_summons,
        "unmatched retained summon binaries"
    );
    observations.sort_by(|a, b| string(a, "identifier").cmp(string(b, "identifier")));

    let definitions_bytes = std::fs::read(out.join("actor_definitions.json"))
        .expect("canonical actor definitions must be generated before actor wire observations");
    let definitions: Vec<Value> =
        serde_json::from_slice(&definitions_bytes).expect("canonical actor definitions JSON");
    let mut descriptions = BTreeMap::new();
    for definition in &definitions {
        let description = &definition["minecraft:entity"]["description"];
        assert!(
            descriptions
                .insert(string(description, "identifier"), description)
                .is_none(),
            "duplicate canonical actor identifier"
        );
    }
    let sequence_bytes = member.consume(&member_root, "join-sequence.json", &mut sources);
    let sequence: Vec<Value> =
        serde_json::from_slice(&sequence_bytes).expect("member join sequence JSON");
    let mut previous_seq = None;
    let mut descriptor_order = Vec::new();
    let mut descriptor_identifiers = BTreeSet::new();
    let mut descriptor_paths = BTreeSet::new();
    let mut property_types = BTreeMap::new();
    for entry in &sequence {
        let seq = entry["seq"].as_u64().expect("join sequence number");
        assert!(
            previous_seq.is_none_or(|previous| seq > previous),
            "member join sequence must be strictly ordered"
        );
        previous_seq = Some(seq);
        if entry["packet"].as_str() != Some("SyncActorPropertyPacket") {
            continue;
        }
        let relative = format!("join/{seq:05}-SyncActorPropertyPacket.bin");
        descriptor_paths.insert(relative.clone());
        let record = member
            .files
            .get(&relative)
            .unwrap_or_else(|| panic!("missing property packet in member manifest: {relative}"));
        assert_eq!(
            record["packet"], entry["packet"],
            "{relative}: packet manifest name"
        );
        assert_eq!(
            record["seq"], entry["seq"],
            "{relative}: packet manifest sequence"
        );
        assert_eq!(
            record["bytes"], entry["bytes"],
            "{relative}: join sequence byte count"
        );
        let bytes = member.consume(&member_root, &relative, &mut sources);
        let McpePacketData::SyncActorPropertyPacket(packet) =
            decode(&bytes).unwrap_or_else(|e| panic!("{relative}: {e}"))
        else {
            panic!("{relative}: expected SyncActorPropertyPacket");
        };
        let mut reader = Reader::new(
            &packet.property_data.0,
            Encoding::NetworkLittleEndian,
            &relative,
        );
        let (_, root) = reader
            .read_root()
            .unwrap_or_else(|e| panic!("{relative}: {e}"));
        assert!(
            reader.is_empty(),
            "{relative}: trailing descriptor NBT bytes"
        );
        let identifier = root
            .get("type")
            .and_then(Tag::as_str)
            .expect("native descriptor actor identifier");
        assert!(
            descriptor_identifiers.insert(identifier.to_owned()),
            "duplicate native property descriptor: {identifier}"
        );
        descriptor_order.push(identifier.to_owned());
        let description = descriptions.get(identifier).unwrap_or_else(|| {
            panic!("native descriptor has no canonical actor definition: {identifier}")
        });
        let properties = root
            .get("properties")
            .and_then(Tag::as_list)
            .expect("native descriptor property list");
        let mut names = BTreeSet::new();
        for property in properties {
            let name = property
                .get("name")
                .and_then(Tag::as_str)
                .expect("native property name");
            assert!(
                names.insert(name),
                "{identifier}: duplicate native property {name}"
            );
            let observed = match property.get("type") {
                Some(Tag::Int(code)) => *code,
                _ => panic!("{identifier}/{name}: native property type must be an int"),
            };
            let declared = description["properties"][name]["type"]
                .as_str()
                .unwrap_or_else(|| {
                    panic!("{identifier}/{name}: missing canonical declared property type")
                });
            observe_type(&mut property_types, declared, observed)
                .unwrap_or_else(|e| panic!("{identifier}/{name}: {e}"));
        }
        let declared_synced: BTreeSet<_> = description["properties"]
            .as_object()
            .expect("canonical descriptor properties")
            .iter()
            .filter(|(_, p)| p["client_sync"].as_bool() == Some(true))
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(
            names, declared_synced,
            "{identifier}: native and canonical client-synced property names differ"
        );
    }
    let retained_descriptors: BTreeSet<_> = member
        .files
        .keys()
        .filter(|p| p.starts_with("join/") && p.ends_with("-SyncActorPropertyPacket.bin"))
        .cloned()
        .collect();
    assert_eq!(
        descriptor_paths, retained_descriptors,
        "unmatched retained property descriptor binaries"
    );
    let catalog = json!({
        "server_version": operator.manifest["server_version"],
        "protocol_version": operator.manifest["protocol_version"],
        "operator_manifest_sha256": operator.manifest_sha256,
        "member_manifest_sha256": member.manifest_sha256,
        "actor_definitions_sha256": hash(&definitions_bytes),
        "sources": sources.into_values().collect::<Vec<_>>(),
        "observations": observations,
        "property_descriptor_order": descriptor_order,
        "property_types": property_types,
    });
    std::fs::write(
        out.join("actor_wire.json"),
        serde_json::to_vec(&catalog).expect("actor wire catalog serialization"),
    )
    .expect("writing actor wire catalog");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_unframed_packets_preserve_actor_values_and_property_identity() {
        let bytes = include_bytes!("../../bds-extractor/captures/1.26.51/operator/summons/cow.bin");
        let McpePacketData::AddActorPacket(actor) = decode(bytes).unwrap() else {
            panic!("native actor packet")
        };
        assert_eq!(actor.actor_type, "minecraft:cow");
        assert_eq!(actor.attributes_list[0].attribute_name, "minecraft:health");
        assert_eq!(actor.attributes_list[0].current_value, 10.0);
        let bytes = include_bytes!(
            "../../bds-extractor/captures/1.26.51/member/join/00029-SyncActorPropertyPacket.bin"
        );
        let McpePacketData::SyncActorPropertyPacket(packet) = decode(bytes).unwrap() else {
            panic!("native property packet")
        };
        let mut reader = Reader::new(
            &packet.property_data.0,
            Encoding::NetworkLittleEndian,
            "native cow properties",
        );
        let (_, root) = reader.read_root().unwrap();
        assert_eq!(
            root.get("type").and_then(Tag::as_str),
            Some("minecraft:cow")
        );
    }

    #[test]
    fn malformed_capture_is_not_silently_accepted() {
        let mut descriptor = include_bytes!(
            "../../bds-extractor/captures/1.26.51/member/join/00029-SyncActorPropertyPacket.bin"
        )
        .to_vec();
        descriptor.pop();
        assert!(decode(&descriptor).is_err());
        let mut descriptor = include_bytes!(
            "../../bds-extractor/captures/1.26.51/member/join/00029-SyncActorPropertyPacket.bin"
        )
        .to_vec();
        descriptor.push(0);
        assert!(decode(&descriptor).is_err());
    }

    #[test]
    fn consumed_files_require_presence_size_and_actual_digest() {
        let root = tempfile::tempdir().unwrap();
        let bytes = [1, 2, 3];
        let relative = "fixture.bin";
        let mut capture = Capture {
            manifest: Value::Null,
            manifest_sha256: String::new(),
            files: BTreeMap::from([(
                relative.to_owned(),
                json!({"bytes": bytes.len(), "sha256": hash(&bytes)}),
            )]),
        };
        let consume =
            |capture: &Capture| capture.consume(root.path(), relative, &mut BTreeMap::new());
        assert!(std::panic::catch_unwind(|| consume(&capture)).is_err());
        std::fs::write(root.path().join(relative), bytes).unwrap();
        assert_eq!(consume(&capture), bytes);
        std::fs::write(root.path().join(relative), [1, 2]).unwrap();
        assert!(std::panic::catch_unwind(|| consume(&capture)).is_err());
        std::fs::write(root.path().join(relative), [1, 2, 4]).unwrap();
        assert!(std::panic::catch_unwind(|| consume(&capture)).is_err());
        capture.files.clear();
        assert!(std::panic::catch_unwind(|| consume(&capture)).is_err());
    }

    #[test]
    fn mappings_reject_inconsistent_native_type_pairs() {
        let mut types = BTreeMap::new();
        observe_type(&mut types, "fixture", 7).unwrap();
        observe_type(&mut types, "fixture", 7).unwrap();
        assert!(observe_type(&mut types, "fixture", 8).is_err());
        assert!(observe_type(&mut types, "other", 7).is_err());
    }

    #[test]
    fn native_schema_membership_mismatch_cannot_publish_a_catalog() {
        let out = tempfile::tempdir().unwrap();
        let mut definitions: Vec<Value> =
            serde_json::from_slice(crate::actor_definitions::VANILLA_JSON).unwrap();
        let cow = definitions
            .iter_mut()
            .find(|definition| {
                definition["minecraft:entity"]["description"]["identifier"] == "minecraft:cow"
            })
            .unwrap();
        cow["minecraft:entity"]["description"]["properties"]["minecraft:climate_variant"]["client_sync"] =
            Value::Bool(false);
        std::fs::write(
            out.path().join("actor_definitions.json"),
            serde_json::to_vec(&definitions).unwrap(),
        )
        .unwrap();
        let captures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../bds-extractor/captures/1.26.51");
        assert!(std::panic::catch_unwind(|| write(&captures, out.path())).is_err());
        assert!(!out.path().join("actor_wire.json").exists());
    }
}
