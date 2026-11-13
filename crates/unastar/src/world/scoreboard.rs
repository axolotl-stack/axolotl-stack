//! The world scoreboard `/scoreboard` keeps: objectives, the three display
//! slots and the scores of players, other actors and fake players (names
//! without an entity).
//!
//! A holder gets a scoreboard id the first time it is scored, counting from
//! 1, and keeps it (measured on BDS 1.26.51: the first player scored is 1,
//! the fake player scored next is 2). The scoreboard is not saved with the
//! world yet, and a player who leaves loses its scores.

use bevy_ecs::prelude::*;

/// Whose score it is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScoreHolder {
    /// A player or another actor.
    Entity(Entity),
    /// A name with no entity behind it.
    Fake(String),
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
        let id = self.holders.last().map_or(1, |(id, _)| id + 1);
        self.holders.push((id, holder.clone()));
        id
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
