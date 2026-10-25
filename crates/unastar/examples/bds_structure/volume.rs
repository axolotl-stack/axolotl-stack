//! Validation and lossless JSON views for persisted Bedrock structure NBT.
//!
//! Format/axis reference: https://wiki.bedrock.dev/nbt/mcstructure . Z changes
//! fastest, then Y, then X; this is NOT the subchunk XZY layout.
//! Native persisted format 2 uses one or two IntArray layers; mcstructure
//! format 1 uses exactly two List<Int> layers. Absent layers stay absent.
//! The codegen reader deliberately normalizes compound order and does not keep
//! an empty list's declared element ID. Consequently the original NBT bytes
//! accompany the typed view and are authoritative for exact reconstruction.

use std::fmt;

use anyhow::{Context, Result, bail, ensure};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use unastar_packs::nbt::{Encoding, Reader, Tag};

pub const INDEX_ORDER: &str = "index = (x * size_y + y) * size_z + z; z changes fastest";

pub struct Volume {
    pub root_name: String,
    pub root: Tag,
    pub format_version: i32,
    pub layer_count: usize,
    pub size: [i32; 3],
    pub world_origin: [i32; 3],
    pub block_count: usize,
}

impl Volume {
    pub fn read(bytes: &[u8], origin: &str) -> Result<Self> {
        let mut reader = Reader::new(bytes, Encoding::LittleEndian, origin);
        let (root_name, root) = reader
            .read_root()
            .with_context(|| format!("decode {origin}"))?;
        ensure!(
            reader.is_empty(),
            "{origin}: trailing bytes after structure root"
        );
        let ValidatedShape {
            size,
            world_origin,
            block_count,
            format_version,
            layer_count,
        } = validate(&root).with_context(|| format!("invalid structure {origin}"))?;
        Ok(Self {
            root_name,
            root,
            format_version,
            layer_count,
            size,
            world_origin,
            block_count,
        })
    }
}

fn child<'a>(tag: &'a Tag, key: &str) -> Result<&'a Tag> {
    tag.get(key)
        .with_context(|| format!("missing required compound child {key:?}"))
}

fn compound(tag: &Tag) -> Result<&std::collections::BTreeMap<String, Tag>> {
    tag.as_compound().context("expected NBT Compound")
}

fn list(tag: &Tag) -> Result<&[Tag]> {
    tag.as_list().context("expected NBT List")
}

fn int(tag: &Tag) -> Result<i32> {
    match tag {
        Tag::Int(value) => Ok(*value),
        _ => bail!("expected NBT Int"),
    }
}

fn int3(tag: &Tag) -> Result<[i32; 3]> {
    let values = list(tag)?;
    ensure!(
        values.len() == 3,
        "expected exactly three NBT Int coordinates"
    );
    Ok([int(&values[0])?, int(&values[1])?, int(&values[2])?])
}

