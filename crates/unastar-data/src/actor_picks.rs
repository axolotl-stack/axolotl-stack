//! The item a creative pick of an actor gives, measured from BDS 1.26.51
//! (`data/measured/actor_pick_items.json`, `tools/bds-sweep/pick_table.py`
//! over `protocol_probe.py pick-actors` runs).
//!
//! BDS ships no table of these: the spawn-egg flag of
//! `AvailableActorIdentifiers` does not decide it (armor stands and the ender
//! dragon give nothing, the wither its egg although the egg is not in the
//! creative inventory), so every summonable actor was picked. Actor names
//! are resolved to [`ActorType`] when the table is decoded.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::actor_identifiers::ActorType;

/// What picking one actor gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorPickItem {
    /// The item identifier (`minecraft:cow_spawn_egg`).
    pub item: String,
    pub aux: i16,
    /// The `GuiDataPickItem` name BDS shows (`item.spawn_egg.entity.cow.name`).
    pub name: String,
    /// The item user data a pick with data gives instead, when it differs
    /// (the actor's contents, for chest boats and minecarts).
    pub data_user_data: Option<Vec<u8>>,
}

/// The measured pick table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorPicks {
    pub items: BTreeMap<ActorType, ActorPickItem>,
    /// Actors whose pick BDS ignored.
    pub no_item: Vec<ActorType>,
}

/// Why the measured table does not decode.
#[derive(Debug, thiserror::Error)]
pub enum ActorPickError {
    #[error("actor_pick_items.json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("actor_pick_items.json: unknown actor `{0}`")]
    UnknownActor(String),
    #[error("actor_pick_items.json: `{actor}` data_user_data is not hex")]
    Hex { actor: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawItem {
    item: String,
    aux: i16,
    name: String,
    #[serde(default)]
    data_user_data: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTable {
    #[allow(dead_code)]
    bds_version: String,
    #[allow(dead_code)]
    method: String,
    #[allow(dead_code)]
    source_sha256: Vec<String>,
    actors: BTreeMap<String, RawItem>,
    no_item: Vec<String>,
}

fn actor(name: &str) -> Result<ActorType, ActorPickError> {
    ActorType::from_name(name).ok_or_else(|| ActorPickError::UnknownActor(name.to_owned()))
}

fn unhex(actor: &str, text: &str) -> Result<Vec<u8>, ActorPickError> {
    let hex = |at: usize| {
        text.get(at..at + 2)
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
    };
    if !text.len().is_multiple_of(2) {
        return Err(ActorPickError::Hex {
            actor: actor.to_owned(),
        });
    }
    (0..text.len())
        .step_by(2)
        .map(|at| {
            hex(at).ok_or_else(|| ActorPickError::Hex {
                actor: actor.to_owned(),
            })
        })
        .collect()
}

impl ActorPicks {
    /// Decodes a measured table.
    pub fn decode(json: &str) -> Result<Self, ActorPickError> {
        let raw: RawTable = serde_json::from_str(json)?;
        let items = raw
            .actors
            .into_iter()
            .map(|(name, row)| {
                let data_user_data = row
                    .data_user_data
                    .as_deref()
                    .map(|text| unhex(&name, text))
                    .transpose()?;
                Ok((
                    actor(&name)?,
                    ActorPickItem {
                        item: row.item,
                        aux: row.aux,
                        name: row.name,
                        data_user_data,
                    },
                ))
            })
            .collect::<Result<_, ActorPickError>>()?;
        let no_item = raw
            .no_item
            .iter()
            .map(|name| actor(name))
            .collect::<Result<_, _>>()?;
        Ok(Self { items, no_item })
    }
}

/// The table measured from BDS 1.26.51.
pub fn vanilla() -> &'static ActorPicks {
    static DATA: LazyLock<ActorPicks> = LazyLock::new(|| {
        ActorPicks::decode(include_str!("../data/measured/actor_pick_items.json"))
            .expect("embedded measured actor pick table decodes")
    });
    &DATA
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_measured_table_decodes_to_known_actors_and_items() {
        let table = vanilla();
        let cow = &table.items[&ActorType::Cow];
        assert_eq!(cow.item, "minecraft:cow_spawn_egg");
        assert_eq!(cow.name, "item.spawn_egg.entity.cow.name");
        for row in table.items.values() {
            assert!(crate::items::get(&row.item).is_some(), "{}", row.item);
        }
        assert!(
            table
                .no_item
                .iter()
                .all(|actor| !table.items.contains_key(actor))
        );
    }

    #[test]
    fn unknown_actors_and_bad_hex_are_typed_errors() {
        let table = |actors: &str| {
            format!(
                r#"{{"bds_version":"1.26.51","method":"m","source_sha256":["s"],"actors":{actors},"no_item":[]}}"#
            )
        };
        assert!(matches!(
            ActorPicks::decode(&table(
                r#"{"minecraft:nope":{"item":"minecraft:stone","aux":0,"name":"n"}}"#
            )),
            Err(ActorPickError::UnknownActor(name)) if name == "minecraft:nope"
        ));
        assert!(matches!(
            ActorPicks::decode(&table(
                r#"{"minecraft:cow":{"item":"minecraft:stone","aux":0,"name":"n","data_user_data":"0g"}}"#
            )),
            Err(ActorPickError::Hex { .. })
        ));
    }
}
