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
        assert_eq!(scan.detections.len(), 1);
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
