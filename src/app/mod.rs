pub mod state;
use crate::{
    cli::Command,
    instance::{InstanceRole, Primary},
    tray,
};
use iced::{
    Element, Subscription, Task,
    widget::{button, column, container, text},
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
                self.state.session += 1;
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
            Message::Opened(_) => Task::none(),
        }
    }
    fn view(&self, _id: window::Id) -> Element<'_, Message> {
        container(
            column![
                text("QR Scanner").size(28),
                text("Scan QR codes locally from your screen or camera."),
                button("Scan screen").on_press(Message::UserCommand(Command::ScanScreen)),
                button("Scan camera").on_press(Message::UserCommand(Command::ScanCamera)),
                text(self.state.error.as_deref().unwrap_or("Ready")),
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
