//! Linux StatusNotifierItem. All delivery is driven by commands or D-Bus signals.
use crate::cli::Command;
use futures_util::StreamExt;
use ksni::TrayMethods;
use tokio::sync::{mpsc, watch};
pub struct TrayService {
    pub hosts: watch::Receiver<Option<bool>>,
    _item: tokio::task::JoinHandle<()>,
    _monitor: tokio::task::JoinHandle<()>,
}
impl Drop for TrayService {
    fn drop(&mut self) {
        self._item.abort();
        self._monitor.abort();
    }
}
struct Item {
    commands: mpsc::Sender<Command>,
    snapshot: crate::desktop::Snapshot,
}
impl ksni::Tray for Item {
    const MENU_ON_ACTIVATE: bool = true;
    fn id(&self) -> String {
        "qr-scanner".into()
    }
    fn title(&self) -> String {
        "QRiff".into()
    }
    fn icon_name(&self) -> String {
        // Use the embedded logo even when no icon theme installation exists.
        String::new()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let pixels = image::load_from_memory(include_bytes!("../assets/qriff-tray.png"))
            .expect("embedded QRiff tray icon")
            .into_rgba8();
        let (width, height) = pixels.dimensions();
        // StatusNotifierItem transports pixels in network-byte-order ARGB.
        let data = pixels
            .pixels()
            .flat_map(|pixel| {
                let [r, g, b, a] = pixel.0;
                [a, r, g, b]
            })
            .collect();
        vec![ksni::Icon {
            width: width as i32,
            height: height as i32,
            data,
        }]
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let mut menu: Vec<ksni::MenuItem<Self>> = [
            ("Scan screen", Command::ScanScreen),
            ("Scan region", Command::ScanRegion),
            ("Scan camera", Command::ScanCamera),
        ]
        .into_iter()
        .map(|(label, command)| Self::button(label.into(), command, true))
        .collect();
        let results = self
            .snapshot
            .results
            .iter()
            .enumerate()
            .map(|(index, result)| {
                let session = self.snapshot.session;
                let mut submenu = vec![Self::button(
                    "Copy".into(),
                    Command::CopyResult(session, index),
                    true,
                )];
                if let Some(action) = result.action {
                    submenu.push(Self::button(
                        if result.pending {
                            "Working…".into()
                        } else {
                            action.into()
                        },
                        Command::ActivateResult(session, index),
                        !result.pending,
                    ));
                }
                if let Some(feedback) = &result.feedback {
                    submenu.push(Self::button(feedback.clone(), Command::Show, false));
                }
                ksni::menu::SubMenu {
                    label: result.label.replace('_', "__"),
                    submenu,
                    ..Default::default()
                }
                .into()
            })
            .collect();
        menu.push(
            ksni::menu::SubMenu {
                label: "Last scan".into(),
                enabled: !self.snapshot.results.is_empty(),
                submenu: results,
                ..Default::default()
            }
            .into(),
        );
        menu.extend([
            Self::button("Open window".into(), Command::Show, true),
            Self::button("Quit".into(), Command::Quit, true),
        ]);
        menu
    }
}
impl Item {
    fn button(label: String, command: Command, enabled: bool) -> ksni::MenuItem<Self> {
        ksni::menu::StandardItem {
            label,
            enabled,
            activate: Box::new(move |item: &mut Self| {
                let _ = item.commands.try_send(command);
            }),
            ..Default::default()
        }
        .into()
    }
}
pub fn start(
    commands: mpsc::Sender<Command>,
    mut snapshots: watch::Receiver<crate::desktop::Snapshot>,
) -> anyhow::Result<TrayService> {
    let (hosts_tx, hosts) = watch::channel(None);
    let failures = hosts_tx.clone();
    let healthy = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let item_health = healthy.clone();
    let item = tokio::spawn(async move {
        let snapshot = snapshots.borrow_and_update().clone();
        match (Item { commands, snapshot }).spawn().await {
            Ok(handle) => {
                while snapshots.changed().await.is_ok() {
                    let snapshot = snapshots.borrow_and_update().clone();
                    if handle
                        .update(move |item| item.snapshot = snapshot)
                        .await
                        .is_none()
                    {
                        break;
                    }
                }
            }
            Err(e) => {
                item_health.store(false, std::sync::atomic::Ordering::SeqCst);
                eprintln!("Tray unavailable: {e}");
                let _ = failures.send(Some(false));
            }
        }
    });
    let monitor = tokio::spawn(async move {
        if let Err(e) = monitor_host(hosts_tx.clone(), healthy).await {
            eprintln!("Tray host monitoring unavailable: {e}");
            let _ = hosts_tx.send(Some(false));
        }
    });
    Ok(TrayService {
        hosts,
        _item: item,
        _monitor: monitor,
    })
}
async fn monitor_host(
    tx: watch::Sender<Option<bool>>,
    healthy: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> anyhow::Result<()> {
    let connection = zbus::Connection::session().await?;
    let proxy: zbus::Proxy<'_> = zbus::proxy::Builder::new(&connection)
        .destination("org.kde.StatusNotifierWatcher")?
        .path("/StatusNotifierWatcher")?
        .interface("org.kde.StatusNotifierWatcher")?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await?;
    // Install both subscriptions before reading the initial value to avoid a startup race.
    let mut signals = proxy.receive_all_signals().await?;
    let mut owners = proxy.receive_owner_changed().await?;
    loop {
        let available = proxy
            .get_property::<bool>("IsStatusNotifierHostRegistered")
            .await
            .unwrap_or(false);
        if tx
            .send(Some(
                available && healthy.load(std::sync::atomic::Ordering::SeqCst),
            ))
            .is_err()
        {
            return Ok(());
        }
        tokio::select! {
         signal=signals.next()=>if signal.is_none(){return Ok(());},
         owner=owners.next()=>if owner.is_none(){return Ok(());},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn primary_click_opens_menu_without_commands() {
        let (commands, mut rx) = mpsc::channel(2);
        let mut item = Item {
            commands,
            snapshot: Default::default(),
        };
        ksni::Tray::activate(&mut item, 0, 0);
        assert!(std::hint::black_box(<Item as ksni::Tray>::MENU_ON_ACTIVATE));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn last_scan_callbacks_use_literal_identity_and_show_failure() {
        let (commands, mut rx) = mpsc::channel(4);
        let mut item = Item {
            commands,
            snapshot: crate::desktop::Snapshot {
                session: 37,
                results: vec![crate::desktop::ResultEntry {
                    label: "safe".into(),
                    action: Some("Call"),
                    pending: true,
                    feedback: Some("Permission denied".into()),
                }],
                notification: None,
            },
        };
        let menu = ksni::Tray::menu(&item);
        let ksni::MenuItem::SubMenu(last) = &menu[3] else {
            panic!("Last scan")
        };
        let ksni::MenuItem::SubMenu(result) = &last.submenu[0] else {
            panic!("result")
        };
        let ksni::MenuItem::Standard(copy) = &result.submenu[0] else {
            panic!("copy")
        };
        (copy.activate)(&mut item);
        assert_eq!(rx.try_recv().unwrap(), Command::CopyResult(37, 0));
        let ksni::MenuItem::Standard(action) = &result.submenu[1] else {
            panic!("action")
        };
        assert!(!action.enabled);
        (action.activate)(&mut item);
        assert_eq!(rx.try_recv().unwrap(), Command::ActivateResult(37, 0));
        let ksni::MenuItem::Standard(feedback) = &result.submenu[2] else {
            panic!("feedback")
        };
        assert_eq!(feedback.label, "Permission denied");
        assert!(!feedback.enabled);
    }
}
