//! Java block model for code generation: the pinned block registry
//! (`java_data/blocks.json`, shared with the terrain toolchain), block types
//! (`java_data/block_kinds.json`), collision shapes (`java_data/collisions.json`),
//! block tags (`java_data/tags/block`), state id resolution and block-set
//! interning, plus the emitter for the generated `java_registry.rs` module
//! (`features::java_blocks`).
//!
//! Every block state referenced by feature or structure data is resolved here
//! to its global Java state id; an unknown block, property or value fails the
//! build.
//!
//! Per-state flags are derived from the collision boxes:
//! - bit 0: Java "legacy solid": non-empty shape whose bounds have an average
//!   edge >= 0.7291666 or a full height (signs, banners and pressure plates
//!   force it on);
//! - bits 1..=6: full-face collision in `Direction` order (down, up, north,
//!   south, west, east), for `has_sturdy_face`;
//! - bit 7: full-cube collision.

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

#[derive(Debug, Deserialize)]
struct BlocksFile {
    blocks: Vec<BlockJson>,
}

#[derive(Debug, Deserialize)]
struct BlockJson {
    name: String,
    first_id: usize,
    default_id: usize,
    properties: Vec<(String, Vec<String>)>,
}

#[derive(Debug, Deserialize)]
struct CollisionsFile {
    shapes: Vec<Vec<[f64; 6]>>,
    indices: Vec<usize>,
}

/// Blocks whose block properties force `isSolid()` on regardless of shape.
const FORCE_SOLID_ON_SUFFIXES: &[&str] = &["_sign", "_banner", "_pressure_plate"];

const EPS: f64 = 1e-7;

/// Whether the union of `boxes` covers one unit face (1/64 grid of cell
/// centres; exact for vanilla's 1/16-step shapes).
fn face_full(boxes: &[[f64; 6]], axis: usize, at_max: bool) -> bool {
    let (u, v) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let plane = if at_max { 1.0 } else { 0.0 };
    let n = 64;
    (0..n).all(|i| {
        (0..n).all(|j| {
            let pu = (i as f64 + 0.5) / n as f64;
            let pv = (j as f64 + 0.5) / n as f64;
            boxes.iter().any(|bx| {
                let touches = if at_max {
                    (bx[axis + 3] - plane).abs() < EPS
                } else {
                    (bx[axis] - plane).abs() < EPS
                };
                touches && bx[u] <= pu && pu <= bx[u + 3] && bx[v] <= pv && pv <= bx[v + 3]
            })
        })
    })
}

