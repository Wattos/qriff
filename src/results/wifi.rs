use std::fmt;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WifiSecurity {
    Open,
    WpaPersonal,
    Sae,
    Unsupported(String),
}
#[derive(Clone)]
pub struct WifiNetwork {
    pub ssid: String,
    pub security: WifiSecurity,
    pub password: Option<String>,
    pub hidden: bool,
}
impl fmt::Debug for WifiNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WifiNetwork")
            .field("ssid", &self.ssid)
            .field("security", &self.security)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .field("hidden", &self.hidden)
            .finish()
    }
}
/// Parse only the common, terminated WIFI format. Errors never include field values.
pub fn parse(input: &str) -> Result<WifiNetwork, String> {
    fn invalid() -> String {
        "Invalid Wi-Fi payload".into()
    }
    let body = input.strip_prefix("WIFI:").ok_or_else(invalid)?;
    if body.chars().any(char::is_control) {
        return Err(invalid());
    }
    let mut fields = std::collections::HashMap::new();
    let mut key = String::new();
    let mut value = String::new();
    let mut in_value = false;
    let mut escaped = false;
    let mut terminated = false;
    for c in body.chars() {
        if terminated {
            return Err(invalid());
        }
        if escaped {
            if !matches!(c, ';' | ':' | ',' | '"' | '\\') {
                return Err(invalid());
            }
            if in_value {
                value.push(c);
            } else {
                return Err(invalid());
            }
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == ':' && !in_value {
            if key.is_empty() {
                return Err(invalid());
            }
            in_value = true;
        } else if c == ';' {
            if key.is_empty() && !in_value {
                terminated = true;
            } else {
                if !in_value || fields.insert(key.clone(), value.clone()).is_some() {
                    return Err(invalid());
                }
                key.clear();
                value.clear();
                in_value = false;
            }
        } else if in_value {
            value.push(c);
        } else {
            key.push(c);
        }
    }
    if escaped || !terminated || !key.is_empty() || in_value {
        return Err(invalid());
    }
    if fields
        .keys()
        .any(|k| !matches!(k.as_str(), "S" | "T" | "P" | "H" | "E" | "I" | "A" | "PH2"))
    {
        return Err(invalid());
    }
    let ssid = fields
        .get("S")
        .filter(|s| !s.is_empty())
        .ok_or_else(invalid)?
        .clone();
    let auth = fields.get("T").map(String::as_str).unwrap_or("nopass");
    let enterprise = ["E", "I", "A", "PH2"]
        .iter()
        .any(|k| fields.contains_key(*k));
    let security = if enterprise {
        WifiSecurity::Unsupported("Enterprise/EAP".into())
    } else {
        match auth {
            "nopass" => WifiSecurity::Open,
            "WPA" | "WPA2" => WifiSecurity::WpaPersonal,
            "SAE" => WifiSecurity::Sae,
            other => WifiSecurity::Unsupported(other.into()),
        }
    };
    let password = fields.get("P").cloned();
    if matches!(security, WifiSecurity::WpaPersonal | WifiSecurity::Sae)
        && password.as_ref().is_none_or(|p| p.is_empty())
    {
        return Err("Wi-Fi password required".into());
    }
    let hidden = match fields.get("H").map(String::as_str) {
        None | Some("false") => false,
        Some("true") => true,
        _ => return Err(invalid()),
    };
    Ok(WifiNetwork {
        ssid,
        security,
        password,
        hidden,
    })
}
