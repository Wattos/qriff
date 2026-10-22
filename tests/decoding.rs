use image::{GrayImage, Luma};
use qr_scanner::{decode, results::QrResult};
// Deterministic fixtures are generated offline by qrcode 0.14, byte-mode input,
// four-module quiet zone and 8 pixels/module. No downloaded fixture data.
fn qr(payload: &[u8]) -> GrayImage {
    qrcode::QrCode::new(payload)
        .unwrap()
        .render::<Luma<u8>>()
        .module_dimensions(8, 8)
        .build()
}
#[test]
fn single_qr() {
    let result = decode::decode(&qr(b"hello local QR")).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].payload, b"hello local QR");
}
#[test]
fn multiple_qrs() {
    let a = qr(b"first");
    let b = qr(b"second");
    let mut canvas =
        GrayImage::from_pixel(a.width() + b.width() + 80, a.height() + 40, Luma([255]));
    image::imageops::replace(&mut canvas, &a, 0, 20);
    image::imageops::replace(&mut canvas, &b, i64::from(a.width() + 80), 20);
    let result = decode::decode(&canvas).unwrap();
    assert_eq!(result.len(), 2);
    let mut payloads: Vec<_> = result.into_iter().map(|r| r.payload).collect();
    payloads.sort();
    assert_eq!(payloads, vec![b"first".to_vec(), b"second".to_vec()]);
}
#[test]
fn rotated_qr() {
    let result = decode::decode(&image::imageops::rotate90(&qr(b"rotated"))).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].payload, b"rotated");
}
#[test]
fn blank_image() {
    assert!(
        decode::decode(&GrayImage::from_pixel(200, 200, Luma([255])))
            .unwrap()
            .is_empty()
    );
}
#[test]
fn invalid_image() {
    assert!(decode::load_image(b"not an encoded image").is_err());
}
#[test]
fn preserves_non_utf8_bytes() {
    let results = decode::decode(&qr(&[0, 255, 128, 65])).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].payload, [0, 255, 128, 65]);
    assert_eq!(results[0].display_text, "Binary payload (hex): 00 ff 80 41");
    assert!(results[0].action().is_none());
}
#[test]
fn safe_urls_only() {
    for s in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "https://",
        "plain text",
        "https://user:pass@example.com",
    ] {
        assert!(
            QrResult::from_payload(s.as_bytes().to_vec())
                .action()
                .is_none(),
            "{s}"
        );
    }
    for s in ["http://example.com/a", "https://example.org/?x=1"] {
        let result = QrResult::from_payload(s.as_bytes().to_vec());
        assert_eq!(result.display_text, s);
        assert!(
            matches!(result.action(), Some(qr_scanner::results::ResultAction::OpenUrl(u)) if u.as_str() == s)
        );
    }
}
#[test]
fn duplicate_codes_are_one_result() {
    let a = qr(b"same");
    let mut canvas = GrayImage::from_pixel(a.width() * 2 + 80, a.height(), Luma([255]));
    image::imageops::replace(&mut canvas, &a, 0, 0);
    image::imageops::replace(&mut canvas, &a, i64::from(a.width() + 80), 0);
    let result = decode::decode(&canvas).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].payload, b"same");
}
