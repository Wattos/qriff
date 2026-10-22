pub mod clipboard;
pub mod state;
use crate::{
    cli::Command,
    instance::{InstanceRole, Primary},
    tray,
};
use iced::{
    Element, Subscription, Task,
    widget::{button, column, container, row, scrollable, text},
    window,
};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
#[derive(Debug, Clone)]
pub enum Message {
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
    OpenResult(usize),
    OpenFailed(String),
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
struct PrimarySource(Arc<Mutex<Option<Primary>>>);
impl std::hash::Hash for PrimarySource {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        std::ptr::hash(Arc::as_ptr(&self.0), h);
    }
}
struct App {
    pub state: state::State,
    preferences: crate::settings::Preferences,
    preferences_loaded: bool,
    preference_path: Option<std::path::PathBuf>,
    preference_revision: u64,
    saving: Option<u64>,
    settings_warning: Option<String>,
    auto_copy: clipboard::AutoCopyPolicy,
    copy_feedback: Option<(crate::jobs::JobId, usize)>,
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
                preferences: Default::default(),
                preferences_loaded: false,
                preference_path: None,
                preference_revision: 0,
                saving: None,
                settings_warning: None,
                auto_copy: Default::default(),
                copy_feedback: None,
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
    .subscription(App::subscription)
    .run()
}
impl App {
    /// All source switches (including camera) must invalidate decode/capture work here.
    fn reset_source(&mut self) -> crate::jobs::JobId {
        self.invalidate_source();
        self.copy_feedback = None;
        self.state.begin_session()
    }
    /// Stop capture and invalidate queued messages while retaining visible results.
    fn invalidate_source(&mut self) {
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
            size: iced::Size::new(760., 680.),
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
            Command::ScanScreen => self.begin_capture(crate::capture::CaptureMode::Quick),
            Command::ScanCamera => {
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
        match message {
            Message::UserCommand(c) | Message::InstanceCommand(c) => self.command(c),
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
                        return Task::batch([self.open(), decode]);
                    }
                    Ok(None) => {
                        self.screenshot = None;
                        self.state.capture_cancelled = true;
                    }
                    Err(e) => self.state.error = Some(e),
                }
                self.open()
            }
            Message::DesktopCapture => self.begin_capture(crate::capture::CaptureMode::Interactive),
            Message::ScanFull => self.scan_screenshot(None),
            Message::SelectRegion => {
                self.crop_epoch = self.crop_epoch.wrapping_add(1);
                self.selecting = true;
                Task::none()
            }
            Message::CancelCrop => {
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
            Message::Copied(session, index) => {
                if session.0 == self.state.session && index < self.state.results.len() {
                    self.copy_feedback = Some((session, index));
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
                    iced::clipboard::write(text).chain(Task::done(Message::Copied(outcome.id, 0)))
                } else {
                    Task::none()
                }
            }
            Message::CopyResult(index) => self
                .state
                .results
                .get(index)
                .map(|result| {
                    iced::clipboard::write(result.display_text.clone()).chain(Task::done(
                        Message::Copied(crate::jobs::JobId(self.state.session), index),
                    ))
                })
                .unwrap_or_else(Task::none),
            Message::OpenResult(index) => {
                let Some(url) =
                    self.state
                        .results
                        .get(index)
                        .and_then(|result| match result.action() {
                            Some(crate::results::ResultAction::OpenUrl(url)) => Some(url),
                            _ => None,
                        })
                else {
                    return Task::none();
                };
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let status = std::process::Command::new("xdg-open")
                                .arg(url.as_str())
                                .status()
                                .map_err(|e| e.to_string())?;
                            if status.success() {
                                Ok(())
                            } else {
                                Err(format!("Could not open URL: {status}"))
                            }
                        })
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r)
                    },
                    |result| result.err().map(Message::OpenFailed),
                )
                .and_then(Task::done)
            }
            Message::OpenFailed(error) => {
                self.state.error = Some(error);
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
                        self.decoder.cancel();
                        self.state.session = self.state.session.wrapping_add(1);
                        self.camera_preview = None;
                        self.state.busy = false;
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
    fn view(&self, _id: window::Id) -> Element<'_, Message> {
        let status = self.status();
        let mut results = column![].spacing(12);
        for (index, result) in self.state.results.iter().enumerate() {
            let mut actions = row![button("Copy").on_press(Message::CopyResult(index))].spacing(8);
            if matches!(
                result.action(),
                Some(crate::results::ResultAction::OpenUrl(_))
            ) {
                actions = actions.push(button("Open").on_press(Message::OpenResult(index)));
            }
            results = results.push(column![text(&result.display_text), actions].spacing(4));
        }
        let mut content = column![
            iced::widget::checkbox(self.preferences.auto_copy_single)
                .label("Automatically copy a single result")
                .on_toggle(Message::AutoCopyChanged)
        ];
        if let Some(warning) = &self.settings_warning {
            content = content.push(text(warning));
        }
        if self
            .copy_feedback
            .is_some_and(|(session, _)| session.0 == self.state.session)
        {
            content = content.push(text("Copied to clipboard."));
        }
        if self.camera_mode {
            content = content.push(
                row![
                    iced::widget::pick_list(
                        self.camera_devices.as_slice(),
                        self.selected_camera.as_ref(),
                        Message::SelectCamera
                    )
                    .placeholder("Select camera"),
                    button("Stop camera").on_press(Message::StopCamera)
                ]
                .spacing(8),
            );
            if let Some(handle) = &self.camera_preview {
                content = content.push(
                    iced::widget::image(handle.clone())
                        .height(300)
                        .width(iced::Length::Fill)
                        .content_fit(iced::ContentFit::Contain),
                );
            }
        }
        if let Some(image) = &self.screenshot {
            content = content.push(
                row![
                    button("Scan full image").on_press(Message::ScanFull),
                    button("Select region").on_press(Message::SelectRegion)
                ]
                .spacing(8),
            );
            if self.selecting {
                content = content.push(text("Drag a rectangle to scan. Escape cancels selection."));
            }
            content = content.push(
                iced::widget::canvas(crate::capture::preview::Preview {
                    handle: image.handle.clone(),
                    size: (image.image.width(), image.image.height()),
                    session: crate::jobs::JobId(self.state.session),
                    selecting: self.selecting,
                    epoch: self.crop_epoch,
                })
                .width(iced::Length::Fill)
                .height(300),
            );
        }
        container(
            column![
                text("QR Scanner").size(28),
                text("Scan QR codes locally from your screen or camera."),
                row![
                    button("Scan screen").on_press(Message::UserCommand(Command::ScanScreen)),
                    button("Scan camera").on_press(Message::UserCommand(Command::ScanCamera)),
                    button("Desktop capture…")
                        .on_press(Message::DesktopCapture)
                        .style(button::text)
                ]
                .spacing(8),
                text(status),
                content,
                scrollable(results).height(iced::Length::Fill),
                button("Quit").on_press(Message::UserCommand(Command::Quit))
            ]
            .spacing(12),
        )
        .padding(24)
        .into()
    }
    fn subscription(&self) -> Subscription<Message> {
        let primary = self.primary.clone();
        let camera_subscription = if let Some(camera) = &self.camera {
            Subscription::run_with(CameraSource(camera.clone()), camera_events)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            camera_subscription,
            window::close_events().map(Message::WindowClosed),
            iced::event::listen_with(|event, _, _| {
                matches!(
                    event,
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        ..
                    })
                )
                .then_some(Message::CancelCrop)
            }),
            Subscription::run_with(DecoderSource(self.decoder.clone()), decode_events),
            Subscription::run_with(PrimarySource(primary), external_events),
        ])
    }
}
fn external_events(source: &PrimarySource) -> impl iced::futures::Stream<Item = Message> + use<> {
    let primary = source.0.clone();
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
            let tray = tray::start(commands.clone());
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
mod capture_tests {
    use super::*;
    pub(super) fn app() -> App {
        App {
            state: Default::default(),
            preferences: Default::default(),
            preferences_loaded: false,
            preference_path: None,
            preference_revision: 0,
            saving: None,
            settings_warning: None,
            auto_copy: Default::default(),
            copy_feedback: None,
            primary: Arc::new(Mutex::new(None)),
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
        }
    }
    #[test]
    fn reset_releases_capture_and_rejects_late_response() {
        let mut app = app();
        let old = app.state.begin_session();
        let image = Arc::new(crate::capture::Prepared::new(
            image::DynamicImage::new_rgb8(2, 2),
        ));
        app.screenshot = Some(image.clone());
        app.selecting = true;
        let id = app.reset_source();
        assert_ne!(id, old);
        assert!(app.screenshot.is_none());
        assert!(!app.selecting);
        let _ = app.update(Message::CaptureFinished(old, Ok(Some(image))));
        assert!(app.screenshot.is_none());
        assert!(app.state.busy);
    }
    #[test]
    fn camera_failure_cannot_be_overwritten_by_a_queued_decode() {
        let mut app = app();
        let id = app.state.begin_session();
        let _ = app.update(Message::CameraOutcome(Arc::new(crate::camera::Outcome {
            id,
            event: crate::camera::Event::Error("unplugged".into()),
        })));
        let _ = app.update(Message::DecodeFinished(Arc::new(
            crate::jobs::DecodeOutcome {
                id,
                result: Ok(vec![]),
            },
        )));
        assert_eq!(app.state.error.as_deref(), Some("Camera failed: unplugged"));
        assert!(!app.state.busy);
    }
    #[test]
    fn camera_frames_after_close_or_stop_cannot_restore_preview() {
        let mut app = app();
        let window = window::Id::unique();
        app.state.window_ids.push(window);
        app.state.windows = 1;
        app.state.host_available = true;
        let old = app.state.begin_session();
        let frame = Arc::new(crate::camera::Outcome {
            id: old,
            event: crate::camera::Event::Frame {
                preview: Some(iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4])),
                gray: Some(Arc::new(image::GrayImage::new(1, 1))),
            },
        });
        let _ = app.update(Message::CameraOutcome(frame.clone()));
        assert!(app.camera_preview.is_some());
        let _ = app.update(Message::WindowClosed(window));
        assert!(app.camera_preview.is_none());
        let _ = app.update(Message::CameraOutcome(frame));
        assert!(app.camera_preview.is_none());
        assert!(!app.state.busy);
    }
    #[test]
    fn close_releases_capture() {
        let mut app = app();
        app.state.host_available = true;
        app.state.windows = 1;
        let id = iced::window::Id::unique();
        app.state.window_ids.push(id);
        app.screenshot = Some(Arc::new(crate::capture::Prepared::new(
            image::DynamicImage::new_rgb8(2, 2),
        )));
        let _ = app.update(Message::WindowClosed(id));
        assert!(app.screenshot.is_none());
        assert!(!app.state.busy);
    }
}

