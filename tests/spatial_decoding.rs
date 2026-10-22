use image::{GrayImage, Luma};
use qr_scanner::decode;
fn qr(payload: &[u8]) -> GrayImage {
    qrcode::QrCode::new(payload)
        .unwrap()
        .render::<Luma<u8>>()
        .module_dimensions(8, 8)
        .build()
}
#[test]
fn duplicate_placements_keep_both_regions() {
    let code = qr(b"same");
    let mut canvas = GrayImage::from_pixel(750, 350, Luma([255]));
    for x in [20, 400] {
        image::imageops::replace(&mut canvas, &code, x, 30);
    }
    let scan = decode::decode_spatial(&canvas).unwrap();
    assert_eq!(scan.results.len(), 1);
    assert_eq!(scan.detections.len(), 2);
    let mut centers = Vec::new();
    for d in scan.detections {
        assert_eq!(d.result_index, 0);
        let center = d
            .corners
            .iter()
            .fold([0., 0.], |a, p| [a[0] + p[0] / 4., a[1] + p[1] / 4.]);
        assert!(center[1] > 30. && center[1] < (30 + code.height()) as f32);
        centers.push(center[0]);
    }
    centers.sort_by(f32::total_cmp);
    for (x, offset) in centers.iter().zip([20, 400]) {
        assert!(*x > offset as f32 && *x < (offset + code.width()) as f32);
    }
}
#[test]
fn rotated_distinct_and_binary_preserve_payload_and_bounds() {
    for payload in [b"distinct".as_slice(), &[0, 255, 128, 65]] {
        let code = image::imageops::rotate90(&qr(payload));
        let scan = decode::decode_spatial(&code).unwrap();
        assert_eq!(scan.results[0].payload, payload);
        assert_eq!(
            scan.detections.len(),
            1,
            "retry must not duplicate a rotated code"
        );
        for p in scan.detections[0].corners {
            assert!(
                p[0] >= 0.
                    && p[0] <= code.width() as f32
                    && p[1] >= 0.
                    && p[1] <= code.height() as f32
            );
        }
        assert_eq!(
            decode::decode(&code).unwrap()[0].payload,
            scan.results[0].payload
        );
    }
}
#[test]
fn blank_is_empty_and_empty_dimensions_fail() {
    let scan = decode::decode_spatial(&GrayImage::from_pixel(200, 200, Luma([255]))).unwrap();
    assert!(scan.results.is_empty() && scan.detections.is_empty());
    assert!(decode::decode_spatial(&GrayImage::new(0, 0)).is_err());
}

#[test]
fn mixed_version_screen_recovers_every_location_without_duplicate_highlights() {
    // Independently generated QR data at browser-like fractional display sizes.
    // No user screenshot or private QR contents are stored in the repository.
    let mut canvas = GrayImage::from_pixel(1784, 1176, Luma([255]));
    let mut expected = Vec::new();
    for (index, version) in [1, 2, 3, 4, 10, 25].into_iter().enumerate() {
        let payload = match version {
            1 => "Ver1".to_owned(),
            2 => "Version 2".to_owned(),
            3 => "Version 3 QR Code".to_owned(),
            4 => "Version 4 QR Code, up to 50 char".to_owned(),
            10 => "VERSION 10 QR CODE, UP TO 174 CHAR AT H LEVEL, WITH 57X57 MODULES AND PLENTY OF ERROR CORRECTION TO GO AROUND. NOTE THAT THERE ARE".to_owned(),
            _ => "A local QR code test with no private information. ".repeat(25),
        };
        let code = qrcode::QrCode::with_version(
            payload.as_bytes(),
            qrcode::Version::Normal(version),
            if version == 25 {
                qrcode::EcLevel::L
            } else {
                qrcode::EcLevel::H
            },
        )
        .unwrap()
        .render::<Luma<u8>>()
        .module_dimensions(8, 8)
        .build();
        let size = [284, 277, 263, 294, 278, 309][index];
        let code =
            image::imageops::resize(&code, size, size, image::imageops::FilterType::Triangle);
        let tile_x = [303, 665, 1026][index % 3];
        let tile_y = [142, 605][index / 3];
        image::imageops::replace(
            &mut canvas,
            &code,
            i64::from(tile_x + (351 - size) / 2),
            i64::from(tile_y + (351 - size) / 2),
        );
        expected.push((payload, tile_x, tile_y));
    }
    let decoder = qr_scanner::jobs::Decoder::new();
    let mut outcomes = decoder.subscribe();
    decoder.submit_screen(qr_scanner::jobs::JobId(1), canvas.clone());
    let outcome = tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(10), outcomes.recv())
            .await
            .unwrap()
            .unwrap()
    });
    assert_eq!(
        outcome.result.as_ref().unwrap().len(),
        6,
        "window screen scans must use the robust retry too"
    );
    let scan = decode::decode_spatial(&canvas).unwrap();
    assert_eq!(
        scan.results.len(),
        6,
        "every valid QR payload must be recovered"
    );
    assert_eq!(
        scan.detections.len(),
        6,
        "retries must not add duplicate highlights"
    );
    for (payload, x, y) in expected {
        let result_index = scan
            .results
            .iter()
            .position(|r| r.payload == payload.as_bytes())
            .unwrap();
        let positions: Vec<_> = scan
            .detections
            .iter()
            .filter(|d| d.result_index == result_index)
            .collect();
        assert_eq!(positions.len(), 1);
        for [px, py] in positions[0].corners {
            assert!(px >= x as f32 && px <= (x + 351) as f32);
            assert!(py >= y as f32 && py <= (y + 351) as f32);
        }
    }
}
