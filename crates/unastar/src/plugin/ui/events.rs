//! Native form intake consumes exact requests before queuing fresh scoped guest callbacks.

use super::*;
use crate::entity::components::PlayerSession;
use crate::server::game::player_events::{PlayerEventHandler, PlayerFormResponse, PlayerQuit};

/// Protocol intake never synchronously reenters guest code.
pub(super) struct Intake(pub(super) Registry);
impl PlayerEventHandler for Intake {
    fn on_form_response(&self, world: &mut World, event: &PlayerFormResponse) {
        let mut book = self.0.0.lock();
        let Some(form) = book.forms.get(&event.form_id) else {
            return;
        };
        if !form.owner.alive()
            || form.sent_at.is_none()
            || !event.identity().matches(world, event.player())
            || !form.target.matches(world, event.player())
        {
            return;
        }
        let response = if event.cancelled {
            wit::FormResponse::Closed(wit::FormCloseReason::Dismissed)
        } else {
            match event
                .data
                .as_deref()
                .filter(|data| data.len() <= 32)
                .and_then(|data| serde_json::from_str::<Option<u32>>(data).ok())
            {
                Some(Some(index)) if index < form.value.buttons => {
                    wit::FormResponse::Selected(index)
                }
                Some(None) => wit::FormResponse::Closed(wit::FormCloseReason::Dismissed),
                _ => wit::FormResponse::Closed(wit::FormCloseReason::InvalidResponse),
            }
        };
        let form = book
            .forms
            .remove(&event.form_id)
            .expect("validated form request");
        let key = (form.target.key(), Widget::Form);
        let current = book.desired.get(&key).is_some_and(|desired| {
            matches!(&desired.value, Value::Form(Some(value)) if value.request == form.value.request)
        });
        if current {
            book.desired.remove(&key);
            book.claims.remove(&key);
        }
        book.notices.push_back(Notice::Response {
            target: Box::new(form.target),
            owner: form.owner,
            request: form.value.request,
            callback: form.value.callback,
            response,
        });
    }
    fn on_quit(&self, world: &mut World, event: &mut PlayerQuit) {
        let Some(target) = world.get::<PlayerLineage>(event.player()) else {
            return;
        };
        if event.identity().matches(world, event.player()) {
            disconnect(&mut self.0.0.lock(), target.key());
        }
    }
}

/// Remove disconnected widgets without ever sending to a replacement channel.
fn disconnect(book: &mut Book, key: uuid::Uuid) {
    book.retire_form(key, wit::FormCloseReason::Disconnected);
    book.claims.retain(|(viewer, _), _| *viewer != key);
    book.desired.retain(|(viewer, _), _| *viewer != key);
    book.viewers.remove(&key);
}

/// Invoke one owner only, retaining the original cycle's fuel and host budgets.
fn notify(manager: &mut PluginManager, world: &mut World, notice: Notice, player: Option<Entity>) {
    let owner = match &notice {
        Notice::Response { owner, .. } | Notice::Closed { owner, .. } => owner,
    };
    let Some(plugin) = manager.plugins.get_mut(&owner.id) else {
        return;
    };
    if plugin.store.data().generation != owner.generation || !owner.alive() {
        return;
    }
    let id = owner.id.clone();
    let result = super::super::lifecycle::invoke(plugin, world, |store, bindings| match notice {
        Notice::Response {
            request,
            callback,
            response,
            ..
        } => {
            let Some(player) = player else {
                return bindings.call_on_form_closed(
                    store,
                    request,
                    callback,
                    wit::FormCloseReason::Disconnected,
                );
            };
            let tx = super::super::make_world_tx(store)?;
            let player = super::super::make_player_resource(store, player)?;
            bindings.call_on_player_form_response(store, tx, player, request, callback, response)
        }
        Notice::Closed {
            request,
            callback,
            reason,
            ..
        } => bindings.call_on_form_closed(store, request, callback, reason),
    });
    if let Err(error) = result {
        tracing::error!(plugin=%id,%error,"Component form callback failed");
        manager.callback_failed(world, &id, &error.to_string());
    }
}

impl PluginManager {
    /// Drain one finite queue snapshot after migration; follow the real exact connection owner.
    pub(crate) fn drain_ui_notices(&mut self, router: &mut crate::server::worlds::DimensionRouter) {
        if self.readiness.check().is_err() {
            return;
        }
        let targets: Vec<_> = self
            .ui_registry
            .0
            .lock()
            .viewers
            .values()
            .map(|viewer| viewer.target.clone())
            .collect();
        for target in targets {
            let present = super::super::players::task_destination(
                router,
                &super::super::tasks::TaskTarget::Player(Box::new(target.clone())),
            )
            .ok()
            .is_some_and(|(dimension, actor)| {
                actor.is_some_and(|actor| {
                    router.world(dimension).is_some_and(|server| {
                        server.ecs.world().get::<PlayerSession>(actor).is_some()
                    })
                })
            });
            if !present {
                disconnect(&mut self.ui_registry.0.lock(), target.key());
            }
        }
        let notices: Vec<_> = self.ui_registry.0.lock().notices.drain(..).collect();
        for notice in notices {
            let destination = if let Notice::Response { target, .. } = &notice {
                super::super::players::task_destination(
                    router,
                    &super::super::tasks::TaskTarget::Player(target.clone()),
                )
                .ok()
                .filter(|(dimension, actor)| {
                    actor.is_some_and(|actor| {
                        router.world(*dimension).is_some_and(|server| {
                            server.ecs.world().get::<PlayerSession>(actor).is_some()
                        })
                    })
                })
            } else {
                None
            };
            let (dimension, player) = destination.unwrap_or((router.primary_dimension(), None));
            let Some(server) = router.world_mut(dimension) else {
                continue;
            };
            notify(self, server.ecs.world_mut(), notice, player);
            if self.readiness.check().is_err() {
                return;
            }
        }
    }
    /// Standalone worlds supply the same scoped form callbacks without a router or foreign borrow.
    pub(in crate::plugin::manager) fn drain_ui_local(&mut self, world: &mut World) {
        if self.readiness.check().is_err() {
            return;
        }
        let notices: Vec<_> = self.ui_registry.0.lock().notices.drain(..).collect();
        for notice in notices {
            let player = if let Notice::Response { target, .. } = &notice {
                match super::super::players::resolve(world, target) {
                    Ok(player) if world.get::<PlayerSession>(player).is_some() => Some(player),
                    Err(super::super::wit_types::AccessError::WrongWorld) => {
                        self.ui_registry.0.lock().notices.push_back(notice);
                        continue;
                    }
                    _ => None,
                }
            } else {
                None
            };
            notify(self, world, notice, player);
            if self.readiness.check().is_err() {
                return;
            }
        }
    }
}
