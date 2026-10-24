//! Emits `java_blocks.rs`: Java state -> Bedrock runtime id and block-class tables.

use proc_macro2::{Literal, TokenStream};
use quote::quote;

use crate::codegen::data::java_blocks::JavaBlocks;

pub fn emit(blocks: &JavaBlocks) -> TokenStream {
    let java_version = &blocks.java_version;
    let state_count = blocks.state_count();
    let bedrock = blocks
        .bedrock_table()
        .iter()
        .map(|&v| Literal::u32_unsuffixed(v));
    let classes = blocks
        .class_table()
        .iter()
        .map(|&v| Literal::u8_unsuffixed(v));
    let bedrock_classes = blocks.bedrock_class_table();
    let bedrock_len = bedrock_classes.len();
    let bedrock_classes = bedrock_classes.iter().map(|&v| Literal::u8_unsuffixed(v));
    quote! {
        //! Java block states of the pinned Java version, mapped to Bedrock runtime ids.
        //! Generated from `java_data/` by `codegen/emitter/java_blocks.rs`.

        /// Java version the state ids belong to.
        pub const JAVA_VERSION: &str = #java_version;

        /// Number of Java block states.
        pub const JAVA_STATE_COUNT: usize = #state_count;

        /// Bedrock runtime id of every Java state id.
        pub static JAVA_TO_BEDROCK: [u32; #state_count] = [#(#bedrock),*];

        /// `block_class` flags of every Java state id.
        pub static JAVA_STATE_CLASSES: [u8; #state_count] = [#(#classes),*];

        /// `block_class` flags per Bedrock runtime id, as seen by the Java-style generator
        /// (`UNMAPPED` where no Java state maps).
        pub static BEDROCK_STATE_CLASSES: [u8; #bedrock_len] = [#(#bedrock_classes),*];
    }
}
