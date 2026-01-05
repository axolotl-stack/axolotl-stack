//! Component schema definitions for typed code generation.
//!
//! Type definitions are in Rust (field names, types, optionality).
//! Default values are loaded from `_defaults.kdl` at codegen time.

use kdl::{KdlDocument, KdlNode, KdlValue};
use std::collections::HashMap;
use std::path::Path;

/// How often a component appears across all entities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentFrequency {
    /// Present on most entities (>50%) - use Table storage
    Common,
    /// Present on some entities (10-50%) - use Table storage
    Moderate,
    /// Rarely present (<10%) - use SparseSet storage
    Sparse,
}

/// Field type in a component schema
#[derive(Debug, Clone)]
pub enum FieldType {
    /// f32
    Float,
    /// i32
    Integer,
    /// bool
    Bool,
    /// String
    String,
    /// Optional wrapper
    Option(Box<FieldType>),
}

/// A field in a component schema
#[derive(Debug, Clone)]
pub struct FieldSchema {
    pub name: &'static str,
    pub rust_name: &'static str,
    pub field_type: FieldType,
    pub is_primary: bool, // If true, use as argument not property
}

/// Schema for a component (type info only, no defaults)
#[derive(Debug, Clone)]
pub struct ComponentSchema {
    pub name: &'static str,
    pub rust_name: &'static str,
    pub frequency: ComponentFrequency,
    pub fields: Vec<FieldSchema>,
    pub is_marker: bool,
}

impl ComponentSchema {
    pub const fn marker(name: &'static str, rust_name: &'static str, frequency: ComponentFrequency) -> Self {
        Self {
            name,
            rust_name,
            frequency,
            fields: Vec::new(),
            is_marker: true,
        }
    }
}

/// Defaults loaded from KDL
#[derive(Debug, Clone, Default)]
pub struct ComponentDefaults {
    /// Map of component name -> field name -> default value string
    pub values: HashMap<String, HashMap<String, String>>,
    /// Primary argument defaults (e.g., health 20 -> "20")
    pub primary_values: HashMap<String, String>,
}

impl ComponentDefaults {
    /// Load defaults from _defaults.kdl
    pub fn load(defaults_path: &Path) -> miette::Result<Self> {
        let content = std::fs::read_to_string(defaults_path)
            .map_err(|e| miette::miette!("Failed to read defaults: {}", e))?;

        let doc: KdlDocument = content
            .parse()
            .map_err(|e| miette::miette!("Failed to parse defaults KDL: {}", e))?;

        let mut defaults = ComponentDefaults::default();

        // Find the "defaults" node with type="entity"
        for node in doc.nodes() {
            if node.name().value() == "defaults" {
                if let Some(children) = node.children() {
                    for child in children.nodes() {
                        let comp_name = child.name().value().to_string();
                        let mut field_defaults = HashMap::new();

                        // Check for primary argument
                        for entry in child.entries() {
                            if entry.name().is_none() {
                                // Positional argument = primary value
                                defaults.primary_values.insert(
                                    comp_name.clone(),
                                    kdl_value_to_string(entry.value()),
                                );
                            } else if let Some(prop_name) = entry.name() {
                                // Named property
                                field_defaults.insert(
                                    prop_name.value().to_string(),
                                    kdl_value_to_string(entry.value()),
                                );
                            }
                        }

                        // Check children for nested properties
                        if let Some(comp_children) = child.children() {
                            for prop_node in comp_children.nodes() {
                                let prop_name = prop_node.name().value();
                                // Skip comments
                                if prop_name.starts_with("//") {
                                    continue;
                                }
                                // Get the value (first argument or property)
                                if let Some(entry) = prop_node.entries().first() {
                                    field_defaults.insert(
                                        prop_name.to_string(),
                                        kdl_value_to_string(entry.value()),
                                    );
                                }
                            }
                        }

                        if !field_defaults.is_empty() {
                            defaults.values.insert(comp_name, field_defaults);
                        }
                    }
                }
            }
        }

        Ok(defaults)
    }

    /// Get default for a field
    pub fn get_field_default(&self, component: &str, field: &str) -> Option<&str> {
        self.values
            .get(component)
            .and_then(|fields| fields.get(field))
            .map(|s| s.as_str())
    }

    /// Get primary value default for a component
    pub fn get_primary_default(&self, component: &str) -> Option<&str> {
        self.primary_values.get(component).map(|s| s.as_str())
    }
}

fn kdl_value_to_string(value: &KdlValue) -> String {
    match value {
        KdlValue::String(s) => s.clone(),
        KdlValue::Integer(i) => i.to_string(),
        KdlValue::Float(f) => f.to_string(),
        KdlValue::Bool(b) => b.to_string(),
        KdlValue::Null => "null".to_string(),
    }
}

