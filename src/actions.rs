//! Explicit result actions. No decoder, clipboard or settings path calls this module.
use crate::{
    jobs::JobId,
    results::{QrContent, QrResult, ResultAction, WifiNetwork, WifiSecurity},
};
use futures_util::StreamExt;
use std::{
    collections::HashMap,
    fmt,
    process::{Command, Stdio},
    time::Duration,
};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionKey {
    pub session: JobId,
    pub result_index: usize,
}
#[derive(Debug, Clone)]
pub enum ActionStatus {
    Pending,
    Succeeded,
    Failed(ActionError),
}
#[derive(Debug, Clone)]
pub struct ActionState {
    pub key: ActionKey,
    pub token: u64,
    pub status: ActionStatus,
}
/// Deliberately carries no provider messages or user input: D-Bus errors can contain secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionError {
    InvalidRequest,
    Unsupported,
    ServiceUnavailable,
    NoDevice,
    WirelessDisabled,
    NetworkUnavailable,
    Authorization,
    PasswordRejected,
    ActivationFailed,
    Timeout,
    NoPhoneHandler,
    DesktopUnavailable,
}
impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidRequest => "This action has invalid connection details.",
            Self::Unsupported => "This Wi-Fi security type is unsupported. Copy the details to connect manually.",
            Self::ServiceUnavailable => "NetworkManager is unavailable on this system.",
            Self::NoDevice => "No compatible managed Wi-Fi device is available.",
            Self::WirelessDisabled => "Wi-Fi is disabled. Enable it in network settings and try again.",
            Self::NetworkUnavailable => "This Wi-Fi network is not currently available. Move closer or try network settings.",
            Self::Authorization => "Permission to connect was denied. Try network settings.",
            Self::PasswordRejected => "Wi-Fi authentication failed. Check the password in network settings.",
            Self::ActivationFailed => "Wi-Fi could not connect. Check network settings and try again.",
            Self::Timeout => "Wi-Fi connection timed out. Check network settings before retrying.",
            Self::NoPhoneHandler => "No telephone app is configured. Copy the number to use it elsewhere.",
            Self::DesktopUnavailable => "The desktop could not open this link. Copy it to use another app.",
        })
    }
}
impl std::error::Error for ActionError {}

fn validated_uri(action: &ResultAction) -> Result<String, ActionError> {
    let (ResultAction::OpenUrl(uri) | ResultAction::Call(uri)) = action else {
        return Err(ActionError::InvalidRequest);
    };
    // Revalidate at the effect boundary even if a caller constructed a public enum directly.
    let result = QrResult::from_payload(uri.as_str().as_bytes().to_vec());
    if matches!(
        (action, result.content),
        (ResultAction::OpenUrl(_), QrContent::Url(_))
            | (ResultAction::Call(_), QrContent::Phone { .. })
    ) {
        Ok(uri.as_str().to_owned())
    } else {
        Err(ActionError::InvalidRequest)
    }
}
pub async fn execute(action: ResultAction) -> Result<(), ActionError> {
    if let ResultAction::ConnectWifi(network) = action {
        return connect_with(&NetworkManager, &network).await;
    }
    let uri = validated_uri(&action)?;
    let phone = matches!(action, ResultAction::Call(_));
    tokio::task::spawn_blocking(move || {
        if phone {
            let handler = Command::new("xdg-mime")
                .args(["query", "default", "x-scheme-handler/tel"])
                .stderr(Stdio::null())
                .output()
                .map_err(|_| ActionError::NoPhoneHandler)?;
            if !handler.status.success() || handler.stdout.iter().all(u8::is_ascii_whitespace) {
                return Err(ActionError::NoPhoneHandler);
            }
        }
        let status = Command::new("xdg-open")
            .arg(uri)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| ActionError::DesktopUnavailable)?;
        if status.success() {
            Ok(())
        } else {
            Err(if phone {
                ActionError::NoPhoneHandler
            } else {
                ActionError::DesktopUnavailable
            })
        }
    })
    .await
    .map_err(|_| ActionError::DesktopUnavailable)?
}

