# valentine_gen Cleanup Implementation Plan

## Overview

This plan covers a comprehensive cleanup of `valentine_gen`:
1. **General code cleanup** - Fix compiler warnings, remove dead code, improve error handling in generator
2. **Custom error types** - Replace `std::io::Error` with `thiserror`-based error enum
3. **Naming convention change** - `PacketText` → `TextPacket` (suffix-based naming)
4. **File consolidation** - Consolidate to `proto.rs` and `types.rs` for faster compilation

## Current State Analysis

### Naming Convention
- Location: [crates/valentine_gen/src/generator/mod.rs:107-112](crates/valentine_gen/src/generator/mod.rs#L107-L112)
- Pattern: `Packet{Name}` prefix (e.g., `PacketText`, `PacketAddPlayer`)
- ~300+ packet types affected

### File Organization
- Location: [crates/valentine_gen/src/generator/utils.rs:172-180](crates/valentine_gen/src/generator/utils.rs#L172-L180)
- Currently: ~100 packet files, ~70 type files grouped by first token
- Output: `packets/{first_token}.rs`, `types/{first_token}.rs`

### Error Handling
- Location: [crates/valentine_gen/src/generator/codec.rs:448-463](crates/valentine_gen/src/generator/codec.rs#L448-L463)
- Currently: `std::io::Error` with `InvalidData` and `UnexpectedEof` kinds
- No structured context, just string messages

### Code Quality Issues
Current issues identified via `cargo clippy` and code analysis:
- **Compile error**: Missing `tracing-subscriber/fmt` feature at [main.rs:13](crates/valentine_gen/src/main.rs#L13)
- **Unused variable**: `size_lit` at [codec.rs:1085](crates/valentine_gen/src/generator/codec.rs#L1085)
- **Unused variable**: `case_name` at [codec.rs:1447](crates/valentine_gen/src/generator/codec.rs#L1447)
- **Unused assignment**: `args_struct` at [mod.rs:479](crates/valentine_gen/src/generator/mod.rs#L479)
- **Dead code**: `McString` variant in IR marked with `#[allow(dead_code)]` at [ir.rs:27](crates/valentine_gen/src/ir.rs#L27)
- **Unwraps without context**: 10 occurrences of `.unwrap()` that could panic without helpful messages
- **Hardcoded version filter**: Versions "0.14", "0.15", "1.0" filtered without explanation at [main.rs:293](crates/valentine_gen/src/main.rs#L293)
- **Clippy allow**: `#[allow(clippy::too_many_arguments)]` at [codec.rs:2136](crates/valentine_gen/src/generator/codec.rs#L2136)

## Desired End State

After this plan is complete:

1. **Code quality**: Zero compiler warnings, zero clippy warnings, no panicking unwraps
2. **Error types**: A `DecodeError` enum in `valentine_bedrock_core` with variants for each error case, using `thiserror` for Display/Error derives
3. **Naming**: All packets use suffix naming (`TextPacket`, `AddPlayerPacket`)
4. **File structure**: Two files per version: `proto.rs` (all packets) and `types.rs` (all types)
5. **Verification**: `cargo build` succeeds, `cargo clippy` clean, existing tests pass

## What We're NOT Doing

- Migration aliases for old names (this is a breaking change)
- Changing the IR representation
- Modifying the parser
- Changing the BedrockCodec trait signature (encode still returns `std::io::Error`)
- Adding new tests beyond verifying existing ones pass

## Implementation Approach

We'll implement in four phases, each independently verifiable:
1. General code cleanup (fix warnings, improve reliability)
2. Error types (foundation for better error handling)
3. Naming convention (affects generated code structure)
4. File consolidation (final output change)

---

## Phase 1: General Code Cleanup

### Overview
Fix all compiler warnings, clippy warnings, and improve code reliability by replacing panicking unwraps with proper error handling.

### Changes Required:

#### 1. Fix tracing-subscriber feature flag
**File**: `crates/valentine_gen/Cargo.toml`
**Changes**: Add the `fmt` feature to tracing-subscriber dependency

```toml
[dependencies]
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
```

#### 2. Fix unused variable `size_lit`
**File**: `crates/valentine_gen/src/generator/codec.rs`
**Changes**: At line 1085, prefix with underscore or remove if truly unused

```rust
// Change:
let size_lit = proc_macro2::Literal::usize_unsuffixed(*size);
// To:
let _size_lit = proc_macro2::Literal::usize_unsuffixed(*size);
```

Or remove the variable entirely if the value is not needed.

#### 3. Fix unused variable `case_name`
**File**: `crates/valentine_gen/src/generator/codec.rs`
**Changes**: At line 1447, prefix with underscore

```rust
// Change:
let (case_name, case_type) = &fields[0];
// To:
let (_case_name, case_type) = &fields[0];
```

#### 4. Fix unused assignment `args_struct`
**File**: `crates/valentine_gen/src/generator/mod.rs`
**Changes**: At line 479, remove the unused `mut` or the assignment

Investigate if `args_struct` is actually used. If not, remove it entirely. If only assigned once, remove `mut`.

#### 5. Remove dead code marker or use McString
**File**: `crates/valentine_gen/src/ir.rs`
**Changes**: At line 27, either remove `#[allow(dead_code)]` and use the variant, or remove the variant entirely

Option A - Remove the variant if unused:
```rust
// Delete:
#[allow(dead_code)]
McString, // The 'pstring' or 'string' type
```

Option B - Use the variant in the parser/generator if it should exist.

#### 6. Replace panicking unwraps with proper error handling
**File**: `crates/valentine_gen/src/main.rs`
**Changes**: Replace `.unwrap()` calls with `.ok_or()` or `.context()` where appropriate

At line 263:
```rust
// Change:
let valentine_root = root.parent().unwrap().join("valentine");
// To:
let valentine_root = root.parent()
    .ok_or_else(|| anyhow::anyhow!("CARGO_MANIFEST_DIR has no parent directory"))?
    .join("valentine");
```

At line 648:
```rust
// Change:
let valentine_cargo = root.parent().unwrap().join("valentine").join("Cargo.toml");
// To:
let valentine_cargo = root.parent()
    .ok_or_else(|| anyhow::anyhow!("CARGO_MANIFEST_DIR has no parent directory"))?
    .join("valentine").join("Cargo.toml");
```

At line 666:
```rust
// Change:
let deps_tbl = doc["dependencies"].as_table_mut().unwrap();
// To:
let deps_tbl = doc["dependencies"].as_table_mut()
    .ok_or_else(|| anyhow::anyhow!("Cargo.toml missing [dependencies] table"))?;
```

At line 691:
```rust
// Change:
let features_tbl = doc["features"].as_table_mut().unwrap();
// To:
let features_tbl = doc["features"].as_table_mut()
    .ok_or_else(|| anyhow::anyhow!("Cargo.toml missing [features] table"))?;
```

**File**: `crates/valentine_gen/src/generator/definitions.rs`
**Changes**: Replace iterator unwraps with safer alternatives

At line 285:
```rust
// Change:
let (_case_name, case_type) = &fields.iter().next().unwrap();
// To:
let Some((_case_name, case_type)) = fields.iter().next() else {
    return Err("switch has no fields".into());
};
```

At line 562 and 606: Apply similar pattern.

**File**: `crates/valentine_gen/src/generator/codec.rs`
**Changes**: At lines 2332, 2479, 2480

At line 2332:
```rust
// Change:
let (case_name, case_type) = fields.first().unwrap();
// To:
let Some((case_name, case_type)) = fields.first() else {
    return Err("switch has no fields".into());
};
```

At lines 2479-2480, ensure the `or()` chain has a valid fallback or add error handling.

#### 7. Add comment explaining version filter
**File**: `crates/valentine_gen/src/main.rs`
**Changes**: At line 293, add a comment explaining why these versions are filtered

```rust
// Filter out legacy protocol versions that have incompatible schema formats
// or are missing required type definitions in minecraft-data.
.filter(|v| v != "0.14" && v != "0.15" && v != "1.0")
```

#### 8. Consider refactoring function with too many arguments
**File**: `crates/valentine_gen/src/generator/codec.rs`
**Changes**: At line 2136, the `#[allow(clippy::too_many_arguments)]` indicates a function that could benefit from a config/options struct

Create a struct to hold the arguments:
```rust
struct SwitchEncodeContext<'a> {
    switch_type: &'a Type,
    switch_ident: &'a syn::Ident,
    compare_to: &'a str,
    fields: &'a [(String, Type)],
    default: &'a Type,
    ctx: &'a mut Context<'a>,
    // ... other fields
}
```

Then refactor the function to take `SwitchEncodeContext` instead of many individual arguments. This is optional but improves maintainability.

### Success Criteria:

#### Automated Verification:
- [x] `cargo build -p valentine_gen` succeeds with zero warnings
- [x] `cargo clippy -p valentine_gen` produces zero warnings
- [x] `cargo run -p valentine_gen -- protocol` succeeds (Note: command is `--proto --latest`)
- [x] `cargo build -p valentine` succeeds
- [x] `cargo test -p valentine` passes

#### Manual Verification:
- [ ] Review code changes to ensure error messages are helpful
- [ ] Verify no functionality was broken by the cleanup

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation before proceeding to Phase 2.

---

## Phase 2: Custom Error Types with thiserror

### Overview
Create a `DecodeError` enum in `valentine_bedrock_core` and update the generator to emit code using these error types instead of `std::io::Error`.

### Changes Required:

#### 1. Add thiserror dependency
**File**: `crates/valentine/bedrock_core/Cargo.toml`
**Changes**: Add thiserror to dependencies

```toml
[dependencies]
thiserror = "1.0"
```

#### 2. Create DecodeError enum
**File**: `crates/valentine/bedrock_core/src/bedrock/error.rs` (new file)
**Changes**: Define error types for all decode failure cases

```rust
use thiserror::Error;

/// Errors that can occur when decoding Bedrock protocol data.
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("unexpected end of buffer: needed {needed} bytes, had {available}")]
    UnexpectedEof { needed: usize, available: usize },

    #[error("negative length not allowed: got {value}")]
    NegativeLength { value: i64 },

    #[error("string length {declared} exceeds remaining buffer {available}")]
    StringLengthExceeded { declared: usize, available: usize },

    #[error("array length {declared} exceeds remaining buffer {available}")]
    ArrayLengthExceeded { declared: usize, available: usize },

    #[error("invalid enum value {value} for {enum_name}")]
    InvalidEnumValue { enum_name: &'static str, value: i64 },

    #[error("invalid packet id: {id}")]
    InvalidPacketId { id: u32 },

    #[error("expected magic byte 0x{expected:02x}, got 0x{actual:02x}")]
    InvalidMagicByte { expected: u8, actual: u8 },

    #[error("packet length {declared} exceeds available {available}")]
    PacketLengthExceeded { declared: usize, available: usize },

    #[error("utf8 decode error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<DecodeError> for std::io::Error {
    fn from(err: DecodeError) -> Self {
        use std::io::ErrorKind;
        match &err {
            DecodeError::UnexpectedEof { .. } => {
                std::io::Error::new(ErrorKind::UnexpectedEof, err)
            }
            _ => std::io::Error::new(ErrorKind::InvalidData, err),
        }
    }
}
```

#### 3. Export error module
**File**: `crates/valentine/bedrock_core/src/bedrock/mod.rs`
**Changes**: Add `pub mod error;` and re-export

```rust
pub mod error;
pub use error::DecodeError;
```

#### 4. Update BedrockCodec trait to use DecodeError
**File**: `crates/valentine/bedrock_core/src/bedrock/codec.rs`
**Changes**: Change decode return type from `std::io::Error` to `DecodeError`

```rust
use super::error::DecodeError;

pub trait BedrockCodec: Sized {
    type Args;

    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error>;
    fn decode<B: Buf>(buf: &mut B, args: Self::Args) -> Result<Self, DecodeError>;
}
```

#### 5. Update primitive BedrockCodec implementations
**File**: `crates/valentine/bedrock_core/src/bedrock/codec.rs`
**Changes**: Update all decode implementations to return `DecodeError`

Example for `bool`:
```rust
impl BedrockCodec for bool {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u8(u8::from(*self));
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if !buf.has_remaining() {
            return Err(DecodeError::UnexpectedEof { needed: 1, available: 0 });
        }
        Ok(buf.get_u8() != 0)
    }
}
```

#### 6. Update generator codec.rs - array length errors
**File**: `crates/valentine_gen/src/generator/codec.rs`
**Changes**: Update array length validation at lines 448-463

Replace:
```rust
return Err(std::io::Error::new(
    std::io::ErrorKind::InvalidData,
    "array length cannot be negative",
));
```

With:
```rust
return Err(crate::bedrock::error::DecodeError::NegativeLength { value: raw });
```

#### 7. Update generator codec.rs - string length errors
**File**: `crates/valentine_gen/src/generator/codec.rs`
**Changes**: Update string validation at lines 565-592

Replace buffer check:
```rust
return Err(std::io::Error::new(
    std::io::ErrorKind::UnexpectedEof,
    format!("string declared length {} exceeds remaining {}", len, buf.remaining()),
));
```

With:
```rust
return Err(crate::bedrock::error::DecodeError::StringLengthExceeded {
    declared: len,
    available: buf.remaining(),
});
```

#### 8. Update generator codec.rs - fixed array EOF
**File**: `crates/valentine_gen/src/generator/codec.rs`
**Changes**: Update fixed array check at lines 481-490

Replace:
```rust
return Err(std::io::Error::new(
    std::io::ErrorKind::UnexpectedEof,
    format!("fixed array requires {} bytes but only {} remaining", #size_lit, buf.remaining()),
));
```

With:
```rust
return Err(crate::bedrock::error::DecodeError::UnexpectedEof {
    needed: #size_lit,
    available: buf.remaining(),
});
```

#### 9. Update generator mod.rs - enum validation
**File**: `crates/valentine_gen/src/generator/mod.rs`
**Changes**: Update enum decode error at line 540

Replace:
```rust
Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Invalid enum value for {}: {}", stringify!(McpePacketName), id)))
```

With:
```rust
Err(crate::bedrock::error::DecodeError::InvalidEnumValue {
    enum_name: stringify!(McpePacketName),
    value: id as i64,
})
```

#### 10. Update generator mod.rs - packet frame validation
**File**: `crates/valentine_gen/src/generator/mod.rs`
**Changes**: Update packet frame errors at lines 632-677

Replace length check:
```rust
return Err(std::io::Error::new(
    std::io::ErrorKind::UnexpectedEof,
    format!("declared game packet length {} exceeds available {}", declared_len, buf.remaining()),
));
```

With:
```rust
return Err(crate::bedrock::error::DecodeError::PacketLengthExceeded {
    declared: declared_len,
    available: buf.remaining(),
});
```

Replace magic byte check:
```rust
return Err(std::io::Error::new(
    std::io::ErrorKind::InvalidData,
    format!("expected GAME_PACKET_ID=0x{GAME_PACKET_ID:02x}, got 0x{leading:02x}"),
));
```

With:
```rust
return Err(crate::bedrock::error::DecodeError::InvalidMagicByte {
    expected: GAME_PACKET_ID,
    actual: leading,
});
```

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build -p valentine_bedrock_core` succeeds
- [ ] `cargo build -p valentine_gen` succeeds
- [ ] Run generator: `cargo run -p valentine_gen -- protocol` succeeds
- [ ] `cargo build -p valentine` succeeds (generated code compiles)
- [ ] `cargo test -p valentine` passes

#### Manual Verification:
- [ ] Inspect generated code to verify DecodeError variants are used
- [ ] Verify error messages are descriptive when triggering a decode error

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation before proceeding to Phase 3.

---

## Phase 3: Naming Convention Change (PacketText → TextPacket)

### Overview
Change packet naming from prefix-based (`PacketText`) to suffix-based (`TextPacket`). This is a breaking change affecting ~300+ packet types.

### Changes Required:

#### 1. Update packet name generation
**File**: `crates/valentine_gen/src/generator/mod.rs`
**Changes**: Modify lines 107-112 to use suffix instead of prefix

Replace:
```rust
let base_name = camel_case(&packet.name);
let struct_name = if base_name.starts_with("Packet") {
    base_name
} else {
    format!("Packet{}", base_name)
};
```

With:
```rust
let base_name = camel_case(&packet.name);
let struct_name = if base_name.ends_with("Packet") {
    base_name
} else if base_name.starts_with("Packet") {
    // Convert old-style PacketFoo to FooPacket
    format!("{}Packet", base_name.trim_start_matches("Packet"))
} else {
    format!("{}Packet", base_name)
};
```

#### 2. Update McpePacketData variant naming
**File**: `crates/valentine_gen/src/generator/mod.rs`
**Changes**: Modify lines 363-369 for enum variant payload types

Replace:
```rust
let payload_ident = if name_pascal.starts_with("Packet") {
    format_ident!("{}", name_pascal)
} else {
    format_ident!("Packet{}", name_pascal)
};
```

With:
```rust
let payload_ident = if name_pascal.ends_with("Packet") {
    format_ident!("{}", name_pascal)
} else if name_pascal.starts_with("Packet") {
    format_ident!("{}Packet", name_pascal.trim_start_matches("Packet"))
} else {
    format_ident!("{}Packet", name_pascal)
};
```

#### 3. Update group name derivation
**File**: `crates/valentine_gen/src/generator/utils.rs`
**Changes**: Modify `get_group_name()` at lines 172-180 to handle suffix naming

Replace:
```rust
pub fn get_group_name(struct_name: &str) -> String {
    if struct_name.starts_with("Packet") {
        let base = struct_name.trim_start_matches("Packet");
        let token = first_token_from_pascal(base);
        return format!("packets/{}", token);
    }
    let token = first_token_from_pascal(struct_name);
    format!("types/{}", token)
}
```

With:
```rust
pub fn get_group_name(struct_name: &str) -> String {
    if struct_name.ends_with("Packet") {
        let base = struct_name.trim_end_matches("Packet");
        let token = first_token_from_pascal(base);
        return format!("packets/{}", token);
    }
    let token = first_token_from_pascal(struct_name);
    format!("types/{}", token)
}
```

#### 4. Update duplicate alias detection
**File**: `crates/valentine_gen/src/generator/utils.rs`
**Changes**: Modify `packet_duplicate_alias()` at lines 114-135

Replace:
```rust
pub fn packet_duplicate_alias(type_name: &str) -> Option<String> {
    let stripped = type_name.strip_prefix("Packet")?;
    // ...
}
```

With:
```rust
pub fn packet_duplicate_alias(type_name: &str) -> Option<String> {
    let stripped = type_name.strip_suffix("Packet")?;
    if stripped.is_empty() {
        return None;
    }

    let snake = stripped.to_case(Case::Snake);
    let mut parts: Vec<&str> = snake.split('_').filter(|s| !s.is_empty()).collect();

    let mut changed = false;
    while parts.len() >= 2 && parts[parts.len() - 1] == parts[parts.len() - 2] {
        parts.pop();
        changed = true;
    }

    if !changed || parts.is_empty() {
        return None;
    }

    let aliased = parts.join("_").to_case(Case::Pascal);
    Some(clean_type_name(&aliased))
}
```

#### 5. Update nested type naming hints
**File**: `crates/valentine_gen/src/generator/definitions.rs`
**Changes**: Ensure inline types for packets also use suffix naming

At line 36, the type hint generation should handle the new naming:
```rust
// If parent is a packet (ends with "Packet"), nested types should be:
// TextPacket -> TextPacketContent, TextPacketExtra, etc.
// This already works as-is since we just append the field name
```

No changes needed here - nested types use `{ParentName}{FieldName}` pattern which remains correct.

### Success Criteria:

#### Automated Verification:
- [x] `cargo build -p valentine_gen` succeeds
- [x] Run generator: `cargo run -p valentine_gen -- protocol` succeeds (Note: command is `--proto --latest`)
- [x] `cargo build -p valentine` succeeds (generated code compiles)
- [x] `cargo test -p valentine` passes

#### Manual Verification:
- [ ] Inspect generated `packets/text.rs` - verify `TextPacket` not `PacketText`
- [ ] Inspect generated `lib.rs` - verify `McpePacketData` variants use new naming
- [ ] Spot-check 5 packet files for correct naming pattern

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation before proceeding to Phase 4.

---

## Phase 4: File Consolidation (proto.rs + types.rs)

### Overview
Consolidate all packets into `proto.rs` and all types into `types.rs` instead of many small files grouped by first token. This should improve compilation speed.

### Changes Required:

#### 1. Remove group name derivation
**File**: `crates/valentine_gen/src/generator/utils.rs`
**Changes**: Simplify `get_group_name()` to return just "packets" or "types"

Replace:
```rust
pub fn get_group_name(struct_name: &str) -> String {
    if struct_name.ends_with("Packet") {
        let base = struct_name.trim_end_matches("Packet");
        let token = first_token_from_pascal(base);
        return format!("packets/{}", token);
    }
    let token = first_token_from_pascal(struct_name);
    format!("types/{}", token)
}
```

With:
```rust
pub fn get_group_name(struct_name: &str) -> String {
    if struct_name.ends_with("Packet") {
        "proto".to_string()
    } else {
        "types".to_string()
    }
}
```

#### 2. Update file writing logic
**File**: `crates/valentine_gen/src/generator/mod.rs`
**Changes**: Modify lines 176-272 to write flat files instead of hierarchical directories

Replace the nested directory logic with flat file writing:

```rust
// Write proto.rs (all packets)
if let Some(packet_tokens) = ctx.definitions_by_group.remove("proto") {
    let proto_path = version_dir.join("proto.rs");
    let mut file = File::create(&proto_path)?;

    let final_code = quote! {
        //! Generated protocol packet definitions.
        #![allow(non_camel_case_types)]
        #![allow(non_snake_case)]
        #![allow(dead_code)]
        #![allow(unused_parens)]
        #![allow(clippy::all)]
        use ::bitflags::bitflags;
        use bytes::{Buf, BufMut};
        use crate::types::*;
        use crate::bedrock::codec::BedrockCodec;
        use crate::bedrock::error::DecodeError;

        #(#packet_tokens)*
    };

    let syntax_tree = syn::parse2(final_code.clone()).map_err(|e| {
        let _ = std::fs::write("debug_gen_error_proto.rs", final_code.to_string());
        format!("Failed to parse proto.rs: {}", e)
    })?;
    let formatted = prettyplease::unparse(&syntax_tree);

    write!(file, "// Generated by valentine_gen. Do not edit.\n\n")?;
    write!(file, "{}", formatted)?;
}

// Write types.rs (all types)
if let Some(type_tokens) = ctx.definitions_by_group.remove("types") {
    let types_path = version_dir.join("types.rs");
    let mut file = File::create(&types_path)?;

    let final_code = quote! {
        //! Generated protocol type definitions.
        #![allow(non_camel_case_types)]
        #![allow(non_snake_case)]
        #![allow(dead_code)]
        #![allow(unused_parens)]
        #![allow(clippy::all)]
        use ::bitflags::bitflags;
        use bytes::{Buf, BufMut};
        use crate::bedrock::codec::BedrockCodec;
        use crate::bedrock::error::DecodeError;

        #(#type_tokens)*
    };

    let syntax_tree = syn::parse2(final_code.clone()).map_err(|e| {
        let _ = std::fs::write("debug_gen_error_types.rs", final_code.to_string());
        format!("Failed to parse types.rs: {}", e)
    })?;
    let formatted = prettyplease::unparse(&syntax_tree);

    write!(file, "// Generated by valentine_gen. Do not edit.\n\n")?;
    write!(file, "{}", formatted)?;
}
```

#### 3. Update lib.rs generation
**File**: `crates/valentine_gen/src/generator/mod.rs`
**Changes**: Modify lines 274-332 to export flat modules

Replace nested mod structure with:
```rust
let lib_path = version_dir.join("lib.rs");
let mut lib_file = File::create(lib_path)?;

let lib_code = quote! {
    //! Generated Bedrock protocol for this version.
    #![allow(ambiguous_glob_reexports)]
    #![allow(unused_imports)]

    pub mod proto;
    pub mod types;

    pub use proto::*;
    pub use types::*;

    // Data modules (items, blocks, etc.)
    #(#data_module_tokens)*

    pub mod bedrock {
        pub use valentine_bedrock_core::bedrock::codec;
        pub use valentine_bedrock_core::bedrock::context;
        pub use valentine_bedrock_core::bedrock::version;
        pub use valentine_bedrock_core::bedrock::error;
    }

    pub mod protocol {
        pub use valentine_bedrock_core::protocol::wire;
    }
};

let syntax_tree = syn::parse2(lib_code)?;
let formatted = prettyplease::unparse(&syntax_tree);

write!(lib_file, "// Generated by valentine_gen. Do not edit.\n\n")?;
write!(lib_file, "{}", formatted)?;
```

#### 4. Remove nested mod.rs generation
**File**: `crates/valentine_gen/src/generator/mod.rs`
**Changes**: Delete lines 254-272 that generate nested `packets/mod.rs` and `types/mod.rs`

These are no longer needed since we have flat `proto.rs` and `types.rs`.

#### 5. Clean up old generated directories
**File**: `crates/valentine_gen/src/generator/mod.rs`
**Changes**: Add cleanup at start of generation to remove old `packets/` and `types/` directories

```rust
// Clean up old hierarchical structure if it exists
let packets_dir = version_dir.join("packets");
let types_dir = version_dir.join("types");
if packets_dir.exists() {
    fs::remove_dir_all(&packets_dir)?;
}
if types_dir.exists() {
    fs::remove_dir_all(&types_dir)?;
}
```

#### 6. Update import paths in generated code
**File**: `crates/valentine_gen/src/generator/mod.rs`
**Changes**: Update file header imports to use new flat structure

In packet file header, change:
```rust
use super::super::types::*;
```
To:
```rust
use crate::types::*;
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo build -p valentine_gen` succeeds
- [x] Run generator: `cargo run -p valentine_gen -- protocol` succeeds (Note: command is `--proto --latest`)
- [x] `cargo build -p valentine` succeeds
- [x] `cargo test -p valentine` passes
- [x] Verify old directories are removed: `packets/` and `types/` should not exist

#### Manual Verification:
- [ ] Inspect generated `lib.rs` - verify flat module structure
- [ ] Inspect `proto.rs` - verify all packets are consolidated
- [ ] Inspect `types.rs` - verify all types are consolidated
- [ ] Time compilation: compare build time before/after (should improve)

**Implementation Note**: After completing this phase and all automated verification passes, the cleanup is complete.

---

## Testing Strategy

### Unit Tests:
- Existing roundtrip tests in `crates/valentine/tests/` should pass unchanged
- Error types can be unit tested for correct Display output

### Integration Tests:
- `bedrock_roundtrip.rs` - Tests encode/decode cycle
- `start_game_roundtrip.rs` - Tests complex packet handling

### Manual Testing Steps:
1. Run `cargo run -p valentine_gen -- protocol` and verify no errors
2. Build and test valentine: `cargo test -p valentine`
3. Inspect generated files for correct naming and structure
4. Test a downstream consumer if available

## Performance Considerations

- **File consolidation**: Reduces file I/O during compilation, fewer incremental build units
- **Single large files**: May increase memory usage during parsing but should reduce total compile time
- **Error types**: Enum matching is efficient; `thiserror` has minimal runtime overhead

## Migration Notes

This is a **breaking change**. Consumers must:
1. Update all `Packet*` references to `*Packet` (e.g., `PacketText` → `TextPacket`)
2. Update import paths from `packets::text::PacketText` to `proto::TextPacket`
3. Update error handling to use `DecodeError` instead of `std::io::Error`

No automated migration tool is provided.

## References

- Original research: `thoughts/shared/research/2025-12-28-valentine-gen-protocol-generation.md`
- BedrockCodec trait: `crates/valentine/bedrock_core/src/bedrock/codec.rs`
- Generator entry: `crates/valentine_gen/src/generator/mod.rs`
- Naming utilities: `crates/valentine_gen/src/generator/utils.rs`
- Codec generation: `crates/valentine_gen/src/generator/codec.rs`
