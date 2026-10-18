/// A decoded payload. Copy uses display_text, preserving readable text or a
/// clearly labelled hex rendering when the original bytes are not UTF-8.
#[derive(Debug, Clone)]
pub struct QrResult {
    pub payload: Vec<u8>,
    pub display_text: String,
    pub open_url: Option<url::Url>,
}
impl QrResult {
    pub fn from_payload(payload: Vec<u8>) -> Self {
        let (display_text, open_url) = match std::str::from_utf8(&payload) {
            Ok(text) => {
                let url = url::Url::parse(text).ok().filter(|url| {
                    matches!(url.scheme(), "http" | "https")
                        && url.host_str().is_some_and(|host| !host.is_empty())
                        && url.username().is_empty()
                        && url.password().is_none()
                        && !text.chars().any(char::is_control)
                });
                (text.to_owned(), url)
            }
            Err(_) => {
                let hex = payload
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                (format!("Binary payload (hex): {hex}"), None)
            }
        };
        Self {
            payload,
            display_text,
            open_url,
        }
    }
}
