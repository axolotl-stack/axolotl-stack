use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct OffspringDenyParentsVariant {
    ///The percentage chance of denying the parents` variant.
    pub chance: Option<f32>,
    ///The inclusive maximum of the variant range.
    pub max_variant: Option<i32>,
    ///The inclusive minimum of the variant range.
    pub min_variant: Option<i32>,
}
impl Default for OffspringDenyParentsVariant {
    fn default() -> Self {
        Self {
            chance: None,
            max_variant: None,
            min_variant: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct OffspringMutationFactor {
    ///The percentage chance of the offspring getting its color as if spawned rather than inheriting color from its parents.
    pub color: Option<f32>,
    ///The percentage chance of a mutation on the entity's extra variant type.
    pub extra_variant: Option<f32>,
    ///The percentage chance of a mutation on the entity's variant type.
    pub variant: Option<f32>,
}
impl Default for OffspringMutationFactor {
    fn default() -> Self {
        Self {
            color: None,
            extra_variant: None,
            variant: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct OffspringOffspringPairs {}
impl Default for OffspringOffspringPairs {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct OffspringPropertyInheritance {
    /// Additional dynamic entries not captured by the upstream schema.
    pub additional: std::collections::HashMap<String, crate::types::BedrockValue>,
}
impl Default for OffspringPropertyInheritance {
    fn default() -> Self {
        Self {
            additional: std::collections::HashMap::new(),
        }
    }
}
/// Bedrock component `minecraft:offspring`. Defines how an offspring of an entity is born
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Offspring {
    ///If true, the entities will blend their attributes in the offspring after they breed.
    pub blend_attributes: Option<bool>,
    ///If true, the babies will have their color be mixed.
    pub combine_parent_colors: Option<bool>,
    ///Determines how likely the baby of parents with the same variant will deny that variant and take a random variant within the given range instead.
    pub deny_parents_variant: Option<OffspringDenyParentsVariant>,
    ///If true, the babies will be automatically tamed if its parents are.
    pub inherit_tamed: Option<bool>,
    ///Determines how likely the babies are to NOT inherit one of their parent's variances. Values are between 0.0 and 1.0, with a higher number meaning more likely to mutate.
    pub mutation_factor: Option<OffspringMutationFactor>,
    ///Strategy used for mutating variants and extra variants for offspring. Current valid alternatives are 'random' and 'none'.
    pub mutation_strategy: Option<String>,
    ///Key-Value Pair of the other entity and the offspring to spawn
    pub offspring_pairs: Option<OffspringOffspringPairs>,
    ///[EXPERIMENTAL] List of attributes that should benefit from parent centric attribute blending. For example, horses blend their health, movement, and jump_strength in their offspring.
    pub parent_centric_attribute_blending: Option<Vec<crate::types::BedrockValue>>,
    ///List of Entity Properties that should be inherited from the parent entities and potentially mutated.
    pub property_inheritance: Option<Vec<OffspringPropertyInheritance>>,
    ///Range used to determine random extra variant.
    pub random_extra_variant_mutation_interval: Option<Vec<f32>>,
    ///Range used to determine random variant.
    pub random_variant_mutation_interval: Option<Vec<f32>>,
}
impl Default for Offspring {
    fn default() -> Self {
        Self {
            blend_attributes: None,
            combine_parent_colors: None,
            deny_parents_variant: None,
            inherit_tamed: None,
            mutation_factor: None,
            mutation_strategy: None,
            offspring_pairs: None,
            parent_centric_attribute_blending: None,
            property_inheritance: None,
            random_extra_variant_mutation_interval: None,
            random_variant_mutation_interval: None,
        }
    }
}
