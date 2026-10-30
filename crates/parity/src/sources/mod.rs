//! Row generators, one per domain.

pub mod blocks;
pub mod commands;
pub mod entities;
pub mod items;
pub mod protocol;
pub mod systems;
pub mod worldgen;

use std::path::Path;

use crate::catalog::{Domain, DomainRows};

/// Every domain's rows, plus the Bedrock worldgen rows on their own (they
/// also feed the committed snapshot) and whether those came from an install.
pub struct Generated {
    pub domains: Vec<DomainRows>,
    pub bedrock_worldgen: Vec<crate::catalog::Row>,
    pub bedrock_from_install: bool,
}

pub fn generate(root: &Path) -> anyhow::Result<Generated> {
    let (bedrock_worldgen, bedrock_from_install) = worldgen::bedrock_rows(root)?;
    let mut worldgen_rows = worldgen::java_rows(root)?;
    worldgen_rows.extend(bedrock_worldgen.iter().cloned());
    let domains = vec![
        DomainRows::new(Domain::Entities, entities::rows())?,
        DomainRows::new(Domain::Blocks, blocks::rows())?,
        DomainRows::new(Domain::Items, items::rows()?)?,
        DomainRows::new(Domain::Commands, commands::rows(root)?)?,
        DomainRows::new(Domain::Protocol, protocol::rows(root)?)?,
        DomainRows::new(Domain::Worldgen, worldgen_rows)?,
        DomainRows::new(Domain::Systems, systems::rows())?,
    ];
    Ok(Generated {
        domains,
        bedrock_worldgen,
        bedrock_from_install,
    })
}
