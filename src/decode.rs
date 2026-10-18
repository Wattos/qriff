use crate::results::QrResult;
use anyhow::{Context, ensure};
use std::{collections::HashSet, io::Cursor};
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
    ensure!(image.width() > 0 && image.height() > 0, "Image is empty");
    ensure!(
        u64::from(image.width()) * u64::from(image.height()) <= 64 * 1024 * 1024,
        "Image is too large to scan"
    );
    let mut prepared = rqrr::PreparedImage::prepare(image.clone());
    let mut seen = HashSet::new();
    let mut results = Vec::new();
    let mut failure = None;
    for grid in prepared.detect_grids() {
        let mut payload = Vec::new();
        // decode_to writes raw bytes; Grid::decode instead requires valid UTF-8.
        match grid.decode_to(&mut payload) {
            Ok(_) => {
                if seen.insert(payload.clone()) {
                    results.push(QrResult::from_payload(payload));
                }
            }
            Err(error) => failure = Some(error),
        }
    }
    if results.is_empty()
        && let Some(error) = failure
    {
        anyhow::bail!("Detected QR code could not be decoded: {error}");
    }
    Ok(results)
}
