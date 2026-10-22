//! In-memory, secret-safe presentation shared by the tray and notification service.
use crate::{
    cli::Command,
    results::{ContentKind, QrContent, QrResult},
};
use futures_util::StreamExt;
use tokio::sync::{mpsc, watch};

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub session: u64,
    pub results: Vec<ResultEntry>,
    pub notification: Option<Notice>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultEntry {
    pub label: String,
    pub action: Option<&'static str>,
    pub pending: bool,
    pub feedback: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
}
/// Labels never contain Wi-Fi raw text (including malformed credentials).
impl ResultEntry {
    pub fn new(result: &QrResult, pending: bool) -> Self {
        let (label, action) = match &result.content {
            QrContent::Url(url) => (url.as_str().to_owned(), Some("Open URL")),
            QrContent::Phone { number, .. } => (number.clone(), Some("Call")),
            QrContent::Wifi(network) => (
                format!("Wi-Fi: {}", network.ssid),
                result.action().map(|_| "Connect Wi-Fi"),
            ),
            QrContent::Invalid {
                kind: ContentKind::Wifi,
                ..
            } => ("Invalid Wi-Fi QR code".into(), None),
            QrContent::Invalid { .. } => ("Invalid QR code".into(), None),
            QrContent::Binary => ("Binary QR code".into(), None),
            QrContent::Text => (result.display_text.clone(), None),
        };
        Self {
            label: plain(&label, 120),
            action,
            pending,
            feedback: None,
        }
    }
}
fn plain(text: &str, limit: usize) -> String {
    let mut chars = text.chars().filter(|c| !c.is_control());
    let mut label: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        label.push('…');
    }
    label
}
fn markup(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
impl Snapshot {
    pub fn actions(&self) -> Vec<String> {
        if self.results.len() != 1 {
            return vec![];
        }
        let mut actions = vec![format!("copy:{}:0", self.session), "Copy".into()];
        let result = &self.results[0];
        if let Some(action) = result.action.filter(|_| !result.pending) {
            actions.extend([format!("activate:{}:0", self.session), action.into()]);
        }
        actions
    }
    pub fn command(&self, key: &str) -> Option<Command> {
        let mut parts = key.split(':');
        let kind = parts.next()?;
        let session: u64 = parts.next()?.parse().ok()?;
        let index: usize = parts.next()?.parse().ok()?;
        if parts.next().is_some() || session != self.session {
            return None;
        }
        let result = self.results.get(index)?;
        match kind {
            "copy" => Some(Command::CopyResult(session, index)),
            "activate" if result.action.is_some() && !result.pending => {
                Some(Command::ActivateResult(session, index))
            }
            _ => None,
        }
    }
}

/// Subscribe before Notify so even an immediate user activation cannot be lost.
/// No polling or payload persistence. Failure leaves all results accessible in the tray.
pub async fn notifications(
    snapshots: watch::Receiver<Snapshot>,
    commands: mpsc::Sender<Command>,
) -> anyhow::Result<()> {
    let connection = zbus::Connection::session().await?;
    notifications_on(connection, snapshots, commands).await
}
async fn notifications_on(
    connection: zbus::Connection,
    mut snapshots: watch::Receiver<Snapshot>,
    commands: mpsc::Sender<Command>,
) -> anyhow::Result<()> {
    let (proxy, mut actions, capabilities) =
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let proxy = zbus::Proxy::new(
                &connection,
                "org.freedesktop.Notifications",
                "/org/freedesktop/Notifications",
                "org.freedesktop.Notifications",
            )
            .await?;
            let actions = proxy.receive_signal("ActionInvoked").await?;
            let capabilities: Vec<String> = proxy.call("GetCapabilities", &()).await?;
            Ok::<_, zbus::Error>((proxy, actions, capabilities))
        })
        .await??;
    let mut id = 0u32;
    let mut shown = Snapshot::default();
    loop {
        let current = snapshots.borrow_and_update().clone();
        if current != shown {
            if let Some(notice) = &current.notification {
                let buttons = if capabilities.iter().any(|v| v == "actions") {
                    current.actions()
                } else {
                    vec![]
                };
                let hints: std::collections::HashMap<&str, zbus::zvariant::Value<'_>> =
                    std::collections::HashMap::from([
                        ("transient", zbus::zvariant::Value::from(true)),
                        ("category", zbus::zvariant::Value::from("transfer.complete")),
                    ]);
                let body = if capabilities.iter().any(|v| v == "body-markup") {
                    markup(&notice.body)
                } else {
                    notice.body.clone()
                };
                // A stalled server must not prevent new snapshots or tray interaction forever.
                if let Ok(Ok(next)) = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    proxy.call::<_, _, u32>(
                        "Notify",
                        &(
                            "QR Scanner",
                            id,
                            "view-barcode-qr",
                            &notice.title,
                            body,
                            buttons,
                            hints,
                            -1i32,
                        ),
                    ),
                )
                .await
                {
                    id = next;
                }
            } else if id != 0 {
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    proxy.call::<_, _, ()>("CloseNotification", &(id,)),
                )
                .await;
                id = 0;
            }
            shown = current;
        }
        tokio::select! {
            changed = snapshots.changed() => if changed.is_err() { return Ok(()); },
            signal = actions.next() => {
                let Some(signal) = signal else { return Ok(()); };
                if let Ok((notification, key)) = signal.body().deserialize::<(u32, String)>()
                    && notification == id && id != 0
                    && let Some(command) = shown.command(&key) {
                    let _ = commands.send(command).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_redact_wifi_and_escape_notification_markup() {
        for text in [
            "WIFI:T:WPA;S:home;P:supersecret;;",
            "WIFI:T:WPA;P:supersecret;;",
        ] {
            let entry = ResultEntry::new(&QrResult::from_payload(text.as_bytes().to_vec()), false);
            assert!(!entry.label.contains("supersecret"));
            assert!(!entry.label.contains("WIFI:"));
        }
        assert_eq!(
            markup("<b>hello & goodbye</b>"),
            "&lt;b&gt;hello &amp; goodbye&lt;/b&gt;"
        );
    }
    #[test]
    fn notification_buttons_are_session_scoped_and_pending_safe() {
        let mut snapshot = Snapshot {
            session: 42,
            results: vec![ResultEntry {
                label: "example".into(),
                action: Some("Open URL"),
                pending: false,
                feedback: None,
            }],
            notification: None,
        };
        assert_eq!(
            snapshot.actions(),
            ["copy:42:0", "Copy", "activate:42:0", "Open URL"]
        );
        assert_eq!(
            snapshot.command("activate:42:0"),
            Some(Command::ActivateResult(42, 0))
        );
        for key in [
            "activate:41:0",
            "activate:42:1",
            "activate:42:0:extra",
            "bogus:42:0",
        ] {
            assert!(snapshot.command(key).is_none());
        }
        snapshot.results[0].pending = true;
        assert!(snapshot.command("activate:42:0").is_none());
        assert_eq!(snapshot.actions(), ["copy:42:0", "Copy"]);
        snapshot.results.push(snapshot.results[0].clone());
        assert!(
            snapshot.actions().is_empty(),
            "multi-code notification must not pick an arbitrary result"
        );
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};
    struct Server {
        calls: mpsc::UnboundedSender<(String, Vec<String>)>,
        stalled: bool,
    }
    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl Server {
        async fn get_capabilities(&self) -> Vec<&str> {
            if self.stalled {
                std::future::pending::<()>().await;
            }
            vec!["actions", "body-markup"]
        }
        #[allow(clippy::too_many_arguments)]
        fn notify(
            &self,
            _app: String,
            _replaces: u32,
            _icon: String,
            _summary: String,
            body: String,
            actions: Vec<String>,
            _hints: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
            _timeout: i32,
        ) -> u32 {
            self.calls.send((body, actions)).unwrap();
            7
        }
        fn close_notification(&self, _id: u32) {}
        #[zbus(signal)]
        async fn action_invoked(
            emitter: &zbus::object_server::SignalEmitter<'_>,
            id: u32,
            action: &str,
        ) -> zbus::Result<()>;
    }
    #[tokio::test]
    async fn notification_wire_contract_and_explicit_session_scoped_actions() {
        // Private bus: no desktop notification, network change, clipboard or handler launch.
        let mut bus = tokio::process::Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(bus.stdout.take().unwrap()).lines();
        let address = lines.next_line().await.unwrap().unwrap();
        let (calls, mut received) = mpsc::unbounded_channel();
        let server = zbus::connection::Builder::address(address.as_str())
            .unwrap()
            .name("org.freedesktop.Notifications")
            .unwrap()
            .serve_at(
                "/org/freedesktop/Notifications",
                Server {
                    calls,
                    stalled: false,
                },
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        let connection = zbus::connection::Builder::address(address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        let (tx, snapshots) = watch::channel(Snapshot::default());
        let (commands, mut incoming) = mpsc::channel(4);
        let worker = tokio::spawn(notifications_on(connection, snapshots, commands));
        let mut snapshot = Snapshot {
            session: 42,
            results: vec![ResultEntry {
                label: "safe".into(),
                action: Some("Open URL"),
                pending: false,
                feedback: None,
            }],
            notification: Some(Notice {
                title: "QR code found".into(),
                body: "<example>&".into(),
            }),
        };
        tx.send(snapshot.clone()).unwrap();
        let (body, actions) =
            tokio::time::timeout(std::time::Duration::from_secs(3), received.recv())
                .await
                .unwrap()
                .unwrap();
        assert_eq!(body, "&lt;example&gt;&amp;");
        assert_eq!(actions, ["copy:42:0", "Copy", "activate:42:0", "Open URL"]);
        assert!(
            incoming.try_recv().is_err(),
            "displaying a QR must never execute an action"
        );
        let interface = server
            .object_server()
            .interface::<_, Server>("/org/freedesktop/Notifications")
            .await
            .unwrap();
        Server::action_invoked(interface.signal_emitter(), 7, "copy:42:0")
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(3), incoming.recv())
                .await
                .unwrap(),
            Some(Command::CopyResult(42, 0))
        );
        snapshot.session = 43;
        tx.send(snapshot).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), received.recv())
            .await
            .unwrap()
            .unwrap();
        Server::action_invoked(interface.signal_emitter(), 7, "activate:42:0")
            .await
            .unwrap();
        Server::action_invoked(interface.signal_emitter(), 7, "activate:43:0")
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(3), incoming.recv())
                .await
                .unwrap(),
            Some(Command::ActivateResult(43, 0))
        );
        worker.abort();
        bus.kill().await.unwrap();
    }
    #[tokio::test]
    async fn stalled_notification_server_fails_finitely_and_restart_retains_results() {
        let mut bus = tokio::process::Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let address = BufReader::new(bus.stdout.take().unwrap())
            .lines()
            .next_line()
            .await
            .unwrap()
            .unwrap();
        let (calls, mut received) = mpsc::unbounded_channel();
        let server = zbus::connection::Builder::address(address.as_str())
            .unwrap()
            .name("org.freedesktop.Notifications")
            .unwrap()
            .serve_at(
                "/org/freedesktop/Notifications",
                Server {
                    calls: calls.clone(),
                    stalled: true,
                },
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        let connection = zbus::connection::Builder::address(address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        let snapshot = Snapshot {
            session: 17,
            results: vec![ResultEntry {
                label: "safe result".into(),
                action: Some("Open URL"),
                pending: false,
                feedback: None,
            }],
            notification: Some(Notice {
                title: "QR code found".into(),
                body: "safe result".into(),
            }),
        };
        let (tx, snapshots) = watch::channel(snapshot.clone());
        let (commands, mut incoming) = mpsc::channel(4);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(7),
            notifications_on(connection, snapshots.clone(), commands.clone()),
        )
        .await;
        assert!(
            result
                .expect("notification initialization must be bounded")
                .is_err()
        );
        assert_eq!(
            *snapshots.borrow(),
            snapshot,
            "tray retains results after failure"
        );
        assert!(
            incoming.try_recv().is_err(),
            "failure must not open a window or act"
        );
        server.close().await.unwrap();
        let _server = zbus::connection::Builder::address(address.as_str())
            .unwrap()
            .name("org.freedesktop.Notifications")
            .unwrap()
            .serve_at(
                "/org/freedesktop/Notifications",
                Server {
                    calls,
                    stalled: false,
                },
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        let connection = zbus::connection::Builder::address(address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        let worker = tokio::spawn(notifications_on(connection, snapshots, commands));
        let (_, actions) = tokio::time::timeout(std::time::Duration::from_secs(3), received.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actions, ["copy:17:0", "Copy", "activate:17:0", "Open URL"]);
        assert_eq!(
            tx.borrow().command("copy:17:0"),
            Some(Command::CopyResult(17, 0))
        );
        assert!(incoming.try_recv().is_err());
        worker.abort();
        bus.kill().await.unwrap();
    }
}
