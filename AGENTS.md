# AGENTS.md

## Repo Summary
- Workspace: `axolotl-stack`
- Domain: Minecraft Bedrock Edition server, protocol, transport, auth, and tooling
- Language: Rust
- Rust edition: `2024` (workspace-level)
- Async runtime: `tokio`
- ECS: `bevy_ecs 0.17`

## Primary Goal
- Keep the workspace modular: protocol/data generation, transport, auth, and server logic are separate layers and should stay that way.
- Prefer maintainable Rust over clever generation shortcuts, especially in protocol code.
- When behavior should survive regeneration, change the generator, not the generated files.

## Working Expectations
- Prefer targeted changes over broad refactors unless the task explicitly asks for restructuring.
- Keep crate boundaries clear. Shared runtime code belongs in `bedrock_core`; version-specific generated protocol code belongs under `bedrock_versions`; generator logic belongs in `valentine_gen`.
- Match existing Rust style in each crate.
- For generator work, prefer `quote`, `syn`, `proc-macro2`, and `prettyplease` over stringly code emission.
- Treat generated artifacts carefully. Hand-edit generated output only when the task is explicitly about debugging or patching a generated file.
- Document non-obvious generator phases, naming transforms, and protocol quirks close to the code.

## Validation
- After code changes, run `cargo clippy`.
- If the change is scoped to one crate, start with `cargo clippy -p <crate>`.
- If the change affects shared code, generated protocol code, or workspace wiring, follow with broader validation.
- Run targeted tests when practical, especially `valentine` roundtrip and packet codec tests.
- If generator output changes, regenerate and verify the generated crate still compiles.

## Workspace Map
- `crates/valentine`: Bedrock protocol surface used by the rest of the workspace.
- `crates/valentine/bedrock_core`: Shared Bedrock codec/runtime primitives and errors.
- `crates/valentine/bedrock_versions/v1_26_0`: Checked-in generated protocol/data crate for the current version.
- `crates/valentine_gen`: Bedrock protocol/data generator.
- `crates/jolyne`: Higher-level protocol engine/state handling.
- `crates/tokio-raknet`: Async RakNet transport.
- `crates/tokio-nethernet`: Async NetherNet transport.
- `crates/axolotl-xbl`: Xbox Live and Minecraft auth flows.
- `crates/axelerator`: Friend-broadcasting gateway/proxy tooling.
- `crates/unastar`: Main server binary and gameplay/server runtime work.
- `crates/unastar-api`: Plugin API surface.
- `crates/unastar-api-macros`: Proc macros for plugin authoring.
- `crates/example-plugin`: Example plugin implementation.
- `crates/unastar-data`: Data/codegen support for server-side content.
- `crates/bds-extractor`: Bedrock data extraction tooling.

## Protocol And Codegen Notes
- Primary protocol work lives in `crates/valentine` and `crates/valentine_gen`.
- Generator inputs live under `crates/valentine_gen/minecraft-data` and `crates/valentine_gen/bedrock-data`.
- The generator should have distinct phases:
  - parse schema input into IR
  - analyze containers, discriminators, and arguments
  - resolve naming and canonical identities
  - emit Rust modules
- Packet argument typing should have one source of truth. Signature building, codec args generation, and packet dispatch should reuse the same resolved analysis.
- Cross-version packet/type dedup is desirable, but only if canonical paths and reuse rules are explicit and easy to inspect.
- Prefer improving generator modularity in `crates/valentine_gen/src/generator/*` rather than embedding more logic into generated modules.
- Emitted Rust should be straightforward to inspect: explicit imports, clear module boundaries, and minimal glob ambiguity.

## Valentine Notes
- `valentine` re-exports generated version crates behind feature flags.
- The checked-in workspace currently exposes `bedrock_1_26_0`.
- Prefer importing from `valentine::bedrock::version::vX_Y_Z` in application code unless the raw generated crate is specifically needed.
- Bedrock strings are intentionally decoded lossily for wire compatibility with existing ecosystem behavior such as `gophertunnel`.

## Valentine Generator Notes
- `crates/valentine_gen/src/main.rs` is the generator entrypoint.
- `src/ir.rs` is the internal representation of parsed schema data.
- `src/generator/resolver.rs` is the main analysis layer for containers, dependencies, and codec args.
- `src/generator/definitions.rs` handles type/container definition emission and canonical registration.
- `src/generator/codec.rs` handles `BedrockCodec` generation.
- `src/generator/mod.rs` coordinates module generation and version crate output.
- Prefer helper modules for naming and emission when logic starts mixing traversal, typing, and file writing.
- If cross-version reuse is changed, also verify the generated version crate `Cargo.toml` dependency wiring.

## Practical Conventions
- Use `rg` / `rg --files` for search.
- Prefer small helper functions and modules when generator logic starts mixing traversal, naming, and token emission.
- Add doc comments where they improve maintainability, especially around protocol layout, generator phases, and non-obvious data transformations.
- Update nearby `README.md` files when behavior, workflow, or architecture changes.
- Avoid hiding protocol quirks in fallback logic. Failing fast is usually better than silently generating guessed wire types.

## Common Commands
- Build workspace: `cargo build --workspace`
- Lint workspace: `cargo clippy --workspace --all-targets`
- Lint a crate: `cargo clippy -p valentine_gen --all-targets`
- Run `valentine` tests: `cargo test -p valentine --tests`
- Generate latest Bedrock protocol: `cargo run -p valentine_gen -- --latest`
- Generate only protocol code: `cargo run -p valentine_gen -- --latest --proto`
- Generate specific versions together for dedup consideration: `cargo run -p valentine_gen -- --versions 1.21.120,1.21.124,1.26.0`

## High-Value Paths
- Root workspace manifest: `Cargo.toml`
- Root overview: `README.md`
- Legacy repo notes: `CLAUDE.md`
- Valentine crate docs: `crates/valentine/README.md`
- Valentine generator docs: `crates/valentine_gen/README.md`
- Generator entrypoint: `crates/valentine_gen/src/main.rs`
- Generator modules: `crates/valentine_gen/src/generator/`
- Runtime protocol surface: `crates/valentine/src/`

## Legacy Note
- `CLAUDE.md` contains older repo guidance.
- Prefer this `AGENTS.md` as the working context file for future repository tasks.