enum LayerIndices<'a> {
    List(&'a [Tag]),
    IntArray(&'a [i32]),
}

impl<'a> LayerIndices<'a> {
    fn read(tag: &'a Tag, version: i32) -> Result<Self> {
        match (version, tag) {
            (1, Tag::List(indices)) => Ok(Self::List(indices)),
            (2, Tag::IntArray(indices)) => Ok(Self::IntArray(indices)),
            (1, _) => bail!("format 1 layer must be NBT List<Int>"),
            (2, _) => bail!("format 2 layer must be NBT IntArray"),
            _ => bail!("unsupported structure format_version {version}"),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::List(indices) => indices.len(),
            Self::IntArray(indices) => indices.len(),
        }
    }

    fn get(&self, position: usize) -> Result<i32> {
        match self {
            Self::List(indices) => int(&indices[position]),
            Self::IntArray(indices) => Ok(indices[position]),
        }
    }
}

#[derive(Debug)]
struct ValidatedShape {
    size: [i32; 3],
    world_origin: [i32; 3],
    block_count: usize,
    format_version: i32,
    layer_count: usize,
}

fn validate(root: &Tag) -> Result<ValidatedShape> {
    compound(root).context("root")?;
    let version = int(child(root, "format_version")?).context("format_version")?;
    ensure!(
        matches!(version, 1 | 2),
        "unsupported structure format_version {version}; expected 1 or 2"
    );
    let size = int3(child(root, "size")?).context("size")?;
    let world_origin =
        int3(child(root, "structure_world_origin")?).context("structure_world_origin")?;
    let mut block_count = 1usize;
    for (axis, extent) in size.iter().copied().enumerate() {
        ensure!(
            extent > 0,
            "size axis {axis} must be positive, got {extent}"
        );
        block_count = block_count
            .checked_mul(extent as usize)
            .context("volume size overflow")?;
        ensure!(
            world_origin[axis].checked_add(extent - 1).is_some(),
            "world bounds overflow i32 on axis {axis}"
        );
    }

    let structure = child(root, "structure")?;
    compound(structure).context("structure")?;
    let layers = list(child(structure, "block_indices")?).context("block_indices")?;
    ensure!(
        match version {
            1 => layers.len() == 2,
            2 => matches!(layers.len(), 1 | 2),
            _ => unreachable!("validated structure format version"),
        },
        "format {version} block_indices must contain {}",
        if version == 1 {
            "exactly two layers"
        } else {
            "one or two layers"
        }
    );
    // Scan the volume once, even when the template has alternate palettes.
    let mut maxima: [Option<(usize, i32)>; 2] = [None; 2];
    for (layer_index, layer) in layers.iter().enumerate() {
        let indices = LayerIndices::read(layer, version)
            .with_context(|| format!("block_indices[{layer_index}]"))?;
        ensure!(
            indices.len() == block_count,
            "block_indices[{layer_index}] has {} entries, expected full volume {block_count}",
            indices.len()
        );
        for position in 0..indices.len() {
            let index = indices
                .get(position)
                .with_context(|| format!("block_indices[{layer_index}][{position}]"))?;
            ensure!(
                index >= -1,
                "block_indices[{layer_index}][{position}] = {index} is outside palette domain; only -1 is void"
            );
            if index >= 0 && maxima[layer_index].is_none_or(|(_, maximum)| index > maximum) {
                maxima[layer_index] = Some((position, index));
            }
        }
    }
    let entities = list(child(structure, "entities")?).context("entities")?;
    for (index, entity) in entities.iter().enumerate() {
        compound(entity).with_context(|| format!("entities[{index}]"))?;
    }
    let palettes = compound(child(structure, "palette")?).context("palette")?;
    ensure!(
        palettes.contains_key("default"),
        "missing required palette.default"
    );
    // Every palette shares the same indices. Preserve alternate palettes too,
    // but never let a valid default palette conceal a truncated alternate one.
    for (name, palette) in palettes {
        compound(palette).with_context(|| format!("palette.{name}"))?;
        let blocks = list(child(palette, "block_palette")?)
            .with_context(|| format!("palette.{name}.block_palette"))?;
        for (index, block) in blocks.iter().enumerate() {
            compound(block).with_context(|| format!("palette.{name}[{index}]"))?;
            ensure!(
                child(block, "name")?.as_str().is_some(),
                "palette.{name}[{index}].name must be NBT String"
            );
            compound(child(block, "states")?)
                .with_context(|| format!("palette.{name}[{index}].states"))?;
            int(child(block, "version")?)
                .with_context(|| format!("palette.{name}[{index}].version"))?;
        }
        for (layer_index, maximum) in maxima[..layers.len()].iter().enumerate() {
            if let Some((position, index)) = maximum {
                ensure!(
                    (*index as usize) < blocks.len(),
                    "block_indices[{layer_index}][{position}] = {index} is outside palette {name:?} ({} entries)",
                    blocks.len()
                );
            }
        }
        let positions = compound(child(palette, "block_position_data")?)
            .with_context(|| format!("palette.{name}.block_position_data"))?;
        for (position, data) in positions {
            let index: usize = position.parse().with_context(|| {
                format!("palette.{name}.block_position_data key {position:?} is not an index")
            })?;
            ensure!(
                index < block_count,
                "block_position_data index {index} is outside volume {block_count}"
            );
            compound(data).with_context(|| format!("block_position_data.{position}"))?;
            if let Some(entity) = data.get("block_entity_data") {
                compound(entity)
                    .with_context(|| format!("block_position_data.{position}.block_entity_data"))?;
                if version == 2 {
                    // Native persisted actors use absolute world coordinates.
                    // Format 1 retains its existing coordinate semantics.
                    let local = local_position(size, index)?;
                    for (axis, coordinate) in ["x", "y", "z"].iter().enumerate() {
                        let actual = int(child(entity, coordinate)?).with_context(|| {
                            format!("block_position_data.{position}.block_entity_data.{coordinate}")
                        })?;
                        let expected = world_origin[axis] + local[axis];
                        ensure!(
                            actual == expected,
                            "block_position_data.{position}.block_entity_data.{coordinate} = {actual} does not match index {index} world coordinate {expected}"
                        );
                    }
                }
            }
        }
    }
    Ok(ValidatedShape {
        size,
        world_origin,
        block_count,
        format_version: version,
        layer_count: layers.len(),
    })
}

/// Recover local XYZ from a flat structure index (Z, then Y, then X).
/// Used in exported corner coordinates so consumers have a concrete axis check.
pub fn local_position(size: [i32; 3], index: usize) -> Result<[i32; 3]> {
    ensure!(
        size.iter().all(|&n| n > 0),
        "all size axes must be positive"
    );
    let [sx, sy, sz] = size.map(|n| n as usize);
    let yz = sy.checked_mul(sz).context("YZ extent overflow")?;
    let count = sx.checked_mul(yz).context("volume size overflow")?;
    ensure!(
        index < count,
        "flat index {index} is outside volume {count}"
    );
    Ok([
        (index / yz) as i32,
        ((index / sz) % sy) as i32,
        (index % sz) as i32,
    ])
}

/// Exact source bytes, without building a second full-volume hex string.
pub struct Hex<'a>(pub &'a [u8]);

