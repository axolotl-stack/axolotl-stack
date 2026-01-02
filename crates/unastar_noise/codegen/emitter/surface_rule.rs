//! Surface rule code emitter.
//!
//! This module generates Rust code from parsed surface rule JSON.
//! The generated code creates `Box<dyn Rule>` objects that use the
//! closure-based block lookup pattern.

use crate::codegen::parser::surface_rule::{ConditionSource, RuleSource, VerticalAnchor};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::path::Path;

/// Emit surface rule code to the output directory.
pub fn emit_surface_rules(
    output_dir: &Path,
    surface_rule: &RuleSource,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut emitter = SurfaceRuleEmitter::new();
    let rule_code = emitter.emit_rule(surface_rule);

    let code = quote! {
        // Generated overworld surface rules - do not edit manually.

        use crate::surface::{
            AbovePreliminarySurface, BiomeCheck, BlockRule, BandlandsRule, CaveSurface, Hole,
            NoiseThreshold, Not, Rule, SequenceRule, Steep, StoneDepthCheck, TestRule,
            VerticalGradient, WaterCheck, YCheck, VerticalAnchor,
        };
        use crate::Biome;
        use crate::noise::DoublePerlinNoise;
        use crate::xoroshiro::Xoroshiro128;

        /// Build the vanilla overworld surface rule from JSON.
        pub fn build_vanilla_surface_rule(seed: i64) -> Box<dyn Rule> {
            #rule_code
        }
    };

    std::fs::write(output_dir.join("surface_rules.rs"), code.to_string())?;

    Ok(())
}

/// Surface rule code emitter.
struct SurfaceRuleEmitter {
    /// Counter for generating unique noise variable names.
    noise_counter: u32,
}

impl SurfaceRuleEmitter {
    fn new() -> Self {
        Self { noise_counter: 0 }
    }

