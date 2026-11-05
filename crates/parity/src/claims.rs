//! Claims: `docs/parity/claims.toml`, the areas someone is actively working on.
//!
//! A claim names one domain and selects rows of it by family, by id (the row
//! and its sub-rows) or by id prefix. Claims are hand-edited and only feed the
//! "Where to help" section of `PARITY.md`; they never change a row's status.
//! `parity check` requires every selector to match a generated row and no row
//! to be covered by two claims, so the free/claimed split is unambiguous.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context as _;
use serde::Deserialize;

use crate::catalog::{Domain, DomainRows, Row};

pub const FILE: &str = "docs/parity/claims.toml";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimsFile {
    #[serde(default)]
    claim: Vec<Claim>,
}

/// One `[[claim]]` table.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    /// Short human description of the area.
    pub area: String,
    /// Who works on it: `@github-handle`, `lane NAME` or a plain description.
    pub owner: String,
    /// Date the claim was made (a TOML local date, `2026-10-29`).
    pub since: toml::value::Datetime,
    /// Status file stem (`blocks`, `systems`, ...); checked by [`validate`].
    pub domain: String,
    #[serde(default)]
    pub families: Vec<String>,
    /// Row ids; each also covers that row's sub-rows.
    #[serde(default)]
    pub ids: Vec<String>,
    /// Plain string prefixes of row ids (`bedrock/`, `world.`).
    #[serde(default)]
    pub prefixes: Vec<String>,
    /// The active OpenSpec change doing the work, `openspec/changes/<change>/`;
    /// checked by [`validate_changes`]. Claims made without a change may omit it.
    #[serde(default)]
    pub change: Option<String>,
}

/// Where OpenSpec keeps in-flight changes, relative to the repository root.
pub const CHANGES_DIR: &str = "openspec/changes";

impl Claim {
    /// Whether this claim covers `row` of `domain`.
    pub fn covers(&self, domain: Domain, row: &Row) -> bool {
        self.domain == domain.name()
            && (self.families.contains(&row.family)
                || self
                    .ids
                    .iter()
                    .any(|id| *id == row.id || row.parent.as_deref() == Some(id.as_str()))
                || self.prefixes.iter().any(|p| row.id.starts_with(p.as_str())))
    }
}

/// Reads the claims file; an absent file means no claims.
pub fn load(root: &Path) -> anyhow::Result<Vec<Claim>> {
    let path = root.join(FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    parse(&text).with_context(|| format!("parsing {FILE}"))
}

pub fn parse(text: &str) -> anyhow::Result<Vec<Claim>> {
    Ok(toml::from_str::<ClaimsFile>(text)?.claim)
}

/// Index of the claim covering `row`, if any. With validated claims there is
/// at most one.
pub fn covering(claims: &[Claim], domain: Domain, row: &Row) -> Option<usize> {
    claims.iter().position(|c| c.covers(domain, row))
}

/// Problems in `claims` against the generated rows.
pub fn validate(claims: &[Claim], domains: &[&DomainRows]) -> Vec<String> {
    let mut problems = Vec::new();
    for claim in claims {
        let mut problem = |text: String| {
            problems.push(format!("{FILE}: claim `{}`: {text}", claim.area));
        };
        if claim.area.trim().is_empty() {
            problem("`area` is empty".to_owned());
        }
        if claim.owner.trim().is_empty() {
            problem("`owner` is empty".to_owned());
        }
        for (field, text) in [("area", &claim.area), ("owner", &claim.owner)] {
            if let Some(pattern) = crate::check::forbidden_provenance(text) {
                problem(format!(
                    "`{field}` contains `{pattern}`; describe behaviour, not engine internals"
                ));
            }
        }
        if claim.since.date.is_none() || claim.since.time.is_some() {
            problem(format!(
                "`since` must be a date (YYYY-MM-DD), not `{}`",
                claim.since
            ));
        }
        let Some(rows) = Domain::from_name(&claim.domain)
            .and_then(|domain| domains.iter().find(|rows| rows.domain == domain))
        else {
            let known: Vec<&str> = Domain::ALL.iter().map(|d| d.name()).collect();
            problem(format!(
                "unknown domain `{}` (one of {})",
                claim.domain,
                known.join(", ")
            ));
            continue;
        };
        if claim.families.is_empty() && claim.ids.is_empty() && claim.prefixes.is_empty() {
            problem("selects nothing: give `families`, `ids` or `prefixes`".to_owned());
        }
        for family in &claim.families {
            if !rows.rows.iter().any(|row| row.family == *family) {
                problem(format!("no {} family `{family}`", claim.domain));
            }
        }
        for id in &claim.ids {
            if !rows.rows.iter().any(|row| row.id == *id) {
                problem(format!("no {} row `{id}`", claim.domain));
            }
        }
        for prefix in &claim.prefixes {
            if prefix.is_empty() {
                problem("empty prefix; list the families instead".to_owned());
            } else if !rows
                .rows
                .iter()
                .any(|row| row.id.starts_with(prefix.as_str()))
            {
                problem(format!("prefix `{prefix}` matches no {} row", claim.domain));
            }
        }
    }
    problems.extend(overlaps(claims, domains));
    problems
}

/// Problems with the OpenSpec `change` each claim names: the change must be
/// an active one under [`CHANGES_DIR`] with a `proposal.md`. A change that has
/// been archived (`archive/<date>-<change>`) means the work is finished and the
/// claim is stale, so the claim must be removed in the archiving commit.
pub fn validate_changes(claims: &[Claim], root: &Path) -> Vec<String> {
    let changes = root.join(CHANGES_DIR);
    let mut problems = Vec::new();
    for claim in claims {
        let Some(change) = &claim.change else {
            continue;
        };
        let mut problem = |text: String| {
            problems.push(format!("{FILE}: claim `{}`: {text}", claim.area));
        };
        let well_formed = !change.is_empty()
            && change
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !well_formed {
            problem(format!(
                "`change` `{change}` is not an OpenSpec change name (kebab-case)"
            ));
            continue;
        }
        if changes.join(change).join("proposal.md").is_file() {
            continue;
        }
        let archived = std::fs::read_dir(changes.join("archive"))
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.get(11..))
                    .is_some_and(|rest| rest == change)
            });
        if archived {
            problem(format!(
                "change `{change}` is archived; the work is finished, so remove this claim"
            ));
        } else {
            problem(format!(
                "no active OpenSpec change `{change}` ({CHANGES_DIR}/{change}/proposal.md)"
            ));
        }
    }
    problems
}