#[cfg(test)]
mod temporary_capture_tests {
    use super::capture_tests::app;
    use super::*;
    #[test]
    fn intentional_last_window_close_preserves_capture_session() {
        let mut app = app();
        let window = window::Id::unique();
        app.state.window_ids.push(window);
        app.state.windows = 1;
        let _ = app.command(Command::ScanScreen);
        let session = app.state.session;
        assert!(app.temporary_closes.contains(&window));
        assert!(app.state.window_ids.is_empty());
        assert_eq!(app.state.windows, 0);
        let _ = app.update(Message::WindowClosed(window));
        assert_eq!(app.state.session, session);
        assert!(app.state.busy);
        assert!(app.temporary_closes.is_empty());
        assert_eq!(app.pending_capture, Some(crate::jobs::JobId(session)));
        let _ = app.update(Message::CaptureFinished(
            crate::jobs::JobId(session),
            Ok(None),
        ));
        assert!(!app.state.busy);
        assert_eq!(app.state.window_ids.len(), 1);
        assert!(app.pending_capture.is_none());
    }
    #[test]
    fn waits_for_every_close_and_restores_after_failure() {
        let mut app = app();
        let first = window::Id::unique();
        let second = window::Id::unique();
        app.state.window_ids = vec![first, second];
        app.state.windows = 2;
        let _ = app.command(Command::ScanScreen);
        let id = crate::jobs::JobId(app.state.session);
        assert_eq!(app.update(Message::WindowClosed(first)).units(), 0);
        assert_eq!(app.update(Message::TrayHostChanged(false)).units(), 0);
        assert_eq!(app.command(Command::Show).units(), 0);
        assert!(app.state.window_ids.is_empty());
        assert!(app.update(Message::WindowClosed(second)).units() > 0);
        assert_eq!(app.state.session, id.0);
        let _ = app.update(Message::CaptureFinished(id, Err("denied".into())));
        assert_eq!(app.state.error.as_deref(), Some("denied"));
        assert!(!app.state.busy);
        assert_eq!(app.state.window_ids.len(), 1);
    }
    #[test]
    fn source_switch_during_temporary_close_ignores_old_completion() {
        let mut app = app();
        let window = window::Id::unique();
        app.state.window_ids.push(window);
        app.state.windows = 1;
        let _ = app.command(Command::ScanScreen);
        let old = crate::jobs::JobId(app.state.session);
        let _ = app.command(Command::ScanCamera);
        let session = app.state.session;
        let _ = app.update(Message::WindowClosed(window));
        assert_eq!(app.state.session, session);
        assert_eq!(app.state.window_ids.len(), 1);
        let _ = app.update(Message::CaptureFinished(old, Err("old failure".into())));
        assert_ne!(app.state.error.as_deref(), Some("old failure"));
    }
}

