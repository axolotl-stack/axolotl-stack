//! Owned UI reconciliation with per-packet progress and exact current viewer bindings.

use super::*;
use crate::entity::components::{Player, PlayerSession};
use bevy_ecs::world::WorldId;
use jolyne::valentine::*;

#[derive(Clone)]
struct Row {
    key: String,
    text: String,
    id: i64,
    score: i32,
}
struct SidebarSent {
    version: u64,
    title: String,
    rows: Vec<Row>,
    world: WorldId,
}
struct Transition {
    packets: VecDeque<McpePacket>,
    after: Option<SidebarSent>,
}
struct BarSent {
    version: u64,
    tick: u64,
    world: WorldId,
}
/// Connection-level state follows migration, without retaining a live world borrow.
pub(super) struct Viewer {
    pub(super) target: PlayerLineage,
    objective: String,
    sidebar: Option<SidebarSent>,
    transition: Option<Transition>,
    next_row: i64,
    action_bar: Option<BarSent>,
    sent_form: Option<(u64, WorldId)>,
    cursor: u8,
}
impl Viewer {
    /// Allocate a private objective namespace independent of guest strings and account names.
    pub(super) fn new(target: PlayerLineage, objective: String) -> Self {
        Self {
            target,
            objective,
            sidebar: None,
            transition: None,
            next_row: 0,
            action_bar: None,
            sent_form: None,
            cursor: 0,
        }
    }
    /// Build one finite transition; newer updates wait until this queue makes progress.
    fn sidebar_goal(&mut self, goal: Option<&Desired>, world: WorldId) {
        let current = goal.and_then(|goal| match &goal.value {
            Value::Sidebar(Some(value)) => Some((goal.version, value)),
            _ => None,
        });
        if current.is_none() {
            if self.sidebar.is_some() || self.transition.is_some() {
                self.transition = Some(Transition {
                    packets: VecDeque::from([RemoveObjectivePacket {
                        objective_name: self.objective.as_str().into(),
                    }
                    .into()]),
                    after: None,
                });
            }
            return;
        }
        if self.transition.is_some() {
            return;
        }
        let (version, value) = current.expect("sidebar goal");
        if self
            .sidebar
            .as_ref()
            .is_some_and(|sent| sent.version == version && sent.world == world)
        {
            return;
        }
        let old = self.sidebar.as_ref();
        let mut rows = Vec::with_capacity(value.lines.len());
        for (index, (key, text)) in value.lines.iter().enumerate() {
            let id = if let Some(row) =
                old.and_then(|old| old.rows.iter().find(|row| row.key == *key))
            {
                row.id
            } else {
                let Some(id) = self.next_row.checked_add(1) else {
                    return;
                };
                self.next_row = id;
                id
            };
            rows.push(Row {
                key: key.clone(),
                text: text.clone(),
                id,
                score: (value.lines.len() - index) as i32,
            });
        }
        let reset = old.is_none_or(|old| old.world != world);
        let mut packets = VecDeque::new();
        if reset || old.is_some_and(|old| old.title != value.title) {
            packets.push_back(
                SetDisplayObjectivePacket {
                    display_slot_name: "sidebar".into(),
                    objective_name: self.objective.as_str().into(),
                    objective_display_name: value.title.as_str().into(),
                    criteria_name: "dummy".into(),
                    sort_order: 1,
                }
                .into(),
            );
        }
        let mut changes = Vec::new();
        if let Some(old) = old {
            for row in &old.rows {
                if reset
                    || !rows.iter().any(|new| {
                        new.id == row.id && new.text == row.text && new.score == row.score
                    })
                {
                    changes.push(SetScorePacketScoreInfoItem::RemoveScore(RemoveScore {
                        action: "remove".into(),
                        scoreboard_id: ScoreboardId {
                            scoreboard_id: row.id,
                        },
                        objective_name: Some(self.objective.as_str().into()),
                    }));
                }
            }
        }
        for row in &rows {
            if reset
                || old.is_none_or(|old| {
                    !old.rows.iter().any(|old| {
                        old.id == row.id && old.text == row.text && old.score == row.score
                    })
                })
            {
                changes.push(SetScorePacketScoreInfoItem::ChangeFakePlayerScore(
                    ChangeFakePlayerScore {
                        action: "changefakeplayer".into(),
                        scoreboard_id: ScoreboardId {
                            scoreboard_id: row.id,
                        },
                        objective_name: self.objective.as_str().into(),
                        score_value: row.score,
                        fake_player_name: row.text.as_str().into(),
                    },
                ));
            }
        }
        if !changes.is_empty() {
            packets.push_back(
                SetScorePacket {
                    score_info: changes,
                }
                .into(),
            );
        }
        self.transition = Some(Transition {
            packets,
            after: Some(SidebarSent {
                version,
                title: value.title.clone(),
                rows,
                world,
            }),
        });
    }
    /// Advance acknowledged packet-by-packet, so a one-slot channel cannot starve display creation.
    fn sidebar_send(&mut self, world: &World, actor: Entity, goal: Option<&Desired>) {
        self.sidebar_goal(goal, world.id());
        let Some(transition) = self.transition.as_mut() else {
            return;
        };
        while let Some(packet) = transition.packets.front() {
            if !world
                .get::<PlayerSession>(actor)
                .is_some_and(|session| session.send(packet.clone()))
            {
                return;
            }
            transition.packets.pop_front();
        }
        self.sidebar = self.transition.take().expect("sidebar transition").after;
    }
    /// Maintained action bars refresh on the world's actual tick counter.
    fn bar_send(&mut self, world: &World, actor: Entity, goal: Option<&Desired>, tick: u64) {
        let value = goal.and_then(|goal| match &goal.value {
            Value::ActionBar(Some(value)) => Some((goal.version, value.as_str())),
            _ => None,
        });
        if let Some((version, _)) = value
            && self.action_bar.as_ref().is_some_and(|sent| {
                sent.version == version
                    && sent.world == world.id()
                    && tick.saturating_sub(sent.tick) < 20
            })
        {
            return;
        }
        if value.is_none() && self.action_bar.is_none() {
            return;
        }
        let packet = SetTitlePacket {
            title_type: EnumsSetTitlePacketPayloadTitleType::Actionbar,
            title_text: value.map_or("", |(_, text)| text).into(),
            fade_in_time: 0,
            stay_time: 40,
            fade_out_time: 0,
            xuid: "".into(),
            platform_online_id: "".into(),
            filtered_title_message: "".into(),
        };
        if world
            .get::<PlayerSession>(actor)
            .is_some_and(|session| session.send(packet.into()))
        {
            self.action_bar = value.map(|(version, _)| BarSent {
                version,
                tick,
                world: world.id(),
            });
        }
    }
    /// A form becomes eligible for a response only after its exact request was actually sent.
    fn form_send(
        &mut self,
        world: &World,
        actor: Entity,
        goal: Option<&Desired>,
        forms: &mut HashMap<u32, ActiveForm>,
    ) {
        let value = goal.and_then(|goal| match &goal.value {
            Value::Form(Some(value)) => Some(value),
            _ => None,
        });
        let Some(value) = value else {
            if self.sent_form.is_some()
                && world
                    .get::<PlayerSession>(actor)
                    .is_some_and(|session| session.send(ClientboundCloseFormPacket {}.into()))
            {
                self.sent_form = None;
            }
            return;
        };
        if self.sent_form == Some((value.request, world.id())) {
            return;
        }
        if world.get::<PlayerSession>(actor).is_some_and(|session| {
            session.send(
                ModalFormRequestPacket {
                    form_id: value.wire,
                    form_uijson: value.payload.as_ref().into(),
                }
                .into(),
            )
        }) {
            self.sent_form = Some((value.request, world.id()));
            if let Some(form) = forms.get_mut(&value.wire) {
                form.sent_at.get_or_insert_with(std::time::Instant::now);
            }
        }
    }
}

