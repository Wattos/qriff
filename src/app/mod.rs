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
    OpenResult(usize),
    OpenFailed(String),
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
    primary: Arc<Mutex<Option<Primary>>>,
    decoder: Arc<crate::jobs::Decoder>,
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
                primary: primary.clone(),
                decoder: Arc::new(crate::jobs::Decoder::new()),
            };
            let task = if command == Command::Background {
                Task::none()
            } else {
                app.command(command)
            };
            (app, task)
        },
        App::update,
        App::view,
    )
    .title("QR Scanner")
    .subscription(App::subscription)
    .run()
}
impl App {
    fn open(&mut self) -> Task<Message> {
        if let Some(&id) = self.state.window_ids.first() {
            return window::gain_focus(id);
        }
        let (id, task) = window::open(window::Settings {
            size: iced::Size::new(460., 280.),
            exit_on_close_request: true,
            ..Default::default()
        });
        self.state.window_ids.push(id);
        self.state.windows = self.state.window_ids.len();
        task.map(Message::Opened)
    }
    fn command(&mut self, command: Command) -> Task<Message> {
        match command {
            Command::Quit => iced::exit(),
            Command::Show => self.open(),
            Command::Background => Task::none(),
            Command::ScanScreen | Command::ScanCamera => {
                self.decoder.cancel();
                self.state.begin_session();
                self.state.busy = false;
                self.state.error = Some(
                    if command == Command::ScanScreen {
                        "Screen capture is not implemented yet."
                    } else {
                        "Camera capture is not implemented yet."
                    }
                    .into(),
                );
                self.open()
            }
        }
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::UserCommand(c) | Message::InstanceCommand(c) => self.command(c),
            Message::TrayHostChanged(available) => {
                if self.state.host_changed(available) {
                    self.open()
                } else {
                    Task::none()
                }
            }
            Message::WindowClosed(id) => {
                self.decoder.cancel();
                self.state.begin_session();
                self.state.busy = false;
                self.state.window_ids.retain(|&v| v != id);
                if self.state.close_window() {
                    iced::exit()
                } else {
                    Task::none()
                }
            }
            Message::SourceUnavailable(e) => {
                self.state.error = Some(e);
                Task::none()
            }
            Message::DecodeRequested(id, image) => {
                if id.0 == self.state.session {
                    self.state.busy = true;
                    self.decoder.submit(id, (*image).clone());
                }
                Task::none()
            }
            Message::DecodeFinished(outcome) => {
                self.state.accept_decode(&outcome);
                Task::none()
            }
            Message::CopyResult(index) => self
                .state
                .results
                .get(index)
                .map(|result| iced::clipboard::write(result.display_text.clone()))
                .unwrap_or_else(Task::none),
            Message::OpenResult(index) => {
                let Some(url) = self
                    .state
                    .results
                    .get(index)
                    .and_then(|result| result.open_url.clone())
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
            Message::Opened(_) => Task::none(),
        }
    }
    fn view(&self, _id: window::Id) -> Element<'_, Message> {
        let status = if self.state.busy {
            "Scanning…"
        } else if let Some(error) = self.state.error.as_deref() {
            error
        } else if self.state.scanned && self.state.results.is_empty() {
            "No QR codes found."
        } else if self.state.scanned {
            "Scan complete"
        } else {
            "Ready"
        };
        let mut results = column![].spacing(12);
        for (index, result) in self.state.results.iter().enumerate() {
            let mut actions = row![button("Copy").on_press(Message::CopyResult(index))].spacing(8);
            if result.open_url.is_some() {
                actions = actions.push(button("Open").on_press(Message::OpenResult(index)));
            }
            results = results.push(column![text(&result.display_text), actions].spacing(4));
        }
        container(
            column![
                text("QR Scanner").size(28),
                text("Scan QR codes locally from your screen or camera."),
                row![
                    button("Scan screen").on_press(Message::UserCommand(Command::ScanScreen)),
                    button("Scan camera").on_press(Message::UserCommand(Command::ScanCamera))
                ]
                .spacing(8),
                text(status),
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
        Subscription::batch([
            window::close_events().map(Message::WindowClosed),
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
        loop {
            if outcomes.changed().await.is_err() {
                break;
            }
            let outcome = outcomes.borrow_and_update().clone();
            if let Some(outcome) = outcome
                && output.send(Message::DecodeFinished(outcome)).await.is_err()
            {
                break;
            }
        }
    })
}
