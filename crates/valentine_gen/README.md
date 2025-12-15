# `valentine_gen`

Developer tool that generates Bedrock protocol crates under `crates/valentine/bedrock_versions/` and updates `crates/valentine/Cargo.toml` + re-exports.

The generated Rust sources are intentionally not committed (they are ignored via `.gitignore`). Run the generator after cloning when you need protocol code.

## Setup

The generator reads protocol schemas from the `minecraft-data` submodule:

```bash
git submodule update --init --recursive
```

## Usage

Generate the latest Bedrock version (this matches `valentine`'s default feature):

```bash
cargo run -p valentine_gen -- --latest
```

Generate a specific version (or multiple):

```bash
cargo run -p valentine_gen -- --versions 1.21.130
cargo run -p valentine_gen -- --versions 1.21.130,1.20.80
```

Generate everything:

```bash
cargo run -p valentine_gen -- --all
```

List available versions:

```bash
cargo run -p valentine_gen -- --list-versions
```

Logging:

```bash
cargo run -p valentine_gen -- --log debug
```

