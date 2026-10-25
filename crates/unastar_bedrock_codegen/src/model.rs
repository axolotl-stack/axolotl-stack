//! Loading and analysis: the resolved BDS worldgen model.
//!
//! [`Model::load`] runs ingestion and IR construction for every family the
//! Bedrock-style generator uses; the analysis methods answer the questions the
//! emitters need (which rules run for a biome in a pass, which features are
//! reachable, which built-in features must exist in Rust).

use crate::blocks::BlockPalette;
use crate::error::{Error, Result};
use crate::ir::{Biome, Feature, FeatureRule, Pass, SpawnRule};
use crate::structures::StructureData;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use unastar_packs::packs::{Family, PackStack};

/// Everything the emitters consume.
pub struct Model {
    pub palette: BlockPalette,
    /// Sorted by identifier.
    pub biomes: Vec<Biome>,
    pub rules: BTreeMap<String, FeatureRule>,
    pub features: BTreeMap<String, Feature>,
    pub spawn_rules: BTreeMap<String, SpawnRule>,
    /// Jigsaw structures, pools, sets, processors and every template they or
    /// the template-based features place.
    pub structures: StructureData,
    /// Files read, for `cargo:rerun-if-changed`.
    pub inputs: Vec<PathBuf>,
}

/// Biome tags that mark a biome as part of overworld generation.
pub const OVERWORLD_TAGS: [&str; 2] = ["overworld", "overworld_generation"];

impl Model {
    /// Load from a BDS server directory and protocolgen's native 1.26.51 data.
    pub fn load(bds_dir: &Path, native_dir: &Path) -> Result<Self> {
        let stack = PackStack::open(bds_dir)?;
        let palette = BlockPalette::load(native_dir)?;
        let ids_path = native_dir.join("biome_id_map.json");
        let ids_bytes = std::fs::read(&ids_path).map_err(|e| Error::io(&ids_path, e))?;
        let ids: BTreeMap<String, u16> =
            serde_json::from_slice(&ids_bytes).map_err(|source| Error::Json {
                origin: ids_path.display().to_string(),
                source,
            })?;

        let mut biomes = stack
            .resolve(Family::Biomes)?
            .values()
            .map(|d| Biome::parse(d, &palette, &ids))
            .collect::<Result<Vec<_>>>()?;
        biomes.sort_by(|a, b| a.identifier.cmp(&b.identifier));
        let rules = stack
            .resolve(Family::FeatureRules)?
            .values()
            .map(|d| FeatureRule::parse(d).map(|r| (r.identifier.clone(), r)))
            .collect::<Result<_>>()?;
        let features = stack
            .resolve(Family::Features)?
            .values()
            .map(|d| Feature::parse(d).map(|f| (f.identifier.clone(), f)))
            .collect::<Result<_>>()?;
        let spawn_rules = stack
            .resolve(Family::SpawnRules)?
            .values()
            .map(|d| SpawnRule::parse(d).map(|r| (r.identifier.clone(), r)))
            .collect::<Result<_>>()?;
        let structures = StructureData::load(&stack, &palette, &features)?;

        let mut inputs = vec![
            native_dir.join("canonical_block_states.nbt"),
            native_dir.join("block_tags.json"),
            native_dir.join("block_state_data.json"),
            native_dir.join("block_properties_table.json"),
            ids_path,
        ];
        for family in [
            Family::Biomes,
            Family::FeatureRules,
            Family::Features,
            Family::SpawnRules,
        ] {
            inputs.extend(stack.inputs(family));
        }
        inputs.extend(structures.inputs.iter().cloned());
        Ok(Model {
            palette,
            biomes,
            rules,
            features,
            spawn_rules,
            structures,
            inputs,
        })
    }

    /// Biomes that take part in overworld generation.
    pub fn overworld_biomes(&self) -> impl Iterator<Item = &Biome> {
        self.biomes
            .iter()
            .filter(|b| OVERWORLD_TAGS.iter().any(|t| b.tags.contains(*t)))
    }

    /// Rules that run for `biome` in `pass`, in execution order.
    ///
    /// Execution order inside a pass is not observable from BDS data; rules run
    /// sorted by identifier so output is deterministic (logged in bds-quirks.md).
    pub fn rules_for(&self, biome: &Biome, pass: Pass) -> Vec<&FeatureRule> {
        self.rules
            .values()
            .filter(|r| r.pass == pass && !r.is_disabled())
            .filter(|r| {
                r.biome_filter
                    .as_ref()
                    .is_none_or(|f| f.matches(&biome.tags))
            })
            .collect()
    }

    /// Rules that can run in at least one overworld biome.
    pub fn active_rules(&self) -> Vec<&FeatureRule> {
        let biomes: Vec<&Biome> = self.overworld_biomes().collect();
        self.rules
            .values()
            .filter(|r| !r.is_disabled())
            .filter(|r| {
                biomes
                    .iter()
                    .any(|b| r.biome_filter.as_ref().is_none_or(|f| f.matches(&b.tags)))
            })
            .collect()
    }

    /// Every feature identifier reachable from active overworld rules.
    ///
    /// Fails if a reference resolves neither to a JSON feature nor to one of
    /// `builtins` (identifiers implemented in Rust).
    pub fn reachable_features(&self, builtins: &[&str]) -> Result<BTreeSet<String>> {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<(String, String)> = self
            .active_rules()
            .into_iter()
            .map(|r| (r.places_feature.clone(), r.source.clone()))
            .collect();
        // Pool feature elements place features from inside structures.
        stack.extend(
            self.structures
                .pool_features
                .iter()
                .map(|id| (id.clone(), "worldgen/template_pools".to_owned())),
        );
        stack.extend(
            crate::builtin::BUILTIN_CALLS
                .iter()
                .map(|id| (id.to_string(), "builtin::BUILTIN_CALLS".to_owned())),
        );
        while let Some((id, referrer)) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            match self.features.get(&id) {
                Some(f) => stack.extend(f.refs.iter().map(|r| (r.clone(), f.source.clone()))),
                None if builtins.contains(&id.as_str()) => {}
                None => {
                    return Err(Error::data(
                        referrer,
                        format!("{id} is neither a JSON feature nor a registered built-in feature"),
                    ));
                }
            }
        }
        Ok(seen)
    }
}