#[cfg(test)]
mod final_review_tests {
    use super::capture_tests::app;
    use super::*;
    #[tokio::test]
    async fn captured_image_automatically_schedules_full_decode() {
        use iced::futures::StreamExt;
        let mut app = app();
        let id = app.state.begin_session();
        let image = Arc::new(crate::capture::Prepared::new(
            image::DynamicImage::new_rgb8(7, 9),
        ));
        let task = app.update(Message::CaptureFinished(id, Ok(Some(image.clone()))));
        assert!(Arc::ptr_eq(app.screenshot.as_ref().unwrap(), &image));
        let mut stream = iced_runtime::task::into_stream(task).unwrap();
        let mut scheduled = false;
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(message @ Message::DecodeRequested(_, _)) = action {
                if let Message::DecodeRequested(_, gray) = &message {
                    assert_eq!(gray.dimensions(), (7, 9));
                }
                let _ = app.update(message);
                scheduled = true;
            }
        }
        assert!(
            scheduled,
            "capture must schedule decoding without another click"
        );
        let outcome = app.decoder.subscribe().recv().await.unwrap();
        let _ = app.update(Message::DecodeFinished(outcome));
        assert!(app.state.scanned);
        assert!(!app.state.busy);
    }
    #[test]
    fn stop_retains_results_and_rejects_stale_camera_and_decode_work() {
        let mut app = app();
        let id = app.state.begin_session();
        app.camera_mode = true;
        app.camera_preview = Some(iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]));
        let _ = app.update(Message::DecodeFinished(Arc::new(
            crate::jobs::DecodeOutcome {
                id,
                result: Ok(vec![crate::results::QrResult::from_payload(
                    b"keep".to_vec(),
                )]),
            },
        )));
        let _ = app.update(Message::StopCamera);
        assert_eq!(
            app.state.results.len(),
            1,
            "Stop must retain scanned results"
        );
        assert!(app.state.scanned);
        assert!(app.camera.is_none());
        assert!(app.camera_preview.is_none());
        assert!(!app.camera_mode);
        let _ = app.update(Message::CameraOutcome(Arc::new(crate::camera::Outcome {
            id,
            event: crate::camera::Event::Frame {
                preview: Some(iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4])),
                gray: Some(Arc::new(image::GrayImage::new(1, 1))),
            },
        })));
        let _ = app.update(Message::DecodeRequested(
            id,
            Arc::new(image::GrayImage::new(1, 1)),
        ));
        let _ = app.update(Message::DecodeFinished(Arc::new(
            crate::jobs::DecodeOutcome {
                id,
                result: Ok(vec![crate::results::QrResult::from_payload(
                    b"stale".to_vec(),
                )]),
            },
        )));
        assert_eq!(app.state.results.len(), 1);
        assert_eq!(app.state.results[0].payload, b"keep");
        assert!(app.camera_preview.is_none());
        assert!(!app.state.busy);
        let _ = app.command(Command::ScanCamera);
        assert!(app.state.results.is_empty());
    }
}