/// Send to current admitted viewers only; native publication barriers remain authoritative.
pub(super) fn reconcile(world: &mut World) {
    let Some(registry) = world.get_resource::<Registry>().cloned() else {
        return;
    };
    let Some(tick) = world
        .get_resource::<crate::ecs::TickCounter>()
        .map(|tick| tick.current)
    else {
        return;
    };
    let actors: Vec<_> = world
        .query_filtered::<Entity, (With<Player>, With<PlayerSession>)>()
        .iter(world)
        .collect();
    let mut book = registry.0.lock();
    book.prune_owners();
    let expired: Vec<_> = book
        .forms
        .values()
        .filter(|form| {
            form.sent_at
                .is_some_and(|at| at.elapsed() >= std::time::Duration::from_secs(60))
        })
        .map(|form| form.target.key())
        .collect();
    for key in expired {
        book.retire_form(key, wit::FormCloseReason::Expired);
        book.claims.remove(&(key, Widget::Form));
        book.desired.remove(&(key, Widget::Form));
    }
    for actor in actors {
        let Some(target) = world.get::<PlayerLineage>(actor).cloned() else {
            continue;
        };
        let key = target.key();
        let Some(mut viewer) = book.viewers.remove(&key) else {
            continue;
        };
        if !viewer.target.matches(world, actor) {
            book.viewers.insert(key, viewer);
            continue;
        }
        if crate::server::game::player_events::initial_state_pending(world, actor)
            || crate::server::game::inventory_effects::is_quarantined(world, actor)
            || crate::server::worlds::is_loading(world, actor)
        {
            book.viewers.insert(key, viewer);
            continue;
        }
        for offset in 0..3 {
            match (viewer.cursor + offset) % 3 {
                0 => {
                    let goal = book.desired.get(&(key, Widget::Form)).map(|goal| Desired {
                        version: goal.version,
                        value: goal.value.clone(),
                    });
                    viewer.form_send(world, actor, goal.as_ref(), &mut book.forms);
                }
                1 => viewer.sidebar_send(world, actor, book.desired.get(&(key, Widget::Sidebar))),
                _ => viewer.bar_send(
                    world,
                    actor,
                    book.desired.get(&(key, Widget::ActionBar)),
                    tick,
                ),
            }
        }
        viewer.cursor = (viewer.cursor + 1) % 3;
        book.viewers.insert(key, viewer);
    }
}
