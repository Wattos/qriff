mod appearance;
pub mod clipboard;
mod focus;
mod icons;
pub mod state;
pub(crate) mod style;
pub(crate) mod view;
use crate::{
    cli::Command,
    instance::{InstanceRole, Primary},
    tray,
};
use iced::{Element, Subscription, Task, window};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
#[derive(Debug, Clone)]
pub enum Message {
    Appearance(bool, iced::Color),
    ToggleMore,
    ToggleSettings,
    RevealPassword(usize),
    Resized(f32),
    FocusNext(bool),
    KeyboardActivate,
    FocusVisible(u64, u64, bool),
    ScrollInput,
    UserCommand(Command),
    InstanceCommand(Command),
    TrayHostChanged(bool),
    SourceUnavailable(String),
    WindowClosed(window::Id),
    Opened(window::Id),
    /// Capture sources must tag frames with the ID returned by begin_session.
    DecodeRequested(crate::jobs::JobId, Arc<image::GrayImage>),
    DecodeFinished(Arc<crate::jobs::DecodeOutcome>),
    CopyResult(usize),
    AutoCopyChanged(bool),
    PreferencesLoaded(crate::settings::LoadedPreferences),
    PreferencesSaved(u64, Result<(), String>),
    Copied(crate::jobs::JobId, usize),
    ClipboardFinished(
        crate::jobs::JobId,
        usize,
        Result<(), clipboard::ClipboardUnavailable>,
    ),
    SelectText(crate::jobs::JobId, usize, iced::widget::text_editor::Action),
    ActivateResult(usize),
    ActionFinished(
        crate::actions::ActionKey,
        u64,
        Result<(), crate::actions::ActionError>,
    ),
    CaptureFinished(
        crate::jobs::JobId,
        Result<Option<Arc<crate::capture::Prepared>>, String>,
    ),
    CameraOutcome(Arc<crate::camera::Outcome>),
    CameraDevices(
        crate::jobs::JobId,
        Result<Vec<crate::camera::Device>, String>,
    ),
    SelectCamera(crate::camera::Device),
    StopCamera,
    ScanFull,
    DesktopCapture,
    SelectRegion,
    CancelCrop,
    CropSelected(crate::jobs::JobId, crate::capture::crop::PixelRect),
}
#[derive(Clone)]
struct PrimarySource(
    Arc<Mutex<Option<Primary>>>,
    tokio::sync::watch::Receiver<crate::desktop::Snapshot>,
);
impl std::hash::Hash for PrimarySource {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        std::ptr::hash(Arc::as_ptr(&self.0), h);
    }
}
struct App {
    pub state: state::State,
    windowless: bool,
    clipboard_failed: bool,
    result_texts: std::collections::HashMap<usize, iced::widget::text_editor::Content>,
    desktop: tokio::sync::watch::Sender<crate::desktop::Snapshot>,
    focus: Option<focus::Target>,
    focus_ticket: u64,
    focus_ready: bool,
    dark: bool,
    accent: iced::Color,
    width: f32,
    more_open: bool,
    settings_open: bool,
    revealed: std::collections::HashSet<usize>,
    preferences: crate::settings::Preferences,
    preferences_loaded: bool,
    preference_path: Option<std::path::PathBuf>,
    preference_revision: u64,
    saving: Option<u64>,
    settings_warning: Option<String>,
    auto_copy: clipboard::AutoCopyPolicy,
    copy_feedback: Option<(crate::jobs::JobId, usize)>,
    result_actions: std::collections::HashMap<usize, crate::actions::ActionState>,
    action_token: u64,
    retained_result_session: Option<crate::jobs::JobId>,
    primary: Arc<Mutex<Option<Primary>>>,
    decoder: Arc<crate::jobs::Decoder>,
    screenshot: Option<Arc<crate::capture::Prepared>>,
    camera_preview: Option<iced::widget::image::Handle>,
    camera: Option<Arc<crate::camera::CameraSession>>,
    retiring_cameras: Vec<Arc<crate::camera::CameraSession>>,
    camera_devices: Vec<crate::camera::Device>,
    selected_camera: Option<crate::camera::Device>,
    camera_mode: bool,
    selecting: bool,
    crop_epoch: u64,
    pending_capture: Option<crate::jobs::JobId>,
    capture_mode: crate::capture::CaptureMode,
    temporary_closes: std::collections::HashSet<window::Id>,
}
pub fn run(command: Command, primary: InstanceRole) -> iced::Result {
    let InstanceRole::Primary(primary) = primary else {
        return Ok(());
    };
    let primary = Arc::new(Mutex::new(Some(primary)));
    iced::daemon(
        move || {
            let mut app = App {
                state: state::State {
                    background: command == Command::Background,
                    ..Default::default()
                },
                windowless: matches!(command, Command::ScanScreen | Command::ScanRegion),
                desktop: tokio::sync::watch::channel(Default::default()).0,
                clipboard_failed: false,
                result_texts: Default::default(),
                focus: None,
                focus_ticket: 0,
                focus_ready: false,
                dark: false,
                accent: style::BLUE,
                width: 1020.0,
                more_open: false,
                settings_open: false,
                revealed: Default::default(),
                preferences: Default::default(),
                preferences_loaded: false,
                preference_path: None,
                preference_revision: 0,
                saving: None,
                settings_warning: None,
                auto_copy: Default::default(),
                copy_feedback: None,
                result_actions: Default::default(),
                action_token: 0,
                retained_result_session: None,
                primary: primary.clone(),
                decoder: Arc::new(crate::jobs::Decoder::new()),
                screenshot: None,
                camera_preview: None,
                camera: None,
                retiring_cameras: Vec::new(),
                camera_devices: Vec::new(),
                selected_camera: None,
                camera_mode: false,
                selecting: false,
                crop_epoch: 0,
                pending_capture: None,
                capture_mode: crate::capture::CaptureMode::Quick,
                temporary_closes: Default::default(),
            };
            let task = if command == Command::Background {
                Task::none()
            } else {
                app.command(command)
            };
            let load = Task::perform(
                async {
                    tokio::task::spawn_blocking(crate::settings::load)
                        .await
                        .unwrap_or_else(|_| crate::settings::LoadedPreferences {
                            preferences: Default::default(),
                            path: None,
                            warning: Some("Settings unavailable.".into()),
                        })
                },
                Message::PreferencesLoaded,
            );
            (app, Task::batch([task, load]))
        },
        App::update,
        App::view,
    )
    .title("QR Scanner")
    .default_font(iced::Font::with_name("Noto Sans"))
    .theme(|app: &App, _| style::theme(app.dark, app.accent))
    .subscription(App::subscription)
    .run()
}
impl App {
    /// All source switches (including camera) must invalidate decode/capture work here.
    fn reset_source(&mut self) -> crate::jobs::JobId {
        self.invalidate_source();
        self.focus_ready = false;
        self.focus = None;
        self.revealed.clear();
        self.copy_feedback = None;
        self.clipboard_failed = false;
        self.result_texts.clear();
        self.result_actions.clear();
        self.retained_result_session = None;
        self.state.begin_session()
    }
    /// Identity of the visible cards, independent of stopped capture generations.
    fn result_session(&self) -> crate::jobs::JobId {
        self.retained_result_session
            .unwrap_or(crate::jobs::JobId(self.state.session))
    }
    /// Stop capture and invalidate queued messages while retaining visible results.
    fn invalidate_source(&mut self) {
        self.retained_result_session = Some(self.result_session());
        self.decoder.cancel();
        if let Some(camera) = self.camera.take() {
            camera.stop();
            self.retiring_cameras.push(camera);
        }
        self.camera_preview = None;
        self.camera_mode = false;
        self.pending_capture = None;
        self.screenshot = None;
        self.selecting = false;
        self.crop_epoch = self.crop_epoch.wrapping_add(1);
        self.state.session = self.state.session.wrapping_add(1);
        self.state.busy = false;
    }
    fn open(&mut self) -> Task<Message> {
        if let Some(&id) = self.state.window_ids.first() {
            return window::set_mode(id, window::Mode::Windowed).chain(window::gain_focus(id));
        }
        let (id, task) = window::open(window::Settings {
            size: iced::Size::new(1020., 680.),
            exit_on_close_request: true,
            ..Default::default()
        });
        self.state.window_ids.push(id);
        self.state.windows = self.state.window_ids.len();
        task.map(Message::Opened)
    }
    fn capture_task(id: crate::jobs::JobId, mode: crate::capture::CaptureMode) -> Task<Message> {
        Task::perform(
            async move {
                match crate::capture::screenshot(mode)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    crate::capture::CaptureOutcome::Cancelled => Ok(None),
                    crate::capture::CaptureOutcome::Captured(image) => {
                        tokio::task::spawn_blocking(move || {
                            Some(Arc::new(crate::capture::Prepared::new(image)))
                        })
                        .await
                        .map_err(|e| e.to_string())
                    }
                }
            },
            move |result| Message::CaptureFinished(id, result),
        )
    }
    fn begin_capture(&mut self, mode: crate::capture::CaptureMode) -> Task<Message> {
        let id = self.reset_source();
        self.capture_mode = mode;
        self.pending_capture = Some(id);
        let windows = std::mem::take(&mut self.state.window_ids);
        self.state.windows = 0;
        self.temporary_closes.extend(windows.iter().copied());
        if self.temporary_closes.is_empty() {
            Self::capture_task(id, self.capture_mode)
        } else {
            Task::batch(windows.into_iter().map(window::close))
        }
    }
    fn command(&mut self, command: Command) -> Task<Message> {
        match command {
            Command::Quit => {
                self.reset_source();
                iced::exit()
            }
            Command::Show if self.pending_capture.is_some() => Task::none(),
            Command::Show => self.open(),
            Command::Background => Task::none(),
            Command::ScanScreen | Command::ScanRegion if self.pending_capture.is_some() => {
                Task::none()
            }
            Command::ScanScreen => self.begin_capture(crate::capture::CaptureMode::Quick),
            Command::ScanRegion => self.begin_capture(crate::capture::CaptureMode::Region),
            Command::CopyResult(session, index) if session == self.result_session().0 => {
                self.update_inner(Message::CopyResult(index))
            }
            Command::ActivateResult(session, index) if session == self.result_session().0 => {
                self.update_inner(Message::ActivateResult(index))
            }
            Command::CopyResult(..) | Command::ActivateResult(..) => Task::none(),
            Command::ScanCamera => {
                self.windowless = false;
                let id = self.reset_source();
                self.camera_mode = true;
                let enumerate = Task::perform(
                    async {
                        tokio::task::spawn_blocking(crate::camera::devices)
                            .await
                            .map_err(|e| e.to_string())
                            .and_then(|r| r.map_err(|e| e.to_string()))
                    },
                    move |r| Message::CameraDevices(id, r),
                );
                Task::batch([self.open(), enumerate])
            }
        }
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        let reveal = matches!(&message, Message::RevealPassword(_));
        let publish = !matches!(&message, Message::CameraOutcome(outcome) if matches!(outcome.event, crate::camera::Event::Frame { .. }))
            && !matches!(
                &message,
                Message::FocusVisible(..)
                    | Message::FocusNext(..)
                    | Message::ScrollInput
                    | Message::SelectText(..)
            );
        let old_identity = (self.result_session(), self.state.results.len());
        let task = self.update_inner(message);
        if reveal || old_identity != (self.result_session(), self.state.results.len()) {
            self.result_texts
                .retain(|index, _| *index < self.state.results.len());
            for (index, result) in self.state.results.iter().enumerate() {
                let text = view::payload(result, self.revealed.contains(&index));
                if self
                    .result_texts
                    .get(&index)
                    .is_none_or(|content| content.text() != text)
                {
                    self.result_texts
                        .insert(index, iced::widget::text_editor::Content::with_text(&text));
                }
            }
        }
        if publish {
            self.publish_desktop();
        }
        task
    }
    fn update_inner(&mut self, message: Message) -> Task<Message> {
        // Geometry may change after feedback, a reveal or newly decoded cards.
        // A previously confirmed viewport must not authorize an invisible action.
        // Preview widgets have fixed dimensions, so replacement camera frames
        // must preserve readiness. Only inserting/removing a preview changes layout.
        let camera_geometry_change = match &message {
            Message::CameraOutcome(outcome) => match &outcome.event {
                crate::camera::Event::Frame { preview, .. } => {
                    self.camera_preview.is_none() && preview.is_some()
                }
                crate::camera::Event::Error(_) | crate::camera::Event::Stopped => true,
            },
            _ => false,
        };
        if camera_geometry_change
            || matches!(
                &message,
                Message::Resized(_)
                    | Message::Copied(..)
                    | Message::ClipboardFinished(..)
                    | Message::ActionFinished(..)
                    | Message::RevealPassword(_)
                    | Message::DecodeFinished(_)
                    | Message::ToggleMore
                    | Message::ToggleSettings
                    | Message::PreferencesLoaded(_)
                    | Message::PreferencesSaved(..)
                    | Message::AutoCopyChanged(_)
                    | Message::SourceUnavailable(_)
                    | Message::CameraDevices(..)
                    | Message::StopCamera
                    | Message::CaptureFinished(..)
                    | Message::DecodeRequested(..)
                    | Message::SelectRegion
            )
        {
            self.focus_ready = false;
            self.focus_ticket = self.focus_ticket.wrapping_add(1);
        }
        match message {
            Message::Appearance(dark, accent) => {
                self.dark = dark;
                self.accent = accent;
                Task::none()
            }
            Message::ToggleMore => {
                self.more_open = !self.more_open;
                self.settings_open = false;
                Task::none()
            }
            Message::ToggleSettings => {
                self.settings_open = !self.settings_open;
                self.more_open = false;
                Task::none()
            }
            Message::RevealPassword(index) => {
                if !self.revealed.remove(&index) {
                    self.revealed.insert(index);
                }
                Task::none()
            }
            Message::FocusVisible(session, ticket, visible) => {
                if session == self.state.session && ticket == self.focus_ticket {
                    self.focus_ready = visible;
                }
                Task::none()
            }
            Message::ScrollInput => {
                self.focus_ready = false;
                self.focus_ticket = self.focus_ticket.wrapping_add(1);
                Task::none()
            }
            Message::KeyboardActivate => {
                if let Some(target) = self.focus.filter(|t| focus::targets(self).contains(t)) {
                    if self.focus_ready {
                        self.update(target.message(self.preferences.auto_copy_single))
                    } else {
                        self.reveal_focus(target)
                    }
                } else {
                    Task::none()
                }
            }
            Message::FocusNext(reverse) => {
                let target = focus::next(self, reverse);
                self.focus = Some(target);
                self.reveal_focus(target)
            }
            Message::Resized(width) => {
                self.focus_ready = false;
                self.width = width;
                Task::none()
            }
            Message::UserCommand(c) => {
                if matches!(c, Command::ScanScreen | Command::ScanRegion) {
                    self.windowless = false;
                }
                self.command(c)
            }
            Message::InstanceCommand(c) => {
                if matches!(c, Command::ScanScreen | Command::ScanRegion)
                    && self.pending_capture.is_none()
                {
                    self.windowless = true;
                }
                self.command(c)
            }
            Message::TrayHostChanged(available) => {
                if self.state.host_changed(available) && self.pending_capture.is_none() {
                    self.open()
                } else {
                    Task::none()
                }
            }
            Message::WindowClosed(id) => {
                // Iced broadcasts Closed after removing the runtime window. Winit's
                // Wayland loop processes its destruction before the AboutToWait
                // phase in which Iced broadcasts these close events.
                if self.temporary_closes.remove(&id) {
                    return if self.temporary_closes.is_empty() {
                        self.pending_capture
                            .map(|id| Self::capture_task(id, self.capture_mode))
                            .unwrap_or_else(Task::none)
                    } else {
                        Task::none()
                    };
                }
                if !self.state.window_ids.contains(&id) {
                    return Task::none();
                }
                self.reset_source();
                self.state.busy = false;
                self.state.window_ids.retain(|&v| v != id);
                if self.state.close_window() {
                    iced::exit()
                } else {
                    Task::none()
                }
            }
            Message::CaptureFinished(id, result) => {
                if id.0 != self.state.session {
                    return Task::none();
                }
                self.pending_capture = None;
                self.state.busy = false;
                match result {
                    Ok(Some(image)) => {
                        self.screenshot = Some(image);
                        let decode = self.scan_screenshot(None);
                        if self.windowless && self.state.host_available {
                            self.screenshot = None;
                            return decode;
                        }
                        return Task::batch([self.open(), decode]);
                    }
                    Ok(None) => {
                        self.screenshot = None;
                        self.state.capture_cancelled = true;
                    }
                    Err(e) => self.state.error = Some(e),
                }
                if self.windowless && self.state.host_available {
                    Task::none()
                } else {
                    self.open()
                }
            }
            Message::DesktopCapture => {
                self.more_open = false;
                self.begin_capture(crate::capture::CaptureMode::Interactive)
            }
            Message::ScanFull => self.scan_screenshot(None),
            Message::SelectRegion => {
                self.crop_epoch = self.crop_epoch.wrapping_add(1);
                self.selecting = true;
                Task::none()
            }
            Message::CancelCrop => {
                self.focus_ready = false;
                self.focus = None;
                self.more_open = false;
                self.settings_open = false;
                self.crop_epoch = self.crop_epoch.wrapping_add(1);
                self.selecting = false;
                Task::none()
            }
            Message::CropSelected(id, rect) => {
                if id.0 != self.state.session {
                    return Task::none();
                }
                self.selecting = false;
                self.scan_screenshot(Some(rect))
            }
            Message::SourceUnavailable(e) => {
                self.state.error = Some(e);
                Task::none()
            }
            Message::DecodeRequested(id, image) => {
                if id.0 == self.state.session {
                    self.state.busy = true;
                    self.decoder.submit(id, image);
                }
                Task::none()
            }
            Message::PreferencesLoaded(loaded) => {
                self.preferences_loaded = true;
                self.preference_path = loaded.path;
                self.settings_warning = loaded.warning;
                if self.preference_revision == 0 {
                    self.preferences = loaded.preferences;
                    Task::none()
                } else {
                    self.save_preferences()
                }
            }
            Message::AutoCopyChanged(enabled) => {
                self.preferences.auto_copy_single = enabled;
                self.preference_revision = self.preference_revision.wrapping_add(1);
                self.save_preferences()
            }
            Message::PreferencesSaved(revision, result) => {
                if self.saving != Some(revision) {
                    return Task::none();
                }
                self.saving = None;
                if self.preference_revision != revision {
                    return self.save_preferences();
                }
                self.settings_warning = result.err().map(|_| {
                    "Settings could not be saved. Your choice applies until the app exits.".into()
                });
                Task::none()
            }
            Message::SelectText(session, index, action) => {
                if session == self.result_session()
                    && !action.is_edit()
                    && (self.revealed.contains(&index)
                        || !matches!(
                            self.state.results.get(index).map(|r| &r.content),
                            Some(crate::results::QrContent::Invalid {
                                kind: crate::results::ContentKind::Wifi,
                                ..
                            })
                        ))
                    && let Some(content) = self.result_texts.get_mut(&index)
                {
                    content.perform(action);
                }
                Task::none()
            }
            Message::ClipboardFinished(session, index, result) => {
                if session == self.result_session() && index < self.state.results.len() {
                    self.clipboard_failed = result.is_err();
                    if result.is_ok() {
                        self.copy_feedback = Some((session, index));
                    }
                }
                Task::none()
            }
            Message::Copied(session, index) => {
                if session == self.result_session() && index < self.state.results.len() {
                    self.copy_feedback = Some((session, index));
                    self.clipboard_failed = false;
                }
                Task::none()
            }
            Message::DecodeFinished(outcome) => {
                if outcome.id.0 != self.state.session {
                    return Task::none();
                }
                let previous = self.state.results.len();
                self.state.accept_decode(&outcome);
                if let Some(text) = self.auto_copy.on_results(
                    self.preferences.auto_copy_single,
                    outcome.id,
                    &self.state.results,
                    self.state.results.len() - previous,
                ) {
                    self.copy_task(text, outcome.id, 0)
                } else {
                    Task::none()
                }
            }
            Message::CopyResult(index) => self
                .state
                .results
                .get(index)
                .map(|result| {
                    self.copy_task(result.display_text.clone(), self.result_session(), index)
                })
                .unwrap_or_else(Task::none),
            Message::ActivateResult(index) => {
                let key = crate::actions::ActionKey {
                    session: self.result_session(),
                    result_index: index,
                };
                if self.result_actions.get(&index).is_some_and(|state| {
                    state.key == key
                        && matches!(state.status, crate::actions::ActionStatus::Pending)
                }) {
                    return Task::none();
                }
                let Some(action) = self
                    .state
                    .results
                    .get(index)
                    .and_then(|result| result.action())
                else {
                    return Task::none();
                };
                self.action_token = self
                    .action_token
                    .checked_add(1)
                    .expect("action request token exhausted");
                let token = self.action_token;
                self.result_actions.insert(
                    index,
                    crate::actions::ActionState {
                        key,
                        token,
                        status: crate::actions::ActionStatus::Pending,
                    },
                );
                Task::perform(crate::actions::execute(action), move |result| {
                    Message::ActionFinished(key, token, result)
                })
            }
            Message::ActionFinished(key, token, outcome) => {
                if key.session == self.result_session()
                    && let Some(state) =
                        self.result_actions
                            .get_mut(&key.result_index)
                            .filter(|state| {
                                state.key == key
                                    && state.token == token
                                    && matches!(state.status, crate::actions::ActionStatus::Pending)
                            })
                {
                    state.status = match outcome {
                        Ok(()) => crate::actions::ActionStatus::Succeeded,
                        Err(error) => crate::actions::ActionStatus::Failed(error),
                    };
                }
                Task::none()
            }
            Message::CameraDevices(id, result) => {
                if id.0 != self.state.session {
                    return Task::none();
                }
                self.state.busy = false;
                match result {
                    Ok(devices) if !devices.is_empty() => {
                        self.camera_devices = devices;
                        self.start_camera(self.camera_devices[0].clone())
                    }
                    Ok(_) => {
                        self.state.error = Some("No supported camera found.".into());
                        Task::none()
                    }
                    Err(error) => {
                        self.state.error = Some(format!("Camera unavailable: {error}"));
                        Task::none()
                    }
                }
            }
            Message::SelectCamera(device) => self.start_camera(device),
            Message::StopCamera => {
                self.invalidate_source();
                self.state.busy = false;
                Task::none()
            }
            Message::CameraOutcome(outcome) => {
                if outcome.id.0 != self.state.session {
                    return Task::none();
                }
                match &outcome.event {
                    crate::camera::Event::Frame { preview, gray } => {
                        if let Some(preview) = preview {
                            self.camera_preview = Some(preview.clone());
                        }
                        if let Some(image) = gray {
                            self.decoder.submit(outcome.id, image.clone());
                        }
                    }
                    crate::camera::Event::Error(error) => {
                        // A completed decode may already be queued in the UI. End the
                        // source generation without discarding results found so far.
                        self.invalidate_source();
                        self.state.error = Some(format!("Camera failed: {error}"));
                        self.camera = None;
                    }
                    crate::camera::Event::Stopped => {
                        self.camera_preview = None;
                        self.state.busy = false;
                        self.camera = None;
                    }
                }
                Task::none()
            }
            Message::Opened(_) => Task::none(),
        }
    }
    /// At most one write is in flight. Intermediate changes are coalesced into the next write.
    fn save_preferences(&mut self) -> Task<Message> {
        if !self.preferences_loaded || self.saving.is_some() {
            return Task::none();
        }
        let Some(path) = self.preference_path.clone() else {
            self.settings_warning =
                Some("Settings unavailable. Your choice applies until the app exits.".into());
            return Task::none();
        };
        let preferences = self.preferences;
        let revision = self.preference_revision;
        self.saving = Some(revision);
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || crate::settings::save(&path, &preferences))
                    .await
                    .map_err(|_| "Settings worker unavailable".to_owned())
                    .and_then(|result| result.map_err(|_| "Could not save settings".to_owned()))
            },
            move |result| Message::PreferencesSaved(revision, result),
        )
    }
    fn start_camera(&mut self, device: crate::camera::Device) -> Task<Message> {
        let id = self.reset_source();
        self.camera_mode = true;
        self.selected_camera = Some(device.clone());
        match crate::camera::start_after(&device.id, id, std::mem::take(&mut self.retiring_cameras))
        {
            Ok(camera) => self.camera = Some(Arc::new(camera)),
            Err(error) => {
                self.state.busy = false;
                self.state.error = Some(format!("Camera failed: {error}"));
            }
        }
        Task::none()
    }
    fn scan_screenshot(&mut self, rect: Option<crate::capture::crop::PixelRect>) -> Task<Message> {
        let Some(image) = self.screenshot.clone() else {
            return Task::none();
        };
        self.decoder.cancel();
        self.focus_ready = false;
        self.focus = None;
        self.revealed.clear();
        let id = self.state.begin_session();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    Arc::new(match rect {
                        Some(r) => image
                            .image
                            .crop_imm(r.x, r.y, r.width, r.height)
                            .into_luma8(),
                        None => image.image.to_luma8(),
                    })
                })
                .await
            },
            move |result| match result {
                Ok(image) => Message::DecodeRequested(id, image),
                Err(e) => Message::CaptureFinished(id, Err(e.to_string())),
            },
        )
    }
    fn status(&self) -> &str {
        if self.camera.is_some() && self.state.error.is_none() {
            "Camera scanning…"
        } else if self.state.busy {
            "Scanning…"
        } else if let Some(error) = self.state.error.as_deref() {
            error
        } else if self.state.capture_cancelled {
            "Capture cancelled."
        } else if self.state.scanned && self.state.results.is_empty() {
            "No QR codes found."
        } else if self.state.scanned {
            "Scan complete"
        } else {
            "Ready"
        }
    }
    fn reveal_focus(&mut self, target: focus::Target) -> Task<Message> {
        self.focus_ready = false;
        self.focus_ticket = self.focus_ticket.wrapping_add(1);
        let ticket = self.focus_ticket;
        let session = self.state.session;
        focus::reveal(target, view::wide(self.width))
            .map(move |visible| Message::FocusVisible(session, ticket, visible))
    }
    fn copy_task(&self, text: String, session: crate::jobs::JobId, index: usize) -> Task<Message> {
        if self.windowless && self.state.window_ids.is_empty() {
            Task::perform(clipboard::write_windowless(text), move |result| {
                Message::ClipboardFinished(session, index, result)
            })
        } else {
            iced::clipboard::write(text).chain(Task::done(Message::Copied(session, index)))
        }
    }
    fn publish_desktop(&self) {
        use crate::desktop::{Notice, ResultEntry, Snapshot};
        let session = self.result_session().0;
        let results: Vec<_> = self
            .state
            .results
            .iter()
            .enumerate()
            .map(|(index, result)| {
                let pending = self.result_actions.get(&index).is_some_and(|state| {
                    state.key.session.0 == session
                        && matches!(state.status, crate::actions::ActionStatus::Pending)
                });
                let mut entry = ResultEntry::new(result, pending);
                entry.feedback = self
                    .result_actions
                    .get(&index)
                    .filter(|state| state.key.session.0 == session)
                    .and_then(|state| match &state.status {
                        crate::actions::ActionStatus::Pending => None,
                        crate::actions::ActionStatus::Succeeded => {
                            Some("QR action completed".into())
                        }
                        crate::actions::ActionStatus::Failed(error) => Some(error.to_string()),
                    });
                entry
            })
            .collect();
        let notification = if !self.windowless || self.pending_capture.is_some() || self.state.busy
        {
            None
        } else if self.state.error.is_some() {
            Some(Notice {
                title: "QR scan failed".into(),
                body: "Could not scan. Try again or open QR Scanner for details.".into(),
            })
        } else if self.state.capture_cancelled {
            None
        } else if self.state.scanned {
            let mut notice = match results.as_slice() {
                [] => Notice {
                    title: "No QR codes found".into(),
                    body: "Try Scan region to select the QR code more precisely.".into(),
                },
                [result] => Notice {
                    title: "QR code found".into(),
                    body: result.label.clone(),
                },
                many => Notice {
                    title: format!("{} QR codes found", many.len()),
                    body: "Results and actions are available under Last scan in the tray menu."
                        .into(),
                },
            };
            if results.len() == 1 {
                if let Some(action) = self
                    .result_actions
                    .get(&0)
                    .filter(|action| action.key.session.0 == session)
                {
                    match &action.status {
                        crate::actions::ActionStatus::Pending => {
                            notice.title = "QR action in progress…".into()
                        }
                        crate::actions::ActionStatus::Succeeded => {
                            notice.title = "QR action completed".into()
                        }
                        crate::actions::ActionStatus::Failed(error) => {
                            notice.title = "QR action failed".into();
                            notice.body = error.to_string();
                        }
                    }
                } else if self.copy_feedback == Some((self.result_session(), 0)) {
                    notice.title = "QR code copied".into();
                }
            }
            if self.clipboard_failed {
                notice.title = "Could not copy QR code".into();
                notice.body = "Windowless copying requires wl-clipboard (wl-copy) and a supported Wayland clipboard. Results remain in Last scan.".into();
            }
            Some(notice)
        } else {
            None
        };
        let snapshot = Snapshot {
            session,
            results,
            notification,
        };
        self.desktop.send_if_modified(|current| {
            if *current == snapshot {
                false
            } else {
                *current = snapshot;
                true
            }
        });
    }
    fn view(&self, id: window::Id) -> Element<'_, Message> {
        view::view(self, id)
    }
    fn subscription(&self) -> Subscription<Message> {
        let primary = self.primary.clone();
        let camera_subscription = if let Some(camera) = &self.camera {
            Subscription::run_with(CameraSource(camera.clone()), camera_events)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            Subscription::run(appearance::events),
            camera_subscription,
            window::resize_events().map(|(_, size)| Message::Resized(size.width)),
            window::close_events().map(Message::WindowClosed),
            iced::event::listen_with(|event, status, _| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key, modifiers, ..
                }) => focus::key_message(&key, modifiers.shift(), status),
                _ => None,
            }),
            Subscription::run_with(DecoderSource(self.decoder.clone()), decode_events),
            Subscription::run_with(
                PrimarySource(primary, self.desktop.subscribe()),
                external_events,
            ),
        ])
    }
}
fn external_events(source: &PrimarySource) -> impl iced::futures::Stream<Item = Message> + use<> {
    let primary = source.0.clone();
    let snapshots = source.1.clone();
    iced::stream::channel::<Message>(
        32,
        async move |mut output: iced::futures::channel::mpsc::Sender<Message>| {
            use iced::futures::SinkExt;
            let primary = primary.lock().expect("primary mutex").take();
            let Some(primary) = primary else {
                std::future::pending::<()>().await;
                return;
            };
            let (commands, mut incoming) = mpsc::channel(32);
            let tray = tray::start(commands.clone(), snapshots.clone());
            let notification_commands = commands.clone();
            let notifications = tokio::spawn(async move {
                if crate::desktop::notifications(snapshots, notification_commands)
                    .await
                    .is_err()
                {
                    eprintln!("Desktop notifications unavailable; results remain in the tray.");
                }
            });
            let mut host = match &tray {
                Ok(t) => t.hosts.clone(),
                Err(_) => {
                    let (_tx, rx) = tokio::sync::watch::channel(Some(false));
                    rx
                }
            };
            // Accept on Tokio readiness; no blocking worker or periodic wakeups while idle.
            primary
                .listener
                .set_nonblocking(true)
                .expect("nonblocking socket");
            let listener = tokio::net::UnixListener::from_std(
                primary.listener.try_clone().expect("clone listener"),
            )
            .expect("async listener");
            let socket_commands = commands.clone();
            let accept = tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    let sender = socket_commands.clone();
                    tokio::spawn(async move {
                        use tokio::io::AsyncReadExt;
                        let mut bytes = Vec::new();
                        let read = tokio::time::timeout(
                            std::time::Duration::from_secs(1),
                            stream.take(256).read_to_end(&mut bytes),
                        )
                        .await;
                        if matches!(read, Ok(Ok(_)))
                            && let Ok(command) = serde_json::from_slice(&bytes)
                        {
                            let _ = sender.send(command).await;
                        }
                    });
                }
            });
            let mut monitor_alive = true;
            loop {
                let available = *host.borrow_and_update();
                if let Some(value) = available
                    && output.send(Message::TrayHostChanged(value)).await.is_err()
                {
                    break;
                }
                tokio::select! {
                 c=incoming.recv()=>{if let Some(c)=c {if output.send(Message::InstanceCommand(c)).await.is_err(){break;}}else{break;}},
                 changed=host.changed(), if monitor_alive=>{if changed.is_err(){monitor_alive=false;let _=output.send(Message::TrayHostChanged(false)).await;}}
                }
            }
            notifications.abort();
            accept.abort();
            drop(primary);
            drop(tray);
        },
    )
}

