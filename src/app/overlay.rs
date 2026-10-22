//! Controller-owned desktop surface lifecycle; the model itself has no OS effects.
use super::*;
use crate::{display::LayoutState, jobs::JobId};

pub(super) struct Surface {
    pub id: window::Id,
    pub model: crate::overlay::Model,
    pub ready: bool,
}
pub(super) struct DeferredAction {
    pub window: window::Id,
    pub key: crate::actions::ActionKey,
    pub token: u64,
    pub action: crate::results::ResultAction,
}
const LAYOUT_FALLBACK: &str = "Desktop highlights need one unchanged, supported display. Results and actions remain in Last scan.";
const REGION_FALLBACK: &str =
    "Region position is unavailable. Results and actions remain in Last scan.";

impl App {
    pub(super) fn spatial_task(
        &mut self,
        id: JobId,
        image: Arc<crate::capture::Prepared>,
    ) -> Task<Message> {
        let revision = self
            .capture_layout
            .as_ref()
            .map_or(0, |layout| layout.revision);
        self.pending_spatial = Some((id, revision));
        self.state.busy = true;
        self.screenshot = Some(image.clone());
        let gate = self.spatial_gate.clone();
        let (task, handle) = Task::perform(
            async move {
                let permit = gate
                    .acquire_owned()
                    .await
                    .map_err(|_| "QR decoder unavailable.".to_owned())?;
                tokio::task::spawn_blocking(move || {
                    // A cancelled caller cannot stop rqrr in mid-grid. Keep the
                    // single worker permit until that bounded local decode ends.
                    let _permit = permit;
                    crate::decode::decode_spatial(&image.image.to_luma8())
                })
                .await
                .map_err(|_| "QR decoder unavailable.".to_owned())
                .and_then(|result| {
                    result
                        .map(Arc::new)
                        .map_err(|_| "Could not decode QR codes.".to_owned())
                })
            },
            move |result| Message::SpatialFinished(id, revision, result),
        )
        .abortable();
        self.spatial_abort = Some(handle.abort_on_drop());
        task
    }
    pub(super) fn spatial_finished(
        &mut self,
        id: JobId,
        revision: u64,
        result: Result<Arc<crate::decode::SpatialScan>, String>,
    ) -> Task<Message> {
        self.spatial_finished_with_copy(id, revision, result, Self::copy_task)
    }
    pub(super) fn spatial_finished_with_copy(
        &mut self,
        id: JobId,
        revision: u64,
        result: Result<Arc<crate::decode::SpatialScan>, String>,
        copy: impl FnOnce(&Self, String, JobId, usize) -> Task<Message>,
    ) -> Task<Message> {
        if id.0 != self.state.session || self.pending_spatial != Some((id, revision)) {
            return Task::none();
        }
        self.pending_spatial = None;
        self.spatial_abort = None;
        self.state.busy = false;
        self.state.scanned = true;
        let image = self.screenshot.take();
        let captured_layout = self.capture_layout.take();
        match result {
            Ok(scan) => {
                self.state.results = scan.results.clone();
                self.state.error = None;
                // A single screen target needs no chooser, even when optional
                // selection/camera auto-copy is disabled. Count locations so
                // duplicate payloads at different positions still get a chooser.
                if self.capture_mode == crate::capture::CaptureMode::Quick
                    && scan.results.len() == 1
                    && scan.detections.len() == 1
                {
                    return copy(self, scan.results[0].display_text.clone(), id, 0);
                }
                if !scan.results.is_empty() {
                    if self.capture_mode == crate::capture::CaptureMode::Region {
                        self.overlay_fallback = Some(REGION_FALLBACK);
                    } else if let (Some(image), Some(layout), LayoutState::Ready(current)) =
                        (image, captured_layout, &self.layout)
                        && layout == *current
                        && layout.revision == revision
                        && layout.eligible_capture([image.image.width(), image.image.height()])
                        && !scan.detections.is_empty()
                        && self.state.host_available
                    {
                        return self.open_overlay(id, revision, image, (*scan).clone());
                    } else {
                        self.overlay_fallback = Some(LAYOUT_FALLBACK);
                    }
                }
            }
            Err(_) => self.state.error = Some("Could not decode QR codes.".into()),
        }
        if !self.state.host_available {
            self.open()
        } else {
            Task::none()
        }
    }
    pub(super) fn open_overlay(
        &mut self,
        session: JobId,
        revision: u64,
        image: Arc<crate::capture::Prepared>,
        scan: crate::decode::SpatialScan,
    ) -> Task<Message> {
        let (id, opened) = window::open(window::Settings {
            fullscreen: true,
            decorations: false,
            exit_on_close_request: true,
            platform_specific: window::settings::PlatformSpecific {
                application_id: "qriff".into(),
                ..Default::default()
            },
            ..Default::default()
        });
        let mut model = crate::overlay::Model::new(session, revision, image, scan);
        if let LayoutState::Ready(layout) = &self.layout {
            let size = layout.outputs[0].logical_size;
            model.resize(iced::Size::new(size[0] as f32, size[1] as f32));
        }
        self.overlay = Some(Surface {
            id,
            model,
            ready: false,
        });
        opened.then(|id| window::size(id).map(move |size| Message::WindowResized(id, size)))
    }
    /// Drop screenshot, revealed editor and hit regions synchronously. A retired
    /// ID remains only until Closed acknowledges the compositor-side teardown.
    pub(super) fn retire_overlay(&mut self) {
        if let Some(surface) = self.overlay.take() {
            self.retired_overlays.insert(surface.id);
            self.overlay_closes.push(surface.id);
            self.revealed.clear();
            self.result_texts.clear();
        }
    }
    pub(super) fn overlay_close_tasks(&mut self) -> Task<Message> {
        Task::batch(
            std::mem::take(&mut self.overlay_closes)
                .into_iter()
                .map(window::close),
        )
    }
    pub(super) fn dismiss_overlay(&mut self) -> Task<Message> {
        self.retire_overlay();
        self.overlay_close_tasks()
    }
    pub(super) fn overlay_closed(&mut self, id: window::Id) -> Option<Task<Message>> {
        let active = self
            .overlay
            .as_ref()
            .is_some_and(|surface| surface.id == id);
        if active {
            self.retire_overlay();
            self.overlay_closes.retain(|&window| window != id);
        }
        if !self.retired_overlays.remove(&id) {
            return None;
        }
        if self
            .deferred_action
            .as_ref()
            .is_some_and(|request| request.window == id)
        {
            let request = self
                .deferred_action
                .take()
                .expect("matching deferred action");
            if request.key.session == self.result_session()
                && self
                    .result_actions
                    .get(&request.key.result_index)
                    .is_some_and(|state| {
                        state.key == request.key
                            && state.token == request.token
                            && matches!(state.status, crate::actions::ActionStatus::Pending)
                    })
            {
                return Some(Self::action_task(
                    request.key,
                    request.token,
                    request.action,
                ));
            }
        }
        Some(Task::none())
    }
    pub(super) fn handle_overlay_event(
        &mut self,
        session: JobId,
        id: window::Id,
        event: crate::overlay::Event,
    ) -> Task<Message> {
        self.handle_overlay_event_with_copy(session, id, event, Self::copy_task)
    }
    pub(super) fn handle_overlay_event_with_copy(
        &mut self,
        session: JobId,
        id: window::Id,
        event: crate::overlay::Event,
        copy: impl FnOnce(&Self, String, JobId, usize) -> Task<Message>,
    ) -> Task<Message> {
        let Some(surface) = self.overlay.as_mut().filter(|surface| {
            surface.id == id && surface.model.session == session && session.0 == self.state.session
        }) else {
            return Task::none();
        };
        if !surface.ready
            && !matches!(
                event,
                crate::overlay::Event::Close | crate::overlay::Event::Escape
            )
        {
            return Task::none();
        }
        let effect = surface
            .model
            .update(event, self.preferences.auto_copy_single);
        let focus = surface
            .model
            .focus_task()
            .map(move |event| Message::OverlayEvent(session, id, event));
        match effect {
            crate::overlay::Effect::None => focus,
            crate::overlay::Effect::Copy(index) => {
                let text = self.state.results[index].display_text.clone();
                Task::batch([focus, copy(self, text, session, index)])
            }
            crate::overlay::Effect::CopySelection => {
                let selected = surface
                    .model
                    .selected_result()
                    .zip(surface.model.selected_text());
                if let Some((index, text)) = selected {
                    Task::batch([focus, copy(self, text, session, index)])
                } else {
                    focus
                }
            }
            crate::overlay::Effect::Activate(index) => {
                self.update_inner(Message::ActivateResult(index))
            }
            crate::overlay::Effect::Dismiss => self.dismiss_overlay(),
        }
    }
    pub(super) fn layout_changed(&mut self, layout: LayoutState) -> Task<Message> {
        if self.layout == layout {
            return Task::none();
        }
        self.layout = layout;
        // Even a later return to the same layout cannot bless a capture made
        // across an unavailable/hotplug interval.
        self.capture_layout = None;
        if self.overlay.is_some() {
            self.overlay_fallback = Some(LAYOUT_FALLBACK);
            self.dismiss_overlay()
        } else {
            Task::none()
        }
    }
    pub(super) fn window_resized(&mut self, id: window::Id, size: iced::Size) -> Task<Message> {
        if let Some(surface) = self.overlay.as_mut().filter(|surface| surface.id == id) {
            let valid = match &self.layout {
                LayoutState::Ready(layout)
                    if layout.revision == surface.model.layout_revision
                        && layout.outputs.len() == 1 =>
                {
                    let expected = layout.outputs[0].logical_size;
                    [size.width, size.height]
                        .into_iter()
                        .zip(expected)
                        .all(|(actual, expected)| {
                            actual.is_finite()
                                && actual > 0.
                                && (actual - expected as f32).abs() <= 1.
                        })
                }
                _ => false,
            };
            if valid {
                surface.model.resize(size);
                surface.ready = true;
                Task::none()
            } else {
                self.overlay_fallback = Some(LAYOUT_FALLBACK);
                self.dismiss_overlay()
            }
        } else if self.retired_overlays.contains(&id) {
            // Native creation can finish after a source switch requested close.
            // The size reply now confirms the window exists; close it again.
            window::close(id)
        } else if self.state.window_ids.contains(&id) {
            self.update_inner(Message::Resized(size.width))
        } else {
            Task::none()
        }
    }
    pub(super) fn action_task(
        key: crate::actions::ActionKey,
        token: u64,
        action: crate::results::ResultAction,
    ) -> Task<Message> {
        Task::perform(crate::actions::execute(action), move |result| {
            Message::ActionFinished(key, token, result)
        })
    }
}