impl fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Serialize for Hex<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

fn tag_type(tag: &Tag) -> &'static str {
    match tag {
        Tag::Byte(_) => "byte",
        Tag::Short(_) => "short",
        Tag::Int(_) => "int",
        Tag::Long(_) => "long",
        Tag::Float(_) => "float",
        Tag::Double(_) => "double",
        Tag::ByteArray(_) => "byte_array",
        Tag::String(_) => "string",
        Tag::List(_) => "list",
        Tag::Compound(_) => "compound",
        Tag::IntArray(_) => "int_array",
        Tag::LongArray(_) => "long_array",
    }
}

/// Typed semantic view; exact wire metadata remains in raw_little_endian_hex.
/// Longs use decimal strings (also in long arrays) for JavaScript-safe i64.
/// Floats use IEEE-754 hex bits, including negative zero and NaN payloads.
pub struct Typed<'a>(pub &'a Tag);

impl Serialize for Typed<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", tag_type(self.0))?;
        match self.0 {
            Tag::Byte(value) => map.serialize_entry("value", value)?,
            Tag::Short(value) => map.serialize_entry("value", value)?,
            Tag::Int(value) => map.serialize_entry("value", value)?,
            Tag::Long(value) => map.serialize_entry("value", &Decimal(value))?,
            Tag::Float(value) => {
                map.serialize_entry("bits", &FloatBits(value.to_bits() as u64, 8))?
            }
            Tag::Double(value) => map.serialize_entry("bits", &FloatBits(value.to_bits(), 16))?,
            Tag::ByteArray(value) => map.serialize_entry("value", value)?,
            Tag::String(value) => map.serialize_entry("value", value)?,
            Tag::List(value) => {
                // Reader::Tag does not retain the wire ID for empty lists.
                // Null here is explicit, not an invented TAG_End assumption.
                map.serialize_entry("element_type", &value.first().map(tag_type))?;
                map.serialize_entry("value", &TypedList(value))?;
            }
            Tag::Compound(value) => map.serialize_entry("value", &TypedCompound(value))?,
            Tag::IntArray(value) => map.serialize_entry("value", value)?,
            Tag::LongArray(value) => map.serialize_entry("value", &LongArray(value))?,
        }
        map.end()
    }
}

