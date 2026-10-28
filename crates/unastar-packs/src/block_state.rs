//! Source-edition block-state semantics shared by build-time template consumers.

/// Pinned Java fluid-bearing blocks need Bedrock's secondary source-water layer.
/// Ordinary Java water already occupies the primary layer. This predicate runs
/// before any lossy state mapping; a Bedrock state cannot recover waterlogging.
pub fn java_secondary_water(name: &str, waterlogged: bool) -> bool {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    name != "water" && (waterlogged || matches!(name,
        "bubble_column" | "kelp" | "kelp_plant" | "seagrass" | "tall_seagrass"
    ))
}
