//! Decoded structure templates (see `codegen/templates.rs` for the blob layout).

use crate::features::JavaState;
use std::sync::LazyLock;

/// Generated template table.
#[allow(clippy::all, missing_docs)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/templates.rs"));
}

pub use generated::{TEMPLATE_NAMES, TEMPLATES_EMBEDDED};

/// One block of a template palette, relative to the template origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateBlock {
    pub x: u8,
    pub y: u8,
    pub z: u8,
    pub state: JavaState,
    /// Index into [`Template::block_entities`], if the block has one.
    pub block_entity: Option<u16>,
}

/// Jigsaw joint type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JointType {
    Rollable,
    Aligned,
}

/// Block entity data kept from a template.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockEntityInfo {
    Jigsaw {
        name: &'static str,
        target: &'static str,
        pool: &'static str,
        final_state: JavaState,
        /// `None`: not stored; derive from the jigsaw's orientation (aligned
        /// when the front faces horizontally).
        joint: Option<JointType>,
        selection_priority: i32,
        placement_priority: i32,
    },
    /// Structure block in data mode (`metadata` names the marker).
    DataMarker { metadata: &'static str },
    /// Source-native Java actor metadata and measured load/save normalization.
    Other {
        actor: super::native_actor::TemplateActor,
        loot: Option<(&'static str, i64)>,
        spawner_entity: Option<&'static str>,
    },
}

/// An entity stored in a template.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityInfo {
    pub pos: [f64; 3],
    pub block_pos: [i32; 3],
    pub id: &'static str,
    pub nbt: &'static [u8],
}

/// A decoded template.
#[derive(Debug, Clone)]
pub struct Template {
    pub name: &'static str,
    pub size: [i32; 3],
    /// Palettes (alternative block sets); each in Java's processing order.
    pub palettes: Vec<Vec<TemplateBlock>>,
    pub block_entities: Vec<BlockEntityInfo>,
    pub entities: Vec<EntityInfo>,
}

struct Cursor {
    data: &'static [u8],
    o: usize,
}

impl Cursor {
    fn bytes(&mut self, n: usize) -> &'static [u8] {
        let s = &self.data[self.o..self.o + n];
        self.o += n;
        s
    }
    fn u8(&mut self) -> u8 {
        self.bytes(1)[0]
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.bytes(2).try_into().expect("2 bytes"))
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.bytes(4).try_into().expect("4 bytes"))
    }
    fn i32(&mut self) -> i32 {
        self.u32() as i32
    }
    fn i64(&mut self) -> i64 {
        i64::from_le_bytes(self.bytes(8).try_into().expect("8 bytes"))
    }
    fn f64(&mut self) -> f64 {
        f64::from_le_bytes(self.bytes(8).try_into().expect("8 bytes"))
    }
    fn str(&mut self) -> &'static str {
        let n = self.u16() as usize;
        std::str::from_utf8(self.bytes(n)).expect("template strings are UTF-8")
    }
    fn blob(&mut self) -> &'static [u8] {
        let n = self.u32() as usize;
        self.bytes(n)
    }
}

fn decode(name: &'static str, data: &'static [u8], offset: usize) -> Template {
    let mut c = Cursor { data, o: offset };
    let size = [c.i32(), c.i32(), c.i32()];
    let palette_count = c.u16() as usize;
    let mut palettes = Vec::with_capacity(palette_count);
    for _ in 0..palette_count {
        let n = c.u32() as usize;
        let mut blocks = Vec::with_capacity(n);
        for _ in 0..n {
            let (x, y, z) = (c.u8(), c.u8(), c.u8());
            let state = JavaState(c.u16());
            let be = c.u16();
            blocks.push(TemplateBlock {
                x,
                y,
                z,
                state,
                block_entity: (be != u16::MAX).then_some(be),
            });
        }
        palettes.push(blocks);
    }
    let be_count = c.u16() as usize;
    let mut block_entities = Vec::with_capacity(be_count);
    for _ in 0..be_count {
        block_entities.push(match c.u8() {
            0 => BlockEntityInfo::Jigsaw {
                name: c.str(),
                target: c.str(),
                pool: c.str(),
                final_state: JavaState(c.u16()),
                joint: match c.u8() {
                    0 => Some(JointType::Rollable),
                    1 => Some(JointType::Aligned),
                    _ => None,
                },
                selection_priority: c.i32(),
                placement_priority: c.i32(),
            },
            1 => BlockEntityInfo::DataMarker { metadata: c.str() },
            _ => BlockEntityInfo::Other {
                actor: super::native_actor::TemplateActor {
                    id: c.str(),
                    source: c.blob(),
                    normalization: c.blob(),
                },
                loot: if c.u8() != 0 {
                    Some((c.str(), c.i64()))
                } else {
                    None
                },
                spawner_entity: if c.u8() != 0 { Some(c.str()) } else { None },
            },
        });
    }
    let entity_count = c.u16() as usize;
    let mut entities = Vec::with_capacity(entity_count);
    for _ in 0..entity_count {
        let pos = [c.f64(), c.f64(), c.f64()];
        let block_pos = [c.i32(), c.i32(), c.i32()];
        entities.push(EntityInfo {
            pos,
            block_pos,
            id: c.str(),
            nbt: c.blob(),
        });
    }
    Template {
        name,
        size,
        palettes,
        block_entities,
        entities,
    }
}

static TEMPLATES: LazyLock<Vec<Template>> = LazyLock::new(|| {
    let data = generated::TEMPLATE_DATA;
    let mut c = Cursor { data, o: 0 };
    let count = c.u32() as usize;
    assert_eq!(
        count,
        TEMPLATE_NAMES.len(),
        "template blob does not match the name table"
    );
    (0..count)
        .map(|i| {
            let off =
                u32::from_le_bytes(data[4 + 4 * i..8 + 4 * i].try_into().expect("offset")) as usize;
            decode(TEMPLATE_NAMES[i], data, off)
        })
        .collect()
});

static EMPTY: Template = Template {
    name: "",
    size: [0, 0, 0],
    palettes: Vec::new(),
    block_entities: Vec::new(),
    entities: Vec::new(),
};

/// `StructureTemplateManager.getOrCreate(id)`: the template, or Java's empty
/// template (size 0, no palette) when the id has no file. Vanilla 1.21.11 data
/// references one such id (`minecraft:ancient_city/walls/intact_horizontal_wall_stairs_5`),
/// which Java then treats as a zero-size piece that places nothing.
pub fn template_or_empty(name: &str) -> &'static Template {
    template(name).unwrap_or(&EMPTY)
}

/// Template by id (`minecraft:igloo/top`).
///
/// # Panics
/// If templates were not embedded (build without `java-templates`); callers
/// that run without them must check [`TEMPLATES_EMBEDDED`] first.
pub fn template(name: &str) -> Option<&'static Template> {
    if !TEMPLATES_EMBEDDED {
        panic!(
            "structure templates are not embedded: build unastar_noise with the `java-templates` \
             feature and UNASTAR_JAVA_SERVER_JAR set"
        );
    }
    TEMPLATE_NAMES
        .binary_search(&name)
        .ok()
        .map(|i| &TEMPLATES[i])
}
