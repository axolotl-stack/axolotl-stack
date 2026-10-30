//! Pinned, data-derived Java actor load/save deltas. Complete template payloads
//! are read losslessly from the user's jar; only measured normalization deltas and
//! per-input fingerprints are retained in the repository.
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;

#[allow(dead_code)]
#[path = "../src/structures/native_actor.rs"]
mod native;

type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

#[derive(serde::Deserialize)]
pub struct Snapshots {
    version: String,
    server_sha1: String,
    mappings_sha1: String,
    code_sha256: String,
    patches: Vec<Vec<u8>>,
    actors: BTreeMap<String, (String, usize)>,
}
impl Snapshots {
    pub fn load() -> Result<Self> {
        let data: Self =
            serde_json::from_str(include_str!("../java_data/native_actor_normalization.json"))?;
        if data.version != "1.21.11"
            || data.server_sha1 != "64bb6d763bed0a9f1d632ec347938594144943ed"
            || data.mappings_sha1 != "5621e9253f05fd57872bbe7f8ddf5f9a7d525955"
            || data.code_sha256
                != "ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc"
        {
            return Err("native actor normalization snapshot pin differs".into());
        }
        Ok(data)
    }
    pub fn patch(&self, origin: &str, index: usize, source: &[u8]) -> Result<&[u8]> {
        let name = format!("{origin}#{index}");
        let (expected, patch) = self
            .actors
            .get(&name)
            .ok_or_else(|| format!("missing native actor observation for {name}"))?;
        let actual = format!("{:x}", Sha256::digest(source));
        if &actual != expected {
            return Err(format!("{name}: native actor source fingerprint differs ({actual}, expected {expected}); regenerate with NativeActorOracle.java and prepare_native_actor_snapshots.py against the pinned Java assets").into());
        }
        self.patches
            .get(*patch)
            .map(Vec::as_slice)
            .ok_or_else(|| "invalid native actor normalization index".into())
    }
}

/// Keep Java strings, list element types and floating bits intact, unlike the
/// convenience template geometry parser's intentionally simpler NBT model.
pub fn source_actors(gzip: &[u8]) -> Result<BTreeMap<usize, Vec<u8>>> {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(gzip).read_to_end(&mut bytes)?;
    let root = native::Tag::read_root(&bytes)?;
    let Some(native::Tag::List(_, blocks)) = root.compound()?.get(&native::key("blocks")) else {
        return Err("template has no typed block list".into());
    };
    let mut actors = BTreeMap::new();
    for (index, block) in blocks.iter().enumerate() {
        if let Some(actor) = block.compound()?.get(&native::key("nbt")) {
            actors.insert(index, actor.write_root()?);
        }
    }
    Ok(actors)
}
