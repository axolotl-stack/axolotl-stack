//! The world scoreboard `/scoreboard` keeps: objectives, the three display
//! slots and the scores of players, other actors and fake players (names
//! without an entity).
//!
//! A holder gets a scoreboard id the first time it is scored, counting from
//! 1, and keeps it (measured on BDS 1.26.51: the first player scored is 1,
//! the fake player scored next is 2).
//!
//! The scoreboard is saved with the world (`crate::world::level_data`, the
//! `scoreboard` record). Players and actors are stored by their persistent
//! unique id, as BDS does: a player who leaves and a mob whose chunk unloads
//! keep their scores under [`ScoreHolder::OfflinePlayer`] /
//! [`ScoreHolder::UnloadedActor`], and get them back when they return
//! ([`PlayerUniqueId`], the mobs' `MobUniqueId`).

use bevy_ecs::prelude::*;

pub use crate::storage::level::records::StoredScoreboard;
use crate::storage::level::records::{StoredDisplay, StoredIdentity, StoredObjective};

/// Whose score it is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScoreHolder {
    /// A player or another actor in the world.
    Entity(Entity),
    /// A name with no entity behind it.
    Fake(String),
    /// A player who is not online, by its [`PlayerUniqueId`].
    OfflinePlayer(i64),
    /// An actor that is not loaded, by its saved unique id.
    UnloadedActor(i64),
}

/// The id a player's scores are saved under: BDS's player `UniqueID`, kept in
/// the player record (unastar's `PlayerData::unique_id`) so a player gets its
/// scores back when it joins again.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct PlayerUniqueId(pub i64);

impl PlayerUniqueId {
    /// A new id from the operating system's randomness, never 0 or -1 (the
    /// invalid actor id), like a new mob's.
    pub fn fresh() -> Self {
        loop {
            let id: i64 = rand::random();
            if id != 0 && id != -1 {
                return Self(id);
            }
        }
    }
}

/// Where an objective is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplaySlot {
    List,
    Sidebar,
    BelowName,
}

impl DisplaySlot {
    pub const ALL: [Self; 3] = [Self::List, Self::Sidebar, Self::BelowName];

    /// The name in commands and in `SetDisplayObjective`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Sidebar => "sidebar",
            Self::BelowName => "belowname",
        }
    }
}

/// Display order of a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

impl SortOrder {
    /// `SetDisplayObjective.sort_order`.
    pub const fn wire(self) -> i32 {
        match self {
            Self::Ascending => 0,
            Self::Descending => 1,
        }
    }
}

/// An objective and its scores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Objective {
    pub name: String,
    pub display_name: String,
    pub criteria: String,
    /// `(scoreboard id, score)` in the order the holders were first scored.
    scores: Vec<(i64, i32)>,
}

impl Objective {
    /// Scores by scoreboard id, in the order they were first set.
    pub fn scores(&self) -> &[(i64, i32)] {
        &self.scores
    }

    fn score(&self, id: i64) -> Option<i32> {
        self.scores.iter().find(|(i, _)| *i == id).map(|(_, s)| *s)
    }
}

/// What a display slot shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    pub objective: String,
    pub order: SortOrder,
}

/// The scoreboard of the world.
#[derive(Resource, Debug, Default, Clone)]
pub struct Scoreboard {
    objectives: Vec<Objective>,
    holders: Vec<(i64, ScoreHolder)>,
    displays: Vec<(DisplaySlot, Display)>,
    /// The last scoreboard id handed out (BDS's `LastUniqueID`): ids are not
    /// reused, even after a holder lost every score and was not saved.
    last_id: i64,
}

impl Scoreboard {
    /// Objectives in the order they were added.
    pub fn objectives(&self) -> &[Objective] {
        &self.objectives
    }

    pub fn objective(&self, name: &str) -> Option<&Objective> {
        self.objectives.iter().find(|o| o.name == name)
    }