#[cfg(test)]
mod cancellation_feedback_tests {
    use super::*;
    #[test]
    fn cancelled_capture_has_distinct_status_cleared_by_next_operation() {
        let mut app = super::capture_tests::app();
        let id = app.state.begin_session();
        let _ = app.update(Message::CaptureFinished(id, Ok(None)));
        assert_eq!(app.status(), "Capture cancelled.");
        assert!(!app.state.busy);
        assert!(!app.state.scanned);
        assert!(app.state.error.is_none());
        let _ = app.command(Command::ScanCamera);
        assert_ne!(app.status(), "Capture cancelled.");
    }
}

#[cfg(test)]
mod quick_capture_tests {
    use super::capture_tests::app;
    use super::*;
    #[test]
    fn explicit_fallback_survives_window_close_then_next_scan_returns_to_quick() {
        let mut app = app();
        let window = window::Id::unique();
        app.state.window_ids.push(window);
        app.state.windows = 1;
        let _ = app.update(Message::DesktopCapture);
        let id = app.pending_capture.unwrap();
        assert_eq!(app.capture_mode, crate::capture::CaptureMode::Interactive);
        assert!(app.temporary_closes.contains(&window));
        assert!(app.update(Message::WindowClosed(window)).units() > 0);
        assert_eq!(app.capture_mode, crate::capture::CaptureMode::Interactive);
        let _ = app.update(Message::CaptureFinished(
            id,
            Err("Permission denied".into()),
        ));
        assert_eq!(app.state.error.as_deref(), Some("Permission denied"));
        assert!(app.pending_capture.is_none());
        assert_eq!(app.state.window_ids.len(), 1);
        let _ = app.command(Command::ScanScreen);
        assert_eq!(app.capture_mode, crate::capture::CaptureMode::Quick);
    }
}

