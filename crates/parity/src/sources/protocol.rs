//! Protocol rows: every packet in the pinned protocolgen 1.26.51 manifest
//! (`crates/valentine_gen/protocolgen/generated/1.26.51/manifest.json`), the
//! same input `valentine_gen` generates the codec from.
//!
//! The family is the manifest direction. The manifest marks client-to-server
//! packets `bidirectional` as well, so `bidirectional` means "the server may
//! receive it", not "both sides send it".

use std::path::Path;

use anyhow::Context as _;
use serde::Deserialize;

use crate::catalog::Row;

pub const MANIFEST: &str = "crates/valentine_gen/protocolgen/generated/1.26.51/manifest.json";

#[derive(Deserialize)]
struct Manifest {
    target: Target,
    packets: Vec<Packet>,
}

#[derive(Deserialize)]
struct Target {
    protocol_version: u32,
}

#[derive(Deserialize)]
struct Packet {
    id: u32,
    name: String,
    direction: String,
}

pub fn rows(root: &Path) -> anyhow::Result<Vec<Row>> {
    let path = root.join(MANIFEST);
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).with_context(|| format!("decoding {}", path.display()))?;
    anyhow::ensure!(
        manifest.target.protocol_version == 2193,
        "{MANIFEST} is protocol {}, the catalog targets 2193",
        manifest.target.protocol_version
    );
    Ok(manifest
        .packets
        .into_iter()
        .map(|packet| {
            Row::new(packet.name, packet.direction).with_label(format!("id {}", packet.id))
        })
        .collect())
}
