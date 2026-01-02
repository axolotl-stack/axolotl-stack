//! Surface rule parser for deserializing surface rules from JSON.
//!
//! This module parses the `surface_rule` field from noise_settings JSON files
//! (like `overworld.json`) into typed Rust structures that can be used by the
//! code generator.

use serde::Deserialize;
use std::collections::HashMap;

/// Vertical anchor for Y coordinate resolution.
///
/// Used in conditions like `y_above` and `vertical_gradient` to specify
/// Y coordinate thresholds relative to world bounds.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum VerticalAnchor {
    /// Y coordinate above the world bottom (min_y + offset).
    AboveBottom {
        above_bottom: i32,
    },
    /// Y coordinate below the world top (max_y - offset).
    BelowTop {
        below_top: i32,
    },
    /// Absolute Y coordinate.
    Absolute {
        absolute: i32,
    },
}

/// Block state from JSON.
///
/// Represents a block with optional block state properties.
#[derive(Debug, Clone, Deserialize)]
pub struct BlockState {
    /// Block name (e.g., "minecraft:grass_block").
    #[serde(rename = "Name")]
    pub name: String,
    /// Optional block state properties (e.g., {"snowy": "false"}).
    #[serde(rename = "Properties", default)]
    pub properties: Option<HashMap<String, String>>,
}

/// Surface rule condition source.
///
/// Conditions determine when a surface rule should be applied.
/// They are parsed from the `if_true` field of condition rules.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ConditionSource {
    /// Biome check - true if current biome matches any in the list.
    #[serde(rename = "minecraft:biome")]
    Biome {
        biome_is: Vec<String>,
    },

    /// Stone depth check - true if stone depth is within threshold.
    #[serde(rename = "minecraft:stone_depth")]
    StoneDepth {
        offset: i32,
        add_surface_depth: bool,
        secondary_depth_range: i32,
        /// "floor" or "ceiling"
        surface_type: String,
    },

    /// Y above check - true if block Y (optionally modified) is above anchor.
    #[serde(rename = "minecraft:y_above")]
    YAbove {
        anchor: VerticalAnchor,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },

    /// Water check - true if block Y is at or above water level (with offsets).
    #[serde(rename = "minecraft:water")]
    Water {
        offset: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },

    /// Noise threshold check - true if noise value is within range.
    #[serde(rename = "minecraft:noise_threshold")]
    NoiseThreshold {
        noise: String,
        min_threshold: f64,
        max_threshold: f64,
    },

    /// Vertical gradient - probabilistic condition based on Y coordinate.
    #[serde(rename = "minecraft:vertical_gradient")]
    VerticalGradient {
        random_name: String,
        true_at_and_below: VerticalAnchor,
        false_at_and_above: VerticalAnchor,
    },

    /// Steep terrain check - true if on steep slope.
    #[serde(rename = "minecraft:steep")]
    Steep {},

    /// Hole check - true if surface depth <= 0.
    #[serde(rename = "minecraft:hole")]
    Hole {},

    /// Above preliminary surface - true if Y is above computed surface level.
    #[serde(rename = "minecraft:above_preliminary_surface")]
    AbovePreliminarySurface {},

    /// Temperature check - true if temperature is within cold range.
    #[serde(rename = "minecraft:temperature")]
    Temperature {},

    /// Negation - inverts the inner condition.
    #[serde(rename = "minecraft:not")]
    Not {
        invert: Box<ConditionSource>,
    },
}

