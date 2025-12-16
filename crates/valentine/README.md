# `valentine`

This crate exposes Bedrock protocol types via feature flags like `bedrock_1_21_130`.

The per-version protocol sources live in `crates/valentine/bedrock_versions/`, but the generated Rust code is not committed to the repository. After cloning, run the generator to produce the version(s) you need.

## Generate protocol code

From the repo root:

```bash
git submodule update --init --recursive
cargo run -p valentine_gen -- --latest
```

## Selecting versions

- Enable a version feature, e.g. `--features bedrock_1_21_130`.
- Import via `valentine::bedrock::protocol::v1_21_130::...` or `valentine::bedrock::version::v1_21_130::*`.

## 🔮 Roadmap

### Expanded Data Coverage
Currently, Valentine focuses on packet definitions. We aim to expand this to include:
- **Block States**: Complete mapping of block state permutations and their serialization.
- **Collision Geometry**: AABB and complex collision data for blocks.
- **Entity Metadata**: Strong typing for entity metadata fields and flags.

### Generator Improvements
- **Goal**: Automate the generation of `Block` and `Item` enums from `minecraft-data`.
- **Goal**: Integrate `Nbt` schema validation directly into the generator.

