pub mod wifi;
pub use wifi::{WifiNetwork, WifiSecurity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentKind {
    Website,
    Phone,
    Wifi,
}
#[derive(Debug, Clone)]
pub enum QrContent {
    Url(url::Url),
    Phone { uri: url::Url, number: String },
    Wifi(WifiNetwork),
    Text,
    Binary,
    Invalid { kind: ContentKind, reason: String },
}
#[derive(Debug, Clone)]
pub enum ResultAction {
    OpenUrl(url::Url),
    Call(url::Url),
    ConnectWifi(WifiNetwork),
}

/// Raw bytes and copy text are preserved separately from validated action data.
#[derive(Clone)]
pub struct QrResult {
    pub payload: Vec<u8>,
    pub display_text: String,
    pub content: QrContent,
}
impl std::fmt::Debug for QrResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QrResult")
            .field("content", &self.content)
            .finish_non_exhaustive()
    }
}
impl QrResult {
    pub fn action(&self) -> Option<ResultAction> {
        match &self.content {
            QrContent::Url(uri) => Some(ResultAction::OpenUrl(uri.clone())),
            QrContent::Phone { uri, .. } => Some(ResultAction::Call(uri.clone())),
            QrContent::Wifi(network)
                if !matches!(network.security, WifiSecurity::Unsupported(_)) =>
            {
                Some(ResultAction::ConnectWifi(network.clone()))
            }
            _ => None,
        }
    }
    pub fn from_payload(payload: Vec<u8>) -> Self {
        let (display_text, content) = match std::str::from_utf8(&payload) {
            Ok(text) => (text.to_owned(), classify(text)),
            Err(_) => {
                let hex = payload
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                (format!("Binary payload (hex): {hex}"), QrContent::Binary)
            }
        };
        Self {
            payload,
            display_text,
            content,
        }
    }
}
fn classify(text: &str) -> QrContent {
    fn invalid(kind: ContentKind, reason: &str) -> QrContent {
        QrContent::Invalid {
            kind,
            reason: reason.into(),
        }
    }
    if text.starts_with("WIFI:") {
        return match wifi::parse(text) {
            Ok(network) => QrContent::Wifi(network),
            Err(reason) => QrContent::Invalid {
                kind: ContentKind::Wifi,
                reason,
            },
        };
    }
    let scheme = text.split_once(':').map(|(scheme, _)| scheme);
    if scheme.is_some_and(|s| s.eq_ignore_ascii_case("tel")) {
        let number = text.split_once(':').unwrap().1;
        let (base, extension) = number
            .split_once(";ext=")
            .map_or((number, None), |(n, e)| (n, Some(e)));
        let digits = base.strip_prefix('+').unwrap_or(base);
        if !digits.is_empty()
            && digits.chars().any(|c| c.is_ascii_digit())
            && digits
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | '(' | ')'))
            && extension.is_none_or(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_digit()))
            && let Ok(uri) = url::Url::parse(text)
        {
            return QrContent::Phone {
                uri,
                number: number.into(),
            };
        }
        return invalid(ContentKind::Phone, "Invalid telephone number");
    }
    if let Ok(uri) = url::Url::parse(text)
        && matches!(uri.scheme(), "http" | "https")
        && uri.host_str().is_some_and(|host| !host.is_empty())
        && uri.username().is_empty()
        && uri.password().is_none()
        && !text.chars().any(char::is_control)
    {
        return QrContent::Url(uri);
    }
    if scheme.is_some_and(|s| s.eq_ignore_ascii_case("http") || s.eq_ignore_ascii_case("https")) {
        invalid(ContentKind::Website, "Invalid website URL")
    } else {
        QrContent::Text
    }
}