type Properties = HashMap<String, Value<'static>>;
type Settings = HashMap<String, Properties>;
struct WifiRequest {
    settings: Settings,
    options: Properties,
}
impl fmt::Debug for WifiRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WifiRequest { settings: [REDACTED], persist: volatile }")
    }
}
impl WifiRequest {
    fn new(network: &WifiNetwork) -> Result<Self, ActionError> {
        if network.ssid.is_empty()
            || network.ssid.len() > 32
            || network.ssid.chars().any(char::is_control)
        {
            return Err(ActionError::InvalidRequest);
        }
        let key = match network.security {
            WifiSecurity::Open if network.password.as_ref().is_some_and(|p| !p.is_empty()) => {
                return Err(ActionError::InvalidRequest);
            }
            WifiSecurity::Open => None,
            WifiSecurity::WpaPersonal => Some("wpa-psk"),
            WifiSecurity::Sae => Some("sae"),
            WifiSecurity::Unsupported(_) => return Err(ActionError::Unsupported),
        };
        let mut settings = Settings::new();
        settings.insert(
            "connection".into(),
            HashMap::from([
                ("id".into(), Value::from("QR Scanner Wi-Fi")),
                ("type".into(), Value::from("802-11-wireless")),
                ("autoconnect".into(), Value::from(false)),
            ]),
        );
        settings.insert(
            "802-11-wireless".into(),
            HashMap::from([
                ("ssid".into(), Value::from(network.ssid.as_bytes().to_vec())),
                ("mode".into(), Value::from("infrastructure")),
                ("hidden".into(), Value::from(network.hidden)),
            ]),
        );
        if let Some(key) = key {
            let password = network
                .password
                .as_ref()
                .ok_or(ActionError::InvalidRequest)?;
            let valid = match network.security {
                WifiSecurity::WpaPersonal => {
                    (8..=63).contains(&password.len())
                        || (password.len() == 64 && password.bytes().all(|b| b.is_ascii_hexdigit()))
                }
                WifiSecurity::Sae => !password.is_empty() && password.len() <= 63,
                _ => false,
            };
            if !valid || password.chars().any(char::is_control) {
                return Err(ActionError::InvalidRequest);
            }
            settings.insert(
                "802-11-wireless-security".into(),
                HashMap::from([
                    ("key-mgmt".into(), Value::from(key)),
                    ("psk".into(), Value::from(password.clone())),
                    // NOT_SAVED: no secret agent should persist this credential.
                    ("psk-flags".into(), Value::from(2u32)),
                ]),
            );
        }
        Ok(Self {
            settings,
            options: HashMap::from([("persist".into(), Value::from("volatile"))]),
        })
    }
}
trait WifiAdapter {
    fn connect(
        &self,
        request: WifiRequest,
        network: &WifiNetwork,
    ) -> impl std::future::Future<Output = Result<(), ActionError>> + Send;
}
async fn connect_with(
    adapter: &impl WifiAdapter,
    network: &WifiNetwork,
) -> Result<(), ActionError> {
    connect_with_timeout(adapter, network, Duration::from_secs(60)).await
}
async fn connect_with_timeout(
    adapter: &impl WifiAdapter,
    network: &WifiNetwork,
    timeout: Duration,
) -> Result<(), ActionError> {
    let request = WifiRequest::new(network)?;
    // Includes device discovery and all D-Bus calls, not just the final monitor.
    tokio::time::timeout(timeout, adapter.connect(request, network))
        .await
        .map_err(|_| ActionError::Timeout)?
}
fn dbus_error(error: zbus::Error) -> ActionError {
    match error {
        zbus::Error::MethodError(name, _, _) => dbus_error_name(name.as_str()),
        _ => ActionError::ActivationFailed,
    }
}
fn dbus_error_name(name: &str) -> ActionError {
    if name.ends_with("PermissionDenied")
        || name.ends_with("NotAuthorized")
        || name.ends_with("AccessDenied")
    {
        ActionError::Authorization
    } else if name.ends_with("ServiceUnknown") || name.ends_with("NameHasNoOwner") {
        ActionError::ServiceUnavailable
    } else {
        ActionError::ActivationFailed
    }
}
fn activation_outcome(state: u32, reason: u32) -> Option<Result<(), ActionError>> {
    activation_outcome_with_device(state, reason, None)
}
fn activation_outcome_with_device(
    state: u32,
    reason: u32,
    device_reason: Option<u32>,
) -> Option<Result<(), ActionError>> {
    match state {
        2 => Some(Ok(())), // NM_ACTIVE_CONNECTION_STATE_ACTIVATED
        3 | 4 => Some(Err(if matches!(reason, 9 | 10) {
            ActionError::PasswordRejected
        } else if matches!(device_reason, Some(7..=11)) {
            // NMDeviceStateReason: missing secrets and supplicant authentication failures.
            ActionError::PasswordRejected
        } else if device_reason == Some(53) {
            ActionError::NetworkUnavailable
        } else {
            ActionError::ActivationFailed
        })),
        _ => None,
    }
}
async fn observed_activation_outcome(
    state: u32,
    reason: u32,
    device: &zbus::Proxy<'_>,
) -> Option<Result<(), ActionError>> {
    let outcome = activation_outcome(state, reason)?;
    if !matches!(outcome, Err(ActionError::ActivationFailed)) {
        return Some(outcome);
    }
    let device_reason = device
        .get_property::<(u32, u32)>("StateReason")
        .await
        .ok()
        .map(|(_, reason)| reason);
    activation_outcome_with_device(state, reason, device_reason)
}
type DeviceChoice = (bool, OwnedObjectPath, OwnedObjectPath);
fn choose_candidate(
    mut candidates: Vec<DeviceChoice>,
    compatible_device: bool,
) -> Result<DeviceChoice, ActionError> {
    candidates.sort_by_key(|(active, _, _)| !*active);
    candidates.into_iter().next().ok_or(if compatible_device {
        ActionError::NetworkUnavailable
    } else {
        ActionError::NoDevice
    })
}
const NM: &str = "org.freedesktop.NetworkManager";
async fn proxy<'a>(
    connection: &'a zbus::Connection,
    path: &'a str,
    interface: &'a str,
) -> Result<zbus::Proxy<'a>, ActionError> {
    zbus::Proxy::new(connection, NM, path, interface)
        .await
        .map_err(dbus_error)
}
struct NetworkManager;
impl WifiAdapter for NetworkManager {
    async fn connect(
        &self,
        request: WifiRequest,
        network: &WifiNetwork,
    ) -> Result<(), ActionError> {
        let connection = zbus::Connection::system()
            .await
            .map_err(|_| ActionError::ServiceUnavailable)?;
        let manager = proxy(&connection, "/org/freedesktop/NetworkManager", NM).await?;
        for property in [
            "NetworkingEnabled",
            "WirelessEnabled",
            "WirelessHardwareEnabled",
        ] {
            if !manager
                .get_property::<bool>(property)
                .await
                .map_err(dbus_error)?
            {
                return Err(ActionError::WirelessDisabled);
            }
        }
        let devices: Vec<OwnedObjectPath> =
            manager.call("GetDevices", &()).await.map_err(dbus_error)?;
        let mut candidates = Vec::new();
        let mut compatible_device = false;
        for path in devices {
            let device = proxy(
                &connection,
                path.as_str(),
                "org.freedesktop.NetworkManager.Device",
            )
            .await?;
            let device_type = device
                .get_property::<u32>("DeviceType")
                .await
                .map_err(dbus_error)?;
            let managed = device
                .get_property::<bool>("Managed")
                .await
                .map_err(dbus_error)?;
            if device_type != 2 || !managed {
                continue;
            }
            let state = device
                .get_property::<u32>("State")
                .await
                .map_err(dbus_error)?;
            if state < 30 {
                continue;
            } // unmanaged/unavailable/unknown
            let wireless = proxy(
                &connection,
                path.as_str(),
                "org.freedesktop.NetworkManager.Device.Wireless",
            )
            .await?;
            let capabilities = wireless
                .get_property::<u32>("WirelessCapabilities")
                .await
                .map_err(dbus_error)?;
            if !device_eligible(device_type, managed, state, capabilities, &network.security) {
                continue;
            }
            compatible_device = true;
            let aps: Vec<OwnedObjectPath> = wireless
                .call("GetAllAccessPoints", &())
                .await
                .map_err(dbus_error)?;
            let mut matching = None;
            for ap in aps {
                let point = proxy(
                    &connection,
                    ap.as_str(),
                    "org.freedesktop.NetworkManager.AccessPoint",
                )
                .await?;
                if point
                    .get_property::<Vec<u8>>("Ssid")
                    .await
                    .map_err(dbus_error)?
                    != network.ssid.as_bytes()
                {
                    continue;
                }
                let flags = point
                    .get_property::<u32>("Flags")
                    .await
                    .map_err(dbus_error)?;
                let wpa = point
                    .get_property::<u32>("WpaFlags")
                    .await
                    .map_err(dbus_error)?;
                let rsn = point
                    .get_property::<u32>("RsnFlags")
                    .await
                    .map_err(dbus_error)?;
                if ap_compatible(&network.security, flags, wpa, rsn) {
                    matching = Some(ap);
                    break;
                }
            }
            if let Some(ap) = matching.or_else(|| {
                network
                    .hidden
                    .then(|| OwnedObjectPath::try_from("/").unwrap())
            }) {
                candidates.push((state == 100, path, ap));
            }
        }
        // Prefer an enabled compatible card already serving the user's desktop.
        let (_, device, ap) = choose_candidate(candidates, compatible_device)?;
        let selected_device = proxy(
            &connection,
            device.as_str(),
            "org.freedesktop.NetworkManager.Device",
        )
        .await?;
        let (_, active, _): (
            OwnedObjectPath,
            OwnedObjectPath,
            HashMap<String, OwnedValue>,
        ) = manager
            .call(
                "AddAndActivateConnection2",
                &(request.settings, device.clone(), ap, request.options),
            )
            .await
            .map_err(dbus_error)?;
        let active = proxy(
            &connection,
            active.as_str(),
            "org.freedesktop.NetworkManager.Connection.Active",
        )
        .await?;
        // Subscribe before reading State so a transition between the two cannot be lost.
        let mut changes = active
            .receive_signal("StateChanged")
            .await
            .map_err(dbus_error)?;
        let state = active
            .get_property::<u32>("State")
            .await
            .map_err(dbus_error)?;
        if let Some(result) = observed_activation_outcome(state, 0, &selected_device).await {
            return result;
        }
        while let Some(message) = changes.next().await {
            let (state, reason): (u32, u32) = message
                .body()
                .deserialize()
                .map_err(|_| ActionError::ActivationFailed)?;
            if let Some(result) = observed_activation_outcome(state, reason, &selected_device).await
            {
                return result;
            }
        }
        Err(ActionError::ActivationFailed)
    }
}
fn ap_compatible(security: &WifiSecurity, flags: u32, wpa: u32, rsn: u32) -> bool {
    match security {
        WifiSecurity::Open => flags & 1 == 0 && wpa == 0 && rsn == 0,
        WifiSecurity::WpaPersonal => (wpa | rsn) & 0x100 != 0,
        WifiSecurity::Sae => rsn & 0x400 != 0,
        WifiSecurity::Unsupported(_) => false,
    }
}
fn device_eligible(
    device_type: u32,
    managed: bool,
    state: u32,
    capabilities: u32,
    security: &WifiSecurity,
) -> bool {
    device_type == 2
        && managed
        && state >= 30
        && match security {
            WifiSecurity::Open => true,
            WifiSecurity::WpaPersonal => capabilities & 0x30 != 0,
            WifiSecurity::Sae => capabilities & 0x20 != 0,
            WifiSecurity::Unsupported(_) => false,
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wifi(auth: &str) -> WifiNetwork {
        let password = if auth == "nopass" {
            ""
        } else {
            "P:very-secret;"
        };
        crate::results::wifi::parse(&format!("WIFI:T:{auth};S:Café;{password}H:true;;")).unwrap()
    }
    #[test]
    fn constructed_open_wifi_with_password_is_rejected_before_adapter() {
        let mut network = wifi("nopass");
        network.password = Some("secret".into());
        assert_eq!(
            WifiRequest::new(&network).unwrap_err(),
            ActionError::InvalidRequest
        );
    }
    #[test]
    fn typed_request_preserves_bytes_security_and_private_lifetime() {
        for (auth, key) in [
            ("nopass", None),
            ("WPA", Some("wpa-psk")),
            ("SAE", Some("sae")),
        ] {
            let request = WifiRequest::new(&wifi(auth)).unwrap();
            let wireless = &request.settings["802-11-wireless"];
            assert_eq!(
                Vec::<u8>::try_from(wireless["ssid"].try_clone().unwrap()).unwrap(),
                "Café".as_bytes()
            );
            assert!(bool::try_from(&wireless["hidden"]).unwrap());
            assert!(!bool::try_from(&request.settings["connection"]["autoconnect"]).unwrap());
            assert_eq!(
                <&str>::try_from(&request.options["persist"]).unwrap(),
                "volatile"
            );
            match key {
                Some(key) => assert_eq!(
                    <&str>::try_from(&request.settings["802-11-wireless-security"]["key-mgmt"])
                        .unwrap(),
                    key
                ),
                None => assert!(!request.settings.contains_key("802-11-wireless-security")),
            }
            assert!(!format!("{request:?}").contains("very-secret"));
        }
    }
    #[test]
    fn malformed_and_unsupported_rejected_before_adapter() {
        let mut n = wifi("WPA");
        n.ssid = "x".repeat(33);
        assert!(WifiRequest::new(&n).is_err());
        n.ssid = "home".into();
        n.password = Some("short".into());
        assert!(WifiRequest::new(&n).is_err());
        n.security = WifiSecurity::Unsupported("WEP".into());
        assert!(WifiRequest::new(&n).is_err());
        assert!(!format!("{:?}", WifiRequest::new(&n)).contains("short"));
        assert!(
            validated_uri(&ResultAction::OpenUrl(
                url::Url::parse("file:///etc/passwd").unwrap()
            ))
            .is_err()
        );
        assert!(validated_uri(&ResultAction::Call(url::Url::parse("tel:bad").unwrap())).is_err());
    }
    #[test]
    fn activation_completion_requires_connected_state() {
        assert_eq!(activation_outcome(1, 0), None);
        assert_eq!(activation_outcome(2, 0), Some(Ok(())));
        assert!(
            activation_outcome(4, 9)
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("password")
        );
        assert!(activation_outcome(4, 0).unwrap().is_err());
    }
    #[test]
    fn device_failure_reasons_refine_activation_feedback() {
        assert_eq!(
            activation_outcome_with_device(4, 3, Some(7)),
            Some(Err(ActionError::PasswordRejected))
        );
        assert_eq!(
            activation_outcome_with_device(4, 3, Some(10)),
            Some(Err(ActionError::PasswordRejected))
        );
        assert_eq!(
            activation_outcome_with_device(4, 3, Some(53)),
            Some(Err(ActionError::NetworkUnavailable))
        );
        assert_eq!(
            activation_outcome_with_device(4, 3, Some(17)),
            Some(Err(ActionError::ActivationFailed))
        );
        assert_eq!(activation_outcome_with_device(2, 0, Some(7)), Some(Ok(())));
    }
    #[test]
    fn device_selection_prefers_connected_card_and_distinguishes_absence() {
        let candidate = |active: bool, n: &str| {
            (
                active,
                OwnedObjectPath::try_from(format!("/dev/{n}")).unwrap(),
                OwnedObjectPath::try_from("/ap/one").unwrap(),
            )
        };
        assert_eq!(
            choose_candidate(
                vec![candidate(false, "idle"), candidate(true, "serving")],
                true
            )
            .unwrap()
            .1
            .as_str(),
            "/dev/serving"
        );
        assert!(matches!(
            choose_candidate(Vec::new(), false),
            Err(ActionError::NoDevice)
        ));
        assert!(matches!(
            choose_candidate(Vec::new(), true),
            Err(ActionError::NetworkUnavailable)
        ));
    }
    #[test]
    fn discovery_rejects_unmanaged_or_incompatible_devices_and_access_points() {
        assert!(!device_eligible(2, false, 100, 0x20, &WifiSecurity::Sae));
        assert!(!device_eligible(1, true, 100, 0x20, &WifiSecurity::Sae));
        assert!(!device_eligible(2, true, 20, 0x20, &WifiSecurity::Sae));
        assert!(!device_eligible(2, true, 30, 0x10, &WifiSecurity::Sae));
        assert!(device_eligible(2, true, 30, 0x20, &WifiSecurity::Sae));
        assert!(ap_compatible(&WifiSecurity::Sae, 1, 0, 0x400));
        assert!(!ap_compatible(&WifiSecurity::Sae, 1, 0x100, 0));
    }
    #[test]
    fn dbus_error_names_cover_service_and_authorization() {
        assert_eq!(
            dbus_error_name("org.freedesktop.DBus.Error.ServiceUnknown"),
            ActionError::ServiceUnavailable
        );
        assert_eq!(
            dbus_error_name("org.freedesktop.NetworkManager.Settings.Connection.NotAuthorized"),
            ActionError::Authorization
        );
    }
    struct FakeAdapter {
        failure: Option<ActionError>,
        accepted: std::sync::atomic::AtomicBool,
    }
    impl WifiAdapter for FakeAdapter {
        async fn connect(
            &self,
            _request: WifiRequest,
            _network: &WifiNetwork,
        ) -> Result<(), ActionError> {
            self.accepted
                .store(true, std::sync::atomic::Ordering::SeqCst);
            // Models completion, never treating acceptance alone as activation success.
            self.failure.clone().map_or(Ok(()), Err)
        }
    }
    #[tokio::test]
    async fn adapter_failures_and_invalid_requests_are_redacted() {
        for failure in [
            ActionError::ServiceUnavailable,
            ActionError::NoDevice,
            ActionError::NetworkUnavailable,
            ActionError::Authorization,
            ActionError::PasswordRejected,
            ActionError::Timeout,
        ] {
            let adapter = FakeAdapter {
                failure: Some(failure.clone()),
                accepted: Default::default(),
            };
            assert_eq!(connect_with(&adapter, &wifi("WPA")).await, Err(failure));
            assert!(adapter.accepted.load(std::sync::atomic::Ordering::SeqCst));
        }
        let adapter = FakeAdapter {
            failure: None,
            accepted: Default::default(),
        };
        assert!(connect_with(&adapter, &wifi("WEP")).await.is_err());
        assert!(!adapter.accepted.load(std::sync::atomic::Ordering::SeqCst));
    }

    struct NeverCompletes;
    impl WifiAdapter for NeverCompletes {
        async fn connect(
            &self,
            _request: WifiRequest,
            _network: &WifiNetwork,
        ) -> Result<(), ActionError> {
            std::future::pending().await
        }
    }
    #[tokio::test]
    async fn adapter_timeout_covers_unfinished_connection() {
        assert_eq!(
            connect_with_timeout(&NeverCompletes, &wifi("WPA"), Duration::from_millis(1)).await,
            Err(ActionError::Timeout)
        );
    }
}
