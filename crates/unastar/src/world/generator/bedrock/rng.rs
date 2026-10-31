//! Bedrock's game random ([`BedrockRandom`], an MT19937 shared with the
//! enchanting table in `unastar_rules::random`) and the seeding of feature
//! rule streams. Byte-exact decoration also needs BDS's per-chunk seeding
//! and each feature's draw order (research W14).

pub use unastar_rules::random::BedrockRandom;

/// Seed of one feature rule's random stream in one chunk.
///
/// BDS gives every feature rule its own stream: the chunk's population seed
/// combined with `rule_hash`, the low 32 bits of the 64-bit FNV-1 hash
/// (multiply, then xor) of the rule identifier, as
/// `((s << 6) + rule_hash + (s >> 2) + 0x9e3779b9) ^ s`. The scatter and the
/// placed feature each start a fresh stream from this seed. Measured on
/// authored single-marker probe rules in three natural seed-12345 BDS 1.26.51
/// worlds: every probe position in all 202 chunk observations matches.
#[inline]
pub const fn feature_rule_seed(population_seed: u32, rule_hash: u32) -> u32 {
    (population_seed << 6)
        .wrapping_add(rule_hash)
        .wrapping_add(population_seed >> 2)
        .wrapping_add(0x9e37_79b9)
        ^ population_seed
}
