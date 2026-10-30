//! Emits `java_blocks.rs`: Java state -> Bedrock runtime id and block-class tables.

use proc_macro2::{Literal, TokenStream};
use quote::quote;

use crate::codegen::data::java_blocks::{JavaBlocks, class};

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
    let shape_support = blocks
        .shape_support_table()
        .iter()
        .map(|&v| Literal::u8_unsuffixed(v));
    let shape_wall_above = blocks
        .shape_wall_above_table()
        .iter()
        .map(|&v| Literal::u8_unsuffixed(v));
    let shape_center_support = blocks
        .shape_center_support_table()
        .iter()
        .map(|&v| Literal::u8_unsuffixed(v));
    let state_physics = blocks
        .state_physics_table()
        .iter()
        .map(|&v| Literal::u8_unsuffixed(v));

    // Java fluid-bearing plants and waterlogged states retain their source water
    // in Bedrock's second storage. Ordinary water is already the primary block.
    let mut secondary_water: Vec<bool> = blocks
        .class_table()
        .iter()
        .map(|flags| flags & class::WATER != 0)
        .collect();
    for id in blocks
        .state_ids("minecraft:water")
        .expect("pinned Java data requires water")
    {
        secondary_water[id as usize] = false;
    }

    let mut by_name: Vec<_> = blocks.blocks().iter().collect();
    by_name.sort_by(|a, b| a.name.cmp(&b.name));
    let block_count = by_name.len();
    let names = by_name.iter().map(|b| {
        let name = &b.name;
        let first = b.first_id;
        let default = b.default_id;
        quote! { (#name, #first, #default) }
    });

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

        /// Actual native isFaceSturdy faces Down/Up/North/South/West/East (bits 0..5),
        /// Block.isExceptionForConnection (bit 6), and blocksMotion (bit 7).
        pub static SHAPE_SUPPORT_FLAGS: [u8; #state_count] = [#(#shape_support),*];

        /// Actual native wall post/North/East/South/West test-shape coverage by the
        /// collision shape's lower face (bits 0..4).
        pub static SHAPE_WALL_ABOVE_FLAGS: [u8; #state_count] = [#(#shape_wall_above),*];

        /// Actual native CENTER isFaceSturdy faces Down/Up/North/South/West/East.
        /// Consumers still apply native tag-specific support exceptions.
        pub static SHAPE_CENTER_SUPPORT_FLAGS: [u8; #state_count] = [#(#shape_center_support),*];

        /// Actual native isSolid (bit 0), isSolidRender (bit 1),
        /// isCollisionShapeFullBlock (bit 2), canBeReplaced (bit 3), and full upper
        /// collision face (bit 4).
        pub static STATE_PHYSICS_FLAGS: [u8; #state_count] = [#(#state_physics),*];

        /// Java states carrying source water separately from their primary block.
        /// Consumed only at the explicit Java-to-Bedrock chunk boundary.
        pub static JAVA_SECONDARY_WATER: [bool; #state_count] = [#(#secondary_water),*];

        /// `block_class` flags per Bedrock runtime id, as seen by the Java-style generator
        /// (`UNMAPPED` where no Java state maps).
        pub static BEDROCK_STATE_CLASSES: [u8; #bedrock_len] = [#(#bedrock_classes),*];

        /// `(name, first state id, default state id)` of every Java block, sorted by name.
        pub static JAVA_BLOCKS: [(&str, u32, u32); #block_count] = [#(#names),*];

        /// Default Java state id of a namespaced Java block name.
        pub fn default_state(name: &str) -> Option<u32> {
            JAVA_BLOCKS
                .binary_search_by(|(n, _, _)| (*n).cmp(name))
                .ok()
                .map(|i| JAVA_BLOCKS[i].2)
        }
    }
}
