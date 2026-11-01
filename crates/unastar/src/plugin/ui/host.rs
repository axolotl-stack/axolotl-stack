//! Typed widget imports validate complete values before reserving presentation state.

use super::super::wit_types;
use super::*;
use crate::plugin::manifest::PluginCapability;
use wasmtime::component::Resource;

/// Permit formatting and line breaks, rejecting controls that cannot form ordinary UI text.
fn text(value: &str, maximum: usize, multiline: bool) -> bool {
    value.len() <= maximum
        && !value
            .chars()
            .any(|ch| ch.is_control() && !(multiline && ch == '\n'))
}
impl HostContext {
    /// Both capabilities must belong to this exact callback and its current world.
    fn ui_target(
        &mut self,
        tx: Resource<wit_types::WorldTx>,
        viewer: Resource<wit_types::Player>,
    ) -> Result<PlayerLineage, wit::PresentationError> {
        self.validate_transaction(tx.rep())
            .map_err(wit::PresentationError::Access)?;
        self.require(PluginCapability::Presentation)
            .map_err(wit::PresentationError::Access)?;
        let (target, actor) = self
            .live_player(viewer.rep())
            .map_err(wit::PresentationError::Access)?;
        if self
            .world_mut()
            .get::<crate::entity::components::PlayerSession>(actor)
            .is_none()
        {
            return Err(wit::PresentationError::Access(
                wit_types::AccessError::Unavailable,
            ));
        }
        Ok(target)
    }
}
impl super::super::wit_bindings::unastar::plugin::presentation::Host for HostContext {
    fn set_sidebar(
        &mut self,
        tx: Resource<wit_types::WorldTx>,
        viewer: Resource<wit_types::Player>,
        value: Option<wit::Sidebar>,
    ) -> Result<(), wit::PresentationError> {
        let target = self.ui_target(tx, viewer)?;
        let value = value
            .map(|value| {
                let mut keys = std::collections::BTreeSet::new();
                if !text(&value.title, 128, false)
                    || value.lines.len() > 32
                    || value.lines.iter().any(|line| {
                        line.key.is_empty()
                            || line.key.len() > 64
                            || !line.key.bytes().all(|byte| {
                                byte.is_ascii_alphanumeric()
                                    || matches!(byte, b'_' | b'-' | b'.' | b':')
                            })
                            || !keys.insert(line.key.clone())
                            || !text(&line.text, 256, false)
                    })
                {
                    return Err(wit::PresentationError::InvalidValue);
                }
                Ok(Sidebar {
                    title: value.title,
                    lines: value
                        .lines
                        .into_iter()
                        .map(|line| (line.key, line.text))
                        .collect(),
                })
            })
            .transpose()?;
        self.stage_ui(target, Widget::Sidebar, Value::Sidebar(value))
    }
    fn set_action_bar(
        &mut self,
        tx: Resource<wit_types::WorldTx>,
        viewer: Resource<wit_types::Player>,
        value: Option<String>,
    ) -> Result<(), wit::PresentationError> {
        let target = self.ui_target(tx, viewer)?;
        if value.as_ref().is_some_and(|value| !text(value, 4096, true)) {
            return Err(wit::PresentationError::InvalidValue);
        }
        self.stage_ui(target, Widget::ActionBar, Value::ActionBar(value))
    }
    fn show_form(
        &mut self,
        tx: Resource<wit_types::WorldTx>,
        viewer: Resource<wit_types::Player>,
        value: wit::SimpleForm,
        callback: u64,
    ) -> Result<u64, wit::PresentationError> {
        let target = self.ui_target(tx, viewer)?;
        if !text(&value.title, 128, false)
            || !text(&value.content, 8192, true)
            || value.buttons.len() > 64
            || value.buttons.iter().any(|label| !text(label, 256, true))
        {
            return Err(wit::PresentationError::InvalidValue);
        }
        let count = value.buttons.len() as u32;
        let buttons: Vec<_> = value
            .buttons
            .into_iter()
            .map(|text| serde_json::json!({ "text": text }))
            .collect();
        let payload = serde_json::json!({
            "type": "form",
            "title": value.title,
            "content": value.content,
            "buttons": buttons,
        })
        .to_string();
        let (request, wire) = {
            let mut book = self.ui_registry.0.lock();
            book.next_request = book
                .next_request
                .checked_add(1)
                .ok_or(wit::PresentationError::LimitExceeded)?;
            book.next_wire = book
                .next_wire
                .checked_add(1)
                .ok_or(wit::PresentationError::LimitExceeded)?;
            (book.next_request, book.next_wire)
        };
        self.stage_ui(
            target,
            Widget::Form,
            Value::Form(Some(Form {
                request,
                wire,
                callback,
                payload: payload.into(),
                buttons: count,
            })),
        )?;
        Ok(request)
    }
    fn close_form(
        &mut self,
        tx: Resource<wit_types::WorldTx>,
        viewer: Resource<wit_types::Player>,
        request: u64,
    ) -> Result<bool, wit::PresentationError> {
        let target = self.ui_target(tx, viewer)?;
        let key = (target.key(), Widget::Form);
        let current = if let Some(action) = self.pending_ui.iter().find(|action| action.key == key)
        {
            match &action.value {
                Value::Form(Some(form)) => Some(form.request),
                _ => None,
            }
        } else {
            let book = self.ui_registry.0.lock();
            if book
                .claims
                .get(&key)
                .is_some_and(|claim| !claim.owner.matches(self))
            {
                return Err(wit::PresentationError::Conflict);
            }
            book.desired
                .get(&key)
                .and_then(|desired| match &desired.value {
                    Value::Form(Some(form)) => Some(form.request),
                    _ => None,
                })
        };
        if current != Some(request) {
            return Ok(false);
        }
        self.stage_ui(target, Widget::Form, Value::Form(None))?;
        Ok(true)
    }
}