/// Get all known component schemas (type definitions only)
pub fn get_component_schemas() -> HashMap<&'static str, ComponentSchema> {
    let mut schemas = HashMap::new();

    // === Core Components (Common) ===

    schemas.insert("health", ComponentSchema {
        name: "health",
        rust_name: "Health",
        frequency: ComponentFrequency::Common,
        fields: vec![
            FieldSchema {
                name: "value",
                rust_name: "value",
                field_type: FieldType::Integer,
                is_primary: true,
            },
            FieldSchema {
                name: "max",
                rust_name: "max",
                field_type: FieldType::Option(Box::new(FieldType::Integer)),
                is_primary: false,
            },
        ],
        is_marker: false,
    });

    schemas.insert("collision_box", ComponentSchema {
        name: "collision_box",
        rust_name: "CollisionBox",
        frequency: ComponentFrequency::Common,
        fields: vec![
            FieldSchema {
                name: "width",
                rust_name: "width",
                field_type: FieldType::Float,
                is_primary: false,
            },
            FieldSchema {
                name: "height",
                rust_name: "height",
                field_type: FieldType::Float,
                is_primary: false,
            },
        ],
        is_marker: false,
    });

    schemas.insert("movement", ComponentSchema {
        name: "movement",
        rust_name: "Movement",
        frequency: ComponentFrequency::Common,
        fields: vec![
            FieldSchema {
                name: "value",
                rust_name: "speed",
                field_type: FieldType::Float,
                is_primary: true,
            },
        ],
        is_marker: false,
    });

    schemas.insert("physics", ComponentSchema {
        name: "physics",
        rust_name: "Physics",
        frequency: ComponentFrequency::Common,
        fields: vec![
            FieldSchema {
                name: "has_gravity",
                rust_name: "has_gravity",
                field_type: FieldType::Bool,
                is_primary: false,
            },
            FieldSchema {
                name: "has_collision",
                rust_name: "has_collision",
                field_type: FieldType::Bool,
                is_primary: false,
            },
        ],
        is_marker: false,
    });

    schemas.insert("pushable", ComponentSchema {
        name: "pushable",
        rust_name: "Pushable",
        frequency: ComponentFrequency::Common,
        fields: vec![
            FieldSchema {
                name: "is_pushable",
                rust_name: "is_pushable",
                field_type: FieldType::Bool,
                is_primary: false,
            },
            FieldSchema {
                name: "is_pushable_by_piston",
                rust_name: "is_pushable_by_piston",
                field_type: FieldType::Bool,
                is_primary: false,
            },
        ],
        is_marker: false,
    });

    schemas.insert("attack", ComponentSchema {
        name: "attack",
        rust_name: "Attack",
        frequency: ComponentFrequency::Moderate,
        fields: vec![
            FieldSchema {
                name: "damage",
                rust_name: "damage",
                field_type: FieldType::Integer,
                is_primary: false,
            },
            FieldSchema {
                name: "effect_name",
                rust_name: "effect_name",
                field_type: FieldType::Option(Box::new(FieldType::String)),
                is_primary: false,
            },
            FieldSchema {
                name: "effect_duration",
                rust_name: "effect_duration",
                field_type: FieldType::Option(Box::new(FieldType::Float)),
                is_primary: false,
            },
        ],
        is_marker: false,
    });

    schemas.insert("flying_speed", ComponentSchema {
        name: "flying_speed",
        rust_name: "FlyingSpeed",
        frequency: ComponentFrequency::Sparse,
        fields: vec![
            FieldSchema {
                name: "value",
                rust_name: "speed",
                field_type: FieldType::Float,
                is_primary: true,
            },
        ],
        is_marker: false,
    });

    schemas.insert("follow_range", ComponentSchema {
        name: "follow_range",
        rust_name: "FollowRange",
        frequency: ComponentFrequency::Moderate,
        fields: vec![
            FieldSchema {
                name: "value",
                rust_name: "range",
                field_type: FieldType::Integer,
                is_primary: true,
            },
        ],
        is_marker: false,
    });

    schemas.insert("scale", ComponentSchema {
        name: "scale",
        rust_name: "Scale",
        frequency: ComponentFrequency::Sparse,
        fields: vec![
            FieldSchema {
                name: "value",
                rust_name: "value",
                field_type: FieldType::Float,
                is_primary: true,
            },
        ],
        is_marker: false,
    });

    // === Marker Components (no data) ===

    schemas.insert("can_climb", ComponentSchema::marker(
        "can_climb", "CanClimb", ComponentFrequency::Moderate
    ));

    schemas.insert("can_fly", ComponentSchema::marker(
        "can_fly", "CanFly", ComponentFrequency::Sparse
    ));

    schemas.insert("can_power_jump", ComponentSchema::marker(
        "can_power_jump", "CanPowerJump", ComponentFrequency::Sparse
    ));

    schemas.insert("fire_immune", ComponentSchema::marker(
        "fire_immune", "FireImmune", ComponentFrequency::Sparse
    ));

    schemas.insert("floats_in_liquid", ComponentSchema::marker(
        "floats_in_liquid", "FloatsInLiquid", ComponentFrequency::Sparse
    ));

    schemas.insert("is_baby", ComponentSchema::marker(
        "is_baby", "IsBaby", ComponentFrequency::Sparse
    ));

    schemas.insert("is_hidden_when_invisible", ComponentSchema::marker(
        "is_hidden_when_invisible", "IsHiddenWhenInvisible", ComponentFrequency::Common
    ));

    schemas.insert("is_ignited", ComponentSchema::marker(
        "is_ignited", "IsIgnited", ComponentFrequency::Sparse
    ));

    schemas.insert("is_saddled", ComponentSchema::marker(
        "is_saddled", "IsSaddled", ComponentFrequency::Sparse
    ));

    schemas.insert("is_sheared", ComponentSchema::marker(
        "is_sheared", "IsSheared", ComponentFrequency::Sparse
    ));

    schemas.insert("is_tamed", ComponentSchema::marker(
        "is_tamed", "IsTamed", ComponentFrequency::Sparse
    ));

    schemas.insert("leashable", ComponentSchema::marker(
        "leashable", "Leashable", ComponentFrequency::Moderate
    ));

    schemas.insert("nameable", ComponentSchema::marker(
        "nameable", "Nameable", ComponentFrequency::Common
    ));

    schemas.insert("burns_in_daylight", ComponentSchema::marker(
        "burns_in_daylight", "BurnsInDaylight", ComponentFrequency::Sparse
    ));

    // === Inventory Component ===

    schemas.insert("inventory", ComponentSchema {
        name: "inventory",
        rust_name: "Inventory",
        frequency: ComponentFrequency::Moderate,
        fields: vec![
            FieldSchema {
                name: "inventory_size",
                rust_name: "size",
                field_type: FieldType::Integer,
                is_primary: false,
            },
            FieldSchema {
                name: "container_type",
                rust_name: "container_type",
                field_type: FieldType::Option(Box::new(FieldType::String)),
                is_primary: false,
            },
            FieldSchema {
                name: "can_be_siphoned_from",
                rust_name: "can_be_siphoned_from",
                field_type: FieldType::Bool,
                is_primary: false,
            },
            FieldSchema {
                name: "private",
                rust_name: "private",
                field_type: FieldType::Bool,
                is_primary: false,
            },
        ],
        is_marker: false,
    });

    // === Breathable Component ===

    schemas.insert("breathable", ComponentSchema {
        name: "breathable",
        rust_name: "Breathable",
        frequency: ComponentFrequency::Common,
        fields: vec![
            FieldSchema {
                name: "total_supply",
                rust_name: "total_supply",
                field_type: FieldType::Integer,
                is_primary: false,
            },
            FieldSchema {
                name: "suffocate_time",
                rust_name: "suffocate_time",
                field_type: FieldType::Integer,
                is_primary: false,
            },
            FieldSchema {
                name: "breathes_air",
                rust_name: "breathes_air",
                field_type: FieldType::Bool,
                is_primary: false,
            },
            FieldSchema {
                name: "breathes_water",
                rust_name: "breathes_water",
                field_type: FieldType::Bool,
                is_primary: false,
            },
            FieldSchema {
                name: "breathes_lava",
                rust_name: "breathes_lava",
                field_type: FieldType::Bool,
                is_primary: false,
            },
            FieldSchema {
                name: "breathes_solids",
                rust_name: "breathes_solids",
                field_type: FieldType::Bool,
                is_primary: false,
            },
            FieldSchema {
                name: "generates_bubbles",
                rust_name: "generates_bubbles",
                field_type: FieldType::Bool,
                is_primary: false,
            },
        ],
        is_marker: false,
    });

    schemas
}

/// Components that should use SparseSet storage
/// These are added/removed frequently or very rare
pub fn get_sparse_components() -> Vec<&'static str> {
    vec![
        // State changes - frequently added/removed
        "is_baby",
        "is_tamed",
        "is_saddled",
        "is_sheared",
        "is_ignited",
        "is_charged",
        "is_stunned",
        "is_illager_captain",

        // Rare entity features
        "can_fly",
        "can_power_jump",
        "fire_immune",
        "burns_in_daylight",
        "floats_in_liquid",
        "flying_speed",
        "glide",

        // Boss-specific
        "boss",

        // Temporary effects
        "angry",
        "on_fire",
        "strength",
    ]
}
