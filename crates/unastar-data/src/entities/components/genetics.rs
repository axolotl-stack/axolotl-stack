use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct GeneticsGenesGeneticVariantsBirthEvent {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for GeneticsGenesGeneticVariantsBirthEvent {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct GeneticsGenesGeneticVariants {
    ///Event to run when this mob is created and matches the above allele conditions.
    pub birth_event: Option<GeneticsGenesGeneticVariantsBirthEvent>,
    ///If this value is non-negative, compare both the mob's main and hidden alleles with this value for a match with both. Can also be a range of integers.
    pub both_allele: Option<crate::types::RangeOrVal<f32>>,
    ///If this value is non-negative, compare both the mob's main and hidden alleles with this value for a match with either. Can also be a range of integers.
    pub either_allele: Option<i32>,
    ///If this value is non-negative, compare the mob's hidden allele with this value for a match. Can also be a range of integers.
    pub hidden_allele: Option<i32>,
    ///If this value is non-negative, compare the mob's main allele with this value for a match. Can also be a range of integers.
    pub main_allele: Option<crate::types::RangeOrVal<f32>>,
    ///If this value is non-negative, overrides the chance for this gene that an allele will be replaced with a random one instead of the parent's allele during birth. Non-negative values greater than 1 will be the same as the value 1.
    pub mutation_rate: Option<f32>,
}
impl Default for GeneticsGenesGeneticVariants {
    fn default() -> Self {
        Self {
            birth_event: None,
            both_allele: None,
            either_allele: None,
            hidden_allele: None,
            main_allele: None,
            mutation_rate: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct GeneticsGenes {
    ///The range of positive integer allele values for this gene. Spawned mobs will have a random number in this range assigned to them.
    pub allele_range: Option<crate::types::RangeOrVal<f32>>,
    ///The list of genetic variants for this gene. These check for particular allele combinations and fire events when all of them are satisfied.
    pub genetic_variants: Option<Vec<GeneticsGenesGeneticVariants>>,
    ///The name of the gene.
    pub name: Option<String>,
}
impl Default for GeneticsGenes {
    fn default() -> Self {
        Self {
            allele_range: None,
            genetic_variants: None,
            name: None,
        }
    }
}
/// Bedrock component `minecraft:genetics`. Defines the way a mob's genes and alleles are passed on to it's offspring, and how those traits manifest in the child. Compatible parent genes are crossed together, the alleles are handed down from the parents to the child, and any matching genetic variants fire off JSON events to modify the child and express the traits.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Genetics {
    ///The list of genes that this entity has and will cross with a partner during breeding.
    pub genes: Option<Vec<GeneticsGenes>>,
    ///Chance that an allele will be replaced with a random one instead of the parent's allele during birth.
    pub mutation_rate: Option<f32>,
}
impl Default for Genetics {
    fn default() -> Self {
        Self {
            genes: None,
            mutation_rate: Some(0.03125f32),
        }
    }
}
