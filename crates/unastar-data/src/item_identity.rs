//! Current item identities measured from BDS recipe descriptors/results.
//!
//! These are aliases, not a complete historical item upgrader. Unknown names
//! remain unknown; consumers resolve canonical targets against their frozen
//! item registry. The projection is decoded once, never in a gameplay tick.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::LazyLock;

/// A borrowed canonical identity. Missing aux means no aux restriction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemIdentity<'a> {
    pub name: &'a str,
    pub data: Option<i16>,
}

#[derive(serde::Deserialize)]
pub(crate) struct LegacyIdentity {
    pub(crate) name: String,
    pub(crate) data: Option<i16>,
}

#[derive(serde::Deserialize)]
struct Projection {
    inputs: HashMap<String, LegacyIdentity>,
    outputs: HashMap<String, LegacyIdentity>,
}

#[derive(Default)]
struct AliasVariants {
    by_aux: HashMap<Option<i16>, LegacyIdentity>,
    has_aux_aliases: bool,
}

type Aliases = HashMap<String, AliasVariants>;

pub(crate) struct LegacyIdentities {
    inputs: Aliases,
    outputs: Aliases,
}

/// Lexically separate a signed aux suffix. Callers must prefer an exact
/// registered identifier first: `addon:123` is also a valid namespaced name.
pub fn split_aux(value: &str) -> Option<(&str, i16)> {
    let (name, suffix) = value.rsplit_once(':')?;
    suffix.parse().ok().map(|data| (name, data))
}

/// Qualify a vanilla short name without allocating for namespaced names.
pub fn qualify(name: &str) -> Cow<'_, str> {
    if name.contains(':') {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(format!("minecraft:{name}"))
    }
}

fn index_aliases(rows: HashMap<String, LegacyIdentity>) -> Aliases {
    let mut indexed = Aliases::new();
    for (mut name, identity) in rows {
        let data = if let Some((base, data)) = split_aux(&name)
            && base.contains(':')
        {
            name.truncate(base.len());
            Some(data)
        } else {
            None
        };
        let variants = indexed.entry(name).or_default();
        variants.has_aux_aliases |= data.is_some();
        variants.by_aux.insert(data, identity);
    }
    indexed
}

pub(crate) fn vanilla() -> &'static LegacyIdentities {
    static IDENTITIES: LazyLock<LegacyIdentities> = LazyLock::new(|| {
        let projection: Projection = serde_json::from_str(include_str!(
            "../data/measured/recipe_legacy_identities.json"
        ))
        .expect("valid embedded BDS item identity measurements");
        LegacyIdentities {
            inputs: index_aliases(projection.inputs),
            outputs: index_aliases(projection.outputs),
        }
    });
    &IDENTITIES
}

impl LegacyIdentities {
    fn lookup<'a>(table: &'a Aliases, name: &str, data: Option<i16>) -> Option<&'a LegacyIdentity> {
        let variants = table.get(name)?;
        variants
            .by_aux
            .get(&data)
            .or_else(|| variants.by_aux.get(&None))
    }

    pub(crate) fn input_alias(&self, name: &str, data: Option<i16>) -> Option<&LegacyIdentity> {
        Self::lookup(&self.inputs, name, data)
    }

    pub(crate) fn result_alias(&self, name: &str, data: i16) -> Option<&LegacyIdentity> {
        Self::lookup(&self.outputs, name, Some(data))
    }
}

/// Measured ingredient identity; exact aux aliases take precedence over a
/// name-only alias. Lookup does not allocate or decode JSON.
pub fn input_alias(name: &str, data: Option<i16>) -> Option<ItemIdentity<'static>> {
    vanilla()
        .input_alias(name, data)
        .map(|identity| ItemIdentity {
            name: &identity.name,
            data: identity.data,
        })
}

/// Measured result identity. This is separate from ingredient matching because
/// a result has a concrete aux while an ingredient may match every aux value.
pub fn result_alias(name: &str, data: i16) -> Option<ItemIdentity<'static>> {
    vanilla()
        .result_alias(name, data)
        .map(|identity| ItemIdentity {
            name: &identity.name,
            data: identity.data,
        })
}

/// Whether the measured family has legacy aux aliases. An unmeasured nonzero
/// aux in such a family must not silently become the canonical base item.
pub fn has_aux_aliases(name: &str) -> bool {
    let identities = vanilla();
    identities
        .inputs
        .get(name)
        .is_some_and(|variants| variants.has_aux_aliases)
        || identities
            .outputs
            .get(name)
            .is_some_and(|variants| variants.has_aux_aliases)
}
