//! The generated denominator: every vanilla thing per domain.
//!
//! Rows come from vanilla data (packs, native tables, captures, protocol
//! manifests); only the gameplay-systems domain is a hand-maintained list,
//! because those systems have no data source. A row's `id` is the key of its
//! status entry in `docs/parity/<domain>.toml`.

use std::fmt;

/// One catalog domain; each has its own status file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Domain {
    Entities,
    Blocks,
    Items,
    Commands,
    Protocol,
    Worldgen,
    Systems,
}

/// Which rows carry a domain's headline numbers: the summary lists them first,
/// and the family table, gap list and "where to help" count them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Headline {
    TopLevel,
    SubRows,
}

impl Headline {
    pub fn includes(self, row: &Row) -> bool {
        match self {
            Headline::TopLevel => row.parent.is_none(),
            Headline::SubRows => row.parent.is_some(),
        }
    }
}

impl Domain {
    pub const ALL: [Domain; 7] = [
        Domain::Entities,
        Domain::Blocks,
        Domain::Items,
        Domain::Commands,
        Domain::Protocol,
        Domain::Worldgen,
        Domain::Systems,
    ];

    /// The domain whose status file stem is `name`.
    pub fn from_name(name: &str) -> Option<Domain> {
        Domain::ALL.into_iter().find(|d| d.name() == name)
    }

    /// File stem under `docs/parity/`.
    pub fn name(self) -> &'static str {
        match self {
            Domain::Entities => "entities",
            Domain::Blocks => "blocks",
            Domain::Items => "items",
            Domain::Commands => "commands",
            Domain::Protocol => "protocol",
            Domain::Worldgen => "worldgen",
            Domain::Systems => "systems",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Domain::Entities => "Entities",
            Domain::Blocks => "Blocks",
            Domain::Items => "Items",
            Domain::Commands => "Commands",
            Domain::Protocol => "Protocol",
            Domain::Worldgen => "World generation",
            Domain::Systems => "Gameplay systems",
        }
    }

    /// What one row of this domain means, rendered into the status file header
    /// and `PARITY.md`.
    pub fn row_meaning(self) -> &'static str {
        match self {
            Domain::Entities => {
                "One row per vanilla actor (`minecraft:cow`): it can be spawned/summoned and is \
                 replicated to clients like BDS. Sub-rows `<actor>/<component>` cover each \
                 behaviour-pack component the actor uses (base and component groups), and \
                 `<actor>/spawn_rules` its natural spawning."
            }
            Domain::Blocks => {
                "One row per vanilla block: it can exist in the world, be placed and broken with \
                 vanilla drops, and its family behaviour (doors open, crops grow, ...) matches BDS."
            }
            Domain::Items => {
                "One row per vanilla item: it exists in the registry and behaves as an inventory \
                 stack. Sub-rows `<item>/<facet>` cover each server-side behaviour the item has \
                 (food, durability, block placement, shooting, ...)."
            }
            Domain::Commands => {
                "One row per command BDS 1.26.51 advertises in `AvailableCommands`: syntax, \
                 permission, execution and output match BDS."
            }
            Domain::Protocol => {
                "One row per 1.26.51 packet: inbound packets are handled, outbound packets are \
                 sent when vanilla would send them."
            }
            Domain::Worldgen => {
                "One row per worldgen element (biome, feature, structure, carver, surface rule) \
                 per edition: `java/` rows target Java 1.21.11, `bedrock/` rows Bedrock 1.26.51."
            }
            Domain::Systems => {
                "Cross-cutting gameplay systems with no data source to enumerate them from; this \
                 list is maintained by hand in `crates/parity/src/sources/systems.rs`."
            }
        }
    }

    /// Item rows only say the item is registered, so a 100% there says nothing
    /// about behaviour; the behaviour sub-rows are the items headline.
    pub fn headline(self) -> Headline {
        match self {
            Domain::Items => Headline::SubRows,
            _ => Headline::TopLevel,
        }
    }

    /// Summary label for top-level rows whose meaning is narrower than the
    /// domain name suggests; `None` when the domain name says it.
    pub fn top_level_label(self) -> Option<&'static str> {
        match self {
            Domain::Entities => Some("actor rows (summon and replication)"),
            Domain::Items => Some("registered (exists in the registry, no behaviour)"),
            _ => None,
        }
    }

    /// Summary label for sub-rows.
    pub fn sub_row_label(self) -> &'static str {
        match self {
            Domain::Entities => "behaviour sub-rows (components, spawn rules)",
            Domain::Items => "behaviour sub-rows (food, durability, placement, ...)",
            _ => "sub-rows",
        }
    }
}

impl fmt::Display for Domain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One generated row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Status key, unique within the domain.
    pub id: String,
    /// Behaviour family used for grouping (`doors`, `hostile`, `java/overworld/feature`, ...).
    pub family: String,
    /// Parent row id for sub-rows (`minecraft:cow` for `minecraft:cow/breedable`).
    pub parent: Option<String>,
    /// Short description for rows whose id alone is not self-explanatory.
    pub label: Option<String>,
}

impl Row {
    pub fn new(id: impl Into<String>, family: impl Into<String>) -> Self {
        Row {
            id: id.into(),
            family: family.into(),
            parent: None,
            label: None,
        }
    }

    pub fn child(parent: &Row, suffix: &str) -> Self {
        Row {
            id: format!("{}/{suffix}", parent.id),
            family: parent.family.clone(),
            parent: Some(parent.id.clone()),
            label: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The part of a sub-row id after its parent (`breedable`), used for
    /// cross-row rollups such as "most-missing entity components".
    pub fn facet(&self) -> Option<&str> {
        let parent = self.parent.as_deref()?;
        self.id.strip_prefix(parent)?.strip_prefix('/')
    }
}

/// All rows of one domain, sorted by family then id, ids unique.
#[derive(Debug, Clone)]
pub struct DomainRows {
    pub domain: Domain,
    pub rows: Vec<Row>,
}

impl DomainRows {
    /// Sorts rows into catalog order and rejects duplicate ids, which would
    /// make two generated things share one status entry.
    pub fn new(domain: Domain, mut rows: Vec<Row>) -> anyhow::Result<Self> {
        rows.sort_by(|a, b| {
            a.family
                .cmp(&b.family)
                .then_with(|| sort_key(a).cmp(&sort_key(b)))
        });
        let mut seen = std::collections::HashSet::new();
        for row in &rows {
            anyhow::ensure!(
                seen.insert(row.id.as_str()),
                "{domain}: generated id `{}` appears twice",
                row.id
            );
        }
        Ok(DomainRows { domain, rows })
    }
}

/// Parents sort before their sub-rows: `(parent or self, is_sub_row, id)`.
fn sort_key(row: &Row) -> (&str, bool, &str) {
    (
        row.parent.as_deref().unwrap_or(&row.id),
        row.parent.is_some(),
        &row.id,
    )
}