    /// Emit code for a rule.
    fn emit_rule(&mut self, rule: &RuleSource) -> TokenStream {
        match rule {
            RuleSource::Block { result_state } => {
                let block_name = &result_state.name;
                quote! {
                    Box::new(BlockRule::new(#block_name))
                }
            }

            RuleSource::Sequence { sequence } => {
                let rules: Vec<TokenStream> = sequence.iter().map(|r| self.emit_rule(r)).collect();
                quote! {
                    Box::new(SequenceRule::new(vec![#(#rules),*]))
                }
            }

            RuleSource::Condition { if_true, then_run } => {
                let condition = self.emit_condition(if_true);
                let then_rule = self.emit_rule(then_run);
                quote! {
                    Box::new(TestRule::new(#condition, #then_rule))
                }
            }

            RuleSource::Bandlands {} => {
                quote! {
                    Box::new(BandlandsRule::new(seed))
                }
            }
        }
    }

    /// Emit code for a condition.
    fn emit_condition(&mut self, condition: &ConditionSource) -> TokenStream {
        match condition {
            ConditionSource::Biome { biome_is } => {
                let biomes: Vec<TokenStream> = biome_is
                    .iter()
                    .map(|name| self.emit_biome_ident(name))
                    .collect();

                if biomes.len() == 1 {
                    let biome = &biomes[0];
                    quote! {
                        Box::new(BiomeCheck::single(#biome))
                    }
                } else {
                    quote! {
                        Box::new(BiomeCheck::multiple(vec![#(#biomes),*]))
                    }
                }
            }

            ConditionSource::StoneDepth {
                offset,
                add_surface_depth,
                secondary_depth_range,
                surface_type,
            } => {
                let surface_type_enum = if surface_type == "ceiling" {
                    quote! { CaveSurface::Ceiling }
                } else {
                    quote! { CaveSurface::Floor }
                };

                quote! {
                    Box::new(StoneDepthCheck {
                        offset: #offset,
                        add_surface_depth: #add_surface_depth,
                        secondary_depth_range: #secondary_depth_range,
                        surface_type: #surface_type_enum,
                    })
                }
            }

            ConditionSource::YAbove {
                anchor,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                let anchor_code = self.emit_anchor(anchor);
                quote! {
                    Box::new(YCheck {
                        anchor: #anchor_code,
                        surface_depth_multiplier: #surface_depth_multiplier,
                        add_stone_depth: #add_stone_depth,
                    })
                }
            }

            ConditionSource::Water {
                offset,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                quote! {
                    Box::new(WaterCheck {
                        offset: #offset,
                        surface_depth_multiplier: #surface_depth_multiplier,
                        add_stone_depth: #add_stone_depth,
                    })
                }
            }

            ConditionSource::NoiseThreshold {
                noise,
                min_threshold,
                max_threshold,
            } => {
                // Generate a noise instance for this threshold check
                let noise_var = format_ident!("noise_{}", self.noise_counter);
                self.noise_counter += 1;

                // We need to create the noise at runtime with the seed
                // For now, just inline the noise creation
                let noise_name = noise.strip_prefix("minecraft:").unwrap_or(noise);

                quote! {
                    {
                        // Create noise for threshold check
                        // Note: In a full implementation, this would use the actual noise parameters
                        // For now, we use a placeholder noise
                        let mut rng = Xoroshiro128::from_seed(seed.wrapping_add(#noise_name.as_bytes().iter().fold(0i64, |acc, &b| acc.wrapping_add(b as i64))));
                        let #noise_var = DoublePerlinNoise::new(&mut rng, &[1.0, 1.0], -6);
                        Box::new(NoiseThreshold {
                            noise: #noise_var,
                            min_threshold: #min_threshold,
                            max_threshold: #max_threshold,
                        })
                    }
                }
            }

            ConditionSource::VerticalGradient {
                random_name: _,
                true_at_and_below,
                false_at_and_above,
            } => {
                let true_y = self.resolve_anchor_value(true_at_and_below);
                let false_y = self.resolve_anchor_value(false_at_and_above);

                quote! {
                    Box::new(VerticalGradient::new(#true_y, #false_y, seed))
                }
            }

            ConditionSource::Steep {} => {
                quote! {
                    Box::new(Steep)
                }
            }

            ConditionSource::Hole {} => {
                quote! {
                    Box::new(Hole)
                }
            }

            ConditionSource::AbovePreliminarySurface {} => {
                quote! {
                    Box::new(AbovePreliminarySurface)
                }
            }

            ConditionSource::Temperature {} => {
                // Temperature check - needs a noise source
                // For simplicity, we'll emit a placeholder that always passes
                // A full implementation would sample the temperature noise
                quote! {
                    {
                        let mut rng = Xoroshiro128::from_seed(seed.wrapping_add(0x54656D7065726174)); // "Temperat"
                        let noise = DoublePerlinNoise::new(&mut rng, &[1.0, 1.0], -6);
                        Box::new(crate::surface::Temperature::new(noise, -1.0, 0.2))
                    }
                }
            }

            ConditionSource::Not { invert } => {
                let inner = self.emit_condition(invert);
                quote! {
                    Box::new(Not::new(#inner))
                }
            }
        }
    }

    /// Emit a vertical anchor.
    fn emit_anchor(&self, anchor: &VerticalAnchor) -> TokenStream {
        match anchor {
            VerticalAnchor::Absolute { absolute } => {
                quote! { VerticalAnchor::Absolute(#absolute) }
            }
            VerticalAnchor::AboveBottom { above_bottom } => {
                quote! { VerticalAnchor::AboveBottom(#above_bottom) }
            }
            VerticalAnchor::BelowTop { below_top } => {
                quote! { VerticalAnchor::BelowTop(#below_top) }
            }
        }
    }

    /// Resolve an anchor to a concrete Y value (using default world bounds).
    fn resolve_anchor_value(&self, anchor: &VerticalAnchor) -> i32 {
        const MIN_Y: i32 = -64;
        const MAX_Y: i32 = 320;

        match anchor {
            VerticalAnchor::Absolute { absolute } => *absolute,
            VerticalAnchor::AboveBottom { above_bottom } => MIN_Y + above_bottom,
            VerticalAnchor::BelowTop { below_top } => MAX_Y - below_top,
        }
    }

    /// Convert a biome name to a Biome enum identifier.
    fn emit_biome_ident(&self, name: &str) -> TokenStream {
        // Strip minecraft: prefix
        let name = name.strip_prefix("minecraft:").unwrap_or(name);

        // Map some biome names that differ between JSON and our enum
        let mapped_name = match name {
            "windswept_gravelly_hills" => "gravelly_mountains",
            "old_growth_birch_forest" => "tall_birch_forest",
            "snowy_taiga" => "snowy_taiga",
            other => other,
        };

        let pascal = to_pascal_case(mapped_name);
        let ident = format_ident!("{}", pascal);
        quote! { Biome::#ident }
    }
}

/// Convert snake_case to PascalCase.
fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("desert"), "Desert");
        assert_eq!(to_pascal_case("snowy_plains"), "SnowyPlains");
        assert_eq!(to_pascal_case("deep_frozen_ocean"), "DeepFrozenOcean");
    }

    #[test]
    fn test_emit_block_rule() {
        let mut emitter = SurfaceRuleEmitter::new();
        let rule = RuleSource::Block {
            result_state: crate::codegen::parser::surface_rule::BlockState {
                name: "minecraft:grass_block".to_string(),
                properties: None,
            },
        };

        let code = emitter.emit_rule(&rule);
        let code_str = code.to_string();
        assert!(code_str.contains("BlockRule"));
        assert!(code_str.contains("minecraft:grass_block"));
    }

    #[test]
    fn test_emit_biome_check() {
        let mut emitter = SurfaceRuleEmitter::new();
        let condition = ConditionSource::Biome {
            biome_is: vec!["minecraft:desert".to_string()],
        };

        let code = emitter.emit_condition(&condition);
        let code_str = code.to_string();
        assert!(code_str.contains("BiomeCheck"));
        assert!(code_str.contains("Desert"));
    }

    #[test]
    fn test_resolve_anchor() {
        let emitter = SurfaceRuleEmitter::new();

        assert_eq!(
            emitter.resolve_anchor_value(&VerticalAnchor::Absolute { absolute: 63 }),
            63
        );
        assert_eq!(
            emitter.resolve_anchor_value(&VerticalAnchor::AboveBottom { above_bottom: 0 }),
            -64
        );
        assert_eq!(
            emitter.resolve_anchor_value(&VerticalAnchor::AboveBottom { above_bottom: 5 }),
            -59
        );
        assert_eq!(
            emitter.resolve_anchor_value(&VerticalAnchor::BelowTop { below_top: 0 }),
            320
        );
    }
}