/// Surface rule source.
///
/// Rules produce block states based on conditions.
/// Parsed from the `surface_rule` field of noise settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum RuleSource {
    /// Sequence of rules - tries each in order until one produces a block.
    #[serde(rename = "minecraft:sequence")]
    Sequence {
        sequence: Vec<RuleSource>,
    },

    /// Conditional rule - runs `then_run` if `if_true` condition passes.
    #[serde(rename = "minecraft:condition")]
    Condition {
        if_true: ConditionSource,
        then_run: Box<RuleSource>,
    },

    /// Block rule - returns a constant block state.
    #[serde(rename = "minecraft:block")]
    Block {
        result_state: BlockState,
    },

    /// Badlands terracotta banding rule.
    #[serde(rename = "minecraft:bandlands")]
    Bandlands {},
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_block_rule() {
        let json = r#"{
            "type": "minecraft:block",
            "result_state": {
                "Name": "minecraft:grass_block",
                "Properties": {
                    "snowy": "false"
                }
            }
        }"#;

        let rule: RuleSource = serde_json::from_str(json).unwrap();
        match rule {
            RuleSource::Block { result_state } => {
                assert_eq!(result_state.name, "minecraft:grass_block");
                assert!(result_state.properties.is_some());
            }
            _ => panic!("Expected Block rule"),
        }
    }

    #[test]
    fn test_parse_condition_biome() {
        let json = r#"{
            "type": "minecraft:condition",
            "if_true": {
                "type": "minecraft:biome",
                "biome_is": ["minecraft:desert", "minecraft:beach"]
            },
            "then_run": {
                "type": "minecraft:block",
                "result_state": {
                    "Name": "minecraft:sand"
                }
            }
        }"#;

        let rule: RuleSource = serde_json::from_str(json).unwrap();
        match rule {
            RuleSource::Condition { if_true, then_run: _ } => {
                match if_true {
                    ConditionSource::Biome { biome_is } => {
                        assert_eq!(biome_is.len(), 2);
                        assert!(biome_is.contains(&"minecraft:desert".to_string()));
                    }
                    _ => panic!("Expected Biome condition"),
                }
            }
            _ => panic!("Expected Condition rule"),
        }
    }

    #[test]
    fn test_parse_sequence() {
        let json = r#"{
            "type": "minecraft:sequence",
            "sequence": [
                {
                    "type": "minecraft:block",
                    "result_state": {
                        "Name": "minecraft:stone"
                    }
                },
                {
                    "type": "minecraft:block",
                    "result_state": {
                        "Name": "minecraft:dirt"
                    }
                }
            ]
        }"#;

        let rule: RuleSource = serde_json::from_str(json).unwrap();
        match rule {
            RuleSource::Sequence { sequence } => {
                assert_eq!(sequence.len(), 2);
            }
            _ => panic!("Expected Sequence rule"),
        }
    }

    #[test]
    fn test_parse_vertical_gradient() {
        let json = r#"{
            "type": "minecraft:condition",
            "if_true": {
                "type": "minecraft:vertical_gradient",
                "false_at_and_above": {
                    "above_bottom": 5
                },
                "random_name": "minecraft:bedrock_floor",
                "true_at_and_below": {
                    "above_bottom": 0
                }
            },
            "then_run": {
                "type": "minecraft:block",
                "result_state": {
                    "Name": "minecraft:bedrock"
                }
            }
        }"#;

        let rule: RuleSource = serde_json::from_str(json).unwrap();
        match rule {
            RuleSource::Condition { if_true, .. } => {
                match if_true {
                    ConditionSource::VerticalGradient {
                        random_name,
                        true_at_and_below,
                        false_at_and_above,
                    } => {
                        assert_eq!(random_name, "minecraft:bedrock_floor");
                        match true_at_and_below {
                            VerticalAnchor::AboveBottom { above_bottom } => {
                                assert_eq!(above_bottom, 0);
                            }
                            _ => panic!("Expected AboveBottom anchor"),
                        }
                        match false_at_and_above {
                            VerticalAnchor::AboveBottom { above_bottom } => {
                                assert_eq!(above_bottom, 5);
                            }
                            _ => panic!("Expected AboveBottom anchor"),
                        }
                    }
                    _ => panic!("Expected VerticalGradient condition"),
                }
            }
            _ => panic!("Expected Condition rule"),
        }
    }

    #[test]
    fn test_parse_stone_depth() {
        let json = r#"{
            "type": "minecraft:stone_depth",
            "add_surface_depth": true,
            "offset": 0,
            "secondary_depth_range": 6,
            "surface_type": "floor"
        }"#;

        let cond: ConditionSource = serde_json::from_str(json).unwrap();
        match cond {
            ConditionSource::StoneDepth {
                offset,
                add_surface_depth,
                secondary_depth_range,
                surface_type,
            } => {
                assert_eq!(offset, 0);
                assert!(add_surface_depth);
                assert_eq!(secondary_depth_range, 6);
                assert_eq!(surface_type, "floor");
            }
            _ => panic!("Expected StoneDepth condition"),
        }
    }

    #[test]
    fn test_parse_not() {
        let json = r#"{
            "type": "minecraft:not",
            "invert": {
                "type": "minecraft:hole"
            }
        }"#;

        let cond: ConditionSource = serde_json::from_str(json).unwrap();
        match cond {
            ConditionSource::Not { invert } => {
                assert!(matches!(*invert, ConditionSource::Hole {}));
            }
            _ => panic!("Expected Not condition"),
        }
    }

    #[test]
    fn test_parse_bandlands() {
        let json = r#"{
            "type": "minecraft:bandlands"
        }"#;

        let rule: RuleSource = serde_json::from_str(json).unwrap();
        assert!(matches!(rule, RuleSource::Bandlands {}));
    }

    #[test]
    fn test_parse_y_above() {
        let json = r#"{
            "type": "minecraft:y_above",
            "add_stone_depth": false,
            "anchor": {
                "absolute": 97
            },
            "surface_depth_multiplier": 2
        }"#;

        let cond: ConditionSource = serde_json::from_str(json).unwrap();
        match cond {
            ConditionSource::YAbove {
                anchor,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                assert!(!add_stone_depth);
                assert_eq!(surface_depth_multiplier, 2);
                match anchor {
                    VerticalAnchor::Absolute { absolute } => {
                        assert_eq!(absolute, 97);
                    }
                    _ => panic!("Expected Absolute anchor"),
                }
            }
            _ => panic!("Expected YAbove condition"),
        }
    }

    #[test]
    fn test_parse_water() {
        let json = r#"{
            "type": "minecraft:water",
            "add_stone_depth": true,
            "offset": -6,
            "surface_depth_multiplier": -1
        }"#;

        let cond: ConditionSource = serde_json::from_str(json).unwrap();
        match cond {
            ConditionSource::Water {
                offset,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                assert!(add_stone_depth);
                assert_eq!(offset, -6);
                assert_eq!(surface_depth_multiplier, -1);
            }
            _ => panic!("Expected Water condition"),
        }
    }

    #[test]
    fn test_parse_noise_threshold() {
        let json = r#"{
            "type": "minecraft:noise_threshold",
            "max_threshold": 1.7976931348623157E308,
            "min_threshold": 0.0,
            "noise": "minecraft:surface_swamp"
        }"#;

        let cond: ConditionSource = serde_json::from_str(json).unwrap();
        match cond {
            ConditionSource::NoiseThreshold {
                noise,
                min_threshold,
                max_threshold,
            } => {
                assert_eq!(noise, "minecraft:surface_swamp");
                assert_eq!(min_threshold, 0.0);
                assert!(max_threshold > 1.0e300);
            }
            _ => panic!("Expected NoiseThreshold condition"),
        }
    }
}