    /// Adds an objective; `false` if one has that name already.
    pub fn add_objective(&mut self, name: &str, display_name: &str, criteria: &str) -> bool {
        if self.objective(name).is_some() {
            return false;
        }
        self.objectives.push(Objective {
            name: name.to_owned(),
            display_name: display_name.to_owned(),
            criteria: criteria.to_owned(),
            scores: Vec::new(),
        });
        true
    }

    /// Removes an objective and every display showing it.
    pub fn remove_objective(&mut self, name: &str) -> Option<Objective> {
        let index = self.objectives.iter().position(|o| o.name == name)?;
        self.displays.retain(|(_, d)| d.objective != name);
        Some(self.objectives.remove(index))
    }

    /// The holder's scoreboard id, if it was ever scored.
    pub fn id_of(&self, holder: &ScoreHolder) -> Option<i64> {
        self.holders
            .iter()
            .find(|(_, h)| h == holder)
            .map(|(id, _)| *id)
    }

    pub fn holder(&self, id: i64) -> Option<&ScoreHolder> {
        self.holders.iter().find(|(i, _)| *i == id).map(|(_, h)| h)
    }

    fn id_or_insert(&mut self, holder: &ScoreHolder) -> i64 {
        if let Some(id) = self.id_of(holder) {
            return id;
        }
        self.last_id += 1;
        let id = self.last_id;
        self.holders.push((id, holder.clone()));
        id
    }

    /// A player joined or an actor loaded: the stored holder becomes `entity`
    /// and keeps its scoreboard id and scores.
    pub fn bind(&mut self, stored: &ScoreHolder, entity: Entity) {
        if let Some((_, holder)) = self.holders.iter_mut().find(|(_, h)| h == stored) {
            *holder = ScoreHolder::Entity(entity);
        }
    }

    /// `entity` left the world: its scores stay under `stored`.
    pub fn unbind(&mut self, entity: Entity, stored: ScoreHolder) {
        let held = ScoreHolder::Entity(entity);
        if let Some((_, holder)) = self.holders.iter_mut().find(|(_, h)| *h == held) {
            *holder = stored;
        }
    }

    /// The scoreboard as the `scoreboard` record stores it. `identity` gives
    /// the stored identity of an entity holder, `None` for an entity that is
    /// not saved (its scores are left out, as its entity will not come back).
    /// Holders without a score are left out, as BDS does.
    pub fn to_stored(
        &self,
        mut identity: impl FnMut(Entity) -> Option<StoredIdentity>,
    ) -> StoredScoreboard {
        let mut entries = Vec::new();
        for (id, holder) in &self.holders {
            if !self.objectives.iter().any(|o| o.score(*id).is_some()) {
                continue;
            }
            let stored = match holder {
                ScoreHolder::Entity(entity) => identity(*entity),
                ScoreHolder::Fake(name) => Some(StoredIdentity::Fake(name.clone())),
                ScoreHolder::OfflinePlayer(player) => Some(StoredIdentity::Player(*player)),
                ScoreHolder::UnloadedActor(actor) => Some(StoredIdentity::Entity(*actor)),
            };
            if let Some(stored) = stored {
                entries.push((*id, stored));
            }
        }
        let kept = |id: &i64| entries.iter().any(|(known, _)| known == id);
        StoredScoreboard {
            objectives: self
                .objectives
                .iter()
                .map(|objective| StoredObjective {
                    name: objective.name.clone(),
                    display_name: objective.display_name.clone(),
                    criteria: objective.criteria.clone(),
                    scores: objective
                        .scores
                        .iter()
                        .filter(|(id, _)| kept(id))
                        .copied()
                        .collect(),
                })
                .collect(),
            // BDS writes the slots in no fixed order (it varies between saves).
            displays: DisplaySlot::ALL
                .into_iter()
                .filter_map(|slot| {
                    self.display(slot).map(|display| StoredDisplay {
                        slot: slot.name().to_owned(),
                        objective: display.objective.clone(),
                        sort_order: display.order.wire() as i8,
                    })
                })
                .collect(),
            entries,
            last_unique_id: self.last_id,
        }
    }