fn shape_flags(boxes: &[[f64; 6]]) -> u8 {
    let mut f = 0u8;
    if !boxes.is_empty() {
        let mut b = [f64::MAX, f64::MAX, f64::MAX, f64::MIN, f64::MIN, f64::MIN];
        for bx in boxes {
            for i in 0..3 {
                b[i] = b[i].min(bx[i]);
                b[i + 3] = b[i + 3].max(bx[i + 3]);
            }
        }
        let (dx, dy, dz) = (b[3] - b[0], b[4] - b[1], b[5] - b[2]);
        if (dx + dy + dz) / 3.0 >= 0.7291666666666666 || dy >= 1.0 {
            f |= 1;
        }
        let faces = [(1, false), (1, true), (2, false), (2, true), (0, false), (0, true)];
        for (i, (axis, max)) in faces.into_iter().enumerate() {
            if face_full(boxes, axis, max) {
                f |= 1 << (i + 1);
            }
        }
        if boxes
            .iter()
            .any(|bx| bx.iter().zip([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).all(|(a, e)| (a - e).abs() < EPS))
        {
            f |= 1 << 7;
        }
    }
    f
}

/// One Java block.
#[derive(Debug, Clone)]
pub struct Block {
    pub name: String,
    pub kind: String,
    pub props: Vec<(String, Vec<String>)>,
    pub first_state: usize,
    pub state_count: usize,
    pub default_offset: usize,
    pub flags: Vec<u8>,
}

impl Block {
    fn stride(&self, prop_index: usize) -> usize {
        self.props[prop_index + 1..]
            .iter()
            .map(|(_, v)| v.len())
            .product()
    }
}

/// The Java block registry plus resolved tags and interned block sets.
#[derive(Debug)]
pub struct JavaBlocks {
    pub blocks: Vec<Block>,
    by_name: HashMap<String, usize>,
    pub state_count: usize,
    /// Resolved tags (name without `#`, e.g. `minecraft:leaves`) -> block indices.
    tags: BTreeMap<String, BTreeSet<usize>>,
    /// Interned sets: index -> members. Tags come first, in name order.
    sets: Vec<BTreeSet<usize>>,
    set_index: HashMap<BTreeSet<usize>, usize>,
    tag_set: BTreeMap<String, usize>,
}

#[derive(Debug, Deserialize)]
struct TagFile {
    values: Vec<TagEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TagEntry {
    Id(String),
    Obj { id: String, required: bool },
}

pub type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

impl JavaBlocks {
    /// Load the block registry, block types, collision shapes and block tags
    /// from `java_data`.
    pub fn load(java_data: &Path) -> Result<Self> {
        let read = |name: &str| -> Result<String> {
            let p = java_data.join(name);
            std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()).into())
        };
        let file: BlocksFile =
            serde_json::from_str(&read("blocks.json")?).map_err(|e| format!("java_data/blocks.json: {e}"))?;
        let kinds: BTreeMap<String, String> = serde_json::from_str(&read("block_kinds.json")?)
            .map_err(|e| format!("java_data/block_kinds.json: {e}"))?;
        let collisions: CollisionsFile = serde_json::from_str(&read("collisions.json")?)
            .map_err(|e| format!("java_data/collisions.json: {e}"))?;
        // Shapes are shared by many states: classify each once.
        let shape_bits: Vec<u8> = collisions.shapes.iter().map(|boxes| shape_flags(boxes)).collect();
        let mut blocks = Vec::with_capacity(file.blocks.len());
        let mut by_name = HashMap::new();
        let mut first = 0;
        for b in file.blocks {
            let count: usize = b.properties.iter().map(|(_, v)| v.len()).product();
            if b.first_id != first {
                return Err(format!("{}: state ids are not contiguous", b.name).into());
            }
            let kind = kinds
                .get(&b.name)
                .ok_or_else(|| format!("java_data/block_kinds.json has no type for {}", b.name))?
                .clone();
            let short = b.name.trim_start_matches("minecraft:");
            let force_solid = FORCE_SOLID_ON_SUFFIXES.iter().any(|s| short.ends_with(s));
            let flags = (first..first + count)
                .map(|s| {
                    let shape = *collisions
                        .indices
                        .get(s)
                        .ok_or_else(|| format!("collisions.json has no shape for state {s}"))?;
                    let bits = *shape_bits
                        .get(shape)
                        .ok_or_else(|| format!("collisions.json: bad shape index {shape}"))?;
                    Ok(if force_solid { bits | 1 } else { bits })
                })
                .collect::<Result<Vec<u8>>>()?;
            by_name.insert(b.name.clone(), blocks.len());
            blocks.push(Block {
                name: b.name,
                kind,
                props: b.properties,
                first_state: first,
                state_count: count,
                default_offset: b.default_id - first,
                flags,
            });
            first += count;
        }
        if first != collisions.indices.len() {
            return Err("java_data block and collision tables disagree on the state count".into());
        }
        let tags_dir = &java_data.join("tags/block");

        // Raw tag files.
        let mut raw: BTreeMap<String, Vec<TagEntry>> = BTreeMap::new();
        for entry in walkdir::WalkDir::new(tags_dir).sort_by_file_name() {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let rel = path
                .strip_prefix(tags_dir)?
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            let tag: TagFile = serde_json::from_str(&std::fs::read_to_string(path)?)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            raw.insert(format!("minecraft:{rel}"), tag.values);
        }

        let mut model = JavaBlocks {
            blocks,
            by_name,
            state_count: first,
            tags: BTreeMap::new(),
            sets: Vec::new(),
            set_index: HashMap::new(),
            tag_set: BTreeMap::new(),
        };
        let names: Vec<String> = raw.keys().cloned().collect();
        for name in &names {
            let mut stack = Vec::new();
            let members = model.resolve_tag(name, &raw, &mut stack)?;
            model.tags.insert(name.clone(), members);
        }
        for name in &names {
            let members = model.tags[name].clone();
            let idx = model.sets.len();
            // Tags always get their own slot so `tags::X` is stable even when two
            // tags have identical members.
            model.sets.push(members.clone());
            model.set_index.entry(members).or_insert(idx);
            model.tag_set.insert(name.clone(), idx);
        }
        Ok(model)
    }

    fn resolve_tag(
        &self,
        name: &str,
        raw: &BTreeMap<String, Vec<TagEntry>>,
        stack: &mut Vec<String>,
    ) -> Result<BTreeSet<usize>> {
        if stack.iter().any(|s| s == name) {
            return Err(format!("block tag cycle through {name}").into());
        }
        let entries = raw
            .get(name)
            .ok_or_else(|| format!("unknown block tag #{name}"))?;
        stack.push(name.to_string());
        let mut out = BTreeSet::new();
        for e in entries {
            let (id, required) = match e {
                TagEntry::Id(s) => (s.as_str(), true),
                TagEntry::Obj { id, required } => (id.as_str(), *required),
            };
            if let Some(t) = id.strip_prefix('#') {
                match self.resolve_tag(t, raw, stack) {
                    Ok(m) => out.extend(m),
                    Err(e) if required => return Err(e),
                    Err(_) => {}
                }
            } else {
                match self.by_name.get(id) {
                    Some(i) => {
                        out.insert(*i);
                    }
                    None if required => {
                        return Err(format!("tag #{name}: unknown block {id}").into());
                    }
                    None => {}
                }
            }
        }
        stack.pop();
        Ok(out)
    }

    /// Block index by registry name.
    pub fn block(&self, name: &str) -> Result<usize> {
        self.by_name
            .get(name)
            .copied()
            .ok_or_else(|| format!("unknown Java block {name}").into())
    }

    /// Global state id of `name` with `props` applied over the default state.
    pub fn state(&self, name: &str, props: &BTreeMap<String, String>) -> Result<u16> {
        let b = &self.blocks[self.block(name)?];
        let mut offset = b.default_offset;
        for (k, v) in props {
            let pi = b
                .props
                .iter()
                .position(|(n, _)| n == k)
                .ok_or_else(|| format!("{name} has no property {k}"))?;
            let values = &b.props[pi].1;
            let vi = values
                .iter()
                .position(|x| x == v)
                .ok_or_else(|| format!("{name}: {k} has no value {v}"))?;
            let stride = b.stride(pi);
            let cur = (offset / stride) % values.len();
            offset = offset - cur * stride + vi * stride;
        }
        Ok((b.first_state + offset) as u16)
    }

    /// `!hasDynamicShape() && isCollisionShapeFullBlock()` for a state id: the
    /// full-cube flag, excluding blocks whose shape depends on context
    /// (shulker boxes, moving pistons, scaffolding).
    pub fn full_cube_static(&self, state: u16) -> bool {
        let s = state as usize;
        let i = self.blocks.partition_point(|b| b.first_state <= s) - 1;
        let b = &self.blocks[i];
        let dynamic = matches!(
            b.kind.as_str(),
            "minecraft:shulker_box" | "minecraft:moving_piston" | "minecraft:scaffolding"
        );
        !dynamic && b.flags[s - b.first_state] & 0x80 != 0
    }

    /// Default state id of a block.
    pub fn default_state(&self, name: &str) -> Result<u16> {
        self.state(name, &BTreeMap::new())
    }

    /// Set index of a tag (`minecraft:leaves`, no `#`).
    pub fn tag(&self, name: &str) -> Result<u16> {
        self.tag_set
            .get(name)
            .map(|i| *i as u16)
            .ok_or_else(|| format!("unknown block tag #{name}").into())
    }

    /// Intern an explicit block list as a set.
    pub fn intern_blocks(&mut self, names: &[String]) -> Result<u16> {
        let mut members = BTreeSet::new();
        for n in names {
            members.insert(self.block(n)?);
        }
        Ok(self.intern(members))
    }

    fn intern(&mut self, members: BTreeSet<usize>) -> u16 {
        if let Some(i) = self.set_index.get(&members) {
            return *i as u16;
        }
        let i = self.sets.len();
        self.sets.push(members.clone());
        self.set_index.insert(members, i);
        i as u16
    }

    /// Emit `java_blocks.rs` plus its binary state tables into `out_dir`.
    pub fn emit(&self, out_dir: &Path) -> Result<()> {
        let words = self.blocks.len().div_ceil(64);

        let mut state_block = Vec::with_capacity(self.state_count * 2);
        let mut state_flags = Vec::with_capacity(self.state_count);
        for (i, b) in self.blocks.iter().enumerate() {
            for f in &b.flags {
                state_block.extend_from_slice(&(i as u16).to_le_bytes());
                state_flags.push(*f);
            }
        }
        std::fs::write(out_dir.join("java_state_block.bin"), &state_block)?;
        std::fs::write(out_dir.join("java_state_flags.bin"), &state_flags)?;

        let block_defs = self.blocks.iter().map(|b| {
            let name = &b.name;
            let kind = &b.kind;
            let first = b.first_state as u16;
            let count = b.state_count as u16;
            let default = (b.first_state + b.default_offset) as u16;
            let props = b.props.iter().enumerate().map(|(pi, (pn, vals))| {
                let stride = b.stride(pi) as u16;
                quote! { PropDef { name: #pn, values: &[#(#vals),*], stride: #stride } }
            });
            quote! {
                JavaBlockDef {
                    name: #name,
                    kind: #kind,
                    first_state: #first,
                    state_count: #count,
                    default_state: #default,
                    props: &[#(#props),*],
                }
            }
        });

        let mut sorted: Vec<(&str, u16)> = self
            .blocks
            .iter()
            .enumerate()
            .map(|(i, b)| (b.name.as_str(), i as u16))
            .collect();
        sorted.sort();
        let by_name = sorted.iter().map(|(n, i)| quote! { (#n, #i) });

        let sets = self.sets.iter().map(|members| {
            let mut w = vec![0u64; words];
            for m in members {
                w[m / 64] |= 1u64 << (m % 64);
            }
            let lits = w.iter().map(|x| Literal::u64_suffixed(*x));
            quote! { [#(#lits),*] }
        });

        let block_consts = self.blocks.iter().enumerate().map(|(i, b)| {
            let id = const_ident(&b.name);
            let i = i as u16;
            quote! { pub const #id: JavaBlock = JavaBlock(#i); }
        });
        let state_consts = self.blocks.iter().map(|b| {
            let id = const_ident(&b.name);
            let s = (b.first_state + b.default_offset) as u16;
            quote! { pub const #id: JavaState = JavaState(#s); }
        });
        let tag_consts = self.tag_set.iter().map(|(name, idx)| {
            let id = const_ident(name);
            let idx = *idx as u16;
            quote! { pub const #id: BlockSet = BlockSet(#idx); }
        });

        let n_blocks = self.blocks.len();
        let n_states = self.state_count;
        let n_sets = self.sets.len();
        let tokens: TokenStream = quote! {
            use super::blocks::{BlockSet, JavaBlock, JavaBlockDef, JavaState, PropDef};

            /// Number of Java blocks.
            pub const BLOCK_COUNT: usize = #n_blocks;
            /// Number of Java block states.
            pub const STATE_COUNT: usize = #n_states;
            /// `u64` words per block set bitset.
            pub const SET_WORDS: usize = #words;

            /// Java blocks in registry order.
            pub static BLOCKS: [JavaBlockDef; #n_blocks] = [#(#block_defs),*];
            /// `(name, block index)` sorted by name.
            pub static BLOCKS_BY_NAME: [(&str, u16); #n_blocks] = [#(#by_name),*];
            /// Block sets: block tags (name order) then inline block lists.
            pub static BLOCK_SETS: [[u64; SET_WORDS]; #n_sets] = [#(#sets),*];

            static STATE_BLOCK_BYTES: [u8; #n_states * 2] =
                *include_bytes!(concat!(env!("OUT_DIR"), "/java_state_block.bin"));
            /// Flag byte per state (`java_data/README.md`).
            pub static STATE_FLAGS: [u8; #n_states] =
                *include_bytes!(concat!(env!("OUT_DIR"), "/java_state_flags.bin"));

            /// Block index of a state id.
            pub struct StateBlockTable;
            impl std::ops::Index<usize> for StateBlockTable {
                type Output = u16;
                #[inline]
                fn index(&self, i: usize) -> &u16 {
                    &STATE_BLOCK_U16[i]
                }
            }
            static STATE_BLOCK_U16: std::sync::LazyLock<Box<[u16]>> = std::sync::LazyLock::new(|| {
                STATE_BLOCK_BYTES
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect()
            });
            /// Block index per state id.
            pub static STATE_BLOCK: StateBlockTable = StateBlockTable;

            /// Every Java block.
            pub mod blocks {
                use super::JavaBlock;
                #(#block_consts)*
            }
            /// Default state of every Java block.
            pub mod states {
                use super::JavaState;
                #(#state_consts)*
            }
            /// Every Java block tag.
            pub mod tags {
                use super::BlockSet;
                #(#tag_consts)*
            }
        };
        write_pretty(&out_dir.join("java_registry.rs"), tokens)
    }
}

/// `minecraft:mineable/axe` -> `MINEABLE_AXE`.
pub fn const_ident(name: &str) -> proc_macro2::Ident {
    let short = name.strip_prefix("minecraft:").unwrap_or(name);
    let s: String = short
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    let s = if s.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{s}")
    } else {
        s
    };
    format_ident!("{}", s)
}

/// Format tokens with prettyplease and write them.
pub fn write_pretty(path: &Path, tokens: TokenStream) -> Result<()> {
    let file: syn::File = syn::parse2(tokens).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::write(path, prettyplease::unparse(&file))?;
    Ok(())
}