/// One problem per pair of claims that cover a common row.
fn overlaps(claims: &[Claim], domains: &[&DomainRows]) -> Vec<String> {
    // (first claim, second claim) -> (shared rows, first shared id)
    let mut shared: BTreeMap<(usize, usize), (usize, &str)> = BTreeMap::new();
    for rows in domains {
        for row in &rows.rows {
            let covering: Vec<usize> = (0..claims.len())
                .filter(|&i| claims[i].covers(rows.domain, row))
                .collect();
            for (n, &a) in covering.iter().enumerate() {
                for &b in &covering[n + 1..] {
                    shared.entry((a, b)).or_insert((0, &row.id)).0 += 1;
                }
            }
        }
    }
    shared
        .into_iter()
        .map(|((a, b), (count, example))| {
            format!(
                "{FILE}: claims `{}` ({}) and `{}` ({}) overlap on {count} {} row(s), e.g. `{example}`",
                claims[a].area,
                claims[a].owner,
                claims[b].area,
                claims[b].owner,
                claims[a].domain
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks() -> DomainRows {
        let door = Row::new("minecraft:oak_door", "doors");
        let rows = vec![
            Row::child(&door, "redstone"),
            door,
            Row::new("minecraft:iron_door", "doors"),
            Row::new("minecraft:water", "liquids"),
            Row::new("minecraft:sponge", "surface_dynamic"),
            Row::new("minecraft:fire", "surface_dynamic"),
        ];
        DomainRows::new(Domain::Blocks, rows).unwrap()
    }

    fn claims(text: &str) -> Vec<Claim> {
        parse(text).unwrap()
    }

    const LIQUIDS: &str = r#"
        [[claim]]
        area = "Liquids"
        owner = "a contributor"
        since = 2026-10-29
        domain = "blocks"
        families = ["liquids"]
        ids = ["minecraft:sponge"]
    "#;

    #[test]
    fn families_ids_and_prefixes_select_rows_and_ids_take_their_sub_rows() {
        let text = format!(
            "{LIQUIDS}
            [[claim]]
            area = \"Doors\"
            owner = \"lane BLOCKS\"
            since = 2026-10-29
            domain = \"blocks\"
            prefixes = [\"minecraft:oak_\"]
            "
        );
        let claims = claims(&text);
        let blocks = blocks();
        assert!(validate(&claims, &[&blocks]).is_empty());
        let owner = |id: &str| {
            let row = blocks.rows.iter().find(|r| r.id == id).unwrap();
            covering(&claims, Domain::Blocks, row).map(|i| claims[i].owner.as_str())
        };
        assert_eq!(owner("minecraft:water"), Some("a contributor"));
        assert_eq!(owner("minecraft:sponge"), Some("a contributor"));
        assert_eq!(owner("minecraft:oak_door/redstone"), Some("lane BLOCKS"));
        assert_eq!(owner("minecraft:iron_door"), None);
        assert_eq!(owner("minecraft:fire"), None);
    }

    #[test]
    fn a_claim_only_covers_rows_of_its_own_domain() {
        let claims = claims(LIQUIDS);
        let water = Row::new("minecraft:water", "liquids");
        assert!(claims[0].covers(Domain::Blocks, &water));
        assert!(!claims[0].covers(Domain::Items, &water));
    }

    #[test]
    fn overlapping_claims_are_rejected_with_the_shared_row_count() {
        let text = format!(
            "{LIQUIDS}
            [[claim]]
            area = \"Surface blocks\"
            owner = \"@someone\"
            since = 2026-10-30
            domain = \"blocks\"
            families = [\"surface_dynamic\", \"liquids\"]
            "
        );
        let problems = validate(&claims(&text), &[&blocks()]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("`Liquids`"), "{problems:?}");
        assert!(problems[0].contains("`Surface blocks`"), "{problems:?}");
        // water (liquids) and sponge (an id inside surface_dynamic)
        assert!(
            problems[0].contains("overlap on 2 blocks row(s)"),
            "{problems:?}"
        );
    }

    #[test]
    fn unknown_domains_families_ids_and_prefixes_are_rejected() {
        let problems = validate(
            &claims(
                r#"
                [[claim]]
                area = "Typos"
                owner = "@someone"
                since = 2026-10-30
                domain = "blocks"
                families = ["door"]
                ids = ["minecraft:oak_dor"]
                prefixes = ["minecraft:birch_"]

                [[claim]]
                area = "Wrong domain"
                owner = "@someone"
                since = 2026-10-30
                domain = "block"
                families = ["doors"]
                "#,
            ),
            &[&blocks()],
        );
        let expected = [
            "no blocks family `door`",
            "no blocks row `minecraft:oak_dor`",
            "prefix `minecraft:birch_` matches no blocks row",
            "unknown domain `block`",
        ];
        assert_eq!(problems.len(), expected.len(), "{problems:?}");
        for text in expected {
            assert!(
                problems.iter().any(|p| p.contains(text)),
                "{text}: {problems:?}"
            );
        }
    }

    #[test]
    fn claims_need_an_owner_a_date_and_a_selector() {
        let problems = validate(
            &claims(
                r#"
                [[claim]]
                area = "Nothing"
                owner = " "
                since = 2026-10-30T12:00:00
                domain = "blocks"
                "#,
            ),
            &[&blocks()],
        );
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("`owner` is empty")));
        assert!(
            problems
                .iter()
                .any(|p| p.contains("`since` must be a date"))
        );
        assert!(problems.iter().any(|p| p.contains("selects nothing")));
    }

    /// A throwaway repository root holding `openspec/changes/` entries.
    fn changes_root(name: &str, active: &[&str], archived: &[&str]) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("parity-claims-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for change in active {
            let dir = root.join(CHANGES_DIR).join(change);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("proposal.md"), "# Proposal\n").unwrap();
        }
        for change in archived {
            let dir = root
                .join(CHANGES_DIR)
                .join("archive")
                .join(format!("2026-11-04-{change}"));
            std::fs::create_dir_all(&dir).unwrap();
        }
        root
    }

    fn claim_with_change(change: &str) -> Vec<Claim> {
        claims(&format!(
            "[[claim]]\narea = 'Liquids'\nowner = '@someone'\nsince = 2026-11-04\n\
             domain = 'blocks'\nfamilies = ['liquids']\nchange = '{change}'\n"
        ))
    }

    #[test]
    fn a_claim_may_name_its_active_openspec_change() {
        let root = changes_root("active", &["liquid-flow"], &[]);
        assert!(validate_changes(&claim_with_change("liquid-flow"), &root).is_empty());
        assert!(validate_changes(&claims(LIQUIDS), &root).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn claims_naming_missing_archived_or_malformed_changes_are_rejected() {
        let root = changes_root("stale", &[], &["liquid-flow"]);
        let archived = validate_changes(&claim_with_change("liquid-flow"), &root);
        assert!(
            archived.len() == 1 && archived[0].contains("is archived"),
            "{archived:?}"
        );
        let missing = validate_changes(&claim_with_change("liquid-flo"), &root);
        assert!(
            missing.len() == 1 && missing[0].contains("no active OpenSpec change"),
            "{missing:?}"
        );
        let malformed = validate_changes(&claim_with_change("Liquid Flow"), &root);
        assert!(
            malformed.len() == 1 && malformed[0].contains("kebab-case"),
            "{malformed:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unknown_fields_and_string_dates_fail_to_parse() {
        assert!(parse("[[claim]]\narea='a'\nowner='b'\nsince=2026-10-30\ndomain='blocks'\nfamily=['doors']").is_err());
        assert!(parse("[[claim]]\narea='a'\nowner='b'\nsince='2026-10-30'\ndomain='blocks'\nfamilies=['doors']").is_err());
    }
}
