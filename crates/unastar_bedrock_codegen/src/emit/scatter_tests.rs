//! Execute emitted Rust against a bounded in-memory block store. These are
//! placement regressions, not source-string or callback-forwarding checks.

use super::*;
use crate::ir::Obj;
use crate::model::Model;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

fn model() -> Model {
    Model {
        palette: Default::default(),
        biomes: Vec::new(),
        rules: BTreeMap::new(),
        features: BTreeMap::new(),
        spawn_rules: BTreeMap::new(),
        structures: Default::default(),
        inputs: Vec::new(),
    }
}

fn parse(value: &Value) -> Scatter {
    let obj = Obj::new("authored BDS coordinate control", value).unwrap();
    let scatter = Scatter::parse_keys(&obj).unwrap();
    obj.finish().unwrap();
    scatter
}

fn place(p: TokenStream) -> TokenStream {
    quote!({ let p = #p; if ctx.world.set(p, 7) { Some(p) } else { None } })
}

fn execute(body: TokenStream, assertions: TokenStream) {
    let runtime = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../unastar/src/world/generator/bedrock")
        .canonicalize()
        .unwrap();
    let rng = runtime.join("rng.rs").to_string_lossy().into_owned();
    let scatter = runtime.join("scatter.rs").to_string_lossy().into_owned();
    let source = quote! {
        #[path = #rng] mod rng;
        #[path = #scatter] mod scatter;
        #[derive(Clone, Copy, Debug)]
        struct BlockPos { x: i32, y: i32, z: i32 }
        impl BlockPos { fn new(x: i32, y: i32, z: i32) -> Self { Self { x, y, z } } }
        #[derive(Default)]
        struct World { blocks: std::collections::BTreeMap<(i32, i32, i32), u32> }
        impl World {
            fn set(&mut self, p: BlockPos, block: u32) -> bool {
                if !(112..160).contains(&p.x) || !(-16..32).contains(&p.z) || !(-64..320).contains(&p.y) {
                    return false;
                }
                let key = (p.x, p.y, p.z);
                if self.blocks.contains_key(&key) { return false; }
                self.blocks.insert(key, block);
                true
            }
        }
        struct Ctx<'a> { rng: &'a mut rng::BedrockRandom, world: &'a mut World }
        fn placement(ctx: &mut Ctx<'_>, origin: BlockPos) -> Option<BlockPos> { #body }
        fn main() {
            let mut random = rng::BedrockRandom::new(12345);
            let mut world = World::default();
            let mut ctx = Ctx { rng: &mut random, world: &mut world };
            placement(&mut ctx, BlockPos::new(128, 0, 0));
            #assertions
        }
    };
    let temp = tempfile::tempdir().unwrap();
    let source_path = temp.path().join("scatter_control.rs");
    let executable = temp
        .path()
        .join(format!("scatter_control{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(
        &source_path,
        prettyplease::unparse(&syn::parse2(source).unwrap()),
    )
    .unwrap();
    let compiler = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let compiled = Command::new(compiler)
        .arg("--edition=2024")
        .arg(&source_path)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "compiling emitted control: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let result = Command::new(&executable).output().unwrap();
    assert!(
        result.status.success(),
        "emitted placement violated the BDS control: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn nested_molang_offsets_place_beside_the_parent_not_the_world_origin() {
    // BDS 26.51 controlled run: input (128,0,0), parent offset (8,287,8),
    // child math.random(1,2)/math.random(3,4) wrote at x=137/138,z=11/12.
    let model = model();
    let em = Emitter {
        model: &model,
        reachable: BTreeSet::new(),
        blocks: BTreeMap::new(),
        used_names: BTreeSet::new(),
    };
    let outer = parse(&json!({"iterations":1,"x":8,"y":287,"z":8}));
    let inner =
        parse(&json!({"iterations":1,"x":"math.random(1, 2)","y":0,"z":"math.random(3, 4)"}));
    let inner = emit(&em, "nested control", &inner, place).unwrap();
    let body = emit(
        &em,
        "parent control",
        &outer,
        |p| quote!({ let origin = #p; #inner }),
    )
    .unwrap();
    execute(
        body,
        quote! {
            let (&(x,y,z), &block) = ctx.world.blocks.iter().next().expect("BDS observed a marker in the parent neighborhood");
            assert!((137..=138).contains(&x), "marker x {x} escaped the measured child range");
            assert_eq!(y, 287);
            assert!((11..=12).contains(&z), "marker z {z} escaped the measured child range");
            assert_eq!(block, 7);
        },
    );
}

#[test]
fn uniform_upper_extent_is_exclusive_in_emitted_placements() {
    // BDS 26.51: 128 draws at each of three isolated origins covered offsets
    // 0..15 for extent [0,16], never 16.
    let model = model();
    let em = Emitter {
        model: &model,
        reachable: BTreeSet::new(),
        blocks: BTreeMap::new(),
        used_names: BTreeSet::new(),
    };
    let scatter = parse(
        &json!({"iterations":128,"x":{"distribution":"uniform","extent":[0,16]},"y":285,"z":0}),
    );
    let body = emit(&em, "uniform control", &scatter, place).unwrap();
    execute(
        body,
        quote! {
            for (&(x,y,z), &block) in &ctx.world.blocks {
                assert!((128..144).contains(&x), "uniform marker {x} hit the excluded upper extent");
                assert_eq!((y,z,block), (285,0,7));
            }
            assert_eq!(ctx.world.blocks.get(&(143,285,0)), Some(&7));
        },
    );
}

#[test]
fn molang_offsets_round_to_nearest_with_ties_away_from_zero_before_translation() {
    // BDS marker controls at all three origins: ±1.25 -> ±1,
    // ±1.75 -> ±2, ±1.5 -> ±2. Dynamic expressions take the runtime path.
    let model = model();
    let em = Emitter {
        model: &model,
        reachable: BTreeSet::new(),
        blocks: BTreeMap::new(),
        used_names: BTreeSet::new(),
    };
    let mut calls = Vec::new();
    for (i, value) in ["1.25", "1.75", "-1.25", "-1.75", "1.5", "-1.5"]
        .into_iter()
        .enumerate()
    {
        for z in [0, 2] {
            let x = if z == 0 {
                value.to_owned()
            } else {
                format!("{value} + variable.originz")
            };
            let scatter = parse(&json!({"iterations":1,"x":x,"y":290+i,"z":z}));
            calls.push(emit(&em, "rounding control", &scatter, place).unwrap());
        }
    }
    execute(
        quote! { #(let _ = { #calls };)* None },
        quote! {
            let mut expected = std::collections::BTreeMap::new();
            for (i, offset) in [1, 2, -1, -2, 2, -2].into_iter().enumerate() {
                for z in [0, 2] {
                    expected.insert((128 + offset, 290 + i as i32, z), 7);
                }
            }
            assert_eq!(ctx.world.blocks, expected);
        },
    );
}
