use qr_scanner::results::{
    ContentKind, QrContent, QrResult, ResultAction,
    wifi::{self, WifiSecurity},
};
fn result(s: &str) -> QrResult {
    QrResult::from_payload(s.as_bytes().to_vec())
}
#[test]
fn only_safe_web_urls_are_actionable() {
    for s in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "https://",
        "https://user:pass@example.com",
        "https://example.com/\n",
    ] {
        assert!(result(s).action().is_none());
    }
    for s in ["http://example.com/a", "https://example.org/?x=1"] {
        assert!(matches!(result(s).action(), Some(ResultAction::OpenUrl(u)) if u.as_str() == s));
    }
}
#[test]
fn telephone_requires_explicit_validated_uri() {
    for s in ["tel:+41446681800", "tel:+41-(44)-668.18.00;ext=123"] {
        assert!(matches!(result(s).action(), Some(ResultAction::Call(u)) if u.as_str() == s));
    }
    assert!(matches!(result("41446681800").content, QrContent::Text));
    for s in [
        "tel:",
        "tel:+",
        "tel:123?x=y",
        "tel:123\n",
        "tel:abc",
        "tel:12%0a34",
        "tel:123;ext=",
        "tel:123;foo=4",
        "tel:123#x",
    ] {
        assert!(
            matches!(
                result(s).content,
                QrContent::Invalid {
                    kind: ContentKind::Phone,
                    ..
                }
            ),
            "{s:?}"
        );
        assert!(result(s).action().is_none());
    }
}
#[test]
fn binary_remains_lossless() {
    let r = QrResult::from_payload(vec![0, 255, 128, 65]);
    assert_eq!(r.payload, [0, 255, 128, 65]);
    assert_eq!(r.display_text, "Binary payload (hex): 00 ff 80 41");
    assert!(matches!(r.content, QrContent::Binary));
    assert!(r.action().is_none());
}
#[test]
fn wifi_fields_and_escaping() {
    for (input, ssid, security, password, hidden) in [
        (
            "WIFI:T:nopass;S:Guest;;",
            "Guest",
            WifiSecurity::Open,
            None,
            false,
        ),
        (
            "WIFI:T:WPA;S:Home;P:secret;H:true;;",
            "Home",
            WifiSecurity::WpaPersonal,
            Some("secret"),
            true,
        ),
        (
            "WIFI:T:SAE;S:Home;P:secret;;",
            "Home",
            WifiSecurity::Sae,
            Some("secret"),
            false,
        ),
        (
            r#"WIFI:T:WPA;S:a\;b\:c\,d\"e\\f;P:p\;q\:r\,s\"t\\u;;"#,
            "a;b:c,d\"e\\f",
            WifiSecurity::WpaPersonal,
            Some("p;q:r,s\"t\\u"),
            false,
        ),
    ] {
        let n = wifi::parse(input).unwrap();
        assert_eq!(n.ssid, ssid);
        assert_eq!(n.security, security);
        assert_eq!(n.password.as_deref(), password);
        assert_eq!(n.hidden, hidden);
        assert!(matches!(
            result(input).action(),
            Some(ResultAction::ConnectWifi(_))
        ));
    }
}
#[test]
fn malformed_wifi_never_connects() {
    for s in [
        "WIFI:T:nopass;;",
        "WIFI:S:;T:nopass;;",
        "WIFI:S:a;S:b;T:nopass;;",
        "WIFI:S:a;T:WPA;;",
        "WIFI:S:a;T:nopass;",
        "WIFI:S:a;T:nopass;P:abc\\",
        "WIFI:S:a;T:nopass;H:maybe;;",
        "WIFI:S:a;T:nopass;X:unknown;;",
    ] {
        assert!(wifi::parse(s).is_err(), "{s}");
        assert!(result(s).action().is_none());
    }
}
#[test]
fn unsupported_wifi_stays_unsupported_and_secrets_are_redacted() {
    for s in [
        "WIFI:S:a;T:WEP;P:sensitive;;",
        "WIFI:S:a;T:WPA2-EAP;P:sensitive;E:PEAP;I:user;;",
        "WIFI:S:a;T:unknown;P:sensitive;;",
        "WIFI:S:a;T:nopass;E:PEAP;I:user;P:sensitive;;",
        "WIFI:S:a;T:WPA;P:sensitive;PH2:MSCHAPV2;;",
    ] {
        let n = wifi::parse(s).unwrap();
        assert!(matches!(n.security, WifiSecurity::Unsupported(_)));
        let r = result(s);
        assert!(r.action().is_none());
        assert!(!format!("{n:?} {r:?}").contains("sensitive"));
    }
    let r = result("WIFI:S:a;T:WPA;P:sensitive;;");
    assert!(!format!("{:?} {:?}", r, r.action()).contains("sensitive"));
}

#[test]
fn malformed_wifi_debug_and_errors_do_not_echo_credentials() {
    let input = "WIFI:S:Home;T:WPA;P:sensitive;S:other;;";
    let r = result(input);
    assert_eq!(r.display_text, input);
    assert_eq!(r.payload, input.as_bytes());
    assert!(!format!("{r:?}").contains("sensitive"));
    assert!(!wifi::parse(input).unwrap_err().contains("sensitive"));
}