#[cfg(test)]
mod preference_tests {
    use super::*;
    #[test]
    fn toggle_during_load_wins_and_queued_saves_coalesce() {
        let mut app = super::capture_tests::app();
        assert_eq!(app.update(Message::AutoCopyChanged(true)).units(), 0);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert!(
            app.update(Message::PreferencesLoaded(
                crate::settings::LoadedPreferences {
                    preferences: Default::default(),
                    path: Some(path),
                    warning: None
                }
            ))
            .units()
                > 0
        );
        assert!(app.preferences.auto_copy_single);
        let first = app.saving.unwrap();
        assert_eq!(app.update(Message::AutoCopyChanged(false)).units(), 0);
        assert_eq!(app.update(Message::AutoCopyChanged(true)).units(), 0);
        assert_eq!(app.update(Message::AutoCopyChanged(false)).units(), 0);
        assert!(app.update(Message::PreferencesSaved(first, Ok(()))).units() > 0);
        assert_eq!(app.saving, Some(app.preference_revision));
        assert!(!app.preferences.auto_copy_single);
    }
    #[test]
    fn settings_warning_preserves_results_and_scan_error() {
        let mut app = super::capture_tests::app();
        app.state
            .results
            .push(crate::results::QrResult::from_payload(b"keep".to_vec()));
        app.state.error = Some("scan error".into());
        let _ = app.update(Message::PreferencesLoaded(
            crate::settings::LoadedPreferences {
                preferences: Default::default(),
                path: None,
                warning: Some("settings error".into()),
            },
        ));
        assert_eq!(app.settings_warning.as_deref(), Some("settings error"));
        assert_eq!(app.state.error.as_deref(), Some("scan error"));
        assert_eq!(app.state.results.len(), 1);
    }
    #[tokio::test]
    async fn accepted_single_requests_exact_copy_and_stale_or_duplicate_requests_none() {
        use iced::futures::StreamExt;
        let mut app = super::capture_tests::app();
        app.preferences.auto_copy_single = true;
        let id = app.state.begin_session();
        let outcome = Arc::new(crate::jobs::DecodeOutcome {
            id,
            result: Ok(vec![
                crate::results::QrResult::from_payload(b"WIFI:T:WPA;S:home;P:secret;;".to_vec()),
                crate::results::QrResult::from_payload(b"WIFI:T:WPA;S:home;P:secret;;".to_vec()),
            ]),
        });
        let task = app.update(Message::DecodeFinished(outcome.clone()));
        let mut stream = iced_runtime::task::into_stream(task).unwrap();
        let mut copied = false;
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Clipboard(iced_runtime::clipboard::Action::Write {
                contents,
                ..
            }) = action
            {
                assert_eq!(contents, "WIFI:T:WPA;S:home;P:secret;;");
                copied = true;
            }
        }
        assert!(copied);
        assert_eq!(
            app.update(Message::DecodeFinished(outcome.clone())).units(),
            0
        );
        assert_eq!(app.update(Message::AutoCopyChanged(true)).units(), 0);
        app.state.begin_session();
        assert_eq!(app.update(Message::DecodeFinished(outcome)).units(), 0);
    }
}

