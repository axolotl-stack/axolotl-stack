//! The pack stacks BDS 1.26.51 lists with `/packstack`.
//!
//! Source: `data/measured/pack_stack.json`, cut from the command probe
//! (`crates/unastar/src/command/exec/testdata/bds-command-probe.json`,
//! `tools/bds-sweep/command_probe.py`): `/packstack server verbose` on the
//! console and `/packstack client verbose` from an operator player of a
//! dedicated server without added packs. Every pack is vanilla.

use std::sync::LazyLock;

use serde::Deserialize;

/// Both stacks, in BDS's order.
#[derive(Debug, Deserialize)]
pub struct PackStacks {
    pub server: Vec<Pack>,
    pub client: Vec<Pack>,
}

/// One pack as `/packstack … verbose` lists it.
#[derive(Debug, Deserialize)]
pub struct Pack {
    /// The name as printed (a translation key for the vanilla server pack).
    pub name: String,
    pub uuid: String,
    /// `Behavior` or `Resources`.
    #[serde(rename = "type")]
    pub kind: String,
    pub version: String,
}

/// Decode the embedded table; consumers normally use [`vanilla`].
pub fn decode_vanilla() -> Result<PackStacks, serde_json::Error> {
    serde_json::from_str(include_str!("../data/measured/pack_stack.json"))
}

/// The embedded table.
pub fn vanilla() -> &'static PackStacks {
    static STACKS: LazyLock<PackStacks> =
        LazyLock::new(|| decode_vanilla().expect("embedded BDS pack stacks decode"));
    &STACKS
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_stacks_decode() {
        let stacks = super::decode_vanilla().unwrap();
        assert_eq!(stacks.server.len(), 58);
        assert_eq!(stacks.client[0].name, "Minecraft Texture Pack");
    }
}
