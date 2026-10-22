use crate::results::QrResult;
use anyhow::{Context, ensure};
use std::{collections::HashMap, io::Cursor};
/// Load a local encoded image with explicit dimensions/allocation limits.
pub fn load_image(bytes: &[u8]) -> anyhow::Result<image::GrayImage> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode().context("Cannot read image")?.into_luma8())
}
/// Decode in detector discovery order; partial failed payloads are discarded.
/// A blank image is successful and empty. Detected but unreadable grids fail
/// only when no other grid decoded successfully.
pub fn decode(image: &image::GrayImage) -> anyhow::Result<Vec<QrResult>> {
    Ok(decode_image(image, false)?.results)
}

/// One successfully decoded grid, in detector corner order.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub result_index: usize,
    pub corners: [[f32; 2]; 4],
}

#[derive(Debug, Clone)]
pub struct SpatialScan {
    pub results: Vec<QrResult>,
    pub detections: Vec<Detection>,
}

/// Screen scans combine adaptive and fixed-threshold detection. Camera decoding
/// retains its original single pass so live frames do not pay for screen retries.
pub fn decode_spatial(image: &image::GrayImage) -> anyhow::Result<SpatialScan> {
    decode_image(image, true)
}
fn decode_image(image: &image::GrayImage, screen_pass: bool) -> anyhow::Result<SpatialScan> {
    ensure!(image.width() > 0 && image.height() > 0, "Image is empty");
    ensure!(
        u64::from(image.width()) * u64::from(image.height()) <= 64 * 1024 * 1024,
        "Image is too large to scan"
    );
    let mut seen = HashMap::new();
    let mut scan = SpatialScan {
        results: Vec::new(),
        detections: Vec::new(),
    };
    let mut failure = None;
    {
        let mut prepared = rqrr::PreparedImage::prepare(image.clone());
        collect_grids(prepared.detect_grids(), &mut seen, &mut scan, &mut failure);
    }
    if screen_pass {
        // Browser scaling creates gray module edges. A second, fixed threshold
        // can recover grids that adaptive binarization joined across QR codes.
        // Preserve first-pass successes: dense codes may prefer that threshold.
        // The first prepared image is already dropped, bounding peak memory.
        const SCREEN_THRESHOLD: u8 = 160;
        let width = image.width() as usize;
        let pixels = image.as_raw();
        let mut prepared =
            rqrr::PreparedImage::prepare_from_bitmap(width, image.height() as usize, |x, y| {
                pixels[y * width + x] < SCREEN_THRESHOLD
            });
        collect_grids(prepared.detect_grids(), &mut seen, &mut scan, &mut failure);
    }
    if scan.results.is_empty()
        && let Some(error) = failure
    {
        anyhow::bail!("Detected QR code could not be decoded: {error}");
    }
    Ok(scan)
}
fn collect_grids<G: rqrr::BitGrid>(
    grids: Vec<rqrr::Grid<G>>,
    seen: &mut HashMap<Vec<u8>, usize>,
    scan: &mut SpatialScan,
    failure: &mut Option<rqrr::DeQRError>,
) {
    for grid in grids {
        let mut payload = Vec::new();
        // decode_to preserves arbitrary bytes, unlike the UTF-8-only decode API.
        match grid.decode_to(&mut payload) {
            Ok(_) => {
                let result_index = *seen.entry(payload.clone()).or_insert_with(|| {
                    let index = scan.results.len();
                    scan.results.push(QrResult::from_payload(payload));
                    index
                });
                let detection = Detection {
                    result_index,
                    corners: grid.bounds.map(|p| [p.x as f32, p.y as f32]),
                };
                // Merge repeated passes over the same physical code, while
                // retaining separate locations with identical payload bytes.
                if !scan.detections.iter().any(|prior| {
                    prior.result_index == result_index && same_location(prior, &detection)
                }) {
                    scan.detections.push(detection);
                }
            }
            Err(error) => *failure = Some(error),
        }
    }
}
fn same_location(a: &Detection, b: &Detection) -> bool {
    fn bounds(d: &Detection) -> [f32; 4] {
        let mut bounds = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for [x, y] in d.corners {
            bounds[0] = bounds[0].min(x);
            bounds[1] = bounds[1].min(y);
            bounds[2] = bounds[2].max(x);
            bounds[3] = bounds[3].max(y);
        }
        bounds
    }
    let a = bounds(a);
    let b = bounds(b);
    let intersection =
        (a[2].min(b[2]) - a[0].max(b[0])).max(0.) * (a[3].min(b[3]) - a[1].max(b[1])).max(0.);
    let union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - intersection;
    union > 0. && intersection >= union * 0.75
}