    /// The scoreboard a `scoreboard` record holds: players offline, actors
    /// unloaded until they come back.
    pub fn from_stored(stored: StoredScoreboard) -> Result<Self, String> {
        let mut board = Self {
            last_id: stored.last_unique_id,
            ..Self::default()
        };
        for (id, identity) in stored.entries {
            if id <= 0 || id > stored.last_unique_id {
                return Err(format!(
                    "ScoreboardId {id} is outside 1..={}",
                    stored.last_unique_id
                ));
            }
            let holder = match identity {
                StoredIdentity::Player(player) => ScoreHolder::OfflinePlayer(player),
                StoredIdentity::Entity(actor) => ScoreHolder::UnloadedActor(actor),
                StoredIdentity::Fake(name) => ScoreHolder::Fake(name),
            };
            if board.id_of(&holder).is_some() {
                return Err(format!("{holder:?} has two scoreboard ids"));
            }
            board.holders.push((id, holder));
        }
        for objective in stored.objectives {
            if board.objective(&objective.name).is_some() {
                return Err(format!("objective {:?} is stored twice", objective.name));
            }
            board.objectives.push(Objective {
                name: objective.name,
                display_name: objective.display_name,
                criteria: objective.criteria,
                scores: objective.scores,
            });
        }
        for display in stored.displays {
            let slot = DisplaySlot::ALL
                .into_iter()
                .find(|slot| slot.name() == display.slot)
                .ok_or_else(|| format!("unknown display slot {:?}", display.slot))?;
            if board.objective(&display.objective).is_none() {
                return Err(format!(
                    "slot {} shows the missing objective {:?}",
                    display.slot, display.objective
                ));
            }
            let order = match display.sort_order {
                0 => SortOrder::Ascending,
                1 => SortOrder::Descending,
                other => return Err(format!("sort order {other}")),
            };
            board.set_display(
                slot,
                Some(Display {
                    objective: display.objective,
                    order,
                }),
            );
        }
        Ok(board)
    }

    pub fn score(&self, holder: &ScoreHolder, objective: &str) -> Option<i32> {
        let id = self.id_of(holder)?;
        self.objective(objective)?.score(id)
    }

    /// Sets a score; returns the holder's scoreboard id. The objective must exist.
    pub fn set_score(&mut self, holder: &ScoreHolder, objective: &str, value: i32) -> i64 {
        let id = self.id_or_insert(holder);
        let objective = self
            .objectives
            .iter_mut()
            .find(|o| o.name == objective)
            .expect("set_score on an existing objective");
        match objective.scores.iter_mut().find(|(i, _)| *i == id) {
            Some((_, score)) => *score = value,
            None => objective.scores.push((id, value)),
        }
        id
    }

    /// Removes one score; returns whether there was one.
    pub fn reset_score(&mut self, holder: &ScoreHolder, objective: &str) -> bool {
        let Some(id) = self.id_of(holder) else {
            return false;
        };
        let Some(objective) = self.objectives.iter_mut().find(|o| o.name == objective) else {
            return false;
        };
        let before = objective.scores.len();
        objective.scores.retain(|(i, _)| *i != id);
        objective.scores.len() != before
    }

    /// The holder's scores, in objective order.
    pub fn scores_of(&self, holder: &ScoreHolder) -> Vec<(&Objective, i32)> {
        let Some(id) = self.id_of(holder) else {
            return Vec::new();
        };
        self.objectives
            .iter()
            .filter_map(|o| o.score(id).map(|score| (o, score)))
            .collect()
    }

    /// Holders with at least one score, by scoreboard id.
    pub fn scored_holders(&self) -> Vec<(i64, &ScoreHolder)> {
        self.holders
            .iter()
            .filter(|(id, _)| self.objectives.iter().any(|o| o.score(*id).is_some()))
            .map(|(id, holder)| (*id, holder))
            .collect()
    }

    pub fn display(&self, slot: DisplaySlot) -> Option<&Display> {
        self.displays
            .iter()
            .find(|(s, _)| *s == slot)
            .map(|(_, d)| d)
    }

