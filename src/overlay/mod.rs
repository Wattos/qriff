pub mod geometry;
mod view;

use crate::{
    actions::{ActionState, ActionStatus},
    capture::Prepared,
    decode::SpatialScan,
    jobs::JobId,
    results::{ContentKind, QrContent},
};
use iced::{Size, widget::text_editor};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Debug, Clone)]
pub enum Event {
    Hover(Option<usize>),
    Select(usize),
    Background,
    Escape,
    Close,
    /// true cycles backwards (Shift+Tab).
    Next(bool),
    ActivateFocused,
    Copy,
    CopySelection,
    Activate,
    Reveal,
    Text(text_editor::Action),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    Copy(usize),
    CopySelection,
    Activate(usize),
    Dismiss,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Detection(usize),
    Text,
    Reveal,
    Copy,
    Activate,
    Close,
}
#[derive(Debug, Clone, Copy)]
pub enum CopyFeedback {
    Copied,
    Failed(crate::app::clipboard::ClipboardUnavailable),
}

/// Frozen screen selection state. All external work is returned as an effect.
pub struct Model {
    pub session: JobId,
    pub layout_revision: u64,
    frozen: Arc<Prepared>,
    scan: SpatialScan,
    pub hovered: Option<usize>,
    pub selected: Option<usize>,
    viewport: Size,
    focus: Option<Focus>,
    text: Option<text_editor::Content>,
    revealed: bool,
    auto_copied: HashSet<usize>,
    actions: HashMap<usize, ActionState>,
    copies: HashMap<usize, CopyFeedback>,
    dismissed: bool,
}
// Restrict both focus and unfocus to this overlay identity. A queued old operation
// must never clear focus in a newer overlay or the ordinary scanner window.
struct EditorFocus {
    id: iced::advanced::widget::Id,
    focused: bool,
}
impl iced::advanced::widget::Operation<Event> for EditorFocus {
    fn traverse(
        &mut self,
        operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<Event>),
    ) {
        operate(self);
    }
    fn focusable(
        &mut self,
        id: Option<&iced::advanced::widget::Id>,
        _: iced::Rectangle,
        state: &mut dyn iced::advanced::widget::operation::Focusable,
    ) {
        if id == Some(&self.id) {
            if self.focused {
                state.focus();
            } else {
                state.unfocus();
            }
        }
    }
}
impl Model {
    pub fn new(
        session: JobId,
        layout_revision: u64,
        frozen: Arc<Prepared>,
        scan: SpatialScan,
    ) -> Self {
        let viewport = Size::new(frozen.image.width() as f32, frozen.image.height() as f32);
        Self {
            session,
            layout_revision,
            frozen,
            scan,
            hovered: None,
            selected: None,
            viewport,
            focus: None,
            text: None,
            revealed: false,
            auto_copied: HashSet::new(),
            actions: HashMap::new(),
            copies: HashMap::new(),
            dismissed: false,
        }
    }
    pub fn resize(&mut self, viewport: Size) {
        self.viewport = viewport;
    }
    /// Only the currently visible editor can provide a partial copy. Hidden or
    /// replaced Wi-Fi editors have no retained selection buffer.
    pub fn selected_text(&self) -> Option<String> {
        self.text
            .as_ref()?
            .selection()
            .filter(|text| !text.is_empty())
    }
    pub fn selected_result(&self) -> Option<usize> {
        self.selected
            .and_then(|i| self.scan.detections.get(i))
            .map(|d| d.result_index)
    }
    pub fn focus(&self) -> Option<Focus> {
        self.focus
    }
    fn editor_id(&self) -> String {
        format!("overlay-text-{}-{}", self.session.0, self.layout_revision)
    }
    /// Controller runs this after a current overlay event, preserving session/window guards.
    /// Native editor focus follows the visible keyboard ring; other targets release it.
    pub fn focus_task(&self) -> iced::Task<Event> {
        iced::advanced::widget::operate(EditorFocus {
            id: self.editor_id().into(),
            focused: self.focus == Some(Focus::Text) && !self.dismissed,
        })
    }
    pub fn transform(&self) -> Option<geometry::Transform> {
        geometry::Transform::new(
            [self.frozen.image.width(), self.frozen.image.height()],
            self.viewport,
        )
    }
    fn panel_bounds(&self, viewport: Size) -> Option<iced::Rectangle> {
        let detection = self.scan.detections.get(self.selected?)?;
        let transform = geometry::Transform::new(
            [self.frozen.image.width(), self.frozen.image.height()],
            viewport,
        )?;
        let highlight = geometry::detection_bounds(detection, &transform).expand(8.);
        let bounds = geometry::panel_bounds(highlight, Size::new(380., 330.), viewport);
        // Keep every detection in the cycle. Temporarily uncover a focused
        // outline instead of allowing the opaque panel to hide its focus ring.
        if let Some(Focus::Detection(index)) = self.focus
            && let Some(focused) = self.scan.detections.get(index)
            && geometry::detection_bounds(focused, &transform)
                .expand(4.)
                .intersects(&bounds)
        {
            return None;
        }
        Some(bounds)
    }
    pub fn set_action_state(&mut self, state: ActionState) {
        if state.key.session == self.session && state.key.result_index < self.scan.results.len() {
            self.actions.insert(state.key.result_index, state);
            self.reconcile_focus();
        }
    }
    pub fn set_copy_feedback(&mut self, index: usize, feedback: CopyFeedback) {
        if index < self.scan.results.len() {
            self.copies.insert(index, feedback);
        }
    }
    fn pending(&self, index: usize) -> bool {
        self.actions
            .get(&index)
            .is_some_and(|s| matches!(s.status, ActionStatus::Pending))
    }
    fn can_reveal(&self) -> bool {
        self.selected_result().is_some_and(|i| {
            matches!(&self.scan.results[i].content,
            QrContent::Wifi(n) if n.password.is_some())
                || matches!(
                    self.scan.results[i].content,
                    QrContent::Invalid {
                        kind: ContentKind::Wifi,
                        ..
                    }
                )
        })
    }
    fn can_activate(&self) -> bool {
        self.selected_result()
            .is_some_and(|i| self.scan.results[i].action().is_some() && !self.pending(i))
    }
    fn focus_targets(&self) -> Vec<Focus> {
        let mut targets: Vec<_> = self
            .scan
            .detections
            .iter()
            .enumerate()
            .filter(|(_, d)| d.result_index < self.scan.results.len())
            .map(|(i, _)| Focus::Detection(i))
            .collect();
        if self.selected_result().is_some() {
            targets.push(Focus::Text);
            if self.can_reveal() {
                targets.push(Focus::Reveal);
            }
            targets.push(Focus::Copy);
            if self.can_activate() {
                targets.push(Focus::Activate);
            }
        }
        targets.push(Focus::Close);
        targets
    }
    fn reconcile_focus(&mut self) {
        if self
            .focus
            .is_some_and(|f| !self.focus_targets().contains(&f))
        {
            self.focus = None;
        }
    }
    fn clear_selection(&mut self) {
        self.selected = None;
        self.text = None;
        self.revealed = false;
        self.reconcile_focus();
    }
    fn refresh_text(&mut self) {
        self.text = self.selected_result().map(|i| {
            text_editor::Content::with_text(&crate::app::view::payload(
                &self.scan.results[i],
                self.revealed,
            ))
        });
    }
    pub fn update(&mut self, event: Event, auto_copy: bool) -> Effect {
        if self.dismissed {
            return Effect::None;
        }
        match event {
            Event::Hover(index) => self.hovered = index.filter(|&i| i < self.scan.detections.len()),
            Event::Select(index) => {
                let Some(d) = self
                    .scan
                    .detections
                    .get(index)
                    .filter(|d| d.result_index < self.scan.results.len())
                else {
                    return Effect::None;
                };
                if self.selected == Some(index) {
                    self.focus = Some(Focus::Text);
                    return Effect::None;
                }
                let result = d.result_index;
                self.clear_selection();
                self.selected = Some(index);
                // Selection opens usable actions even if a large/edge code has
                // no panel placement that leaves the selected outline uncovered.
                self.focus = Some(Focus::Text);
                self.refresh_text();
                if auto_copy && self.auto_copied.insert(result) {
                    return Effect::Copy(result);
                }
            }
            Event::Background => self.clear_selection(),
            Event::Escape if self.selected.is_some() => self.clear_selection(),
            Event::Escape | Event::Close => {
                self.clear_selection();
                self.hovered = None;
                self.focus = None;
                self.dismissed = true;
                return Effect::Dismiss;
            }
            Event::Next(backward) => {
                let targets = self.focus_targets();
                let next = match self
                    .focus
                    .and_then(|f| targets.iter().position(|&t| t == f))
                {
                    Some(i) if backward => (i + targets.len() - 1) % targets.len(),
                    Some(i) => (i + 1) % targets.len(),
                    None if backward => targets.len() - 1,
                    None => 0,
                };
                self.focus = Some(targets[next]);
            }
            Event::ActivateFocused => {
                let e = match self.focus {
                    Some(Focus::Detection(i)) => Event::Select(i),
                    Some(Focus::Reveal) => Event::Reveal,
                    Some(Focus::Copy) => Event::Copy,
                    Some(Focus::Activate) if self.can_activate() => Event::Activate,
                    Some(Focus::Close) => Event::Close,
                    _ => return Effect::None,
                };
                return self.update(e, auto_copy);
            }
            Event::Copy => return self.selected_result().map_or(Effect::None, Effect::Copy),
            Event::CopySelection if self.selected_text().is_some() => return Effect::CopySelection,
            Event::Activate if self.can_activate() => {
                return Effect::Activate(self.selected_result().unwrap());
            }
            Event::Reveal if self.can_reveal() => {
                self.revealed = !self.revealed;
                self.refresh_text();
            }
            Event::Text(action) if !action.is_edit() => {
                if let Some(text) = &mut self.text {
                    if !matches!(action, text_editor::Action::Scroll { .. }) {
                        self.focus = Some(Focus::Text);
                    }
                    text.perform(action);
                }
            }
            _ => {}
        }
        Effect::None
    }
    pub fn view(&self) -> iced::Element<'_, Event> {
        view::view(self)
    }
}
#[cfg(test)]
mod tests;
