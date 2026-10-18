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
}
impl ksni::Tray for Item {
    fn id(&self) -> String {
        "qr-scanner".into()
    }
    fn title(&self) -> String {
        "QR Scanner".into()
    }
    fn icon_name(&self) -> String {
        "view-barcode-qr".into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        // A self-contained high-contrast finder-pattern icon, ARGB byte order.
        let mut data = vec![255; 32 * 32 * 4];
        for y in 0..32 {
            for x in 0..32 {
                let black = [(3, 3), (19, 3), (3, 19)].iter().any(|&(a, b)| {
                    let dx = x - a;
                    let dy = y - b;
                    (0..10).contains(&dx)
                        && (0..10).contains(&dy)
                        && (!(2..8).contains(&dx)
                            || !(2..8).contains(&dy)
                            || ((4..6).contains(&dx) && (4..6).contains(&dy)))
                });
                if black {
                    let i = (y * 32 + x) as usize * 4;
                    data[i + 1..i + 4].fill(0);
                }
            }
        }
        vec![ksni::Icon {
            width: 32,
            height: 32,
            data,
        }]
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.commands.try_send(Command::Show);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        [
            ("Scan screen", Command::ScanScreen),
            ("Scan camera", Command::ScanCamera),
            ("Show window", Command::Show),
            ("Quit", Command::Quit),
        ]
        .into_iter()
        .map(|(label, command)| {
            ksni::menu::StandardItem {
                label: label.into(),
                activate: Box::new(move |item: &mut Self| {
                    let _ = item.commands.try_send(command);
                }),
                ..Default::default()
            }
            .into()
        })
        .collect()
    }
}
pub fn start(commands: mpsc::Sender<Command>) -> anyhow::Result<TrayService> {
    let (hosts_tx, hosts) = watch::channel(None);
    let failures = hosts_tx.clone();
    let healthy = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let item_health = healthy.clone();
    let item = tokio::spawn(async move {
        match (Item { commands }).spawn().await {
            Ok(_handle) => std::future::pending::<()>().await,
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