struct Decimal<'a>(&'a i64);
impl Serialize for Decimal<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self.0)
    }
}

struct FloatBits(u64, usize);
impl fmt::Display for FloatBits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:0width$x}", self.0, width = self.1)
    }
}
impl Serialize for FloatBits {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

struct TypedList<'a>(&'a [Tag]);
impl Serialize for TypedList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for tag in self.0 {
            seq.serialize_element(&Typed(tag))?;
        }
        seq.end()
    }
}

struct TypedCompound<'a>(&'a std::collections::BTreeMap<String, Tag>);
impl Serialize for TypedCompound<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, tag) in self.0 {
            map.serialize_entry(name, &Typed(tag))?;
        }
        map.end()
    }
}

struct LongArray<'a>(&'a [i64]);
impl Serialize for LongArray<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            seq.serialize_element(&Decimal(value))?;
        }
        seq.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn object(items: &[(&str, Tag)]) -> Tag {
        Tag::Compound(
            items
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone()))
                .collect(),
        )
    }

    fn valid_root() -> Tag {
        object(&[
            ("format_version", Tag::Int(1)),
            (
                "size",
                Tag::List(vec![Tag::Int(2), Tag::Int(3), Tag::Int(4)]),
            ),
            (
                "structure_world_origin",
                Tag::List(vec![Tag::Int(-7), Tag::Int(-64), Tag::Int(9)]),
            ),
            (
                "structure",
                object(&[
                    (
                        "block_indices",
                        Tag::List(vec![
                            Tag::List(vec![Tag::Int(0); 24]),
                            Tag::List(vec![Tag::Int(-1); 24]),
                        ]),
                    ),
                    ("entities", Tag::List(vec![])),
                    (
                        "palette",
                        object(&[(
                            "default",
                            object(&[
                                (
                                    "block_palette",
                                    Tag::List(vec![object(&[
                                        ("name", Tag::String("minecraft:air".to_owned())),
                                        ("states", Tag::Compound(BTreeMap::new())),
                                        ("version", Tag::Int(18168865)),
                                    ])]),
                                ),
                                ("block_position_data", Tag::Compound(BTreeMap::new())),
                            ]),
                        )]),
                    ),
                ]),
            ),
        ])
    }

    fn field_mut<'a>(tag: &'a mut Tag, path: &[&str]) -> &'a mut Tag {
        if path.is_empty() {
            return tag;
        }
        let Tag::Compound(map) = tag else {
            panic!("test fixture compound");
        };
        field_mut(
            map.get_mut(path[0]).expect("test fixture child"),
            &path[1..],
        )
    }

    fn valid_v2_root(layer_count: usize) -> Tag {
        let mut root = valid_root();
        *field_mut(&mut root, &["format_version"]) = Tag::Int(2);
        *field_mut(&mut root, &["structure", "block_indices"]) = Tag::List(
            (0..layer_count)
                .map(|layer| Tag::IntArray(vec![if layer == 0 { 0 } else { -1 }; 24]))
                .collect(),
        );
        root
    }

    // Fixture-only encoding of the tag kinds used below, so Volume::read sees
    // real little-endian IntArray lengths/payloads rather than just typed tags.
    fn little_endian_fixture(root: &Tag) -> Vec<u8> {
        fn id(tag: &Tag) -> u8 {
            match tag {
                Tag::Int(_) => 3,
                Tag::String(_) => 8,
                Tag::List(_) => 9,
                Tag::Compound(_) => 10,
                Tag::IntArray(_) => 11,
                _ => panic!("unsupported fixture tag"),
            }
        }
        fn string(out: &mut Vec<u8>, value: &str) {
            out.extend_from_slice(&(value.len() as u16).to_le_bytes());
            out.extend_from_slice(value.as_bytes());
        }
        fn payload(out: &mut Vec<u8>, tag: &Tag) {
            match tag {
                Tag::Int(value) => out.extend_from_slice(&value.to_le_bytes()),
                Tag::String(value) => string(out, value),
                Tag::List(values) => {
                    out.push(values.first().map_or(0, id));
                    out.extend_from_slice(&(values.len() as i32).to_le_bytes());
                    for value in values {
                        payload(out, value);
                    }
                }
                Tag::Compound(values) => {
                    for (name, value) in values {
                        out.push(id(value));
                        string(out, name);
                        payload(out, value);
                    }
                    out.push(0);
                }
                Tag::IntArray(values) => {
                    out.extend_from_slice(&(values.len() as i32).to_le_bytes());
                    for value in values {
                        out.extend_from_slice(&value.to_le_bytes());
                    }
                }
                _ => panic!("unsupported fixture tag"),
            }
        }
        let mut bytes = vec![10, 0, 0];
        payload(&mut bytes, root);
        bytes
    }

    #[test]
    fn native_v2_preserves_present_layers_and_typed_arrays() {
        for layer_count in [1, 2] {
            let root = valid_v2_root(layer_count);
            let bytes = little_endian_fixture(&root);
            let volume = Volume::read(&bytes, "native-v2-fixture").unwrap();
            assert_eq!(volume.format_version, 2);
            assert_eq!(volume.layer_count, layer_count);
            assert_eq!(volume.block_count, 24);
            assert_eq!(volume.root, root);
            let typed = serde_json::to_value(Typed(&volume.root)).unwrap();
            let indices = &typed["value"]["structure"]["value"]["block_indices"];
            assert_eq!(indices["element_type"], "int_array");
            assert_eq!(indices["value"].as_array().unwrap().len(), layer_count);
            assert_eq!(indices["value"][0]["type"], "int_array");
            assert_eq!(indices["value"][0]["value"].as_array().unwrap().len(), 24);
            if layer_count == 2 {
                assert_eq!(indices["value"][1]["value"][0], -1);
            }
            for end in 0..bytes.len() {
                assert!(Volume::read(&bytes[..end], "truncated-v2-fixture").is_err());
            }
            let mut trailing = bytes;
            trailing.push(0);
            assert!(Volume::read(&trailing, "trailing-v2-fixture").is_err());
        }
    }

    #[test]
    fn native_v2_rejects_truncated_layers_and_invalid_secondary_indices() {
        for layer_index in [0, 1] {
            let mut root = valid_v2_root(2);
            let Tag::List(layers) = field_mut(&mut root, &["structure", "block_indices"]) else {
                unreachable!()
            };
            let Tag::IntArray(indices) = &mut layers[layer_index] else {
                unreachable!()
            };
            indices.pop();
            assert!(
                validate(&root)
                    .unwrap_err()
                    .to_string()
                    .contains("expected full volume 24")
            );
        }
        for invalid in [-2, 1] {
            let mut root = valid_v2_root(2);
            let Tag::List(layers) = field_mut(&mut root, &["structure", "block_indices"]) else {
                unreachable!()
            };
            let Tag::IntArray(secondary) = &mut layers[1] else {
                unreachable!()
            };
            secondary[0] = invalid;
            assert!(
                validate(&root)
                    .unwrap_err()
                    .to_string()
                    .contains("outside palette")
            );
        }
    }

    #[test]
    fn structure_versions_enforce_their_measured_layer_schema() {
        for version in [-1, 0, 3] {
            let mut root = valid_root();
            *field_mut(&mut root, &["format_version"]) = Tag::Int(version);
            assert!(
                validate(&root)
                    .unwrap_err()
                    .to_string()
                    .contains("unsupported")
            );
        }
        for (version, counts) in [(1, vec![0, 1, 3]), (2, vec![0, 3])] {
            for count in counts {
                let mut root = valid_v2_root(count);
                *field_mut(&mut root, &["format_version"]) = Tag::Int(version);
                assert!(
                    validate(&root)
                        .unwrap_err()
                        .to_string()
                        .contains("must contain")
                );
            }
        }
        for (version, wrong_layer) in [
            (1, Tag::IntArray(vec![0; 24])),
            (2, Tag::List(vec![Tag::Int(0); 24])),
            (2, Tag::LongArray(vec![0; 24])),
        ] {
            let mut root = if version == 1 {
                valid_root()
            } else {
                valid_v2_root(2)
            };
            let Tag::List(layers) = field_mut(&mut root, &["structure", "block_indices"]) else {
                unreachable!()
            };
            layers[1] = wrong_layer;
            assert!(validate(&root).is_err());
        }
        let mut root = valid_root();
        let Tag::List(layers) = field_mut(&mut root, &["structure", "block_indices"]) else {
            unreachable!()
        };
        let Tag::List(secondary) = &mut layers[1] else {
            unreachable!()
        };
        secondary[0] = Tag::Short(0);
        assert!(validate(&root).is_err());
    }

    #[test]
    fn structure_axis_order_is_z_then_y_then_x() {
        assert_eq!(local_position([2, 3, 4], 1).unwrap(), [0, 0, 1]);
        assert_eq!(local_position([2, 3, 4], 4).unwrap(), [0, 1, 0]);
        assert_eq!(local_position([2, 3, 4], 12).unwrap(), [1, 0, 0]);
        assert_eq!(local_position([2, 3, 4], 23).unwrap(), [1, 2, 3]);
        assert!(local_position([2, 3, 4], 24).is_err());
        assert!(local_position([2, 0, 4], 0).is_err());
    }

    #[test]
    fn both_layers_must_cover_the_entire_volume() {
        let mut root = valid_root();
        assert!(validate(&root).is_ok());
        let Tag::List(layers) = field_mut(&mut root, &["structure", "block_indices"]) else {
            unreachable!()
        };
        let Tag::List(secondary) = &mut layers[1] else {
            unreachable!()
        };
        secondary.pop();
        assert!(
            validate(&root)
                .unwrap_err()
                .to_string()
                .contains("expected full volume 24")
        );
    }

    #[test]
    fn secondary_palette_indices_and_void_sentinel_are_validated() {
        for invalid in [-2, 1] {
            let mut root = valid_root();
            let Tag::List(layers) = field_mut(&mut root, &["structure", "block_indices"]) else {
                unreachable!()
            };
            let Tag::List(secondary) = &mut layers[1] else {
                unreachable!()
            };
            secondary[0] = Tag::Int(invalid);
            assert!(
                validate(&root)
                    .unwrap_err()
                    .to_string()
                    .contains("outside palette")
            );
        }
    }

    #[test]
    fn required_entity_payload_cannot_be_silently_omitted() {
        let mut root = valid_root();
        let Tag::Compound(structure) = field_mut(&mut root, &["structure"]) else {
            unreachable!()
        };
        structure.remove("entities");
        assert!(
            validate(&root)
                .unwrap_err()
                .to_string()
                .contains("entities")
        );
    }

    #[test]
    fn block_position_data_must_reference_a_position_in_the_volume() {
        let mut root = valid_root();
        let Tag::Compound(positions) = field_mut(
            &mut root,
            &["structure", "palette", "default", "block_position_data"],
        ) else {
            unreachable!()
        };
        positions.insert("24".to_owned(), Tag::Compound(BTreeMap::new()));
        assert!(
            validate(&root)
                .unwrap_err()
                .to_string()
                .contains("outside volume")
        );
    }

    #[test]
    fn impossible_sizes_and_overflowed_world_bounds_are_rejected() {
        let mut root = valid_root();
        *field_mut(&mut root, &["size"]) = Tag::List(vec![Tag::Int(-1), Tag::Int(3), Tag::Int(4)]);
        assert!(validate(&root).is_err());
        let mut root = valid_root();
        *field_mut(&mut root, &["structure_world_origin"]) =
            Tag::List(vec![Tag::Int(i32::MAX), Tag::Int(0), Tag::Int(0)]);
        assert!(
            validate(&root)
                .unwrap_err()
                .to_string()
                .contains("world bounds overflow")
        );
    }

    fn set_actor(root: &mut Tag, index: &str, coordinates: [i32; 3]) {
        let Tag::Compound(positions) = field_mut(
            root,
            &["structure", "palette", "default", "block_position_data"],
        ) else {
            unreachable!()
        };
        positions.insert(
            index.to_owned(),
            object(&[(
                "block_entity_data",
                object(&[
                    ("id", Tag::String("Bed".to_owned())),
                    ("x", Tag::Int(coordinates[0])),
                    ("y", Tag::Int(coordinates[1])),
                    ("z", Tag::Int(coordinates[2])),
                ]),
            )]),
        );
    }

    #[test]
    fn native_v2_actor_coordinates_match_absolute_index_bounds() {
        assert_eq!(local_position([64, 128, 64], 224293).unwrap(), [27, 48, 37]);
        assert_eq!(local_position([64, 128, 64], 232485).unwrap(), [28, 48, 37]);
        assert_eq!(local_position([64, 128, 64], 240673).unwrap(), [29, 48, 33]);
        let mut root = valid_v2_root(2);
        set_actor(&mut root, "23", [-6, -62, 12]);
        assert!(validate(&root).is_ok());
        for invalid in [
            [-7, -62, 12], // In volume, but does not match this flat index.
            [-8, -62, 12],
            [-5, -62, 12],
            [-6, -65, 12],
            [-6, -61, 12],
            [-6, -62, 8],
            [-6, -62, 13],
        ] {
            set_actor(&mut root, "23", invalid);
            assert!(
                validate(&root)
                    .unwrap_err()
                    .to_string()
                    .contains("does not match index")
            );
        }
        for index in ["-1", "24", "not-an-index"] {
            let mut root = valid_v2_root(1);
            set_actor(&mut root, index, [-6, -62, 12]);
            assert!(validate(&root).is_err());
        }
    }

    #[test]
    fn native_v2_actor_coordinates_require_all_three_int_tags() {
        for coordinate in ["x", "y", "z"] {
            let mut root = valid_v2_root(1);
            set_actor(&mut root, "23", [-6, -62, 12]);
            let Tag::Compound(actor) = field_mut(
                &mut root,
                &[
                    "structure",
                    "palette",
                    "default",
                    "block_position_data",
                    "23",
                    "block_entity_data",
                ],
            ) else {
                unreachable!()
            };
            let removed = actor.remove(coordinate).unwrap();
            assert!(validate(&root).is_err());
            let Tag::Compound(actor) = field_mut(
                &mut root,
                &[
                    "structure",
                    "palette",
                    "default",
                    "block_position_data",
                    "23",
                    "block_entity_data",
                ],
            ) else {
                unreachable!()
            };
            actor.insert(
                coordinate.to_owned(),
                Tag::Long(int(&removed).unwrap() as i64),
            );
            assert!(validate(&root).is_err());
        }
        // V1 actor coordinate behavior is unchanged without native evidence.
        let mut root = valid_root();
        set_actor(&mut root, "23", [0, 0, 0]);
        assert!(validate(&root).is_ok());
    }

    #[test]
    fn mcstructure_v1_still_decodes_two_list_layers() {
        let root = valid_root();
        let bytes = little_endian_fixture(&root);
        let volume = Volume::read(&bytes, "mcstructure-v1-fixture").unwrap();
        assert_eq!(volume.format_version, 1);
        assert_eq!(volume.layer_count, 2);
        assert_eq!(volume.root, root);
        let typed = serde_json::to_value(Typed(&volume.root)).unwrap();
        let indices = &typed["value"]["structure"]["value"]["block_indices"];
        assert_eq!(indices["element_type"], "list");
        assert_eq!(indices["value"][1]["element_type"], "int");
        assert_eq!(indices["value"][1]["value"][0]["value"], -1);
    }

    #[test]
    fn native_v2_checks_indices_against_every_palette() {
        let mut root = valid_v2_root(2);
        let mut alternate = child(
            child(child(&root, "structure").unwrap(), "palette").unwrap(),
            "default",
        )
        .unwrap()
        .clone();
        *field_mut(&mut alternate, &["block_palette"]) = Tag::List(vec![]);
        let Tag::Compound(palettes) = field_mut(&mut root, &["structure", "palette"]) else {
            unreachable!()
        };
        palettes.insert("alternate".to_owned(), alternate);
        let error = validate(&root).unwrap_err().to_string();
        assert!(error.contains("outside palette \"alternate\""));
    }
}