#[cfg(test)]
mod persistence_action_tests {
    use super::*;
    use iced::futures::StreamExt;
    async fn complete(task: Task<Message>) -> Message {
        let mut stream = iced_runtime::task::into_stream(task).unwrap();
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(message) = action {
                return message;
            }
        }
        panic!("save must report completion")
    }
    #[tokio::test]
    async fn serialized_writes_retain_last_toggle_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/settings.json");
        let mut app = super::capture_tests::app();
        let _ = app.update(Message::PreferencesLoaded(
            crate::settings::LoadedPreferences {
                preferences: Default::default(),
                path: Some(path.clone()),
                warning: None,
            },
        ));
        let first = app.update(Message::AutoCopyChanged(true));
        let _ = app.update(Message::AutoCopyChanged(false));
        let _ = app.update(Message::AutoCopyChanged(true));
        let _ = app.update(Message::AutoCopyChanged(false));
        let message = complete(first).await;
        let last = app.update(message);
        let message = complete(last).await;
        assert_eq!(app.update(message).units(), 0);
        let saved: crate::settings::Preferences =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(!saved.auto_copy_single);
        assert!(app.saving.is_none());
    }
    #[tokio::test]
    async fn manual_copy_uses_exact_binary_text_and_multicode_never_autocopies() {
        let mut app = super::capture_tests::app();
        app.preferences.auto_copy_single = true;
        let id = app.state.begin_session();
        let binary = crate::results::QrResult::from_payload(vec![255, 0]);
        let task = app.update(Message::DecodeFinished(Arc::new(
            crate::jobs::DecodeOutcome {
                id,
                result: Ok(vec![
                    binary.clone(),
                    crate::results::QrResult::from_payload(b"other".to_vec()),
                ]),
            },
        )));
        assert_eq!(task.units(), 0);
        let mut stream =
            iced_runtime::task::into_stream(app.update(Message::CopyResult(0))).unwrap();
        let mut text = None;
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Clipboard(iced_runtime::clipboard::Action::Write {
                contents,
                ..
            }) = action
            {
                text = Some(contents);
            }
        }
        assert_eq!(text, Some(binary.display_text));
        assert_eq!(app.update(Message::AutoCopyChanged(true)).units(), 0);
    }
    #[test]
    fn save_failure_does_not_change_results_or_scan_error() {
        let mut app = super::capture_tests::app();
        app.state
            .results
            .push(crate::results::QrResult::from_payload(b"keep".to_vec()));
        app.state.error = Some("decode failure".into());
        app.saving = Some(3);
        app.preference_revision = 3;
        let _ = app.update(Message::PreferencesSaved(3, Err("disk full".into())));
        assert!(app.settings_warning.is_some());
        assert_eq!(app.state.results[0].payload, b"keep");
        assert_eq!(app.state.error.as_deref(), Some("decode failure"));
    }
}
