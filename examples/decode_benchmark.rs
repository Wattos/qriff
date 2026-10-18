//! Deterministic full-frame decode benchmark, no screen/camera data persisted.
use image::{GrayImage, Luma};
use std::time::Instant;
fn main() {
    let qr = qrcode::QrCode::new(b"local benchmark https://example.org/")
        .unwrap()
        .render::<Luma<u8>>()
        .module_dimensions(8, 8)
        .build();
    for (width, height) in [(1920, 1080), (3840, 2160)] {
        let mut frame = GrayImage::from_pixel(width, height, Luma([255]));
        image::imageops::replace(&mut frame, &qr, 100, 100);
        for _ in 0..2 {
            assert_eq!(qr_scanner::decode::decode(&frame).unwrap().len(), 1);
        }
        let mut samples = Vec::new();
        for _ in 0..10 {
            let start = Instant::now();
            assert_eq!(qr_scanner::decode::decode(&frame).unwrap().len(), 1);
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "{width}x{height} n=10 min_ms={:.3} median_ms={:.3} max_ms={:.3}",
            samples[0],
            (samples[4] + samples[5]) / 2.0,
            samples[9]
        );
    }
}