#[derive(Clone)]
struct CameraSource(Arc<crate::camera::CameraSession>);
impl std::hash::Hash for CameraSource {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        std::ptr::hash(Arc::as_ptr(&self.0), h);
    }
}
fn camera_events(source: &CameraSource) -> impl iced::futures::Stream<Item = Message> + use<> {
    let mut incoming = source.0.subscribe();
    iced::stream::channel(1, async move |mut output| {
        use iced::futures::SinkExt;
        loop {
            let outcome = incoming.borrow_and_update().clone();
            if let Some(outcome) = outcome
                && output.send(Message::CameraOutcome(outcome)).await.is_err()
            {
                break;
            }
            if incoming.changed().await.is_err() {
                break;
            }
        }
    })
}

#[derive(Clone)]
struct DecoderSource(Arc<crate::jobs::Decoder>);
impl std::hash::Hash for DecoderSource {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        std::ptr::hash(Arc::as_ptr(&self.0), h);
    }
}
fn decode_events(source: &DecoderSource) -> impl iced::futures::Stream<Item = Message> + use<> {
    let mut outcomes = source.0.subscribe();
    iced::stream::channel(1, async move |mut output| {
        use iced::futures::SinkExt;
        while let Some(outcome) = outcomes.recv().await {
            if output.send(Message::DecodeFinished(outcome)).await.is_err() {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests;