    /// Shows `display` in `slot`, or clears the slot.
    pub fn set_display(&mut self, slot: DisplaySlot, display: Option<Display>) {
        self.displays.retain(|(s, _)| *s != slot);
        if let Some(display) = display {
            self.displays.push((slot, display));
        }
    }

    /// The slots showing `objective`.
    pub fn slots_showing(&self, objective: &str) -> Vec<DisplaySlot> {
        DisplaySlot::ALL
            .into_iter()
            .filter(|slot| {
                self.display(*slot)
                    .is_some_and(|d| d.objective == objective)
            })
            .collect()
    }
}

/// Observer: a player came online with its saved id; it gets its scores back.
pub fn player_joined(
    added: On<Insert, PlayerUniqueId>,
    ids: Query<&PlayerUniqueId>,
    board: Option<ResMut<Scoreboard>>,
) {
    if let (Ok(id), Some(mut board)) = (ids.get(added.entity), board) {
        board.bind(&ScoreHolder::OfflinePlayer(id.0), added.entity);
    }
}

/// Observer: a player left; its scores stay under its saved id.
pub fn player_left(
    replaced: On<Replace, PlayerUniqueId>,
    ids: Query<&PlayerUniqueId>,
    board: Option<ResMut<Scoreboard>>,
) {
    if let (Ok(id), Some(mut board)) = (ids.get(replaced.entity), board) {
        board.unbind(replaced.entity, ScoreHolder::OfflinePlayer(id.0));
    }
}

/// Observer: a saved actor loaded (or was given its unique id).
pub fn actor_loaded(
    added: On<Insert, crate::entity::mob::persistence::MobUniqueId>,
    ids: Query<&crate::entity::mob::persistence::MobUniqueId>,
    board: Option<ResMut<Scoreboard>>,
) {
    if let (Ok(id), Some(mut board)) = (ids.get(added.entity), board) {
        board.bind(&ScoreHolder::UnloadedActor(id.0), added.entity);
    }
}

/// Observer: a saved actor left the world (its chunk unloaded, it died or
/// despawned); its scores stay under its unique id.
pub fn actor_unloaded(
    replaced: On<Replace, crate::entity::mob::persistence::MobUniqueId>,
    ids: Query<&crate::entity::mob::persistence::MobUniqueId>,
    board: Option<ResMut<Scoreboard>>,
) {
    if let (Ok(id), Some(mut board)) = (ids.get(replaced.entity), board) {
        board.unbind(replaced.entity, ScoreHolder::UnloadedActor(id.0));
    }
}

/// Registers the observers that keep scores with players and saved actors
/// across leaving, unloading and restarts.
pub fn install(world: &mut World) {
    world.add_observer(player_joined);
    world.add_observer(player_left);
    world.add_observer(actor_loaded);
    world.add_observer(actor_unloaded);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holders_get_ids_in_the_order_they_are_first_scored() {
        let mut board = Scoreboard::default();
        assert!(board.add_objective("kills", "Kills", "dummy"));
        assert!(!board.add_objective("kills", "kills", "dummy"));
        let fake = ScoreHolder::Fake("Fake".into());
        let other = ScoreHolder::Fake("Other".into());
        assert_eq!(board.set_score(&fake, "kills", 3), 1);
        assert_eq!(board.set_score(&other, "kills", 4), 2);
        assert_eq!(board.set_score(&fake, "kills", 5), 1);
        assert_eq!(board.score(&fake, "kills"), Some(5));
        assert!(board.reset_score(&fake, "kills"));
        assert!(!board.reset_score(&fake, "kills"));
        assert_eq!(board.scored_holders(), vec![(2, &other)]);
        // An id is kept after its scores are gone.
        assert_eq!(board.set_score(&fake, "kills", 1), 1);

        board.set_display(
            DisplaySlot::Sidebar,
            Some(Display {
                objective: "kills".into(),
                order: SortOrder::Ascending,
            }),
        );
        assert_eq!(board.slots_showing("kills"), [DisplaySlot::Sidebar]);
        assert!(board.remove_objective("kills").is_some());
        assert!(board.display(DisplaySlot::Sidebar).is_none());
        assert!(board.scores_of(&fake).is_empty());
    }
}
